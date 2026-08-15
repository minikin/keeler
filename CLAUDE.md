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
- **There is no `crap-baseline.json` here.** The harness has no library
  or binary targets, so coverage, CRAP and mutation have nothing to
  measure and the recipes say so instead of failing (spec 01). Spec 04
  ends this divergence: once the release tooling becomes an xtask crate,
  the baseline returns and the gates measure real sources.

