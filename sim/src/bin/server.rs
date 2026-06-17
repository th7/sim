//! WebSocket game server: speaks the Phoenix Channels v2 protocol (topics,
//! events, and payloads per `contract/contract.json`) over `/socket/websocket`,
//! backed by the interaction-clustered simulation. The native `client` crate
//! connects here.
//!
//! Run: `cargo run --release --bin server` (listens on `SIM_PORT`, default 4000).
//!
//! Persistence: set `SIM_DATABASE_URL` to a libpq URL to persist through
//! Postgres (players/structures/depletions survive a restart); unset uses an
//! in-memory store. On SIGTERM/SIGINT the server flushes pending writes before
//! exiting, so a restart resumes the latest state.

use sim::pgstore::PgStore;
use sim::sim::Sim;
use sim::transport::{serve, Shared};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() {
    let port: u16 = std::env::var("SIM_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(4000);

    // Build the Sim with the chosen durable backend.
    let mut sim = match std::env::var("SIM_DATABASE_URL") {
        Ok(url) if !url.is_empty() => match PgStore::connect(&url) {
            Ok(store) => {
                eprintln!("sim: persisting to Postgres");
                Sim::with_store(store)
            }
            Err(e) => {
                eprintln!("sim: Postgres connect failed ({e}); aborting");
                std::process::exit(1);
            }
        },
        _ => {
            eprintln!("sim: in-memory store (set SIM_DATABASE_URL to persist)");
            Sim::new()
        }
    };
    // The deployed game runs the wildlife ecosystem (NPCs + Motivation). Off by
    // default in the library so core tests see an empty world; on here unless
    // SIM_WILDLIFE=0.
    sim.set_wildlife(std::env::var("SIM_WILDLIFE").map(|v| v != "0").unwrap_or(true));

    // Anchor the clock to wall-clock so depletion respawn times are absolute and
    // survive a restart (matching the Elixir wall-clock `depleted_until`).
    let now_ms = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
    sim.set_clock_ms(now_ms);

    // Drive the parallel tick: islands are entity-disjoint, so independent
    // islands tick across a persistent worker pool (one dense island is the
    // single-core floor). Leave a couple of cores for the async transport.
    let workers = std::thread::available_parallelism().map(|n| n.get().saturating_sub(2)).unwrap_or(1).max(1);
    sim.enable_pool(workers);
    eprintln!("sim: parallel tick on {workers} worker thread(s)");

    let shared = Shared::with_sim(sim);

    let listener = TcpListener::bind(("0.0.0.0", port)).await.expect("bind");
    eprintln!("sim server listening on :{port} (Phoenix Channels v2 at /socket/websocket)");

    let serve_shared = shared.clone();
    tokio::spawn(async move { serve(listener, serve_shared).await });

    // Flush pending writes on graceful shutdown.
    shutdown_signal().await;
    eprintln!("sim: shutdown — flushing pending writes");
    shared.flush();
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut term = signal(SignalKind::terminate()).expect("SIGTERM handler");
        let mut int = signal(SignalKind::interrupt()).expect("SIGINT handler");
        tokio::select! {
            _ = term.recv() => {}
            _ = int.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
