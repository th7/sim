//! Interaction-clustered simulation — Rust shared-memory prototype.
//!
//! The model: one shared ECS
//! world, partitioned by *interaction locality* into **islands**; islands
//! are packed onto **workers** for execution; a single **Cartographer** owns the
//! partition and serializes topology changes. Actions run inside an island
//! (single authority); observation is a separate changed-only delta stream.
//!
//! This crate is feature- and wire-compatible with the Elixir implementation
//! under `apps/`, but its internal structure is entirely different: there are
//! no per-chunk processes and no message handoffs.

// Shared with the native client via the `protocol` crate. Re-exported so the
// server's internal `crate::geometry` / `crate::ids` / `crate::phx` /
// `crate::consts` paths keep working unchanged.
pub use protocol::{consts, geometry, ids, phx};

// The movement integrator, collision, and footprint catalogue live in the
// shared `simcore` crate (one implementation for the server and the client's
// Mirror); re-exported so `crate::collision` keeps working.
pub use simcore::collision;

pub mod catalogue;
pub mod chunkgraph;
pub mod contract;
pub mod components;
pub mod datastore;
pub mod delta;
pub mod dev;
pub mod ecosystem;
pub mod harness;
pub mod cartographer;
pub mod motivation;
pub mod parallel;
pub mod pgstore;
pub mod repack;
pub mod server;
pub mod sim;
pub mod transport;
pub mod actions;
pub mod wire;
pub mod world;
pub mod worldgen;
