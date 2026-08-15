# Spec 04 — Release tooling moves to cargo xtask

**Status:** Approved
**Effort:** Medium
**Module:** `xtask/`, `.github/workflows/release.yml`, `Justfile`, `tests/release.rs`

## Context

The release machinery (spec 02) lives in three shell scripts. They are
small, shellcheck-gated and property-tested — but shell is the wrong
long-term home for logic in a Rust repository, and this project has felt
exactly why: a BSD/GNU sed incompatibility broke a "working" script
mid-session, and awk one-liners resist every gate except shellcheck. The
community standard for repo automation in Rust is the **xtask pattern**
(rust-analyzer, cargo itself): a plain Rust binary in the workspace,
invoked as `cargo xtask <command>`, no runtime dependencies beyond the
toolchain that is already there.

What changes: the three scripts become subcommands of one `xtask` crate —
`cargo xtask release-notes <version> <changelog>`,
`cargo xtask checksum <file>`, `cargo xtask release-guard <tag>` — with
**contracts identical to spec 02's scenarios**, which remain the oracle.
The release workflow calls xtask; `scripts/` goes away; the shell gate
shrinks back to `install.sh`.

The quiet prize: the repository regains real Rust sources. The mutation
gate — honestly idle since spec 01 because the deliverable was bash — gets
genuine targets again: a surviving mutant in the CHANGELOG parser means a
weak test, and `mutants-diff` on an xtask change actually measures it.
Coverage and CRAP likewise get something true to say.

