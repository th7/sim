//! Async WebSocket runtime serving the Phoenix Channels v2 protocol, backed by
//! the [`Sim`]. The pure routing lives in [`crate::server`]; this module owns
//! the sockets, the subscriber registry, and the tick / broadcast / stats
//! loops. Exposed from the lib (not just the binary) so end-to-end wire tests
//! can run it in-process.

use crate::consts::{BROADCAST_EVERY, TICK_MS};
use crate::phx::{push, PhxMessage};
use crate::server::{chunk_snapshot_push, parse_topic, route, stats_payload, ConnState, Topic};
use crate::sim::{OutboundEvent, Sim};
use crate::wire::{inventory_payload, relocated_payload};
use futures_util::{SinkExt, StreamExt};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
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
        Arc::new(Shared {
            sim: Mutex::new(Sim::new()),
            conns: Mutex::new(HashMap::new()),
        })
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
            sim.tick();
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
    stream: TcpStream,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
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
