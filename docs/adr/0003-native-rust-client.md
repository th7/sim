---
status: proposed
---

# The game client is a native Rust app, not a browser client

## Context

The backend is Rust ([ADR-0002](./0002-rust-clustered-simulation-runtime.md)), fronted by a Vite + Three.js
(TypeScript) **browser** client that speaks Phoenix Channels v2 over WebSocket, is served as a bundle by the
server, and is exercised by a Playwright e2e suite. We want a **single-language stack** and to **avoid the
browser's memory footprint**. WASM-in-the-browser would give the first but not the second — it still runs
atop Chromium's baseline and can use more memory than the JS it replaces — so the client becomes a **native
desktop application** in Rust. (This deliberately reverses the recently-built web-client + single-binary
serve + browser-e2e setup; the reversal is the point of this record.)

## Decision

- **Native client** on **`three-d`** (a Three.js-shaped Rust rendering crate — a near 1:1 port of the
  existing meshes / flat shading / directional+ambient light + shadows / perspective camera / click→world
  raycast) with **egui** for the inventory + dev HUD and world-anchored name labels.
- **Cargo workspace** with a shared **`protocol`** crate: the phx codec, the wire payload structs (now
  deriving `Serialize` **and** `Deserialize`, both directions), `geometry`, `ids`/`Realm`, consts, and the
  pure component enums — depended on by both the server and the client. The client carries none of the
  server's tokio / postgres / hecs.
- **`contract.json` is generated from the Rust structs** (an export binary + a freshness check); the structs
  are the single source of truth, the JSON a spec/doc hedge for a possible future non-Rust consumer.
- **Testing** splits the client into a pure, tested **model** (observable state: players, inventory, realm,
  view-window — the native analog of today's `window.__game`) and a thin **`three-d` view**. Rust
  integration tests boot the server in-process and drive the model with scripted input, re-pinning the
  phase1/3/5/6.5/8/9 behaviours. The browser Playwright suite is retired.
- **Migration**: keep the TS client working throughout; do the `protocol` refactor first (server-only, TS
  unaffected), build the native client to its parity bar (model/view tests green + a clean manual visual
  pass), then cut over in one step — delete `frontend/`, remove the server's now-vestigial HTTP
  static-serving (back to WS-only), update docs.

## Considered options

- **Rust→WASM in the browser** — single-language, keeps Playwright and zero-install web reach, but does not
  escape browser memory bloat (a primary driver). Rejected.
- **Bevy** — batteries-included and scales to a richer client, but heavier on binary / compile / RAM and
  imposes its own ECS; overkill for a thin renderer of server-authoritative snapshots. Rejected for now;
  revisit if the client grows substantial logic.
- **Retire `contract.json` entirely** — defensible once both ends share structs, but cheaply giving up the
  spec + a coding target for any future non-Rust consumer wasn't worth it. Rejected in favour of generating it.

## Consequences

- **Lost:** the browser Playwright e2e (the prior "load-bearing end-to-end description") and zero-install web
  access. Server behaviour stays covered by the `sim` tests; client behaviour moves to the new Rust model
  tests; **actual rendering is verified manually** (optional headless wgpu screenshot tests can be added later).
- **Gained:** one language end-to-end, the client deserializing the exact structs the server serializes (no
  codegen, no cross-language drift), and a lean native binary instead of a browser process.
- **Distribution** shifts from "serve a bundle" to "ship a per-OS binary"; the client takes a server URL +
  username/chunk/dev flags in place of the URL params. Packaging is out of scope here.
- **Risk concentrates in the renderer:** world-anchored text labels, shadow setup, and click→world picking
  are the parts of the Three.js scene with the least direct `three-d` analog and the most likely to stall —
  which is exactly why the migration builds to parity behind the working TS client before cutting over.
