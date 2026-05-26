#!/usr/bin/env bash
# Kill the running e2e phx.server and start a new one in the background.
# Used by phase3 and phase8 Playwright specs to prove that Player state
# survives a real BEAM restart. Scoped to the e2e BEAM so a dev BEAM
# running in parallel on :4000 is left alone.
set -euo pipefail

cd "$(dirname "$0")/.."
REPO="$(pwd)"

# Rust-backend mode (E2E_BACKEND=rust): restart the Rust sim server instead of
# the BEAM, to prove the Rust Postgres persistence survives a real restart.
# SIGTERM lets the server flush pending writes before it exits; the fresh
# process rehydrates from Postgres. Default (Elixir) path below is unchanged.
if [ "${E2E_BACKEND:-}" = "rust" ]; then
  PORT="${SIM_PORT:-4000}"
  for pid in $(pgrep -f 'release/server' || true); do
    kill -TERM "$pid" 2>/dev/null || true
  done
  for _ in $(seq 1 100); do
    pgrep -f 'release/server' >/dev/null 2>&1 || break
    sleep 0.1
  done
  RLOG="${SIM_LOG:-/tmp/sim-server.log}"
  ( cd "$REPO/sim" && \
    SIM_PORT="$PORT" SIM_DATABASE_URL="${SIM_DATABASE_URL:?set SIM_DATABASE_URL for rust e2e}" \
    nohup ./target/release/server >"$RLOG" 2>&1 & )
  for _ in $(seq 1 120); do
    if (exec 3<>"/dev/tcp/127.0.0.1/$PORT") 2>/dev/null; then exec 3>&-; exit 0; fi
    sleep 0.1
  done
  echo "rust sim server failed to come back up. Log tail:" >&2
  tail -30 "$RLOG" >&2 || true
  exit 1
fi

bin/kill-e2e.sh

LOG=/tmp/phx-e2e-restart.log
nohup env MIX_ENV=e2e PORT=4001 mix phx.server >"$LOG" 2>&1 &
disown || true

for _ in $(seq 1 120); do
  if curl -sf -o /dev/null http://localhost:4001/; then
    exit 0
  fi
  sleep 0.5
done

echo "e2e phx.server failed to come back up within 60s. Log tail:" >&2
tail -30 "$LOG" >&2 || true
exit 1
