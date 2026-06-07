# Targeting & WYSIWYG — decision record

Outcome of a design grill (2026-06-07). Canonical terms live in `glossary.md` (Tick, Verb,
Target, Frontier, Lawful render, Verb button, Target marker); this records the decisions,
their order, and what was rejected or parked. Increment sequencing is in `PLAN.md`.

## The feature

Select-then-act. Clicking a targetable entity (Gatherable, Structure, NPC — not Players,
not Portals) designates it the **Target** and does nothing else; the click-priority
heuristic in the client dies (its build branch survives as click-on-ground, to be improved
separately). The **Verb button** (`E` + contextual HUD button, one concept) is the only
issuer of entity-directed Verbs: Gatherable → harvest, Structure/NPC → damage. A Target is
sticky observation: cleared only by Escape, retargeting, despawn, leaving the View window,
or world transitions — never by distance, never by clicking elsewhere; depleted stays
targeted. Display is the diegetic **Target marker** only — deliberately no HUD target
frame (Demeanor/Health stay readable from the entity itself).

## The mechanism ladder

Each decision was locked in order; later ones depend on earlier ones.

1. **Verbs act on identity.** Entity-directed Verbs carry the Target's WireId (already the
   snapshot key on the wire; the server already indexes by it); the Island resolves through
   `wire_index` and judges against authoritative state. Position payloads (`{x,y}`) leave
   the wire for harvest/damage. The Mirror's speculation plays no part in any Verb outcome.
2. **No hard client gate.** The client gates only facts it holds non-speculatively (no
   Target → inert; kind→verb mapping). Range is never client-denied: a denial based on
   speculated positions is a permanent false negative (the one Mirror divergence that can
   never resolve, because nothing was sent). The button *dims* on a speculated
   out-of-range hint but still sends; the Island judges.
3. **Intents bind to named Ticks; one lock each.** Tick outcomes are a pure function of
   the locked-intent log under a fixed neutral simultaneity law (verbs before movement,
   deterministic order) — never network arrival order. Verbs are seq-pinned: resolved at
   the tick the player pressed in, so own-position eligibility is replay-exact
   (eliminates "ran at the tree, pressed too early" rejections by construction).
4. **Preemptive resolution (eager facts, final emission).** A tick's facts may resolve
   and emit before all intents arrive, iff no missing input could affect them: each
   missing intent casts a *could-affect shadow* (last position ⊕ max speed ⊕ enforced
   verb reach, ≈1.6 u, lifetime ≤ INTENT_GRACE_TICKS); outside all shadows, facts are
   Resolved and emittable immediately. Emittable ⇔ Resolved; finalization is internal
   bookkeeping (Datastore cuts, state retention); speculated values never leave the
   Island. Sim state for tick T is discardable once T+1 finalizes; a separate ~10-tick
   position/intent ring serves judging (below). Monotone refinement ⇒ confluence: values
   are schedule-independent, replay needs only the intent log.
5. **Lawful-render judging (press-frame eligibility).** A session continuously asserts
   its **Frontier** (last incorporated authoritative tick) on every input frame; verbs
   carry nothing and inherit it. Hard checks: delivered-tick reality, never-future
   (violation = proof of cheating), monotone, `M − frontier ≤ LEAD_BOUND` (asserting
   staleness forces freezing your own inputs, as an honest lagged Mirror would). Range
   eligibility for entity-directed Verbs is judged in the **press frame**: own exact
   position vs the Target's lawful render (server-recomputed via the shared integrator
   from its ring). Forgiveness is continuous-only (never liveness/depletion/yields);
   effects land at the resolve tick, never backdated. Residual exploit: the permanent
   max-lag persona cherry-picking extrapolation noise — bounded by the honest-max-lag
   constant, indistinguishable from honest lag by design, delegated to offline
   statistics. **Revisit-at-PvP**: the whole mechanism re-prices under PvP.

Geometry check: max press-frame reach ≈ interact 1.0 u + v_max×lead 2.1 u = 3.1 u, an
order of magnitude inside the Labeler's 3×3-chunk merge margin — never-under-merge
absorbs it untouched. (Judging-ring data must survive Island merges within the Lead
window; recomputable either way.)

