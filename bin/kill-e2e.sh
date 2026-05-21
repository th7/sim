#!/usr/bin/env bash
# Kill any running e2e phx.server BEAM. Idempotent: exits 0 if none.
# Identifies the e2e BEAM via `MIX_ENV=e2e` in its environment, since
# Mix loads project paths at runtime and they don't appear in argv.
set -euo pipefail

e2e_pids() {
  local pid
  for pid in $(pgrep -f beam.smp 2>/dev/null || true); do
    if [ -r "/proc/$pid/environ" ] && \
       tr '\0' '\n' < "/proc/$pid/environ" 2>/dev/null | grep -qx 'MIX_ENV=e2e'; then
      echo "$pid"
    fi
  done
}

for pid in $(e2e_pids); do
  kill "$pid" 2>/dev/null || true
done
while [ -n "$(e2e_pids)" ]; do sleep 0.2; done
