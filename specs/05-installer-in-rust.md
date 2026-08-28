# Spec 05 — The installer becomes Rust, and the shell ends

**Status:** Retired
**Effort:** Large
**Module:** `keeler/`, `xtask/`, `.github/workflows/release.yml`, `tests/`

## Why this spec is retired

The migration was attempted, measured, and found to cost more than it
bought. Four tasks of eight were built on `feat/installer-in-rust`:
248 lines of working shell became ~880 lines of product Rust plus ~1,500
of tests, and along the way the rewrite introduced three classes of
defect the shell version never had — one of them destroying a project's
own file while reporting success. Every one was caught by review; none
by the 165 green tests, 111 killed mutants or the clean CRAP run. The
gates this spec promised to extend were measuring the code we wrote, not
the promises the installer makes.

An installer is thirty careful copies — shell's native genre and Rust's
worst one. `install.sh` stays: it is exercised by 26 subprocess
scenarios against real projects on every CI run, and its weakness (it
sits outside the mutation gate) has cost one sed-dialect bug in the
project's life. The full record, including what to test first if anyone
ever resumes this, is `HANDOVER.md` on the parked branch.

## Context

Spec 04 moved the release logic out of shell and the result was not subtle:
the gates went from having nothing to measure to 41 mutants caught, and
four defects surfaced in a week that shell had hidden for months — a test
gate that ran nothing, a CRAP gate that died, a mutation gate that measured
nothing, and a `main` that could report success while failing.

What remains outside the gates is the part that matters most. `install.sh`
is 248 lines and is the whole product: everything an adopter ever sees.
`scripts/integration-check.sh` is another 219. Neither is mutated, neither
is covered, and both are dialect-fragile — a BSD/GNU `sed` difference broke
a "working" installer during development.

The cost is also paid in the tests. `tests/installer.rs` is 1323 lines that
drive the installer as a subprocess 26 times; those are the slowest tests
in the suite (9.1s, 4.0s, 3.9s) and the reason its property tests run 12
cases instead of 256. Spec 04 showed what changes when logic becomes
library: the CHANGELOG property went from 12 subprocess cases to 256
in-process ones, in 0.01s.

**What changes.** The installer becomes a published Rust binary. Its
content — the commands, the skills, `KEELER.md`, the `Justfile`, the
workflow template — is embedded in the binary with `include_dir!`, so
laying files down needs no network and no tarball at all. The contract
checker becomes `cargo xtask integration-check`, calling the installer's
library directly instead of spawning a shell. `install.sh` and
`scripts/integration-check.sh` are deleted; `scripts/` goes with them.

**Spec 01 is the oracle.** Every scenario it defines — completeness,
conflicts kept as `<name>.keeler`, the project's own content untouched,
idempotence, the rules file replaced with a `.bak`, the refusal of a
non-Rust directory — must pass unchanged against the binary. This spec adds
scenarios only for what is genuinely new: embedding, distribution, and the
end of shell. The same discipline spec 04 applied to spec 02.

**Distribution: crates.io, plus prebuilt binaries.** This is what the Rust
ecosystem does for a cargo-adjacent developer tool, and Keeler already
depends on the mechanism: every tool it installs — `cargo-nextest`,
`cargo-binstall`, `just`, `cargo-llvm-cov`, `cargo-mutants` — is published
there, and `install.sh` uses `cargo binstall` to fetch them. Publishing
makes Keeler a participant in the scheme it relies on: `cargo binstall
keeler` takes the prebuilt binary from the release, `cargo install keeler
--locked` compiles when no binary matches.

`curl | bash` is right for a tool that bootstraps the toolchain (`rustup`)
or whose users may not have Rust at all (`ripgrep`). Neither is true here:
the installer's first act is to refuse a directory without a `Cargo.toml`,
so cargo is present by definition. Dropping it is a breaking change to a
documented interface, and acceptable only because the project has no users
yet — that window is the reason to do this now rather than later.

**Rejected alternatives.** *A thin `curl | bash` loader that downloads and
execs the binary*: keeps the familiar one-liner and would make the release
checksum load-bearing, but leaves shell in the repository — which is the
one thing this spec exists to end. *`cargo-dist`*: automates the build
matrix, checksums and installers, and is worth revisiting — but it takes
over the release workflow built in specs 02 and 04, which has not yet run
on a real tag. Adopting it now would replace something unproven with
something unproven. *Keeping `install.sh` and rewriting only the checker*:
the checker's job is to run the installer, so it would stay a wrapper
around bash and gain nothing.

**v0.1.0 stays.** Recreating it was raised and is not worth doing: spec
02's scenario says a published release is never overwritten, and that
invariant is worth more than a tidy version history. v0.2.0 supersedes it.

