# Changelog

All notable changes to Keeler are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions
follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Installations pin a version with `KEELER_REF` and record it at the top of
`.claude/keeler.md`.

## [Unreleased]

### Changed

- The shipped workflow rules no longer talk about Keeler itself. Notes
  about this repository's own divergences — a shell deliverable, per-area
  test files, no CRAP baseline — were sitting in `.claude/keeler.md`,
  which installs into every adopting project and should describe only the
  project it lands in. They now live in Keeler's own `CLAUDE.md`, the file
  the installer reserves for project-specific instructions.

## [0.1.0] — 2026-08-14

First release: the workflow, the gates, a one-command installer — and the
repository that holds itself to the same standard it installs.

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
  shared in adopting projects, and the delta gate fails on any per-function
  regression, locally and in CI. Legacy debt is grandfathered; new debt is
  not.
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
- **Release machinery** (spec 02) — pushing a tag `vX.Y.Z` cuts the GitHub
  release mechanically, after the repository proves the tag honest: tag ↔
  `VERSION` ↔ rules marker ↔ CHANGELOG agreement, gates before publishing,
  notes extracted as exactly the version's CHANGELOG section, and
  `install.sh` + its SHA256 attached so adopters can pin *and verify*
  (`sha256sum -c`). A lying tag publishes nothing; reruns never overwrite
  a published release.
- `CONTRIBUTING.md`, `SECURITY.md`, dependabot for actions and crates, and
  a committed `Cargo.lock` with a CI freshness gate.
- A CI job that forces `cargo binstall`'s source-compile fallback, so the
  `--locked` path stays proven even when release downloads are healthy.
- A harness test that fails when any file under the command or skill trees
  does not land in an installed project.

### Changed

- The repository's own gates measure the deliverable, not a placeholder
  (spec 01). The example crate is gone; `tests/installer.rs` drives
  `install.sh` against generated projects — offline, property-tested —
  and `shellcheck` statically gates the installer and the release scripts.
  Recipes report honestly when there is nothing to measure: `cov`/`crap`
  probe cargo metadata for targets (so workspace members are still
  measured), and `mutants-diff` says when a change is outside its reach
  instead of measuring a placeholder. What adopters receive is unchanged
  and test-enforced.

### Fixed

- The installed CI workflow no longer carries Keeler's own repository jobs.
  `install.sh` copied `.github/workflows/ci.yml` verbatim, so every adopting
  project inherited the `version`, `installer` and `installer-bootstrap`
  jobs — which read `VERSION`, `CHANGELOG.md` and `./install.sh`, none of
  which exist in a user's project — and went red on its first push. The
  shipped workflow now comes from `templates/keeler.yml` and runs only the
  gates: lints, test, coverage + CRAP, mutation testing.
- Upgrades no longer print a conflict note with no filename in it
  (`·  differs — wrote .keeler, merge by hand`). The rules file is now
  installed on its own terms rather than being installed like every other
  file and then undone, which is what left an empty entry in the list of
  conflicts to report.
- Upgrading no longer discards local edits to `.claude/keeler.md` without a
  trace. The rules file is still Keeler's to replace — that is how rule
  changes reach existing projects — but the copy it replaces is now kept as
  `.claude/keeler.md.bak`. The README said "nothing of yours is overwritten",
  which was not true of this one file; it now says what actually happens.
- An existing `.github/workflows/keeler.yml` is no longer left untouched on
  re-install. It now follows the same rule as every other installed file:
  identical means silence, different means the new workflow lands alongside
  as `keeler.yml.keeler` to merge by hand. Projects were previously stranded
  on whatever workflow they first installed, so no gate fix ever reached
  them — including the one above.
- The shipped `test` job no longer runs `cargo test --doc` directly, which
  failed with "no library targets found" in binary-only projects. Every job
  now goes through the `just` recipes, so CI runs what `just dev` runs.

[Unreleased]: https://github.com/minikin/keeler/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/minikin/keeler/releases/tag/v0.1.0
