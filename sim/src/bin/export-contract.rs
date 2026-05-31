//! Regenerate `contract/contract.json` from the server's types
//! ([`sim::contract::contract`]). The committed file is the generator's output;
//! the `committed_contract_is_freshly_generated` test guards that it stays so.

fn main() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../contract/contract.json");
    let json = serde_json::to_string_pretty(&sim::contract::contract()).expect("serialize contract");
    std::fs::write(path, format!("{json}\n")).expect("write contract.json");
    eprintln!("wrote {path}");
}
