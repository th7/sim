//! Async WebSocket runtime serving the Phoenix Channels v2 protocol, backed by
//! the [`Sim`]. The pure routing lives in [`crate::server`]; this module owns
//! the sockets, the subscriber registry, and the tick / broadcast / stats
//! loops. Exposed from the lib (not just the binary) so end-to-end wire tests
//! can run it in-process.

use crate::consts::{BROADCAST_EVERY, TICK_MS};
use crate::phx::{push, PhxMessage};
use crate::dev::stats_payload;
use crate::server::{chunk_snapshot_push, parse_topic, route, ConnState, Topic};
use crate::sim::{OutboundEvent, Sim};
use crate::wire::{
    action_rejected_payload, inventory_payload, move_ack_payload, relocated_payload,
};
use futures_util::{SinkExt, StreamExt};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};
use tokio_tungstenite::tungstenite::Message;

type Tx = UnboundedSender<Message>;

struct ConnHandle {
    tx: Tx,
    state: ConnState,
}

/// Shared server state: the single Sim and the live connection registry.
pub struct Shared {
    sim: Mutex<Sim>,
    conns: Mutex<HashMap<u64, ConnHandle>>,
}

impl Shared {
    pub fn new() -> Arc<Self> {
        Shared::with_sim(Sim::new())
    }

    /// Build shared state around a pre-configured `Sim` (e.g. one backed by
    /// Postgres with its clock anchored to wall-clock).
    pub fn with_sim(sim: Sim) -> Arc<Self> {
        Arc::new(Shared {
            sim: Mutex::new(sim),
            conns: Mutex::new(HashMap::new()),
        })
    }

    /// Flush pending writes to durable storage — call on graceful shutdown so a
    /// restart resumes the latest state.
    pub fn flush(&self) {
        self.sim.lock().unwrap().flush_now();
    }
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Run the server on `listener` until the process ends. Spawns the tick and
/// stats loops, then accepts connections.
pub async fn serve(listener: TcpListener, shared: Arc<Shared>) {
    spawn_tick_loop(shared.clone());
    spawn_stats_loop(shared.clone());
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                let shared = shared.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_conn(shared, stream).await {
                        eprintln!("connection error: {e}");
                    }
                });
            }
            Err(e) => eprintln!("accept error: {e}"),
        }
    }
}

fn spawn_tick_loop(shared: Arc<Shared>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(TICK_MS));
        loop {
            interval.tick().await;
            let mut sim = shared.sim.lock().unwrap();
            if sim.tick_or_flush().is_err() {
                // A tick panicked: the runtime is presumed corrupt. We've already
                // flushed durable state (loss bounded to the unflushed window);
                // now take the whole runtime down for a supervisor to restart.
                eprintln!("sim: tick panicked — flushed durable state, taking the runtime down");
                std::process::abort();
            }
            let broadcast = sim.tick_count() % BROADCAST_EVERY == 0;
            let events = sim.drain_events();
            let conns = shared.conns.lock().unwrap();

            for ev in events {
                let (topic, frame) = match ev {
                    OutboundEvent::SelfInventory { username, inventory } => {
                        let topic = format!("player:{username}");
                        let f = push(&topic, "self", inventory_payload(&inventory));
                        (topic, f)
                    }
                    OutboundEvent::Relocated { username, realm, coord } => {
                        let topic = format!("player:{username}");
                        let f = push(&topic, "relocated", relocated_payload(realm, coord));
                        (topic, f)
                    }
                    OutboundEvent::ActionRejected { username, verb, at, reason } => {
                        let topic = format!("player:{username}");
                        let f = push(
                            &topic,
                            "action_rejected",
                            action_rejected_payload(verb, &at, reason),
                        );
                        (topic, f)
                    }
                    OutboundEvent::MoveAck { username, seq, tick } => {
                        let topic = format!("player:{username}");
                        let f = push(&topic, "ack", move_ack_payload(seq, tick));
                        (topic, f)
                    }
                };
                send_to_topic(&conns, &topic, &frame);
            }

            if broadcast {
                for topic in distinct_chunk_topics(&conns) {
                    if let Some(Topic::Chunk(realm, coord)) = parse_topic(&topic) {
                        if let Some(frame) = chunk_snapshot_push(&sim, realm, coord, &topic) {
                            send_to_topic(&conns, &topic, &frame);
                        }
                    }
                }
            }
        }
    });
}