---

## Acceptance Tests

### Scenario: Every spec 01 scenario holds against the binary

```
Given the scenarios spec 01 defines for the installer
When  each is exercised against `keeler init` instead of install.sh
Then  every one passes unchanged — completeness, conflicts kept as
      <name>.keeler, the project's own content untouched, idempotence,
      the rules file replaced with its .bak, a non-Rust directory refused
```

### Scenario: Laying the files down needs no network

```
Given a machine with no network access
When  `keeler init` runs against a Rust project, asking for no tools
Then  every file lands, from content carried inside the binary
And   nothing is fetched, downloaded or cloned
```

### Scenario: What the binary carries is what the repository holds

```
Given a file that the repository ships to adopters
When  the binary is built
Then  that file is embedded in it
And   a file added to the shipped set without being embedded fails the gate
```

### Scenario: The contract checker runs without a shell

```
Given a project directory
When  `cargo xtask integration-check` runs against it
Then  it asserts the same contract the shell checker did
And   it installs by calling the library, not by spawning a process
```

### Scenario: No shell remains in the repository

```
Given the repository after the migration
When  its files are listed
Then  there is no .sh file anywhere
And   the lint gate no longer runs shellcheck
And   coverage, CRAP and mutation reach every line of the deliverable
```

### Scenario: A release publishes what each platform needs

```
Given a pushed tag that the guard accepts
When  the release workflow runs
Then  a prebuilt binary is attached for each supported platform, each with
      its checksum
And   the crate is published to crates.io
And   the gates still strictly precede publication
```

### Scenario: An adopter installs and pins a version

```
Given a published release
When  an adopter installs the pinned version and runs `keeler init`
Then  the rules file records that version in its marker
And   the version the binary reports matches the tag that produced it
```

### Scenario: Adopters receive nothing of the repository's own

```
Given a project the binary has initialised
When  its files are listed
Then  no workspace member, xtask alias or repository workflow is among them
And   nothing installed describes the Keeler repository
```

---

## Tasks

_Empty until /keeler:tasks runs against an approved spec._

---

## Implementation Notes

**Shape.** A third workspace member, `keeler/` — a bin plus a lib,
`publish = true`. Not `xtask`: that crate is repository machinery and
`publish = false` by definition, while this is the product. The bin is
`keeler init [path] [--no-tools]`; the library is where every decision
lives, so the tests and the mutation gate reach it without a subprocess.

**Embedding.** `include_dir!` carries the shipped tree — `.claude/commands`,
`.claude/skills`, `.claude/keeler.md`, `KEELER.md`, `Justfile`,
`specs/TEMPLATE.md`, `clippy.toml`, `rustfmt.toml`, `.cargo-mutants.toml`,
and `templates/keeler.yml` as `.github/workflows/keeler.yml`. This removes
the tarball fetch, the `KEELER_TARBALL` override and the local-checkout
sentinel in one move: there is no "source" to find, because the binary is
the source. The embedded-vs-repository drift is a gate, not a hope — the
completeness scenario above.

**Tests.** `tests/installer.rs` keeps its scenario names and stops spawning
processes: fixtures become directories the library is pointed at. The
existing properties (idempotence, own-content-never-overwritten, every
conflict named) survive as properties over the same invariants, but at
proptest's normal case counts rather than the dozen a subprocess-per-case
budget allowed. `tests/wild.rs` follows the checker into xtask.

**Tools.** Installing `cargo-nextest` and friends still shells out to
`cargo`/`cargo binstall`, because that is what installing them means. That
is process orchestration with an exit code to check, not logic — and it is
the one place where a stub in the tests still earns its keep.

**Release.** The workflow gains a build matrix — `x86_64` and `aarch64`,
Linux and macOS — attaching each binary with the checksum `cargo xtask
checksum` already produces, then publishes the crate. The guard and `just
ci` keep their place strictly before anything is published.

**Invariants worth property tests.** Idempotence (`init(init(x))` leaves
the tree byte-identical), own-content preservation (for any pre-existing
file outside the three append targets, the bytes are unchanged), conflict
totality (the set of `.keeler` files equals the set reported), and
`.gitignore` entry merging (no duplicate under any equivalent form —
`/target`, `target/`, `target`).

### Non-goals

- Windows. The shipped `Justfile` and gates are not Windows-ready; a
  cross-platform binary does not change that, and claiming support without
  a CI job proving it would be a lie.
- `cargo-dist`. Its own decision, once the current release machinery has
  run on a real tag.
- Package managers — Homebrew, AUR, nixpkgs.
- Keeping a `curl | bash` entry point, in any form.
- Recreating or replacing the v0.1.0 release.
