//! Postgres-backed [`DurableStore`] — durable persistence so the server's
//! players, structures, and depletions survive a real process restart (the
//! ADR-0002 acceptance item; what `MemStore` cannot do).
//!
//! The blocking `postgres` client drives its own runtime internally, so it
//! cannot be called from a Tokio worker thread (that panics with "runtime
//! within a runtime"). So the client lives on its **own dedicated OS thread**
//! — the same shape as the Elixir Datastore being a separate process — and this
//! store is a synchronous request/response handle to it. Every call blocks the
//! caller until the DB thread replies, which makes `flush` durable before it
//! returns (so the shutdown flush is safe). Write volume is low and batched on
//! a ~1s cadence, so blocking the tick briefly is acceptable for the POC.
//!
//! The store ensures its own schema on connect, so it needs only an empty
//! database (its own, not the Elixir Ecto schema — see `DESIGN.md`). Positions are
//! sub-unit `BIGINT`s; depletion respawn is stored as an absolute epoch-ms
//! `BIGINT` (the server anchors its clock to wall-clock so it survives restart).

use crate::components::{Item, ResourceKind, StructureKind};
use crate::datastore::{DepletionRecord, DurableStore, PlayerRecord, StructureRecord};
use crate::geometry::ChunkCoord;
use postgres::{Client, NoTls};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::sync::mpsc::{channel, Sender};
use std::thread;

enum Req {
    LoadPlayer(String),
    SavePlayer(PlayerRecord),
    LoadStructures(ChunkCoord),
    SaveStructure(StructureRecord),
    DeleteStructure(i64, i64),
    LoadDepletions(ChunkCoord),
    SaveDepletion(DepletionRecord),
    DeleteDepletion(i64, i64),
}

enum Resp {
    Player(Option<PlayerRecord>),
    Structures(Vec<StructureRecord>),
    Depletions(Vec<DepletionRecord>),
    Ack,
}

pub struct PgStore {
    tx: Sender<(Req, Sender<Resp>)>,
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS players (
  username TEXT PRIMARY KEY, chunk_x INT NOT NULL, chunk_y INT NOT NULL,
  x BIGINT NOT NULL, y BIGINT NOT NULL, inventory JSONB NOT NULL DEFAULT '{}'
);
CREATE TABLE IF NOT EXISTS structures (
  x BIGINT NOT NULL, y BIGINT NOT NULL, chunk_x INT NOT NULL, chunk_y INT NOT NULL,
  owner_username TEXT NOT NULL, type TEXT NOT NULL, hp BIGINT NOT NULL, PRIMARY KEY (x, y)
);
CREATE TABLE IF NOT EXISTS depletions (
  x BIGINT NOT NULL, y BIGINT NOT NULL, chunk_x INT NOT NULL, chunk_y INT NOT NULL,
  type TEXT NOT NULL, respawn_at_ms BIGINT NOT NULL, PRIMARY KEY (x, y)
);
";

impl PgStore {
    /// Connect to `url` (libpq-style), ensure the schema, and spawn the DB
    /// thread. Returns once the connection + schema are ready.
    pub fn connect(url: &str) -> Result<Self, String> {
        let (cmd_tx, cmd_rx) = channel::<(Req, Sender<Resp>)>();
        let (ready_tx, ready_rx) = channel::<Result<(), String>>();
        let url = url.to_string();

        thread::Builder::new()
            .name("pgstore".into())
            .spawn(move || {
                let mut client = match Client::connect(&url, NoTls) {
                    Ok(c) => c,
                    Err(e) => {
                        let _ = ready_tx.send(Err(e.to_string()));
                        return;
                    }
                };
                if let Err(e) = client.batch_execute(SCHEMA) {
                    let _ = ready_tx.send(Err(e.to_string()));
                    return;
                }
                let _ = ready_tx.send(Ok(()));

                while let Ok((req, reply)) = cmd_rx.recv() {
                    let resp = handle(&mut client, req);
                    let _ = reply.send(resp);
                }
            })
            .map_err(|e| e.to_string())?;

        match ready_rx.recv() {
            Ok(Ok(())) => Ok(PgStore { tx: cmd_tx }),
            Ok(Err(e)) => Err(e),
            Err(e) => Err(e.to_string()),
        }
    }

