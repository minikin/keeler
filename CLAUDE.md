# CLAUDE.md

This file provides guidance to Claude Code when working with code in this repository.

The Keeler workflow rules live in their own file — the plugin's, at the
root of this repository, which is the plugin. In an adopting project the
`SessionStart` hook prints it; here the import below does the same job:

@keeler.md

Those rules are the law, and two chapters sit beside them in the plugin
root: [graph-mode.md](graph-mode.md) and [gates.md](gates.md).
[docs/KEELER.md](docs/KEELER.md) says why they are shaped that way, and
[README.md](README.md) is the front door — install, first day, and what
Keeler is not. What follows here is this repository only.

<!-- Add project-specific instructions below this line. -->

## How this repository diverges from the rules it ships

`keeler.md` is the deliverable: it is written for the projects Keeler is
installed into, and says nothing about Keeler itself. The places
where this repository cannot follow its own rules literally are recorded
here instead — project-specific instructions, in the file meant for them.

- **The deliverable is shell, not Rust.** The crate is the installer's
  test harness, so the suite is not one `tests/acceptance.rs` but
  per-area files: `tests/installer.rs` (spec 01), `tests/release.rs`
  (spec 02) and `tests/wild.rs` (spec 03). One test per scenario, named
  after it, as usual.
- **Proptest seeds live beside the tests.** With no `lib.rs` to anchor
  proptest's default path, the harness sets
  `FileFailurePersistence::WithSource("proptest-regressions")`, so seeds
  land at `tests/installer.proptest-regressions`. Commit them.
- **The gates measure `xtask/` and `keeler-top/`.** The harness itself has
  no library or binary target — its job is to drive `install.sh` as a
  subprocess — so coverage, CRAP and mutation see the two members that
  have one, and nothing else. Both are real code doing a real job, so
  `crap-baseline.json` is committed and `just crap-delta` ratchets against
  it. Until spec 04 there was nothing here to measure at all, and the
  recipes said so rather than failing (spec 01). The two members are not
  alike: `xtask/` is machinery that never reaches an adopter, while
  `keeler-top/` ships inside the plugin and is built from its tree by
  `keeler keeler-top`. Its own suite is `keeler-top/tests/top.rs`, beside
  the crate rather than in the root harness, because that is the only
  place `CARGO_BIN_EXE_keeler-top` resolves; the recipes that launch it
  are shell, so they are tested from the harness like every other recipe
  (spec 10).
- **Run the gates with the workspace selected.** The shipped recipes
  already pass `--workspace`; a bare `cargo nextest run` or `cargo mutants`
  in this repository silently skips the xtask member and reports success
  having tested nothing.

## The demo board

`python3 demo/board.py` builds a throwaway repository whose board shows
every state at once, so `keeler-top` can be filmed or looked at after a
change to the frame without spending an agent on a wave;
`--clean` takes it away again. It is repository machinery — never
installed, never reached by a recipe — and `demo/README.md` says what it
fabricates and what is real.

## Comments

**A comment that restates the code is not allowed.** Delete it. A comment
earns its place by saying what the code cannot: why this way and not the
obvious one, what breaks if it changes, which upstream behavior it is
matching. `# increment the counter` above `n += 1` is noise; the tests'
Given/When/Then lines and a rationale a reader could not derive from the
code are not.
