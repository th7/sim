//! The showcase's headless guarantee: every scenario's synthetic payloads
//! survive the real `ClientModel` pipeline and the resulting `RenderState`
//! contains everything the scenario promises to display. The manual pass on a
//! real display then only judges *appearance* — presence is checked here.

use client::session::RenderState;
use client::showcase::scenarios;
use protocol::types::{NpcKind, PortalDirection};
use protocol::wire::{ChunkLifecycle, RealmWire};

fn state_of(name: &str) -> RenderState {
    scenarios().into_iter().find(|s| s.name == name).unwrap_or_else(|| panic!("scenario {name}")).state(0.0)
}

#[test]
fn overworld_scenario_displays_a_live_tree() {
    let rs = state_of("overworld");
    assert!(rs.nodes.values().any(|n| !n.depleted), "a live tree is displayed");
}

/// Every appearance-affecting state is present at once. The kind/lifecycle
/// loops iterate the protocol's ALL consts, so a new variant fails here (after
/// failing the showcase build's compile) until the scene displays it.
#[test]
fn overworld_scenario_displays_every_ui_element_state() {
    let rs = state_of("overworld");

    // Nodes: both depleted states.
    assert!(rs.nodes.values().any(|n| !n.depleted), "live tree");
    assert!(rs.nodes.values().any(|n| n.depleted), "depleted tree");

    // Portals: a disc per direction.
    for d in PortalDirection::ALL {
        assert!(rs.portals.values().any(|p| p.direction == d.as_str()), "portal {}", d.as_str());
    }

    // NPCs: every kind.
    for k in NpcKind::ALL {
        assert!(rs.npcs.values().any(|n| n.kind == k.as_str()), "npc {}", k.as_str());
    }

    // A wall and a carcass.
    assert!(!rs.structures.is_empty(), "wall");
    assert!(!rs.carcasses.is_empty(), "carcass");

    // Players: the six-colour palette plus the camera-anchoring own player.
    assert!(rs.players.len() >= 6, "palette players, got {}", rs.players.len());
    assert!(rs.players.contains_key(&rs.own), "own player anchors the camera");

    // HUD: populated inventory and a sample rejection.
    assert!(!rs.inventory.is_empty(), "inventory items");
    assert!(rs.last_error.is_some(), "error line");

    // Dev overlay: stats present and covering every chunk lifecycle, with a
    // countdown on the idle-armed chunk.
    let stats = rs.stats.as_ref().expect("dev stats present");
    for l in ChunkLifecycle::ALL {
        assert!(stats.around.iter().any(|e| e.lifecycle == l), "lifecycle {l:?}");
    }
    assert!(
        stats
            .around
            .iter()
            .any(|e| e.lifecycle == ChunkLifecycle::IdleArmed && e.idle_ms_remaining.is_some()),
        "idle countdown bar"
    );

    // The overworld background.
    assert_eq!(rs.realm, RealmWire::Overworld);
}

/// The two animation paths run: the pacing player exercises the lerp, the
/// perpetually re-arming chunk exercises the countdown bar.
#[test]
fn overworld_scenario_animates_the_pacer_and_the_idle_countdown() {
    let s = scenarios().into_iter().find(|s| s.name == "overworld").expect("overworld scenario");
    let (a, b) = (s.state(0.0), s.state(1_000.0));

    let pos = |rs: &RenderState| {
        let p = rs.players.get("pacer").expect("pacer player");
        (p.x, p.y)
    };
    assert_ne!(pos(&a), pos(&b), "the pacer moves between instants");

    let rem = |rs: &RenderState| {
        rs.stats.as_ref().unwrap().around.iter().find_map(|e| e.idle_ms_remaining).unwrap()
    };
    assert_ne!(rem(&a), rem(&b), "the idle countdown advances");
}

/// The states the overworld scene can't show at the same time: the instance
/// background and the empty inventory (no error line).
#[test]
fn instance_scenario_shows_the_instance_realm_with_an_empty_inventory() {
    let rs = state_of("instance");
    assert!(matches!(rs.realm, RealmWire::Instance { .. }), "instance background");
    assert!(rs.inventory.is_empty(), "empty inventory");
    assert!(rs.last_error.is_none(), "no error line");
    assert!(rs.players.contains_key(&rs.own), "own player anchors the camera");
    assert!(!rs.frozen, "a live scene is not frozen");
}

/// The frozen-Mirror state: authority has gone quiet, the Mirror has hit its
/// Lead bound, and the view says so — entities still drawn, signal shown —
/// instead of silently animating stale speculation.
#[test]
fn frozen_scenario_shows_a_frozen_mirror_over_a_populated_scene() {
    let rs = state_of("frozen");
    assert!(rs.frozen, "the Mirror is frozen at its Lead bound");
    assert!(rs.players.contains_key(&rs.own), "the world is still drawn, just frozen");
    assert!(!rs.nodes.is_empty(), "entities remain visible under the freeze signal");
    // And every live scenario renders unfrozen.
    assert!(!state_of("overworld").frozen, "overworld is live");
}
