#!/usr/bin/env bash
# Kill the running phx.server and start a new one in the background.
# Used by the Phase 3 Playwright smoke test to prove that Player state
# survives a real BEAM restart.
set -euo pipefail

cd "$(dirname "$0")/.."

pkill -f beam.smp 2>/dev/null || true
while pgrep -f beam.smp >/dev/null; do sleep 0.2; done

LOG=/tmp/phx-restart.log
nohup mix phx.server >"$LOG" 2>&1 &
disown || true

for _ in $(seq 1 120); do
  if curl -sf -o /dev/null http://localhost:4000/ && \
     curl -sf -o /dev/null http://localhost:3000/; then
    exit 0
  fi
  sleep 0.5
done

echo "phx.server failed to come back up within 60s. Log tail:" >&2
tail -30 "$LOG" >&2 || true
exit 1
