//! Contract conformance: validate the payloads this implementation emits against
//! the committed wire schemas in `contract/contract.json`.
//! A minimal JSON-Schema-subset validator covers exactly the constructs the
//! contract uses (strict objects, map-objects, arrays, enums, oneOf, nullable).

use serde_json::Value;
use sim::components::{Inventory, Item, Position, StructureKind};
use sim::geometry::ChunkCoord;
use sim::ids::Realm;
use sim::dev::stats_payload;
use sim::sim::{Action, Sim};
use sim::wire::{chunk_snapshot, inventory_payload, relocated_payload};

fn load_contract() -> Value {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../contract/contract.json");
    let text = std::fs::read_to_string(path).expect("read contract.json");
    serde_json::from_str(&text).expect("parse contract.json")
}

/// The `payload` schema for an outbound message `event`.
fn payload_schema(contract: &Value, event: &str) -> Value {
    contract["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["event"] == event && m["direction"] == "out")
        .unwrap_or_else(|| panic!("no out message {event}"))
        ["payload"]
        .clone()
}

/// Validate `value` against `schema` (the subset used by contract.json).
fn validate(schema: &Value, value: &Value, path: &str) -> Result<(), String> {
    if let Some(variants) = schema.get("oneOf").and_then(|v| v.as_array()) {
        let ok = variants.iter().any(|s| validate(s, value, path).is_ok());
        return if ok { Ok(()) } else { Err(format!("{path}: matched none of oneOf")) };
    }
    if let Some(en) = schema.get("enum").and_then(|v| v.as_array()) {
        let s = value.as_str().ok_or(format!("{path}: expected enum string"))?;
        return if en.iter().any(|e| e == s) {
            Ok(())
        } else {
            Err(format!("{path}: {s:?} not in enum"))
        };
    }
    match &schema["type"] {
        Value::String(t) => validate_typed(t, schema, value, path),
        Value::Array(types) => {
            // e.g. ["integer","null"]
            let ok = types.iter().any(|t| {
                let t = t.as_str().unwrap_or("");
                (t == "null" && value.is_null())
                    || validate_typed(t, schema, value, path).is_ok()
            });
            if ok { Ok(()) } else { Err(format!("{path}: no type in {types:?} matched")) }
        }
        _ => Ok(()), // no type constraint
    }
}

fn validate_typed(t: &str, schema: &Value, value: &Value, path: &str) -> Result<(), String> {
    match t {
        "object" => {
            let obj = value.as_object().ok_or(format!("{path}: expected object"))?;
            if let Some(props) = schema.get("properties").and_then(|v| v.as_object()) {
                // Strict object: required present, no extras, each prop valid.
                if let Some(req) = schema.get("required").and_then(|v| v.as_array()) {
                    for r in req {
                        let k = r.as_str().unwrap();
                        if !obj.contains_key(k) {
                            return Err(format!("{path}: missing required '{k}'"));
                        }
                    }
                }
                let extra_allowed = schema.get("additionalProperties").map(|v| v != &Value::Bool(false)).unwrap_or(true);
                for (k, v) in obj {
                    match props.get(k) {
                        Some(ps) => validate(ps, v, &format!("{path}.{k}"))?,
                        None if extra_allowed => {}
                        None => return Err(format!("{path}: unexpected property '{k}'")),
                    }
                }
            } else if let Some(ap) = schema.get("additionalProperties") {
                // Map object: every value matches the additionalProperties schema.
                if let Some(vs) = ap.as_object().map(|_| ap) {
                    for (k, v) in obj {
                        validate(vs, v, &format!("{path}.{k}"))?;
                    }
                }
            }
            Ok(())
        }
        "array" => {
            let arr = value.as_array().ok_or(format!("{path}: expected array"))?;
            if let Some(min) = schema.get("minItems").and_then(|v| v.as_u64()) {
                if (arr.len() as u64) < min {
                    return Err(format!("{path}: fewer than {min} items"));
                }
            }
            if let Some(max) = schema.get("maxItems").and_then(|v| v.as_u64()) {
                if (arr.len() as u64) > max {
                    return Err(format!("{path}: more than {max} items"));
                }
            }
            if let Some(items) = schema.get("items") {
                for (i, e) in arr.iter().enumerate() {
                    validate(items, e, &format!("{path}[{i}]"))?;
                }
            }
            Ok(())
        }
        "integer" => value.as_i64().map(|_| ()).ok_or(format!("{path}: expected integer")),
        "number" => value.as_f64().map(|_| ()).ok_or(format!("{path}: expected number")),
        "string" => value.as_str().map(|_| ()).ok_or(format!("{path}: expected string")),
        "boolean" => value.as_bool().map(|_| ()).ok_or(format!("{path}: expected boolean")),
        _ => Ok(()),
    }
}

#[test]
fn committed_contract_is_freshly_generated() {
    // The committed file must equal what the generator produces, so the schema
    // can never drift from the code. Regenerate with the `export-contract` bin.
    let committed = load_contract();
    let generated = sim::contract::contract();
    if committed != generated {
        // Point at the first differing message for a fast diagnosis.
        let empty = vec![];
        let cms = committed["messages"].as_array().unwrap_or(&empty);
        let gms = generated["messages"].as_array().unwrap_or(&empty);
        for cm in cms {
            let key = (&cm["direction"], &cm["event"]);
            match gms.iter().find(|g| (&g["direction"], &g["event"]) == key) {
                None => panic!("generator is missing message {key:?}"),
                Some(g) if *g != *cm => panic!("message {key:?} differs:\n committed={cm:#}\n generated={g:#}"),
                _ => {}
            }
        }
        let extra: Vec<_> = gms.iter().map(|g| (&g["direction"], &g["event"]))
            .filter(|k| !cms.iter().any(|c| (&c["direction"], &c["event"]) == *k)).collect();
        assert!(extra.is_empty(), "generator emits messages not in the committed file: {extra:?}");
        panic!("contract mismatch (count: committed {} vs generated {})", cms.len(), gms.len());
    }
}

#[test]
fn snapshot_payload_conforms() {
    let contract = load_contract();
    let schema = payload_schema(&contract, "snapshot");

    // A chunk with a player, worldgen trees + portal, and a built wall.
    let mut sim = Sim::new();
    let mut inv = Inventory::default();
    inv.items.insert(Item::Wood, 5);
    sim.connect_at("alice", Position { x: 2_700, y: 3_000 }, inv);
    sim.enqueue_action("alice", Action::Build { kind: StructureKind::Wall, x: 3_500, y: 3_000 });
    sim.tick();

    let states = sim.overworld().snapshot_states();
    let snap = chunk_snapshot(&states, ChunkCoord::new(0, 0));
    let value = serde_json::to_value(&snap).unwrap();

    // Has all four categories populated.
    assert!(value["players"].get("alice").is_some());
    assert_eq!(value["resource_nodes"].as_object().unwrap().len(), 5);
    assert!(value["structures"].get("structure:3500:3000").is_some());
    assert_eq!(value["portals"].as_object().unwrap().len(), 1);

    validate(&schema, &value, "snapshot").expect("snapshot conforms to contract");
}

#[test]
fn self_payload_conforms() {
    let contract = load_contract();
    let schema = payload_schema(&contract, "self");
    let mut inv = Inventory::default();
    inv.items.insert(Item::Wood, 7);
    let value = inventory_payload(&inv);
    validate(&schema, &value, "self").expect("self conforms");
}

#[test]
fn relocated_payload_conforms_both_realms() {
    let contract = load_contract();
    let schema = payload_schema(&contract, "relocated");

    let over = relocated_payload(Realm::Overworld, ChunkCoord::new(0, 0));
    validate(&schema, &over, "relocated/overworld").expect("overworld relocated conforms");

    let inst = relocated_payload(Realm::Instance(7), ChunkCoord::new(1, 1));
    validate(&schema, &inst, "relocated/instance").expect("instance relocated conforms");
}

#[test]
fn stats_payload_conforms() {
    let contract = load_contract();
    let schema = payload_schema(&contract, "stats");

    let mut sim = Sim::new();
    sim.connect("dev", ChunkCoord::new(0, 0));
    let value = stats_payload(&sim, Some("dev"));
    validate(&schema, &value, "stats").expect("stats conforms");
}

#[test]
fn validator_rejects_extra_and_missing_keys() {
    // Sanity-check the validator itself against the strict snapshot schema.
    let contract = load_contract();
    let schema = payload_schema(&contract, "self");
    // Missing required "inventory".
    assert!(validate(&schema, &serde_json::json!({}), "x").is_err());
    // Extra property.
    assert!(validate(&schema, &serde_json::json!({"inventory":{}, "extra":1}), "x").is_err());
    // Valid.
    assert!(validate(&schema, &serde_json::json!({"inventory":{"wood":3}}), "x").is_ok());
}
