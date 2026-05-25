//! Wire-compatible WebSocket server: speaks the Phoenix Channels v2 protocol the
//! existing frontend's `phoenix` JS client uses, backed by the interaction-
//! clustered simulation. Drop-in for the Elixir `GameWeb` socket — same topics,
//! events, and payloads (`apps/game_web/priv/contract`).
//!
//! Run: `cargo run --release --bin server` (listens on `SIM_PORT`, default
//! 4000). In dev, Vite on :3000 proxies `/socket` here.

use sim::transport::{serve, Shared};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() {
    let port: u16 = std::env::var("SIM_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(4000);
    let listener = TcpListener::bind(("0.0.0.0", port)).await.expect("bind");
    eprintln!("sim server listening on :{port} (Phoenix Channels v2 at /socket/websocket)");
    serve(listener, Shared::new()).await;
}
