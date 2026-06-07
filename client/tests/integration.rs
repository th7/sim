//! End-to-end client integration: boot the real sim server in-process and drive
//! the native client's Session over a real WebSocket, re-pinning the phase
//! behaviours (the browser Playwright suite's job) without a browser.

use client::model::ClientModel;
use client::session::Session;
use protocol::geometry::ChunkCoord;
use protocol::wire::RealmWire;
use std::time::{Duration, Instant};

async fn start_server() -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(sim::transport::serve(listener, sim::transport::Shared::new()));
    port
}

/// Boot a server with the wildlife ecosystem enabled (NPCs materialize near players).
async fn start_server_wild() -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let mut sim = sim::sim::Sim::new();
    sim.set_wildlife(true);
    tokio::spawn(sim::transport::serve(listener, sim::transport::Shared::with_sim(sim)));
    port
}

/// A deer-rich Overworld chunk on the x-axis, so wildlife reliably materializes.
fn deer_rich_chunk() -> ChunkCoord {
    let sim = sim::sim::Sim::new();
    for k in 0..80 {
        let (cx, cy) = protocol::geometry::chunk_center(ChunkCoord::new(k, 0));
        if sim.region_levels_at(cx, cy).deer > 0.55 {
            return ChunkCoord::new(k, 0);
        }
    }
    panic!("no deer-rich region found");
}

fn url(port: u16) -> String {
    format!("ws://127.0.0.1:{port}/socket/websocket?vsn=2.0.0")
}

// Generous so the suite stays reliable when every crate's in-process server runs
// concurrently under `cargo test --workspace` (snapshots can lag under load).
const T: Duration = Duration::from_secs(10);

/// Is the instance's `out_of_instance` return portal currently in view?
/// Fine-position with single-tick taps: hold a direction for exactly one input
/// frame (one tick = 200 sub-units), stop, settle, observe — until `pred`.
/// The closed-loop way a player nudges into place; robust to frame timing.
async fn nudge_until(
    alice: &mut Session,
    (n, s, e, w): (bool, bool, bool, bool),
    pred: impl Fn(&ClientModel) -> bool,
) -> bool {
    for _ in 0..20 {
        if pred(alice.model()) {
            return true;
        }
        alice.movement(n, s, e, w).await.unwrap();
        // One frame goes out at pump entry; 40ms is under the renewal tick, so
        // exactly one — one tick of movement.
        alice.pump_for(Duration::from_millis(40)).await;
        alice.movement(false, false, false, false).await.unwrap();
        // The zero-frame goes out at the next pump entry; settle and observe.
        alice.pump_for(Duration::from_millis(240)).await;
    }
    pred(alice.model())
}

fn return_portal_visible(m: &ClientModel) -> bool {
    m.portals().values().any(|p| p.direction == "out_of_instance")
}

/// Drive alice from her overworld spawn northwest onto the `into_instance`
/// portal at world (4,4) and wait for the realm switch, then release movement so
/// she ends up standing still inside a fresh instance.
async fn enter_instance(alice: &mut Session) {
    assert!(alice.pump_until(T, |m| m.player_pos("alice").is_some()).await);
    assert_eq!(alice.model().realm(), RealmWire::Overworld);
    alice.movement(true, false, false, true).await.unwrap(); // northwest
    let entered = alice
        .pump_until(Duration::from_secs(15), |m| matches!(m.realm(), RealmWire::Instance { .. }))
        .await;
    alice.movement(false, false, false, false).await.unwrap();
    assert!(entered, "alice should relocate into an instance");
}

#[tokio::test]
async fn connect_and_see_self() {
    let port = start_server().await;
    let mut alice = Session::connect(&url(port), "alice", ChunkCoord::new(0, 0)).await.unwrap();
    assert!(
        alice.pump_until(T, |m| m.player_pos("alice").is_some()).await,
        "alice should appear in her own view after connecting"
    );
    // Spawned at chunk-(0,0) centre.
    let p = alice.model().player_pos("alice").unwrap();
    assert_eq!((p.x, p.y), (8_000, 8_000));
}

#[tokio::test]
async fn two_clients_see_each_other() {
    let port = start_server().await;
    let mut alice = Session::connect(&url(port), "alice", ChunkCoord::new(0, 0)).await.unwrap();
    let mut bob = Session::connect(&url(port), "bob", ChunkCoord::new(0, 0)).await.unwrap();
    assert!(alice.pump_until(T, |m| m.player_pos("bob").is_some()).await, "alice sees bob");
    assert!(bob.pump_until(T, |m| m.player_pos("alice").is_some()).await, "bob sees alice");
}

