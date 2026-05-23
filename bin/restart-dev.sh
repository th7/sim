#!/usr/bin/env bash
# Kill the running dev phx.server and start a new one in the background.
# Phoenix's CodeReloader picks up most file changes, but state-carrying
# refactors (Registry key shape, Session struct fields, channel topology)
# can leave the running BEAM in a weird half-reloaded state where new
# clients don't behave like they would on a cold boot. This script forces
# a clean restart for those situations.
#
# Scoped to the dev BEAM (MIX_ENV=dev) so an e2e BEAM running in parallel
# on :4001 is left alone.
set -euo pipefail

cd "$(dirname "$0")/.."

bin/kill-dev.sh

LOG=/tmp/phx-dev-restart.log
nohup env MIX_ENV=dev mix phx.server >"$LOG" 2>&1 &
disown || true

for _ in $(seq 1 120); do
  if curl -sf -o /dev/null http://localhost:4000/; then
    exit 0
  fi
  sleep 0.5
done

echo "dev phx.server failed to come back up within 60s. Log tail:" >&2
tail -30 "$LOG" >&2 || true
exit 1