Required repair found en route: `build` lacks the server-side `in_range` check its
siblings have (client-only gate today → cross-chunk build is possible from a hostile
client, and the unenforced reach inflates could-affect shadows to chunk scale). Fix ships
with increment 1.

## Engineering deviations (made at the engineer's discretion — review welcome)

Two points where implementation deviated from the grilled design, each with the
options weighed and the choice taken:

1. **The Frontier rides entity-directed Verbs, not every input frame.** The
   grill locked "frontier on every input frame; verbs carry nothing" — but the
   protocol sends *no* input frames while idle (Intent renewal goes silent), so
   a standing-only assertion goes stale exactly when a stationary player
   presses. Options: (a) assert on idle keepalive frames (new traffic, changes
   the idle-silence protocol), (b) assert per-Verb with per-player monotonicity
   (this choice), (c) both. Choice (b): identical judging power; per-press
   shopping stays impossible (the assertion is clamped monotone per player —
   regressing claims clamp *up*), never-future and Lead-window clamps as
   designed. Revisit if input-frame-cadence assertions are ever wanted for
   analytics.
2. **Eligibility is either-frame, not press-frame-only.** Judging *only* the
   lawful render would reject a press whose target is authoritatively in range
   but speculated out (the lunging wolf — the exact case the always-send rule
   exists for). Eligible ⇔ in range in the press frame **or** the
   authoritative present. Generosity stays bounded by the same Lead constant;
   the deer's exploit ceiling is unchanged.

Also one scope note: the never-future clamp is against the global tick (a tick
that was never simulated cannot have been delivered); per-session
delivered-tick tracking is transport bookkeeping deferred until it buys
something observable.

3. **Preemptive resolution (ladder step 4) implemented as its observable
   promises, not as shadow-scheduling machinery.** Implementation-time
   finding: the could-affect-shadow machine's *driver* — resolution waiting on
   missing intents — does not exist in this architecture. Intent is perishable
   and the tick never stalls (grace absorbs missing frames); the only fact
   that ever waits is a seq-pinned Verb, whose pin *is* its could-affect
   dependency, already minimal. Shadows become load-bearing only under the
   parked lockstep trio (tick-named intent *locking* with gated emission).
   Options: (a) build the trio now to give shadows a driver (contradicts the
   parked decision — anti-backdating pays only under PvP), (b) build the
   shadow scheduler anyway, dormant (dead machinery, untestable behavior),
   (c) implement and pin the step's *observable* promises and re-attach the
   shadow machine to the trio's revisit trigger (this choice). What is pinned:
   **schedule-confluence** (`outcomes_are_invariant_to_intent_arrival_schedule`
   — the same logical inputs under different arrival timings produce a
   bit-identical world; this would fail under arrival-judged semantics),
   **emittable ⇔ resolved** (outcome events and snapshots already emit only
   computed-final facts; nothing provisional has ever crossed the wire), and
   **bounded retention** (`the_judging_ring_never_outgrows_the_lead_window` —
   the judging ring is the only per-tick history and is Lead-bounded;
   determinism makes anything older recomputable from the intent log).
   When the trio lands, the shadow scheduler slots under it unchanged.

## Rejected / parked

- **Arrival-order resolution** (within or across ticks) — rejected. Determinism degrades
  from derived to recorded; contests become latency auctions; the Mirror's exact replay
  and both locked judging mechanisms are tick-shaped. Maximal responsiveness was the only
  virtue, and press-frame judging recovers the perceived part of it.
- **Lockstep trio** (one-shot intents + lock-gated emission + eager per-Island finality) —
  sound, exploit-clean anti-backdating; parked because backdating only pays under PvP and
  the trio doesn't touch the perceptual gap (the deer). Composes with everything locked.
- **Async Island clocks** (conservative PDES; skew budget = spatial gap / 2·v_max; merges
  are rendezvous in time) — sound and invariant-cheap; parked for the same reason: pacing
  win, not WYSIWYG. Natural successor to increment 4 if straggler pacing ever hurts.