#[tokio::test]
async fn movement_moves_the_player() {
    let port = start_server().await;
    let mut alice = Session::connect(&url(port), "alice", ChunkCoord::new(0, 0)).await.unwrap();
    assert!(alice.pump_until(T, |m| m.player_pos("alice").is_some()).await);
    let x0 = alice.model().player_pos("alice").unwrap().x;

    alice.movement(false, false, true, false).await.unwrap(); // east
    let moved = alice
        .pump_until(T, |m| m.player_pos("alice").map(|p| p.x > x0).unwrap_or(false))
        .await;
    assert!(moved, "walking east increases x");
}

#[tokio::test]
async fn click_targets_the_tree_and_the_verb_button_harvests_it() {
    let port = start_server().await;
    let mut alice = Session::connect(&url(port), "alice", ChunkCoord::new(0, 0)).await.unwrap();
    // Wait for the chunk snapshot carrying the centre tree.
    assert!(
        alice.pump_until(T, |m| m.nodes().contains_key("tree:8000:8000")).await,
        "the centre tree should be visible"
    );
    // Clicking the tree designates it the Target — and issues no Verb.
    alice.click(8.0, 8.0).await.unwrap();
    assert_eq!(alice.model().target(), Some("tree:8000:8000"));
    assert!(
        !alice.pump_until(Duration::from_millis(500), |m| !m.inventory().is_empty()).await,
        "clicking selects only — nothing harvested before the Verb button is pressed"
    );
    // The Verb button issues the harvest at the Target's identity.
    alice.press_verb().await.unwrap();
    assert!(
        alice.pump_until(T, |m| m.inventory().get("wood").copied().unwrap_or(0) >= 1).await,
        "pressing the Verb button should harvest the targeted tree"
    );
}

#[tokio::test]
async fn walking_across_multiple_chunk_boundaries_stays_visible() {
    let port = start_server().await;
    let mut alice = Session::connect(&url(port), "alice", ChunkCoord::new(0, 0)).await.unwrap();
    assert!(alice.pump_until(T, |m| m.player_pos("alice").is_some()).await);

    // Walk south past the first boundary, then far east across several more.
    alice.movement(false, true, false, false).await.unwrap(); // south
    assert!(
        alice.pump_until(T, |m| m.player_pos("alice").map(|p| p.y > 10_000).unwrap_or(false)).await
    );
    alice.movement(false, false, false, false).await.unwrap();

    // Walk east to x > 33u — across the boundaries at 16u and 32u — sampling as
    // we go. The window pans to keep alice's chunk subscribed, so she stays in
    // the merged view the whole way and her x never jumps backward.
    alice.movement(false, false, true, false).await.unwrap();
    let mut prev_x = alice.model().player_pos("alice").unwrap().x;
    let mut reached = false;
    for _ in 0..120 {
        alice.pump_for(Duration::from_millis(100)).await;
        let p = alice.model().player_pos("alice").expect("alice stays visible across boundaries");
        assert!(p.x + 1 >= prev_x, "x is monotonic (no backward glitch on a pan)");
        prev_x = p.x;
        if p.x > 33_000 {
            reached = true;
            break;
        }
    }
    alice.movement(false, false, false, false).await.unwrap();
    assert!(reached, "alice reaches x > 33u within the sampling budget");
    // She crossed into chunk x=2 and the view window followed her.
    assert_eq!(alice.model().window_center().cx, 2);
}

