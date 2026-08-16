# Changelog

All notable changes to Keeler are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions
follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Installations pin a version with `KEELER_REF` and record it at the top of
`.claude/keeler.md`.

## [Unreleased]

## [0.3.0] — 2026-08-16

Everything Keeler had never reviewed, reviewed. Three areas — the
installer, the release tooling, the contract checker — went through a
review stage that had been skipped for every task in specs 01 through 04.
It found 37 defects. Two of them destroyed adopters' work.

### Fixed

- **The installer left projects unbuildable.** A `Cargo.toml` with
  `[lints] workspace = true` — the standard modern idiom — had
  `[lints.clippy]` appended beside it. Cargo refuses to let a manifest
  override inherited lints, so every cargo command in the project failed
  afterwards, the installer reported success, and running it again did not
  repair the file.
- **The installer wrote outside the project.** `[ -e ]` is false for a
  symlink whose target does not exist, so a `KEELER.md` symlinked
  elsewhere had Keeler's copy written *through* the link — outside the
  destination, which is the one boundary the installer promises to keep —
  counted as installed and reported as no conflict.
- Nothing parsed `Cargo.toml` after editing it. It is read with cargo
  before and after now, and a manifest broken by the edits is restored
  rather than shipped.
- A failed `cargo add` was swallowed: the run printed "Keeler installed",
  exited 0, and left a project with no proptest whose first `just dev`
  cannot compile.
- A project's own `.keeler` and `.bak` files were overwritten, so a second
  upgrade destroyed the first one's backup — the original text, which is
  the only reason that backup exists.
- The proptest check read the whole manifest, so a `[workspace.dependencies]`
  entry counted as a dev-dependency and the package could not compile its
  property tests. It is scoped to `[dev-dependencies]` and understands
  every declaration form, including `proptest.workspace = true`.
- `--help` aborted on the documented `curl | bash` path, and an
  unrecognised flag silently became the destination directory.
- An explicit `KEELER_REF` was ignored when the script was piped from
  inside a Keeler clone, installing the checkout while the caller believed
  they had pinned a version.

### Changed

- **The release refuses more, and lies less.** An empty CHANGELOG section
  used to publish a release with blank notes that the create-only policy
  could not repair; a workspace with inherited versions had *zero*
  manifests checked and reported as agreement; a `##` inside a fenced code
  block truncated the notes; a comment beside `version =` produced a
  garbled refusal. A release candidate is no longer published as the
  latest release, the release resolves with `--locked` as its own comment
  claimed, and two pushes of one tag cannot race.
- **The contract checker stopped being green and blind.** It verified that
  files existed and that a second run changed nothing, and almost nothing
  about what was installed: an installer that wiped `.gitignore`, left an
  unparseable manifest, kept no conflicts, installed empty files, added
  scratch files, deleted the lockfile or wrote through a symlink all
  passed. The reference tree is kept now and used as the oracle.
- **The pipeline leads through its own review stage.** `/keeler:tdd` ended
  by naming the next *task* rather than the next *stage*, so working task
  by task the review never came up — which is how twenty tasks skipped it
  unnoticed. Each stage names its successor, and the task checkbox is
  ticked by `/keeler:mutants`, the only stage that can honestly say the
  pipeline ran.
- `cargo xtask release-guard` checks the crate versions too, in every
  workspace member and in `[workspace.package]`. They agreed before only
  by coincidence.

### Documentation

- Keeler says which agent it is for. The README and `KEELER.md` described
  a workflow for "projects built with AI" while everything installed is
  Claude Code's own furniture: slash commands under
  `.claude/commands/keeler/`, skills beside them, and a rules file
  imported by `CLAUDE.md`. Another agent finds none of it. The method is
  portable and the gates are plain `cargo` and `just`, which anyone can
  run — but the pipeline assumes Claude Code, and the documentation now
  says so instead of implying otherwise.

### Known

- The tarball the installer fetches is not checksummed, and
  `KEELER_TARBALL` will substitute any URL. `SECURITY.md` says so;
  closing it means publishing per-release archive digests.

## [0.2.0] — 2026-08-15

### Added

- `cargo xtask` — a repository task runner (bin + lib, never published,
  never installed into an adopting project). Spec 04 moves the release
  logic into it, out of shell.
- `cargo xtask release-notes <version> <changelog>` — the CHANGELOG parser
  in Rust, byte-for-byte identical to the awk it will replace. Its
  extraction-totality property now runs in-process, so it checks 256 cases
  in the time the subprocess version needed for 12.
