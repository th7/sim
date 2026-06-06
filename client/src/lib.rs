//! Native game client for the sim server.
//!
//! Split into a pure, fully-tested [`model`] (the observable client state and
//! the protocol decisions — the native analog of the old `window.__game`) and a
//! thin `three-d`/egui view + WebSocket driver layered on top. Only the model
//! is unit/integration tested; the rendering is verified manually.

pub mod conn;
pub mod dev;
pub mod model;
pub mod render;
pub mod session;
