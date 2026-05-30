# Retired — behaviour lives in `stories/`, the *why* in `design/`

What the running system does, from outside, is now specified by the **user stories** in
**[`stories/`](./stories/)** (the observable acceptance criteria, in `design/glossary.md`
vocabulary) and proven by the test suite. Concrete parameters the stories deliberately omit
(View-window size, tick/snapshot rates, HP-per-click, grid extents) are engineering decisions
that live in the code and its tests, not a separate record. Product intent and rationale live
in **[`design/`](./design/)**.

This file is a tombstone so existing in-code references (`// ... (DESIGN.md)`) still resolve.