**Why not fold the installer in now.** Rewriting `install.sh` in Rust is
the same trajectory but a far bigger decision (distribution, bootstrap,
`curl | bash` story — spec 01's rejected-alternatives note). This spec
deliberately moves only the repo-side tooling; the installer stays shell
until its own spec. When that spec comes, the xtask crate is the codebase
it grows from.

**Rejected alternatives.** Python/Deno scripts: a new runtime in CI for no
gain over the toolchain already present. Keeping shell and adding bats:
still outside the strongest gates, still dialect-fragile.

---

## Acceptance Tests

### Scenario: The xtask commands honor every spec 02 contract

```
Given the spec 02 scenarios for notes extraction, checksum and the guard
When  each is exercised against `cargo xtask` instead of the shell script
Then  every scenario passes unchanged — exact section, loud absence,
      checksum round-trip both ways, refusal of a lying tag
```

### Scenario: The release workflow speaks xtask

```
Given the release workflow
When  its steps are read
Then  guard, notes and checksum run through `cargo xtask`
And   no step invokes a script under scripts/
And   guard and gates still strictly precede `gh release create`
```

### Scenario: The shell gate covers exactly the shell that remains

```
Given the repository after the migration
When  its shell scripts are listed
Then  install.sh and the spec 03 contract checker are the only ones
And   the lint gate shellchecks exactly those two
And   no release logic remains in shell
```

### Scenario: The mutation gate is back in business

```
Given a change that touches xtask source
When  the mutation gate runs
Then  it mutates the changed lines instead of reporting them out of reach
```

### Scenario: Coverage and CRAP measure the xtask crate

```
Given the repository after the migration
When  the coverage recipe runs
Then  it measures the xtask sources
And   it no longer reports that there is nothing to measure
```

### Scenario: Adopters receive nothing new

```
Given a fresh Rust project
When  Keeler is installed into it
Then  no xtask crate, alias or workflow lands in the project
And   the shipped file set is unchanged
```

---

## Tasks

The three commands come first, one at a time, each with its own tests while
the shell scripts still stand — so every step leaves the suite green. Only
when all three exist does the switch happen: spec 02's acceptance suite
re-points at `cargo xtask`, the scripts go, and the workflow follows. The
gates come last, because they need real sources to measure.

The crate gets both a `lib.rs` and a `main.rs`: the binary is the CLI, the
library is what unit tests, property tests and cargo-mutants reach into
without paying a subprocess per case.

- [x] **T1 — Two members, an alias, and nothing new for adopters.**
      Scenarios: _Adopters receive nothing new_. Tests: acceptance — after
      `cargo xtask --help` works in this repository, an installed project
      contains no `xtask/`, no `.cargo/config.toml` and no new workflow,
      and the shipped file set is byte-for-byte what it was. Deliverable:
      `xtask/` (bin + lib, `publish = false`), the root manifest gaining
      `[workspace] members = ["xtask"]`, `.cargo/config.toml` with the
      alias. Deps: none.
- [x] **T2 — `cargo xtask release-notes`, the CHANGELOG parser in Rust.**
      Scenarios: none of its own — contributes to _The xtask commands honor
      every spec 02 contract_, owned by T5. Tests: unit beside the parser
      (exact section, absent version, the trailing link block, a link-style
      line inside a body); property — extraction totality, carried over
      from spec 02 and now running in-process instead of a subprocess per
      case. Deps: T1.
- [x] **T3 — `cargo xtask checksum`, and the portability dance ends.**
      Scenarios: contributes to _The xtask commands honor every spec 02
      contract_ (T5). Tests: unit — a known vector hashes correctly;
      property — output format stability: for any file name and content the
      line is exactly `<64 lowercase hex>  <basename>`, which is what
      `sha256sum -c` reads. Deps: T1.
- [x] **T4 — `cargo xtask release-guard`, and a diagnosis that names names.**
      Scenarios: contributes to _The xtask commands honor every spec 02
      contract_ (T5). Tests: unit — tag/VERSION disagreement, marker
      disagreement, missing CHANGELOG section, the lookalike heading;
      property — guard diagnosis: for any disagreeing (tag, VERSION,
      marker) triple the error names every mismatched pair, never just the
      first. Deps: T1.
- [ ] **T5 — The switch: xtask is what runs, and the scripts go.**
      Scenarios: _The xtask commands honor every spec 02 contract_, _The
      release workflow speaks xtask_, _The shell gate covers exactly the
      shell that remains_. Tests: spec 02's
      acceptance suite keeps every scenario name and fixture but drives
      `cargo xtask`; static assertions on `release.yml` — guard, notes and
      checksum through `cargo xtask`, no step under `scripts/`, guard and
      gates still strictly before `gh release create`. Deliverable: the
      three scripts deleted, `release.yml` and the `lint` recipe updated.
      Deps: T2, T3, T4.
- [ ] **T6 — The gates get something true to say.**
      Scenarios: _The mutation gate is back in business_, _Coverage and
      CRAP measure the xtask crate_. Tests: acceptance — `just cov` and
      `just crap` measure the xtask sources instead of reporting nothing to
      measure, and `just mutants-diff` mutates changed xtask lines instead
      of calling them out of reach; spec 01's honest-skip scenarios still
      pass unchanged, because they are about *adopter* shapes. Deliverable:
      whatever the `--path` filter needs to see a workspace member, and
      `crap-baseline.json` restored in its own deliberate commit. Deps: T5.

**Why the shell gate scenario keeps two files.** This spec was written when
the only shell besides `install.sh` was the three release scripts. Spec 03
has since shipped `scripts/integration-check.sh`, whose whole job is to run
a shell installer against a checkout, so it is not part of this migration —
it would move only alongside the installer, which this spec defers. The
scenario was amended (approved) to say so: what must leave shell is the
*release logic*, not every script in the repository.

---

## Implementation Notes

**Shape.** The repository becomes a two-member workspace: the existing
harness package and `xtask/` (a bin crate, `publish = false`). A
`.cargo/config.toml` alias makes `cargo xtask` work. The release logic
moves into plain Rust: a CHANGELOG section parser (replacing the awk), a
SHA-256 via a small pure-Rust digest dependency (replacing the
sha256sum/shasum portability dance), and the consistency guard.

**Tests move with the logic.** `tests/release.rs` keeps its scenario
names and fixtures but drives `cargo xtask` (or calls the xtask library
functions directly for the property test, dropping the
subprocess-per-case cost). The extraction-totality property becomes a
plain proptest over the parser. Unit tests live next to the parser code —
and the mutation gate keeps them honest, which is the point.

**Gates snap back on.** With real sources in the workspace, the
`cov`/`crap` recipes' target probe finds library/binary targets and
measures them; `crap-baseline.json` returns (its own deliberate commit);
`mutants-diff` gets reachable lines. The spec 01 scenarios about honest
skipping still hold for *adopter* shapes — a workspace root, a no-target
harness — and their tests keep passing untouched.

**Invariants worth property tests.** Extraction totality (carried over);
checksum output format stability (`<64 hex>  <basename>`); guard
diagnosis: for any disagreeing (tag, VERSION, marker) triple, the error
names every mismatched pair.

### Non-goals

- Rewriting `install.sh` in Rust — its own spec, grown from this crate.
- Shipping xtask, the workspace layout, or any alias to adopters.
- Changing any release behavior — spec 02's scenarios are the oracle and
  must pass verbatim.
- Publishing xtask to crates.io.
