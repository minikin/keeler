# Spec 01 — The deliverable is gated

**Status:** Implemented
**Effort:** Medium
**Module:** `install.sh`, `tests/installer.rs`, `Justfile`, `.github/workflows/ci.yml`

## Context

Keeler's deliverable is `install.sh` and the files it ships: the slash
commands, the skills, `specs/TEMPLATE.md`, `KEELER.md`, `Justfile`, the gate
configs and `templates/keeler.yml`. None of that is Rust.

Every gate this repository runs on itself points at `src/lib.rs`, a 14-line
placeholder (`checked_total`) that is never shipped to anyone — the installer
copies no `src/` and no `tests/`. The placeholder exists only so the gates
have something to measure.

The result is a signal that cannot fail. Four defects in the deliverable were
found and fixed in a single session — a workflow shipped to adopters that
referenced files only this repository has, an upgrade path that could never
deliver a fix, an upgrade that destroyed local edits with no backup, and a
conflict report naming no file. Across all four, `just dev` and
`just mutants-diff` reported PASS: the first three said nothing about shell
at all, and the mutation gate reported `No changed src lines`, mutated the
placeholder instead, and exited green. A green that cannot go red is worse
than a missing gate, because it reads as evidence.

For a project whose entire argument is that a defect rarely survives
independent gates, the argument does not currently apply to its own code.

What changes: the crate is inverted. `src/lib.rs` and `tests/acceptance.rs`
are deleted, `Cargo.toml` and `tests/` remain as the harness where the
product's tests live, and the gates this repository runs measure the
deliverable — statically via `shellcheck`, behaviourally via a test suite
that drives `install.sh` over generated project states. Signals that measure
nothing say so instead of passing.

**Constraint that shapes the design:** the `Justfile` is itself a shipped
artifact, and this repository runs the same copy it ships. Removing `src/`
therefore cannot be done by editing the recipes to suit this repository —
the recipes must handle a project with no Rust sources honestly, which is
also correct for a workspace root.

**Constraint on the property tests:** every case runs the installer as a
subprocess, so case counts stay low and generated projects must not force the
installer onto the network.

**Rejected alternatives.** Coverage for shell via `bats` + `kcov`: kcov is
effectively Linux-only, and no usable mutation testing exists for bash, so it
can never deliver Keeler's strongest gate for the deliverable — and none of
the four defects needed line coverage to be caught. Rewriting the installer
in Rust: the only option that puts the deliverable under every gate, but it
changes distribution, release machinery and the `curl | bash` story, and is a
separate decision. The properties specified here survive that rewrite intact,
because they describe observable behaviour rather than implementation.

---

## Acceptance Tests

### Scenario: A defect in the shipped workflow fails the gate

```
Given install.sh ships a CI workflow that references a file which exists
      only in the Keeler repository
When  the quality gate runs
Then  it fails
And   the report names the offending file and the reference it found
```

### Scenario: A statically detectable shell defect fails the gate

```
Given install.sh contains an unquoted expansion that can split on whitespace
When  the quality gate runs
Then  it fails
And   the report names the file and line
```

### Scenario: A change touching no Rust is still measured

```
Given a change that edits only install.sh and the files it ships
When  the quality gate runs
Then  the installer test suite runs against that change
And   no gate reports a pass for code it did not examine
```

### Scenario: Mutation testing reports what it did not measure

```
Given a change that touches no file under src/
When  the mutation gate runs
Then  it reports that the change lies outside its reach
And   it does not report surviving-mutant counts as evidence for that change
```

### Scenario: Coverage and CRAP recipes are honest about a project with no Rust targets

```
Given a project whose crates define no library or binary targets
When  the coverage, CRAP, baseline or delta recipe runs
Then  it reports that there are no Rust sources to measure
And   it does not fail the build for the absence
```

### Scenario: A workspace with Rust targets is still measured

```
Given a workspace whose member crates define library or binary targets
When  the coverage recipe runs
Then  it measures them
And   it does not report the sources as missing
```

### Scenario: A piped upgrade never mistakes the project for the source

```
Given a project that already contains the files Keeler ships
When  the installer runs piped from stdin inside that project
Then  it fetches the Keeler source
And   it does not treat the project's own files as the thing to install
```

### Scenario: An adopter's install.sh is not Keeler's to gate

