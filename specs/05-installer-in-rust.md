# Spec 05 — The installer becomes Rust, and the shell ends

**Status:** Approved
**Effort:** Large
**Module:** `keeler/`, `xtask/`, `.github/workflows/release.yml`, `tests/`

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

The binary grows one capability at a time while `install.sh` still stands,
so every step leaves the suite green and the two can be compared against
each other on real input — the same discipline that made spec 04 safe.
Only when the binary does everything does the switch happen: the suite
re-points at the library, the shell goes, and the release learns to ship a
binary.

The crate is `keeler/` — bin plus lib, `publish = true`. Not `xtask`: that
one is repository machinery and unpublishable by definition, while this is
the product.

- [ ] **T1 — The crate, the embedded tree, and both directions of the set.**
      Scenarios: _What the binary carries is what the repository holds_,
      _Adopters receive nothing of the repository's own_. Tests:
      acceptance — the embedded set equals the repository's shipped set,
      failing by name in either direction (a shipped file not embedded, or
      something embedded that adopters must never see); `keeler init` into
      an empty Rust project lays every one of them down. Deliverable:
      `keeler/` with `include_dir!`. Deps: none.
- [ ] **T2 — A project's own content survives.**
      Scenarios: contributes to _Every spec 01 scenario holds_ (T5).
      Tests: acceptance — a pre-existing file with its own content is kept
      and Keeler's copy lands as `<name>.keeler`; the rules file is the
      documented exception, replaced with its `.bak`. Property — conflict
      totality: the set of `.keeler` files written equals the set
      reported; own-content preservation: for any pre-existing file
      outside the three append targets, the bytes are unchanged. Deps: T1.
- [ ] **T3 — The manifest and the .gitignore.**
      Scenarios: contributes to _Every spec 01 scenario holds_ (T5).
      Tests: acceptance — proptest added as a dev-dependency, detected in
      both declaration forms (`proptest = …` and
      `[dev-dependencies.proptest]`) and not confused with
      `proptest-derive`; `[profile.mutants]` and `[lints.clippy]` appended
      when absent; a workspace root told to manage its own manifest; a
      `.gitignore` with no final newline still converges. Property —
      idempotence: `init(init(x))` leaves the tree byte-identical;
      `.gitignore` merging: no entry is duplicated under any equivalent
      form (`/target`, `target/`, `target`). Deps: T1.
- [ ] **T4 — The tools, and the refusal.**
      Scenarios: contributes to _Every spec 01 scenario holds_ (T5).
      Tests: acceptance — missing tools are installed and present ones
      skipped, `--no-tools` installs none, and a directory without a
      `Cargo.toml` is refused before anything is written. The tool calls
      stay process orchestration, so this is the one place a stub `cargo`
      still earns its keep. Deps: T1.
- [ ] **T5 — The switch: the suite drives the library, install.sh goes.**
      Scenarios: _Every spec 01 scenario holds against the binary_,
      _Laying the files down needs no network_. Tests: `tests/installer.rs`
      keeps every scenario name and stops spawning processes — fixtures
      become directories the library is pointed at, and the properties run
      at proptest's normal case counts instead of the dozen a
      subprocess-per-case budget allowed. Offline: no fetch, clone or
      download remains in the crate, and a full run leaves the network log
      untouched. Deliverable: `install.sh` deleted. Deps: T2, T3, T4.
- [ ] **T6 — The checker becomes xtask, and the last shell goes.**
      Scenarios: _The contract checker runs without a shell_, _No shell
      remains in the repository_. Tests: `cargo xtask integration-check`
      asserts the same contract, installing by calling the library rather
      than spawning anything; `tests/wild.rs` follows it; the repository
      contains no `.sh` file, the lint gate no longer runs shellcheck, and
      the real-world CI job calls the xtask command. Deliverable:
      `scripts/` deleted. Deps: T5.
- [ ] **T7 — The release ships a binary, and a version can be pinned.**
      Scenarios: _A release publishes what each platform needs_, _An
      adopter installs and pins a version_. Tests: static over
      `release.yml` — a build matrix covering the supported targets, each
      binary attached with the checksum `cargo xtask checksum` produces,
      the crate published, and the guard and gates still strictly before
      publication; acceptance — the version the binary reports is the one
      it writes into the rules-file marker, so a pinned install is
      self-evident. Deps: T6.

**Two gaps found while breaking this down — both need approval.**

The spec changes how Keeler is installed but says nothing about the two
places that tell people how to install it.

1. The shipped `Justfile` has a `keeler-upgrade` recipe that runs
   `curl … install.sh | bash -s .`. After T5 that URL 404s and every
   adopter's upgrade path breaks silently. Proposed scenario:

```
### Scenario: The upgrade path works after the installer moves

Given a project with Keeler installed
When  the shipped upgrade recipe runs
Then  it fetches the current release through cargo, not curl
And   the rules-file marker afterwards names the version it fetched
```

2. `README.md` documents `curl … | bash` as *the* way in, and spec 02's
   scenario requires the verify story to live "where adopters look".
   Proposed scenario:

```
### Scenario: The documented way in is the one that works

Given the documentation an adopter reads first
When  the install instructions are followed literally
Then  they use the published crate, and no instruction names install.sh
```

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
