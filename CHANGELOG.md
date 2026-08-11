# Changelog

All notable changes to Keeler are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions
follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Installations pin a version with `KEELER_REF` and record it at the top of
`.claude/keeler.md`.

## [Unreleased]

## [0.1.0] — 2026-08-11

First release: the workflow, the gates, and a one-command installer.

### Added

- **The pipeline** — `/keeler:spec` → approval → `/keeler:tasks` →
  `/keeler:tdd` → `/keeler:qa` → `/keeler:review` → `/keeler:mutants`, with
  `/keeler:feature` orchestrating the whole run and `/keeler:fix` covering
  the bugfix road (reproduce with a failing test before touching code).
  Commands are namespaced, so they never collide with a project's own.
- **Change classes** — feature, bugfix and a documented fast path for
  trivial edits, so the pipeline is not bypassed for small changes.
- **Quality gates** — `rustfmt`, `clippy` (pedantic), `cargo-nextest`,
  line coverage (`cargo-llvm-cov`, ≥ 90%), CRAP score (`cargo-crap`,
  threshold 15), and mutation testing (`cargo-mutants`, changed lines only).
- **The CRAP baseline ratchet** — `crap-baseline.json` is committed and
  shared, and the delta gate fails on any per-function regression, locally
  and in CI. Legacy debt is grandfathered; new debt is not.
- **Skills** — `property-testing` (invariant catalog for proptest) and
  `gherkin-specs` (scenario-writing rules) load themselves when relevant.
- **Installer** — one command sets up tools, workflow files, `Cargo.toml`
  sections and `.gitignore` entries in any Rust project. Idempotent; an
  existing `CLAUDE.md` is never rewritten (a single `@.claude/keeler.md`
  import is appended instead). `KEELER_REF` pins a version;
  `just keeler-upgrade` re-runs it later.
- **CI** — the same gates as GitHub Actions, plus jobs that install Keeler
  into fresh binary and library projects on Linux and macOS, verify
  idempotency and `CLAUDE.md` preservation, and exercise the path where the
  installer bootstraps the toolchain itself.
- **Docs** — `README.md` for getting started, `KEELER.md` for the workflow
  in plain words with diagrams, `.claude/keeler.md` as the rules the agent
  operates under.

[Unreleased]: https://github.com/minikin/keeler/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/minikin/keeler/releases/tag/v0.1.0