fn spawn_stats_loop(shared: Arc<Shared>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(1_000));
        loop {
            interval.tick().await;
            let sim = shared.sim.lock().unwrap();
            let conns = shared.conns.lock().unwrap();
            for handle in conns.values() {
                if handle.state.topics.contains("dev:stats") {
                    let payload = stats_payload(&sim, handle.state.dev_username.as_deref());
                    let _ = handle.tx.send(text(&push("dev:stats", "stats", payload)));
                }
            }
        }
    });
}

fn distinct_chunk_topics(conns: &HashMap<u64, ConnHandle>) -> HashSet<String> {
    let mut out = HashSet::new();
    for handle in conns.values() {
        for t in &handle.state.topics {
            if matches!(parse_topic(t), Some(Topic::Chunk(..))) {
                out.insert(t.clone());
            }
        }
    }
    out
}

fn send_to_topic(conns: &HashMap<u64, ConnHandle>, topic: &str, frame: &PhxMessage) {
    let msg = text(frame);
    for handle in conns.values() {
        if handle.state.topics.contains(topic) {
            let _ = handle.tx.send(msg.clone());
        }
    }
}

fn text(frame: &PhxMessage) -> Message {
    Message::Text(frame.encode())
}

async fn handle_conn(
    shared: Arc<Shared>,
    mut stream: TcpStream,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Peek (don't consume) the request head to tell a WebSocket upgrade from a
    // plain HTTP GET. Non-upgrade requests get a plain health response (used by
    // readiness checks); the game speaks only the Phoenix-Channels socket.
    let mut buf = [0u8; 2048];
    let n = stream.peek(&mut buf).await.unwrap_or(0);
    let head = String::from_utf8_lossy(&buf[..n]);
    if !head.to_ascii_lowercase().contains("upgrade: websocket") {
        return serve_http(&mut stream).await;
    }

    let ws = tokio_tungstenite::accept_async(stream).await?;
    let (mut sink, mut read) = ws.split();
    let (tx, mut rx) = unbounded_channel::<Message>();
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);

    shared
        .conns
        .lock()
        .unwrap()
        .insert(id, ConnHandle { tx: tx.clone(), state: ConnState::default() });

    let writer = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if sink.send(msg).await.is_err() {
                break;
            }
        }
    });

    while let Some(frame) = read.next().await {
        let frame = match frame {
            Ok(f) => f,
            Err(_) => break,
        };
        match frame {
            Message::Text(txt) => {
                let parsed = match PhxMessage::decode(&txt) {
                    Ok(m) => m,
                    Err(_) => continue,
                };
                let mut sim = shared.sim.lock().unwrap();
                let mut conns = shared.conns.lock().unwrap();
                let outcome = match conns.get_mut(&id) {
                    Some(handle) => route(&mut sim, &mut handle.state, &parsed),
                    None => break,
                };
                drop(conns);
                drop(sim);
                if let Some(reply) = &outcome.reply {
                    let _ = tx.send(text(reply));
                }
                for p in &outcome.pushes {
                    let _ = tx.send(text(p));
                }
            }
            Message::Ping(p) => {
                let _ = tx.send(Message::Pong(p));
            }
            Message::Close(_) => break,
            _ => {}
        }
    }

    let username = shared.conns.lock().unwrap().remove(&id).and_then(|h| h.state.username);
    if let Some(user) = username {
        shared.sim.lock().unwrap().disconnect(&user);
    }
    writer.abort();
    Ok(())
}

/// Serve a non-WebSocket request with a plain 200 health response. One-shot
/// (Connection: close); just enough for liveness/readiness checks.
async fn serve_http(
    stream: &mut TcpStream,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Consume the request head (we only peeked it). Closing the socket with
    // unread bytes in the receive buffer makes the OS send a TCP RST instead of
    // a clean FIN, which clients surface as a connection reset.
    let mut scratch = [0u8; 1024];
    let mut seen = Vec::new();
    while !seen.windows(4).any(|w| w == b"\r\n\r\n") && seen.len() < 64 * 1024 {
        match stream.read(&mut scratch).await {
            Ok(0) | Err(_) => break,
            Ok(n) => seen.extend_from_slice(&scratch[..n]),
        }
    }

    let body = b"ok";
    let header = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(header.as_bytes()).await?;
    stream.write_all(body).await?;
    stream.flush().await?;
    Ok(())
}
