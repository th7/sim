#!/usr/bin/env bash
# Kill the running e2e phx.server and start a new one in the background.
# Used by phase3 and phase8 Playwright specs to prove that Player state
# survives a real BEAM restart. Scoped to the e2e BEAM so a dev BEAM
# running in parallel on :4000 is left alone.
set -euo pipefail

cd "$(dirname "$0")/.."

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