```
Given an adopting project with a script of its own named install.sh
When  the lint recipe runs there
Then  shellcheck is not applied to it
```

### Scenario: Committed src changes on a branch stay measured

```
Given a branch whose earlier commits changed src/ and whose latest commit
      did not
When  the mutation gate runs
Then  it mutates the changed lines against the branch base
```

### Scenario: Installing twice leaves the second run with nothing to do

```
Given any project state the installer accepts
When  the installer runs, and then runs again
Then  every file in the project is byte-identical to its state after the
      first run
And   the second run reports that it installed no files
```

### Scenario: A project's own content is never overwritten

```
Given a project containing a file Keeler installs, with content of its own
When  the installer runs
Then  that file's content is unchanged
And   the version Keeler would have written is available alongside it as
      <name>.keeler
And   the run names that file in its conflict report
```

### Scenario: The rules file is replaced, and the replaced text is kept

```
Given a project whose .claude/keeler.md differs from the shipped rules
When  the installer runs
Then  .claude/keeler.md matches the shipped rules
And   the text it replaced is available as .claude/keeler.md.bak
And   no .claude/keeler.md.keeler is left behind
```

### Scenario: Every conflict is reported by name

```
Given a project with several files that conflict with what Keeler installs
When  the installer runs
Then  it names every conflicting file
And   it reports no conflict without a filename
```

### Scenario: The installer refuses a directory that is not a Rust project

```
Given a directory with no Cargo.toml
When  the installer runs against it
Then  it exits with a non-zero status
And   it explains that the directory is not a Rust project
And   it creates no files there
```

### Scenario: The property suite runs without network access

```
Given generated project states
When  the property suite runs with no network available
Then  every case completes
```

### Scenario: What adopters receive describes their project, not ours

```
Given a freshly installed project
When  the files Keeler put there are read
Then  none of them describes the Keeler repository's own internals — its
      test files, its spec numbers, its divergences from the rules it ships
```

### Scenario: An adopting project still receives every gate

```
Given a fresh Rust project
When  Keeler is installed into it
Then  the installed workflow runs lints, tests, coverage, the CRAP gate and
      mutation testing
And   the installed Justfile still provides the cov, crap, crap-baseline,
      crap-delta and mutants recipes
```

### Scenario: Every workflow file in the repository is installed

```
Given the repository's command and skill trees
When  Keeler is installed into a fresh project
Then  every file of every kind under those trees exists in the project
And   a path merely mentioned in an installer comment does not count as
      shipped
```

### Scenario: The source-compile fallback is exercised deterministically

```
Given a CI run that forces binstall's compile strategy with nextest absent
      and no tool cache to satisfy the probe
When  the installer runs
Then  it reports installing nextest through its own tool path
And   the compiled tool works
```

### Scenario: The repository presents no placeholder to replace

```
Given a clone of the Keeler repository
When  its Rust sources are listed
Then  there is no source file whose only purpose is to be measured
And   Cargo.toml describes a test harness for the installer
```

---

## Tasks

- [x] **T1 — `cov` and `crap` recipes are honest about a project with no
      Rust sources.**
      Scenarios: _Coverage and CRAP recipes are honest about a project with
      no Rust sources_. Tests: acceptance — run the shipped `Justfile`'s
      `cov` and `crap` recipes in a generated project without `src/`; they
      report the absence and exit 0. Deps: none.
- [x] **T2 — `mutants-diff` says when a change is outside its reach.**
      Scenarios: _Mutation testing reports what it did not measure_.
      Tests: acceptance — in a generated git repo with only non-`src`
      changes, `just mutants-diff` reports the change is out of reach and
      does not run mutants (stub `cargo` proves it). Deps: none.
- [x] **T3 — `shellcheck` gates the installer.**
      Scenarios: _A statically detectable shell defect fails the gate_.
      Tests: acceptance — a fixture script with an unquoted expansion fails
      shellcheck with file and line; `just lint` runs shellcheck over the
      repo's shell scripts only when any are present (a project with none —
      every adopter — skips it, keeping the shipped gates unchanged);
      `install.sh` itself comes out clean. Deps: none.