    fn call(&self, req: Req) -> Resp {
        let (rtx, rrx) = channel();
        if self.tx.send((req, rtx)).is_err() {
            return Resp::Ack; // DB thread gone; degrade to no-op
        }
        rrx.recv().unwrap_or(Resp::Ack)
    }
}

fn handle(client: &mut Client, req: Req) -> Resp {
    match req {
        Req::LoadPlayer(username) => Resp::Player(load_player(client, &username)),
        Req::SavePlayer(rec) => {
            let inv = inventory_to_json(&rec.inventory);
            let _ = client.execute(
                "INSERT INTO players (username, chunk_x, chunk_y, x, y, inventory)
                 VALUES ($1,$2,$3,$4,$5,$6)
                 ON CONFLICT (username) DO UPDATE SET
                   chunk_x=$2, chunk_y=$3, x=$4, y=$5, inventory=$6",
                &[&rec.username, &rec.chunk.cx, &rec.chunk.cy, &rec.x, &rec.y, &inv],
            );
            Resp::Ack
        }
        Req::LoadStructures(coord) => Resp::Structures(load_structures(client, coord)),
        Req::SaveStructure(rec) => {
            let _ = client.execute(
                "INSERT INTO structures (x, y, chunk_x, chunk_y, owner_username, type, hp)
                 VALUES ($1,$2,$3,$4,$5,$6,$7)
                 ON CONFLICT (x, y) DO UPDATE SET
                   chunk_x=$3, chunk_y=$4, owner_username=$5, type=$6, hp=$7",
                &[&rec.x, &rec.y, &rec.coord.cx, &rec.coord.cy, &rec.owner, &rec.kind.as_str(), &rec.hp],
            );
            Resp::Ack
        }
        Req::DeleteStructure(x, y) => {
            let _ = client.execute("DELETE FROM structures WHERE x=$1 AND y=$2", &[&x, &y]);
            Resp::Ack
        }
        Req::LoadDepletions(coord) => Resp::Depletions(load_depletions(client, coord)),
        Req::SaveDepletion(rec) => {
            let _ = client.execute(
                "INSERT INTO depletions (x, y, chunk_x, chunk_y, type, respawn_at_ms)
                 VALUES ($1,$2,$3,$4,$5,$6)
                 ON CONFLICT (x, y) DO UPDATE SET
                   chunk_x=$3, chunk_y=$4, type=$5, respawn_at_ms=$6",
                &[&rec.x, &rec.y, &rec.coord.cx, &rec.coord.cy, &rec.kind.as_str(), &(rec.respawn_at_ms as i64)],
            );
            Resp::Ack
        }
        Req::DeleteDepletion(x, y) => {
            let _ = client.execute("DELETE FROM depletions WHERE x=$1 AND y=$2", &[&x, &y]);
            Resp::Ack
        }
    }
}

fn load_player(client: &mut Client, username: &str) -> Option<PlayerRecord> {
    let rows = client
        .query(
            "SELECT chunk_x, chunk_y, x, y, inventory FROM players WHERE username = $1",
            &[&username],
        )
        .ok()?;
    let row = rows.first()?;
    let inv: Value = row.get(4);
    Some(PlayerRecord {
        username: username.to_string(),
        chunk: ChunkCoord::new(row.get::<_, i32>(0), row.get::<_, i32>(1)),
        x: row.get(2),
        y: row.get(3),
        inventory: inventory_from_json(&inv),
    })
}

fn load_structures(client: &mut Client, coord: ChunkCoord) -> Vec<StructureRecord> {
    let rows = match client.query(
        "SELECT x, y, owner_username, type, hp FROM structures WHERE chunk_x=$1 AND chunk_y=$2",
        &[&coord.cx, &coord.cy],
    ) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    rows.iter()
        .filter_map(|row| {
            Some(StructureRecord {
                coord,
                x: row.get(0),
                y: row.get(1),
                owner: row.get(2),
                kind: StructureKind::parse(row.get::<_, &str>(3))?,
                hp: row.get(4),
            })
        })
        .collect()
}

fn load_depletions(client: &mut Client, coord: ChunkCoord) -> Vec<DepletionRecord> {
    let rows = match client.query(
        "SELECT x, y, type, respawn_at_ms FROM depletions WHERE chunk_x=$1 AND chunk_y=$2",
        &[&coord.cx, &coord.cy],
    ) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    rows.iter()
        .filter_map(|row| {
            Some(DepletionRecord {
                coord,
                x: row.get(0),
                y: row.get(1),
                kind: ResourceKind::parse(row.get::<_, &str>(2))?,
                respawn_at_ms: row.get::<_, i64>(3) as u64,
            })
        })
        .collect()
}

fn inventory_to_json(items: &BTreeMap<Item, u32>) -> Value {
    let mut map = Map::new();
    for (k, v) in items {
        map.insert(k.as_str().to_string(), Value::from(*v));
    }
    Value::Object(map)
}

fn inventory_from_json(v: &Value) -> BTreeMap<Item, u32> {
    let mut out = BTreeMap::new();
    if let Some(obj) = v.as_object() {
        for (k, val) in obj {
            if let (Some(item), Some(n)) = (Item::parse(k), val.as_u64()) {
                out.insert(item, n as u32);
            }
        }
    }
    out
}

impl DurableStore for PgStore {
    fn load_player(&self, username: &str) -> Option<PlayerRecord> {
        match self.call(Req::LoadPlayer(username.to_string())) {
            Resp::Player(p) => p,
            _ => None,
        }
    }
    fn save_player(&mut self, rec: &PlayerRecord) {
        self.call(Req::SavePlayer(rec.clone()));
    }
    fn load_structures(&self, coord: ChunkCoord) -> Vec<StructureRecord> {
        match self.call(Req::LoadStructures(coord)) {
            Resp::Structures(s) => s,
            _ => Vec::new(),
        }
    }
    fn save_structure(&mut self, rec: &StructureRecord) {
        self.call(Req::SaveStructure(rec.clone()));
    }
    fn delete_structure(&mut self, x: i64, y: i64) {
        self.call(Req::DeleteStructure(x, y));
    }
    fn load_depletions(&self, coord: ChunkCoord) -> Vec<DepletionRecord> {
        match self.call(Req::LoadDepletions(coord)) {
            Resp::Depletions(d) => d,
            _ => Vec::new(),
        }
    }
    fn save_depletion(&mut self, rec: &DepletionRecord) {
        self.call(Req::SaveDepletion(rec.clone()));
    }
    fn delete_depletion(&mut self, x: i64, y: i64) {
        self.call(Req::DeleteDepletion(x, y));
    }
}
