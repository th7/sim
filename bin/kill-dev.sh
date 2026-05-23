#!/usr/bin/env bash
# Kill any running dev phx.server BEAM and the Vite watcher Phoenix spawned
# alongside it. Idempotent: exits 0 if neither is running.
#
# The "dev BEAM" is identified as any `phx.server` BEAM **not** tagged
# `MIX_ENV=e2e` (the only other env that runs an HTTP listener). This is
# more robust than positive-matching `MIX_ENV=dev`: Mix defaults to :dev
# when MIX_ENV isn't set, so a BEAM started with bare `mix phx.server`
# has no `MIX_ENV=dev` in its environ at all.
set -euo pipefail

dev_pids() {
  local pid env
  for pid in $(pgrep -f phx.server 2>/dev/null || true); do
    if [ ! -r "/proc/$pid/environ" ]; then continue; fi
    env=$(tr '\0' '\n' < "/proc/$pid/environ" 2>/dev/null || true)
    # Skip e2e BEAM — restart-e2e.sh handles that one.
    if echo "$env" | grep -qx 'MIX_ENV=e2e'; then continue; fi
    # Skip anything explicitly tagged test/prod, just in case.
    if echo "$env" | grep -qE '^MIX_ENV=(test|prod)$'; then continue; fi
    echo "$pid"
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