- [x] **T4 — What adopters receive is pinned: every gate, no repo-only
      references.**
      Scenarios: _A defect in the shipped workflow fails the gate_, _An
      adopting project still receives every gate_. Tests: acceptance —
      extend the shipped-workflow test to assert all four gate jobs and the
      absence of repo-only paths (naming file and reference on failure), and
      parse the shipped `Justfile` for the `cov`, `crap`, `crap-baseline`,
      `crap-delta` and `mutants` recipes. Deps: none.
- [x] **T5 — The crate is inverted: the placeholder goes, the harness
      stays.**
      Scenarios: _The repository presents no placeholder to replace_, _A
      change touching no Rust is still measured_. Tests: acceptance — repo
      shape: no `src/`, no `tests/acceptance.rs`, `Cargo.toml` framed as the
      installer's test harness, `ci.yml` carries no coverage/CRAP/mutation
      job aimed at a placeholder and its test job runs the harness. Includes
      the README/`KEELER.md` touch-ups this makes true. Deps: T1–T4.

      `crap-baseline.json` also went at this point, and was read as evidence
      of the placeholder while there was nothing else to measure. That
      stopped being true in spec 04: the xtask crate is real code with a
      real job, so the baseline is back deliberately and the check that
      carries this scenario is the absence of `src/`.
- [x] **T6 — Idempotence, as a property.**
      Scenarios: _Installing twice leaves the second run with nothing to
      do_. Tests: property (8–16 cases) — invariant: for any accepted
      generated project state, `install(install(x)) == install(x)` by full
      tree hash, and the second run reports zero files installed. Builds the
      generator + tree-hash harness the later properties reuse. Deps: T5.
- [x] **T7 — Never-overwrite, as a property.**
      Scenarios: _A project's own content is never overwritten_. Tests:
      property — invariant: any pre-existing file from the install set with
      arbitrary content survives byte-for-byte and `<name>.keeler` appears.
      Deps: T6.
- [x] **T8 — The rules file is the documented exception.**
      Scenarios: _The rules file is replaced, and the replaced text is
      kept_. Tests: acceptance — a differing `.claude/keeler.md` is
      replaced, the old text lands in `.claude/keeler.md.bak`, no
      `.claude/keeler.md.keeler` is written. Deps: T6.
- [x] **T9 — Conflict reports equal the conflicts, as a property.**
      Scenarios: _Every conflict is reported by name_. Tests: property —
      invariant: the set of files named in the conflict report equals the
      set of `.keeler` files written; no report line lacks a filename.
      Deps: T7.
- [x] **T10 — A non-Rust directory is refused, untouched.**
      Scenarios: _The installer refuses a directory that is not a Rust
      project_. Tests: acceptance — no `Cargo.toml` → non-zero exit, an
      explanation, and a byte-identical directory. Deps: T6 (tree hash).
- [x] **T11 — The suite proves it never reaches the network.**
      Scenarios: _The property suite runs without network access_. Tests:
      acceptance — the harness puts failing `curl` and logging `cargo`
      stubs on `PATH` for every case; a sweep over the generated states
      completes with no network tool invoked. Deps: T6–T9.
- [x] **T12 — A piped upgrade fetches the source.** (review finding 2)
      Scenarios: _A piped upgrade never mistakes the project for the
      source_. Tests: acceptance — pipe the installer into bash inside an
      already-Keelered project; it must reach for the source (the stubbed
      curl refuses) instead of silently no-opping. Deps: T11.
- [x] **T13 — The no-sources probe asks cargo, not the filesystem.**
      (review finding 3; amends T1) Scenarios: the reworded _no Rust
      targets_ scenario and _A workspace with Rust targets is still
      measured_. Tests: acceptance — a workspace with a member crate runs
      the coverage recipe for real (stub cargo delegates `metadata`).
      Deps: T1.
- [x] **T14 — shellcheck is keyed to this repository.** (review finding 4;
      amends T3) Scenarios: _An adopter's install.sh is not Keeler's to
      gate_. Tests: acceptance — an adopter project with its own install.sh
      lints clean without shellcheck; the defect scenario now carries the
      repo marker. Deps: T3.
- [x] **T15 — mutants-diff diffs against the branch base and quotes its
      paths.** (review findings 5, 9; amends T2) Scenarios: _Committed src
      changes on a branch stay measured_. Tests: acceptance — src change
      two commits back with a docs-only tip still runs mutants; an
      untracked src file with a space in its name does not break the gate.
      Deps: T2.
