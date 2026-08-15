# CLAUDE.md

This file provides guidance to Claude Code when working with code in this repository.

The Keeler workflow rules live in their own file so they can be updated
without touching your project's instructions:

@.claude/keeler.md

<!-- Add project-specific instructions below this line. -->

## How this repository diverges from the rules it ships

`.claude/keeler.md` is the deliverable: it is written for the projects
Keeler installs into, and says nothing about Keeler itself. The places
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
- **The gates measure `xtask/`, and only it.** The harness itself has no
  library or binary target — its job is to drive `install.sh` as a
  subprocess — so coverage, CRAP and mutation see the release tooling and
  nothing else. That is real code doing a real job, so `crap-baseline.json`
  is committed and `just crap-delta` ratchets against it. Until spec 04
  there was nothing here to measure at all, and the recipes said so rather
  than failing (spec 01).
- **Run the gates with the workspace selected.** The shipped recipes
  already pass `--workspace`; a bare `cargo nextest run` or `cargo mutants`
  in this repository silently skips the xtask member and reports success
  having tested nothing.