#[tokio::test]
async fn gather_build_and_destroy_a_wall() {
    let port = start_server().await;
    let mut alice = Session::connect(&url(port), "alice", ChunkCoord::new(0, 0)).await.unwrap();
    // The five worldgen trees around the chunk centre load in.
    assert!(alice.pump_until(T, |m| m.nodes().len() >= 5).await, "all five trees visible");

    // Chop all five (alice spawns within interact range of the cluster) → 5 wood.
    let (cx, cy) = (8_000_i64, 8_000_i64);
    for (dx, dy) in [(500, 500), (500, -500), (-500, 500), (-500, -500), (0, 0)] {
        alice.send_harvest(&format!("tree:{}:{}", cx + dx, cy + dy)).await.unwrap();
        alice.pump_for(Duration::from_millis(150)).await;
    }
    assert!(
        alice.pump_until(T, |m| m.inventory().get("wood").copied().unwrap_or(0) >= 5).await,
        "five trees yield five wood"
    );

    // Walk east, clear of the depleted-but-still-solid tree cluster, then settle.
    alice.movement(false, false, true, false).await.unwrap();
    assert!(
        alice.pump_until(T, |m| m.player_pos("alice").map(|p| p.x >= 10_500).unwrap_or(false)).await,
        "alice walks east out of the cluster"
    );
    alice.movement(false, false, false, false).await.unwrap();
    alice.pump_for(Duration::from_millis(500)).await;

    // Place the wall 1u east: its AABB clears alice's body and sits exactly at
    // interact range (mirrors the old phase-8 e2e's hand-computed placement).
    let me = alice.model().player_pos("alice").unwrap();
    let (wx, wy) = (me.x + 1_000, me.y);
    alice.send_build("wall", wx, wy).await.unwrap();

    assert!(alice.pump_until(T, |m| !m.structures().is_empty()).await, "the wall appears");
    let wall = alice.model().structures().values().next().cloned().unwrap();
    assert_eq!(wall.hp, 100);
    assert_eq!(wall.owner, "alice");
    assert_eq!(wall.kind, "wall");
    assert!(
        alice.pump_until(T, |m| m.inventory().get("wood").copied().unwrap_or(0) == 0).await,
        "the five wood is spent on the wall"
    );

    // Damage it to destruction: 4 presses × 25 HP = 100 — by its identity.
    for _ in 0..4 {
        alice.send_damage(&format!("structure:{wx}:{wy}")).await.unwrap();
        alice.pump_for(Duration::from_millis(150)).await;
    }
    assert!(
        alice.pump_until(T, |m| m.structures().is_empty()).await,
        "the wall is destroyed after 100 damage"
    );
}

#[tokio::test]
async fn dev_mode_receives_stats() {
    let port = start_server().await;
    let mut alice = Session::connect(&url(port), "alice", ChunkCoord::new(0, 0)).await.unwrap();
    assert!(alice.pump_until(T, |m| m.player_pos("alice").is_some()).await);
    // No stats until dev mode is on.
    assert!(alice.model().stats().is_none());

    alice.set_dev(true).await.unwrap();
    // The server pushes dev:stats once per second to subscribers.
    let got = alice.pump_until(Duration::from_secs(3), |m| m.stats().is_some()).await;
    assert!(got, "enabling dev mode should start receiving dev:stats pushes");

    let stats = alice.model().stats().unwrap();
    assert!(stats.active_chunks >= 1, "alice's chunk is hot");
    assert!(stats.total_players >= 1);
    // The overlay ring is the 7×7 chunks centred on alice (chunk 0,0).
    assert_eq!(stats.around.len(), 49);
    assert!(stats.around.iter().any(|c| c.cx == 0 && c.cy == 0));

    // Turning dev off drops the cached stats.
    alice.set_dev(false).await.unwrap();
    assert!(alice.model().stats().is_none());
}

#[tokio::test]
async fn walking_into_the_portal_enters_an_instance() {
    let port = start_server().await;
    // chunk (0,0) holds the into_instance portal at world (4,4); spawn at (8,8).
    let mut alice = Session::connect(&url(port), "alice", ChunkCoord::new(0, 0)).await.unwrap();
    assert!(alice.pump_until(T, |m| m.player_pos("alice").is_some()).await);
    assert_eq!(alice.model().realm(), RealmWire::Overworld);

    // Walk northwest toward the portal (west = -x, north = -y).
    alice.movement(true, false, false, true).await.unwrap();
    let entered = alice
        .pump_until(Duration::from_secs(15), |m| matches!(m.realm(), RealmWire::Instance { .. }))
        .await;
    alice.movement(false, false, false, false).await.unwrap();
    assert!(entered, "overlapping the portal relocates the player into an instance");
    // The client re-subscribed to the instance's chunks and sees the return portal.
    assert!(
        alice
            .pump_until(T, |m| m.portals().values().any(|p| p.direction == "out_of_instance"))
            .await,
        "the instance's return portal should be visible after the realm switch"
    );
}

// --- flicker regressions -----------------------------------------------------
//
// "Flicker" = a visible object blinks out and back while the player stays inside
// the instance. At the model layer that means an object disappears from the
// merged view (`portals()` / `players()`) on some broadcast and returns on a
// later one. These tests pin the invariant *do not flicker* by sampling the view
// at ~20 Hz (twice the 10 Hz broadcast rate, so no dropped broadcast can be
// skipped over) and asserting an object that has appeared never goes missing.
//
// The instance is a 3×3 chunk grid whose return portal sits in the centre chunk
// (1,1); the player is clamped to that grid, so she is never more than one chunk
// from centre. Hence (1,1) is *always* inside both her 3×3 view window and her
// cluster's footprint — the portal has no legitimate reason to ever leave view.
// If it does, that is the bug, and it is exercised end-to-end (real server, real
// client model over a real socket), so it catches the fault whether it lives in
// the server's per-chunk snapshot building or the client's subscription/merge.

