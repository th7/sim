#!/usr/bin/env bash
# Kill any running dev phx.server BEAM and the Vite watcher Phoenix spawned
# alongside it. Idempotent: exits 0 if neither is running. Identifies the
# dev BEAM via `MIX_ENV=dev` in its environment, since Mix loads project
# paths at runtime and they don't appear in argv.
set -euo pipefail

dev_pids() {
  local pid
  for pid in $(pgrep -f beam.smp 2>/dev/null || true); do
    if [ -r "/proc/$pid/environ" ] && \
       tr '\0' '\n' < "/proc/$pid/environ" 2>/dev/null | grep -qx 'MIX_ENV=dev'; then
      echo "$pid"
    fi
  done
}

for pid in $(dev_pids); do
  kill "$pid" 2>/dev/null || true
done
while [ -n "$(dev_pids)" ]; do sleep 0.2; done

# Vite is a child of Phoenix's `:watchers` config, so it normally dies with
# the BEAM — but the wrapping `sh -c vite` can outlive a SIGKILL, leaving
# the next `phx.server` startup blocked on a port-3000 collision. Mop up.
pkill -f 'sh -c vite' 2>/dev/null || true
pkill -f 'node .*\.bin/vite' 2>/dev/null || true
