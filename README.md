# Sim

A cooperative, persistent, isometric, real-time world where players fight, craft, and gather. PvP is an eventual concern, not v1.

Elixir/Phoenix backend over an ECS with chunks-as-processes; Vite + Three.js client speaking Phoenix Channels.

## Layout

- `apps/game_core/` — ECS, chunks, sessions, collision, worldgen
- `apps/game_persistence/` — Datastore (single per-node write chokepoint) and Ecto schemas
- `apps/game_web/` — Phoenix Channels (no LiveView)
- `frontend/` — Vite + Three.js client
  - `frontend/test/` — vitest specs against a live Phoenix on `:4000`
  - `frontend/e2e/` — Playwright specs against a dedicated e2e Phoenix on `:4001`
- [`CONTEXT.md`](./CONTEXT.md) — the locked language: glossary + relationships
- [`PLAN.md`](./PLAN.md) — work not yet implemented, in three triage buckets

## Running locally

Requires Elixir, Node, and a running Postgres reachable via Unix socket at `/tmp`.

```bash
mix deps.get && (cd frontend && npm install)
mix ecto.create && mix ecto.migrate

mix phx.server                  # Phoenix on :4000
(cd frontend && npm run dev)    # Vite on :3000, proxies /api + /socket to :4000
```

Open <http://localhost:3000/?u=alice>.

## Tests

```bash
mix test                                # Elixir unit + integration
(cd frontend && npm test)               # vitest channel specs (needs mix phx.server up)
(cd frontend && npm run test:e2e)       # Playwright (spins up its own Phoenix on :4001)
```

The e2e specs under `frontend/e2e/` are the load-bearing description of what the game does end-to-end — read them when you want to know what "works" means.
