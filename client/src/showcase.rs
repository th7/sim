//! Showcase scenarios: synthetic wire payloads fed through the real
//! [`ClientModel`] so the showcase bin renders every UI element through the
//! same pipeline the game uses. Each scenario is a pure function of time —
//! `state(t_ms)` rebuilds the model from payloads for that instant, so what is
//! displayed is deterministic and headlessly testable (`tests/showcase.rs`).

use crate::model::ClientModel;
use crate::session::RenderState;
use protocol::geometry::ChunkCoord;
use protocol::types::ResourceKind;
use protocol::wire::{ChunkSnapshot, NodeWire};

/// A named showcase scene. `state(t_ms)` is everything the view draws at that
/// instant; time-varying payloads (the pacing player, the idle countdown) make
/// the animation paths run.
pub struct Scenario {
    pub name: &'static str,
    build: fn(f64) -> RenderState,
}

impl Scenario {
    pub fn state(&self, t_ms: f64) -> RenderState {
        (self.build)(t_ms)
    }
}

/// Every scenario the showcase cycles through.
pub fn scenarios() -> Vec<Scenario> {
    vec![Scenario { name: "overworld", build: overworld }]
}

fn overworld(_t_ms: f64) -> RenderState {
    let (mut model, _) = ClientModel::new("showcase", ChunkCoord::new(0, 0));
    let mut snap = ChunkSnapshot::default();
    snap.resource_nodes.insert(
        "tree:live".into(),
        NodeWire { kind: ResourceKind::Tree.as_str().into(), x: 2_000, y: 2_000, depleted: false },
    );
    model.on_snapshot(ChunkCoord::new(0, 0), snap);
    RenderState::from_model(&model)
}