/// Standing still just inside an instance, the return portal and the player must
/// stay continuously visible. Spans ~7 s — past the 5 s chunk idle-deactivation
/// timeout and ~70 broadcasts — so a per-broadcast drop *or* an idle-deactivation
/// of the centre chunk would surface as a missing object on some sample.
#[tokio::test]
async fn instance_objects_do_not_flicker_while_standing_still() {
    let port = start_server().await;
    let mut alice = Session::connect(&url(port), "alice", ChunkCoord::new(0, 0)).await.unwrap();
    enter_instance(&mut alice).await;

    // Let the instance's content load into view before we start watching.
    assert!(
        alice.pump_until(T, |m| return_portal_visible(m) && m.player_pos("alice").is_some()).await,
        "the return portal and alice should be visible once inside the instance"
    );

    let deadline = Instant::now() + Duration::from_secs(7);
    let mut samples = 0;
    while Instant::now() < deadline {
        alice.pump_for(Duration::from_millis(50)).await;
        samples += 1;
        assert!(
            return_portal_visible(alice.model()),
            "return portal vanished at sample {samples} — flicker while standing in the instance"
        );
        assert!(
            alice.model().player_pos("alice").is_some(),
            "player vanished at sample {samples} — flicker while standing in the instance"
        );
    }
    assert!(samples >= 50, "expected to sample many broadcast cycles, got {samples}");
}

/// Walking a circuit through the instance's chunks pans the view window (chunks
/// are subscribed and dropped), but the return portal in the centre chunk (1,1)
/// — and the player, who is always in her own window — must never blink out.
///
/// The circuit roams *away* from the return portal at (24000,24000): the player
/// spawns one unit west of it, so we head west/north/east/south through chunks
/// (0,1),(0,0),(1,0) and back, staying clear of the portal's overlap trigger (we
/// must not re-enter it and leave the instance) while keeping (1,1) in view the
/// whole way. Any disappearance is flicker, not a legitimate view change.
#[tokio::test]
async fn instance_objects_do_not_flicker_while_walking_around() {
    let port = start_server().await;
    let mut alice = Session::connect(&url(port), "alice", ChunkCoord::new(0, 0)).await.unwrap();
    enter_instance(&mut alice).await;
    assert!(
        alice.pump_until(T, |m| return_portal_visible(m)).await,
        "the return portal should be visible once inside the instance"
    );

    // (north, south, east, west, dwell_ms). From spawn (23000,24000): west into
    // chunk (0,1), north into (0,0), east along y≈8000 into (1,0) (far below the
    // portal), south into (1,1). ~4 world-units/sec keeps every leg inside the
    // 3×3 grid and well clear of the return portal.
    let legs = [
        (false, false, false, true, 3_000),
        (true, false, false, false, 4_000),
        (false, false, true, false, 4_000),
        (false, true, false, false, 2_000),
    ];
    let mut panned = false;
    for (n, s, e, w, dwell_ms) in legs {
        alice.movement(n, s, e, w).await.unwrap();
        let leg_end = Instant::now() + Duration::from_millis(dwell_ms);
        while Instant::now() < leg_end {
            alice.pump_for(Duration::from_millis(50)).await;
            assert!(
                matches!(alice.model().realm(), RealmWire::Instance { .. }),
                "the circuit must stay inside the instance (not re-enter the return portal)"
            );
            assert!(
                alice.model().player_pos("alice").is_some(),
                "player vanished while walking — flicker inside the instance"
            );
            assert!(
                return_portal_visible(alice.model()),
                "return portal flickered while walking inside the instance"
            );
            panned |= alice.model().window_center() != ChunkCoord::new(1, 1);
        }
    }
    alice.movement(false, false, false, false).await.unwrap();
    assert!(panned, "the circuit should have panned the view window off the centre chunk");
}