- [x] **T17 — The guards that shipped as a chore get their scenarios.**
      (review finding 10 of the quick-wins branch — retroactive coverage,
      approved) Scenarios: _Every workflow file in the repository is
      installed_ (owned by the same-named harness test, which asserts on
      the installed tree) and _The source-compile fallback is exercised
      deterministically_ (owned by CI's `installer-locked-fallback` job —
      like the bootstrap job, it is observable only in CI). Deps: none.
- [x] **T16 — Housekeeping the review demanded.** (findings 1, 6, 7, 8, 10)
      CI test job runs `just test` (doc-test guard) and provisions
      shellcheck; the shellcheck defect test asserts on SC2086; the proptest
      seed file is committed and its comment names the real path;
      TempProject::git pins the global config away; the approved
      divergence note is recorded in this repository's own CLAUDE.md, not
      in the rules file it ships. Deps: none.
- [x] **T18 — The deliverable is scanned for talk about us.**
      Scenarios: _What adopters receive describes their project, not ours_.
      Tests: acceptance — a prose scanner over every installed file, naming
      file, line and marker on failure; pinned by a synthetic defect so the
      scan can fail, and run over a real installed tree. Markers are prose
      about this repository ("Keeler repository", "documented divergence",
      `tests/installer`, "spec 0N"), never mechanism: the shellcheck branch
      keyed on `templates/keeler.yml` and the upgrade URL are legitimately
      in the shipped Justfile. Deps: none.

---

## Implementation Notes

**Shape.** `Cargo.toml` keeps `proptest` as a dev-dependency and loses its
"replace it with your code" framing — the package is the installer's test
harness. `src/lib.rs` and `tests/acceptance.rs` are deleted;
`tests/installer.rs` grows into the suite. `[lints.clippy]`, `clippy.toml`,
`rustfmt.toml` and `.cargo-mutants.toml` stay: they are shipped artifacts and
still gate the harness code itself.

**Driving the installer.** Cases build a project directory, run `install.sh
<dir> --no-tools` as a subprocess, and assert on the resulting tree and the
run's output. Generated manifests declare `proptest` under
`[dev-dependencies]` so the installer's `cargo add` path is never taken and
no case reaches the registry. `--no-tools` keeps tool installation out of
scope.

**Invariants worth a property test** — generated over which files pre-exist
and what they contain:

- _Idempotence_ — `install(install(x)) == install(x)`, compared as a full
  tree hash, not a file list.
- _Never-overwrite_ — for any pre-existing file in the install set with
  arbitrary content, the content survives and `<name>.keeler` appears. The
  rules file is the single documented exception, with `.bak` as its
  compensating guarantee.
- _Conflict reporting_ — the set of files reported as conflicting equals the
  set of `.keeler` files written; no report entry lacks a filename.

Keep cases at 8–16: each one runs the installer.

**Honest signals.** `mutants-diff` distinguishes "no mutants survived" from
"nothing in reach was changed", and says which. The `cov` and `crap` recipes
detect a project with no Rust sources and report that instead of failing on
a missing path — correct for this repository and for a workspace root.

**Static gate.** `shellcheck` runs against `install.sh` in the lints job and
in `just lint`. Whether it flags the dead `grep -q | grep` construct at
`install.sh:175` (item 4) is **unverified** — if it does not, that defect
needs a harness test of its own rather than being assumed covered.

**CI.** This repository's coverage, CRAP and mutation jobs are removed with
`src/`; the installer E2E jobs and the version-consistency job stay, and the
test job now runs the harness. `crap-baseline.json` and its delta gate go
with them — there is nothing here to baseline. (Spec 04 ended that: with
the xtask crate in the workspace there is, so the baseline and the delta
gate came back.) None of this touches `templates/keeler.yml`, which is what
adopters receive.

### Non-goals

- Rewriting `install.sh` in Rust. Separate decision, separate spec.
- Coverage or mutation testing for shell (`bats`, `kcov`) — see rejected
  alternatives.
- Shipping an example crate, or any change to what adopters receive. The
  gates users get must come out of this unchanged.
- Unit-testing the tool-installation path (`cargo binstall`, `rustup
  component`) in the harness. It is covered in CI by the bootstrap job and
  the forced source-compile fallback job (see _The source-compile fallback
  is exercised deterministically_).
- Windows support for the installer.
