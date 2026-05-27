#!/usr/bin/env bash
# Build and run the sim server + native client together for local play on the
# host. Blocks until you interrupt it (Ctrl-C) or either process exits, then
# shuts both down. Builds release binaries first and runs them directly (so the
# child PIDs are the real processes, not a `cargo run` wrapper).
#
# Usage:
#   bin/dev.sh                    # server on :4000 + client as "alice"
#   bin/dev.sh --user bob         # name the player
#   bin/dev.sh --dev              # extra client flags are forwarded
#
# Env:
#   SIM_PORT          server port (default 4000); the client points here too
#   SIM_DATABASE_URL  libpq URL to persist through restarts (default: in-memory)
set -euo pipefail

cd "$(dirname "$0")/.."

PORT="${SIM_PORT:-4000}"
SERVER_PID=""
CLIENT_PID=""

cleanup() {
  trap - INT TERM EXIT
  echo
  echo "dev: shutting down…"
  [ -n "$CLIENT_PID" ] && kill -TERM "$CLIENT_PID" 2>/dev/null || true
  [ -n "$SERVER_PID" ] && kill -TERM "$SERVER_PID" 2>/dev/null || true
  wait 2>/dev/null || true
}
trap cleanup INT TERM EXIT

echo "dev: building server + client (release)…"
cargo build --release --bin server --bin game

echo "dev: starting server on :$PORT"
SIM_PORT="$PORT" ./target/release/server &
SERVER_PID=$!

# Wait for the server to accept connections before launching the client, so the
# client doesn't race the bind and exit on "connection refused". Check our
# server is still alive *first* — otherwise a bind failure (e.g. :PORT already
# in use) would be masked by curl succeeding against whatever else holds it.
for _ in $(seq 1 150); do
  kill -0 "$SERVER_PID" 2>/dev/null || {
    echo "dev: server exited before binding (is :$PORT already in use?)" >&2
    exit 1
  }
  if curl -sf -o /dev/null "http://127.0.0.1:$PORT/"; then break; fi
  sleep 0.2
done

# Default the player name unless the caller passed --user.
case " $* " in
  *" --user "*) ;;
  *) set -- --user alice "$@" ;;
esac

echo "dev: starting client ($*)"
./target/release/game --server "ws://localhost:$PORT/socket/websocket?vsn=2.0.0" "$@" &
CLIENT_PID=$!

# Block until either process exits (Ctrl-C trips the trap; closing the game
# window exits the client; a server crash exits the server) — then clean up.
while kill -0 "$SERVER_PID" 2>/dev/null && kill -0 "$CLIENT_PID" 2>/dev/null; do
  sleep 1
done
