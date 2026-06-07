//! Showcase scenarios: synthetic wire payloads fed through the real
//! [`ClientModel`] so the showcase bin renders every UI element through the
//! same pipeline the game uses. Each scenario is a pure function of time —
//! `state(t_ms)` rebuilds the model from payloads for that instant, so what is
//! displayed is deterministic and headlessly testable (`tests/showcase.rs`).

use crate::model::ClientModel;
use crate::session::RenderState;
use protocol::consts::IDLE_TIMEOUT_MS;
use protocol::geometry::ChunkCoord;
use protocol::types::{NpcKind, PortalDirection, PortalKind, ResourceKind, StructureKind};
use protocol::wire::{
    CarcassWire, ChunkLifecycle, ChunkSnapshot, ChunkStatWire, NodeWire, NpcWire, PlayerWire,
    PortalWire, RealmWire, RelocatedPayload, SelfPayload, StatsPayload, StructureWire,
};

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

/// The pacer's out-and-back walk period.
const PACE_PERIOD_MS: f64 = 4_000.0;

/// Every scenario the showcase cycles through.
pub fn scenarios() -> Vec<Scenario> {
    vec![
        Scenario { name: "overworld", build: overworld },
        Scenario { name: "instance", build: instance },
    ]
}

/// The scene grid lives in chunk (0,0) — 16 units = 16_000 sub-units — well
/// inside the model's 3×3 subscription window, one element family per row.
fn overworld(t_ms: f64) -> RenderState {
    let (mut model, _) = ClientModel::new("showcase", ChunkCoord::new(0, 0));
    let mut snap = ChunkSnapshot::default();

    // Row 1 — resource nodes: live, then depleted (bare trunk).
    let tree = ResourceKind::Tree.as_str();
    snap.resource_nodes.insert(
        "tree:live".into(),
        NodeWire { kind: tree.into(), x: 2_000, y: 2_000, depleted: false },
    );
    snap.resource_nodes.insert(
        "tree:depleted".into(),
        NodeWire { kind: tree.into(), x: 4_000, y: 2_000, depleted: true },
    );

    // Row 2 — a wall and a carcass.
    snap.structures.insert(
        "wall:1".into(),
        StructureWire {
            kind: StructureKind::Wall.as_str().into(),
            x: 2_000,
            y: 4_000,
            hp: 100,
            owner: "showcase".into(),
        },
    );
    snap.carcasses.insert("carcass:1".into(), CarcassWire { x: 4_000, y: 4_000, meat: 3 });

    // Row 3 — a portal disc per direction.
    for (i, d) in PortalDirection::ALL.into_iter().enumerate() {
        snap.portals.insert(
            format!("portal:{}", d.as_str()),
            PortalWire {
                kind: PortalKind::Dungeon.as_str().into(),
                direction: d.as_str().into(),
                x: 2_000 + 2_000 * i as i64,
                y: 6_000,
            },
        );
    }

    // Row 4 — an NPC per kind.
    for (i, k) in NpcKind::ALL.into_iter().enumerate() {
        snap.npcs.insert(
            format!("npc:{}", k.as_str()),
            NpcWire {
                kind: k.as_str().into(),
                x: 2_000 + 2_000 * i as i64,
                y: 8_000,
                hp: 10,
                ..NpcWire::default()
            },
        );
    }

    // Row 5 — players "a".."f" hash to the six distinct palette colours
    // (single-byte names: 97..=102 ≡ 1,2,3,4,5,0 mod 6).
    for (i, name) in ["a", "b", "c", "d", "e", "f"].into_iter().enumerate() {
        snap.players
            .insert(name.into(), PlayerWire { x: 2_000 + 2_000 * i as i64, y: 10_000, ..PlayerWire::default() });
    }
    // The showcase's own player anchors the camera at the scene centre.
    snap.players.insert("showcase".into(), PlayerWire { x: 8_000, y: 6_000, ..PlayerWire::default() });
    // The pacer walks a triangle wave below the grid, exercising the lerp.
    let phase = (t_ms % PACE_PERIOD_MS) / PACE_PERIOD_MS;
    let tri = (phase * 2.0 - 1.0).abs(); // 1 → 0 → 1 over the period
    snap.players.insert(
        "pacer".into(),
        PlayerWire { x: 4_000 + ((1.0 - tri) * 8_000.0) as i64, y: 12_000, ..PlayerWire::default() },
    );

    model.on_snapshot(ChunkCoord::new(0, 0), snap);

    // HUD: a populated inventory and a sample rejection line.
    model.on_self(SelfPayload {
        inventory: [("wood".to_string(), 5), ("meat".to_string(), 2), ("hide".to_string(), 1)]
            .into_iter()
            .collect(),
    });
    model.on_verb_error("sample_rejection".into());

    // Dev overlay on, with the 3×3 ring cycling through every lifecycle and a
    // countdown on the idle-armed chunks.
    let _ = model.set_dev(true);
    let around = (-1..=1)
        .flat_map(|cy| (-1..=1).map(move |cx| (cx, cy)))
        .enumerate()
        .map(|(i, (cx, cy))| {
            let lifecycle = ChunkLifecycle::ALL[i % ChunkLifecycle::ALL.len()];
            ChunkStatWire {
                cx,
                cy,
                lifecycle,
                // Perpetually re-arming: counts down to zero and snaps back,
                // so the shrinking bar animation runs for as long as you look.
                idle_ms_remaining: (lifecycle == ChunkLifecycle::IdleArmed)
                    .then_some(IDLE_TIMEOUT_MS as i64 - (t_ms as i64).rem_euclid(IDLE_TIMEOUT_MS as i64)),
                entity_count: 0,
            }
        })
        .collect();
    model.on_stats(StatsPayload { active_chunks: 3, total_players: 7, total_npcs: 2, around });

    RenderState::from_model(&model)
}

/// The states the overworld scene can't show at the same time: the instance
/// background (dark purple) and the empty inventory. Entered through the real
/// relocation path, exactly as a portal overlap relocates the game client.
fn instance(_t_ms: f64) -> RenderState {
    let (mut model, _) = ClientModel::new("showcase", ChunkCoord::new(0, 0));
    let _ = model.on_relocated(RelocatedPayload {
        realm: RealmWire::Instance { id: 7 },
        coord: [0, 0],
    });

    let mut snap = ChunkSnapshot::default();
    snap.players.insert("showcase".into(), PlayerWire { x: 8_000, y: 8_000, ..PlayerWire::default() });
    // The way back out.
    snap.portals.insert(
        "portal:return".into(),
        PortalWire {
            kind: PortalKind::Dungeon.as_str().into(),
            direction: PortalDirection::OutOfInstance.as_str().into(),
            x: 6_000,
            y: 8_000,
        },
    );
    model.on_snapshot(ChunkCoord::new(0, 0), snap);
    RenderState::from_model(&model)
}