// --- "build on the depleted cluster" is correctly rejected -------------------
//
// Depleted trees keep their Footprint by design: the cell looks visually clear
// but stays solid until the tree respawns. From the chunk-centre spawn, every
// cell-centre within interact range *is* a depleted-tree spot — the client
// optimistically emits Build for each click (it doesn't know about footprints)
// and the server correctly rejects each with `footprint_blocked`. (The reject
// reason isn't currently surfaced to the player; that's a UX gap, tracked
// separately. Walls placed *next to* the cluster work — see the sim test
// `a_wall_can_be_built_next_to_the_depleted_cluster`.)

#[tokio::test]
async fn building_on_the_depleted_cluster_is_rejected() {
    let port = start_server().await;
    let mut alice = Session::connect(&url(port), "alice", ChunkCoord::new(0, 0)).await.unwrap();
    assert!(alice.pump_until(T, |m| m.nodes().len() >= 5).await);

    // Harvest the five trees clustered at chunk centre → 5 wood. They become
    // `depleted: true` but their Footprint stays solid at (±500, ±500).
    for (dx, dy) in [(500, 500), (500, -500), (-500, 500), (-500, -500), (0, 0)] {
        alice.send_harvest(&format!("tree:{}:{}", 8_000 + dx, 8_000 + dy)).await.unwrap();
        alice.pump_for(Duration::from_millis(150)).await;
    }
    let centre_trees = [(8_000, 8_000), (7_500, 7_500), (7_500, 8_500), (8_500, 7_500), (8_500, 8_500)];
    assert!(
        alice
            .pump_until(T, |m| {
                m.inventory().get("wood").copied().unwrap_or(0) >= 5
                    && centre_trees.iter().all(|(x, y)| {
                        m.nodes().get(&format!("tree:{x}:{y}")).map(|n| n.depleted).unwrap_or(false)
                    })
            })
            .await,
        "five wood gathered and the centre cluster is depleted (footprints still solid)"
    );

    // Stay put. The four cell-centres within interact range of (8000, 8000) are
    // exactly (±500, ±500) — every one of them is on a depleted-tree footprint.
    // Click each via the user's path (the model's click selector). Each emits a
    // Build the server rejects; the wall never appears.
    for (dx, dy) in [(500, 500), (500, -500), (-500, 500), (-500, -500)] {
        let wx = (8_000 + dx) as f64 / 1_000.0;
        let wy = (8_000 + dy) as f64 / 1_000.0;
        alice.click(wx, wy).await.unwrap();
        alice.pump_for(Duration::from_millis(200)).await;
    }
    assert!(
        alice.model().structures().is_empty(),
        "no wall is placed — every reachable cell at chunk centre is footprint-blocked"
    );
    assert_eq!(
        alice.model().inventory().get("wood").copied().unwrap_or(0),
        5,
        "wood stays at 5: rejected builds don't deduct materials"
    );
}

/// The GUI version of `a_wall_can_be_built_next_to_the_depleted_cluster` (sim):
/// drive the full ground-pick → cell-snap → server path the way an actual click
/// does. The wall must land in the cell **immediately** adjacent to a depleted
/// tree (centre-to-centre 1000 = one cell pitch); no "several wall-widths" of
/// padding required. The diagonally-offset NW position is the key: from any
/// pure-cardinal walk the player sits *on* the cell-grid axis and their body
/// then overlaps the adjacent cell's AABB.
#[tokio::test]
async fn click_builds_a_wall_in_the_cell_directly_next_to_the_cluster() {
    let port = start_server().await;
    let mut alice = Session::connect(&url(port), "alice", ChunkCoord::new(0, 0)).await.unwrap();
    assert!(alice.pump_until(T, |m| m.nodes().len() >= 5).await);

    let centre_trees = [(8_000, 8_000), (7_500, 7_500), (7_500, 8_500), (8_500, 7_500), (8_500, 8_500)];
    for (x, y) in centre_trees {
        alice.send_harvest(&format!("tree:{x}:{y}")).await.unwrap();
        alice.pump_for(Duration::from_millis(150)).await;
    }
    assert!(
        alice
            .pump_until(T, |m| {
                m.inventory().get("wood").copied().unwrap_or(0) >= 5
                    && centre_trees.iter().all(|(x, y)| {
                        m.nodes().get(&format!("tree:{x}:{y}")).map(|n| n.depleted).unwrap_or(false)
                    })
            })
            .await
    );

    // Walk NW (north past the cluster, then a hair west) so alice's body sits
    // off the cell-grid axis the wall's AABB is on. From here she can click the
    // adjacent cell without her own body blocking the placement. Fine-position
    // with single-tick taps (input frames are consumed one per tick, so a tap
    // is exactly 200 sub-units): the placement window next to the cell is only
    // about one tick wide, so open-loop walking would overshoot it.
    assert!(
        nudge_until(&mut alice, (true, false, false, false), |m| {
            m.player_pos("alice").map(|p| p.y <= 6_500).unwrap_or(false)
        })
        .await,
        "taps north past the cluster"
    );
    assert!(
        nudge_until(&mut alice, (false, false, false, true), |m| {
            m.player_pos("alice")
                .map(|p| {
                    // The placement precondition itself: body clear of the
                    // target cell's AABB, click target within interact range.
                    let (dx, dy) = (8_500 - p.x, 6_500 - p.y);
                    p.x <= 7_700 && dx * dx + dy * dy <= 1_000_000
                })
                .unwrap_or(false)
        })
        .await,
        "taps west into placement range of the adjacent cell"
    );

    // Click the cell at world (8.5, 6.5) — its centre is (8500, 6500), exactly
    // one cell pitch (1000 sub-units, one wall-width centre-to-centre) north of
    // the depleted (8500, 7500) tree.
    alice.click(8.5, 6.5).await.unwrap();
    assert!(
        alice.pump_until(T, |m| !m.structures().is_empty()).await,
        "the click placed a wall in the cell directly adjacent to the cluster"
    );
    let wall = alice.model().structures().values().next().cloned().unwrap();
    assert_eq!((wall.x, wall.y), (8_500, 6_500), "wall is in the cell directly N of (8500, 7500)");
    assert_eq!((wall.kind.as_str(), wall.hp), ("wall", 100));
}