- `cargo xtask checksum <file>` — a pure-Rust SHA-256, ending the
  `sha256sum`-or-`shasum` portability dance. Its output is byte-identical
  to the script it replaces and verifies with the real `shasum -a 256 -c`.
  This adds the repository's first dependency, `sha2`, pinned by the
  committed lockfile.
- `cargo xtask release-guard <tag>` — refuses a tag that disagrees with
  VERSION, the rules-file marker or the CHANGELOG, and now names *every*
  disagreement instead of stopping at the first, so one refusal is enough
  to fix everything that is wrong.


- `scripts/integration-check.sh` — the contract checker from spec 03. It
  installs Keeler into a project directory and asserts the installer's
  contract on it: exit zero, and every file a clean install produces
  present afterwards. The tracked set is derived from a reference install
  rather than listed, so it cannot drift from what `install.sh` does. CI
  will point it at pinned real-world clones; the script never clones, so
  the local suite stays offline.
- The checker holds the installer to the project's own content: every file
  the project already had must come out byte-identical, except the three
  documented append targets (`CLAUDE.md`, `.gitignore`, `Cargo.toml`), and
  the conflicts the installer reports must match the `.keeler` files on
  disk exactly — an unnamed one is a surprise, a named one that does not
  exist is a lie. A refreshed `Cargo.lock` is excused when, and only when,
  the manifest was edited: `cargo add` cannot add a dev-dependency without
  one, but a lockfile that moves with nothing behind it is still the
  project's loss.
- The checker installs a second time and requires the tree to be
  byte-identical afterwards. The second run has no exemptions: not one
  byte may move, including the three files the first run was allowed to
  append to.
- CI gained an `installer-real-world` job: it shallow-fetches
  `dtolnay/anyhow`, `serde-rs/serde` and `BurntSushi/ripgrep` at pinned
  commit SHAs — never a branch head, so nobody else's push can change what
  our CI tests — and runs the contract checker against each. Bumping a pin
  is a deliberate, reviewable diff.
- The checker also holds the installer to its workspace contract: a root
  with no `[package]` of its own must be told that its manifest is the
  project's to manage, since Keeler cannot add proptest and the mutants
  profile to member crates on its behalf.

### Changed

- The release runs through `cargo xtask`, not shell. `scripts/`
  release-notes.sh, checksum.sh and release-guard.sh are gone; the release
  workflow and CI's version job call the commands directly. Spec 02's
  acceptance suite kept every scenario name and now drives the binary, so
  the contracts are the same ones, verified the same way.

- A gate now scans every installed file for prose about the Keeler
  repository — its test files, spec numbers and divergences — and fails
  naming the file, line and marker. Two leaks got in by hand before this
  existed; the scan reproduces both. It matches prose, never mechanism, so
  the shipped `Justfile` may still name `templates/keeler.yml` and the
  upgrade URL.
- The shipped `Justfile` no longer explains Keeler's own repository to the
  projects it lands in. Its `lint` recipe carried a comment about the
  Keeler repository's shell deliverable, `templates/keeler.yml` and the
  release scripts — internals of no use to an adopter reading their own
  Justfile. The comment now says what the recipe does for *them*, and that
  the shellcheck branch is inert in their project.

- The shipped workflow rules no longer talk about Keeler itself. Notes
  about this repository's own divergences — a shell deliverable, per-area
  test files, no CRAP baseline — were sitting in `.claude/keeler.md`,
  which installs into every adopting project and should describe only the
  project it lands in. They now live in Keeler's own `CLAUDE.md`, the file
  the installer reserves for project-specific instructions.

### Fixed

- The mutation gate can see a workspace member. `cargo mutants` without
  `--workspace` reports "Found 0 mutants" for a member crate's file and
  passes having tested nothing, and `mutants-diff` watched only
  `src/*.rs`, which no member's sources match. Both are fixed, so the gate
  measures instead of reporting the change out of reach.

- The shipped gates no longer miss a workspace. `just test` ran only the
  root package, so in a project whose root manifest is itself a package a
  member crate's tests never ran at all; `just crap` hard-coded
  `--path src` and failed with "path does not exist" on any workspace root
  that has no `src/` of its own. Both now ask cargo where the code is
  (`--workspace`). Found while making Keeler's own repository a workspace —
  and spec 01's workspace scenario could not see it, because its test runs
  against a stub cargo. That test now pins the flag.

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

[Unreleased]: https://github.com/minikin/keeler/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/minikin/keeler/releases/tag/v0.3.0
[0.2.0]: https://github.com/minikin/keeler/releases/tag/v0.2.0
[0.1.0]: https://github.com/minikin/keeler/releases/tag/v0.1.0
