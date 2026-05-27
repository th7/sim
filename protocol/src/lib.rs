//! Shared wire protocol + geometry for the sim server and the native client.
//!
//! Pure (serde + serde_json only) — no ECS, no tokio, no database. Both the
//! `sim` server and the `client` depend on this so they share one definition of
//! the Phoenix-Channels codec, the wire payloads, the chunk geometry, and the
//! game's constants/kinds. The wire structs deriving both `Serialize` and
//! `Deserialize` mean server and client cannot disagree about the wire.

pub mod consts;
pub mod geometry;
pub mod ids;
pub mod phx;
pub mod types;
pub mod wire;
