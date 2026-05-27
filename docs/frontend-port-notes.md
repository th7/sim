# Frontend → Rust port: decisions to revisit

Running notes for the ADR-0003 native-client port. Things decided autonomously
that we should sanity-check together later.

## Status

- **Behavioural parity: done and tested.** The client *logic* (the `client` crate's `ClientModel` +
  `Session`) re-pins the phase behaviours via 11 model unit tests + 5 integration tests that boot the real
  server in-process and drive the native WS/phx client (connect/see-self, two clients, movement, harvest→
  inventory, portal→instance realm switch). 114 workspace tests green, zero warnings.
- **Rendering parity: written, compiles, NOT visually verified.** `client/src/bin/game.rs` (three-d +
  egui) compiles, but this environment has no display/GL context, so it cannot be *run*. The manual
  visual pass — the rendering half of ADR-0003's parity bar — must be done on a machine with a display.
  **The cutover (deleting `frontend/`, removing the server's static-serving) is therefore NOT done**; the
  TS client stays as the working/reference client until the native render is visually confirmed.

## Decisions to revisit

- **`export-contract` (ADR step) deferred.** `contract/contract.json` is still hand-maintained and the sim
  conformance test still guards it; generating it from the Rust structs (+ freshness check) isn't built
  yet. Not parity-blocking. (Task left open.)
- **`WALL_COST = 5` hardcoded in the client model** (a build-affordability UX gate). Mirrors the server
  catalogue but could drift — consider exposing the cost via `protocol`.
- **Renderer is intentionally minimal** (to revisit on a display): every entity is a coloured cube (the old
  `models.ts` had nicer tree/player/portal meshes); **no name labels yet** (three-d's `text` feature exists
  — follow-up); camera framing is approximate; the scene is rebuilt every frame (fine for a low-poly scene,
  but no object reuse); **no client-side position lerp yet**, so cubes step at the 10 Hz broadcast rate
  rather than interpolating like the old client did. None affect behaviour; all are visual polish.
- **Per-frame render reads the model's merged players directly** (no mesh-removal "grace"); a player briefly
  absent from a snapshot during a chunk crossing could blink. Port the TS debounce if it shows on a display.