/// When the Island rejects a Verb, the reason is observable on the client model
/// (which the view will show in the HUD), rather than failing silently. The
/// press always sends — a depleted Target is the Island's to judge — and the
/// async `action_rejected` lands in `last_error`. (The old build-on-depleted
/// path died with the click heuristic: clicking a depleted tree now targets it,
/// so no wood can be wasted on its cell; the server-side footprint rule itself
/// stays pinned in the sim suite.)
#[tokio::test]
async fn a_rejected_press_surfaces_the_islands_reason_in_last_error() {
    let port = start_server().await;
    let mut alice = Session::connect(&url(port), "alice", ChunkCoord::new(0, 0)).await.unwrap();
    assert!(alice.pump_until(T, |m| m.nodes().len() >= 5).await);

    let centre_trees = [(8_000, 8_000), (7_500, 7_500), (7_500, 8_500), (8_500, 7_500), (8_500, 8_500)];
    for (x, y) in centre_trees {
        alice.send_harvest(&format!("tree:{x}:{y}")).await.unwrap();
        alice.pump_for(Duration::from_millis(150)).await;
    }
    assert!(
        alice
            .pump_until(T, |m| {
                m.inventory().get("wood").copied().unwrap_or(0) >= 5
                    && centre_trees.iter().all(|(x, y)| {
                        m.nodes().get(&format!("tree:{x}:{y}")).map(|n| n.depleted).unwrap_or(false)
                    })
            })
            .await
    );

    assert!(alice.model().last_error().is_none(), "no errors before any verb");

    // Target a depleted tree and press: the client sends anyway (state is the
    // Island's to judge) and the Island answers `depleted`, asynchronously.
    alice.click(8.5, 8.5).await.unwrap();
    assert_eq!(alice.model().target(), Some("tree:8500:8500"));
    alice.press_verb().await.unwrap();
    assert!(
        alice.pump_until(T, |m| m.last_error() == Some("depleted")).await,
        "the Island's rejection reason becomes visible to the client"
    );
    // The view reads from RenderState, not directly from the model: the reason
    // has to be wired through for the HUD to show it.
    assert_eq!(alice.render_state().last_error.as_deref(), Some("depleted"));
}

/// With wildlife enabled, a connected client sees NPCs materialize in its view —
/// the full server→wire→client path for the new entity kind.
#[tokio::test]
async fn client_sees_wildlife_materialize() {
    let chunk = deer_rich_chunk();
    let port = start_server_wild().await;
    let mut alice = Session::connect(&url(port), "alice", chunk).await.unwrap();
    assert!(
        alice.pump_until(T, |m| !m.npcs().is_empty()).await,
        "wildlife should materialize near the player and reach the client",
    );
    // The dev stats also report a non-zero world NPC count.
    alice.set_dev(true).await.unwrap();
    assert!(
        alice
            .pump_until(T, |m| m.stats().map(|s| s.total_npcs > 0).unwrap_or(false))
            .await,
        "dev stats should report live NPCs",
    );
}
