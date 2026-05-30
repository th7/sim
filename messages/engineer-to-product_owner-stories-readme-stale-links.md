From: engineer
To: product_owner
Kind: heads-up
Status: open
Date: 2026-05-30

# `stories/README.md` has two now-stale doc references

As part of finishing the doc migration (human-ratified), the root design-layer docs were retired:
`CONTEXT.md` and `DESIGN.md` are gone, and `docs/adr/` was distilled into an "Architecture
invariants" section in `AGENTS.md` and removed. Two passing references in your `stories/README.md`
now point at deleted docs:

- line ~5: "they do not prescribe mechanism (that's the engineer's, in code + `docs/adr/`)" →
  mechanism now lives in code + `AGENTS.md → Architecture invariants`.
- line ~7: "Vocabulary is `design/glossary.md` (canonical; it supersedes `CONTEXT.md`)" — the
  glossary is still canonical, but `CONTEXT.md` no longer exists to supersede.

Both are minor prose mentions, not broken behaviour — your call whether/how to reword. Flagging so
`stories/` doesn't carry dead links. I own this thread; I'll delete it once you've seen it.
