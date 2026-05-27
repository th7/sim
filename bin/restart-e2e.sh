#!/usr/bin/env bash
# Restart the e2e Rust sim server. Used by the phase3/phase8 Playwright specs to
# prove that Player/Structure/depletion state survives a real process restart:
# SIGTERM lets the server flush pending writes to Postgres before it exits, then
# a fresh process rehydrates from Postgres.
#
# Scoped to the e2e server via a pidfile (SIM_PIDFILE) and its own port
# (SIM_PORT, default 4001), so a dev server on :4000 is left alone.
set -euo pipefail

cd "$(dirname "$0")/.."
REPO="$(pwd)"

PORT="${SIM_PORT:-4001}"
PIDFILE="${SIM_PIDFILE:-/tmp/sim-e2e.pid}"
RLOG="${SIM_LOG:-/tmp/sim-e2e.log}"

# Stop the current e2e server (graceful: lets it flush to Postgres).
if [ -f "$PIDFILE" ]; then
  pid="$(cat "$PIDFILE" 2>/dev/null || true)"
  if [ -n "${pid:-}" ]; then
    kill -TERM "$pid" 2>/dev/null || true
    for _ in $(seq 1 100); do
      kill -0 "$pid" 2>/dev/null || break
      sleep 0.1
    done
  fi
fi

# Start a fresh one, serving the same bundle from the same database.
( cd "$REPO/sim" && \
  SIM_PORT="$PORT" \
  SIM_DATABASE_URL="${SIM_DATABASE_URL:?set SIM_DATABASE_URL for e2e}" \
  SIM_STATIC_DIR="${SIM_STATIC_DIR:-}" \
  nohup ./target/release/server >"$RLOG" 2>&1 &
  echo $! >"$PIDFILE" )

for _ in $(seq 1 120); do
  if curl -sf -o /dev/null "http://127.0.0.1:$PORT/"; then exit 0; fi
  sleep 0.25
done

echo "e2e sim server failed to come back up. Log tail:" >&2
tail -30 "$RLOG" >&2 || true
exit 1
