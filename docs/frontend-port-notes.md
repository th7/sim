# Frontend → Rust port: decisions to revisit

Running notes for the ADR-0003 native-client port. Things decided autonomously
that we should sanity-check together later.

## Status

- **Behavioural parity: done and tested.** The client *logic* (`ClientModel` + `Session`) re-pins every
  phase the old Playwright suite covered, via model unit tests + 8 integration tests that boot the real
  server in-process and drive the native WS/phx client:
  - phase1 — two clients see each other;
  - phase3 — movement (server-restart persistence is a server concern, covered by sim's own tests);
  - phase5 — walking across multiple chunk boundaries stays visible + monotonic (window pan);
  - phase6.5 — dev-mode `dev:stats` subscription + overlay ring;
  - phase8 — gather → build → damage-to-destruction round-trip (hp/owner/cost);
  - phase9 — walk into a portal, switch realm, see the return portal.

  118 workspace tests green, zero warnings. (The `Session::send_harvest/build/damage` helpers are the
  test seam analog of the old client's `__game.{harvest,build,damage}` hooks — used by phase8 to place a
  wall at hand-computed coords where the click cell-snap + collision margins are too tight to drive.)
- **Rendering parity: ported faithfully, compiles, NOT visually verified.** `client/src/bin/game.rs`
  (three-d + egui) is a direct port of the old `main.ts` / `models.ts` rendering — same meshes, palette,
  camera offset, backgrounds, interpolation, dev overlay (see below). It compiles, but this environment has
  no display/GL context, so it cannot be *run*. The manual visual pass — the rendering half of ADR-0003's
  parity bar — must be done on a machine with a display.
- **Cutover done.** After the visual pass on macOS, `frontend/` and the server's static-serving were
  removed; the native client is the only client. The Playwright e2e suite went with it — `client/tests/
  integration.rs` now re-pins the phases. The wire contract (`contract/contract.json` +
  `sim/tests/contract.rs`) survives the cutover.

## What the native view now mirrors from the old frontend

- Meshes: player body + head; tree trunk (cylinder) + two conical foliage tiers that vanish when depleted;
  three-plank walls; flat portal disc coloured by direction. Same hex palette and per-name hash colour.
- Isometric camera at the old `(12,12,12)` offset, re-framing the local player each frame.
- Position **lerp** over the 100 ms snapshot interval + 400 ms **removal grace** (anti-blink on chunk
  crossings), cleared on realm switch — a faithful port of the old lerp/grace logic.
- Overworld/instance background colours; dev chunk-lifecycle overlay (hot/idle-armed/cold fills + shrinking
  idle countdown bar); dev HUD with user/realm/pos/chunk/view/active/total.

## Decisions to revisit

- **`export-contract` (ADR step) deferred.** `contract/contract.json` is still hand-maintained and the sim
  conformance test still guards it; generating it from the Rust structs (+ freshness check) isn't built
  yet. Not parity-blocking. (Task left open.)
- **`WALL_COST = 5` hardcoded in the client model** (a build-affordability UX gate). Mirrors the server
  catalogue but could drift — consider exposing the cost via `protocol`.
- **Dev toggle bound to `Tab`, not backtick** — three-d's `Key` enum has no backtick/grave variant. The
  `--dev`/`?dev=1` startup path is unchanged.
- **Portal torus ring omitted** — three-d has no torus primitive; the disc is drawn, the floating ring is
  not. The old client's `GridHelper` lines and the dev overlay's chunk **borders + coordinate labels** are
  also omitted (labels need three-d's `text` feature). All cosmetic; revisit on a display.
- **No shadows** (the old client cast shadow maps). Cosmetic.
- **Own player no longer highlighted** — matched the old client, which used the hash colour for everyone
  (the camera centring already identifies you). A brief earlier draft tinted self green; reverted for parity.
