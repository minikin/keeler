# Spec 03 — The installer against the wild

**Status:** Approved
**Effort:** Small
**Module:** `.github/workflows/ci.yml`, `scripts/`

## Context

The harness proves the installer against *generated* project states, and
the CI E2E jobs prove it against `cargo new`. Every real installer bug this
project has fixed, though, came from a shape nobody generated: a
table-form `[dev-dependencies.proptest]`, a workspace with no root `src/`,
a `.gitignore` with no final newline. Real repositories are a free
generator of exactly these surprises.

What changes: a CI job installs Keeler into a small set of **pinned,
real-world Rust projects by different authors** and asserts the
installer's contract on each — not Keeler's quality gates (legacy code
fails those *by design*; the gates guard the delta, not the past), but the
install invariants the specs already name: everything lands, nothing of
theirs is overwritten, every conflict is named, a second run changes
nothing.

Three shapes, three authors, chosen for diversity of manifest and tree:

| Project | Shape it exercises |
| --- | --- |
| `dtolnay/anyhow` | small single-crate library |
| `serde-rs/serde` | workspace root with member crates |
| `BurntSushi/ripgrep` | binary workspace with its own `.github/workflows/` and a lived-in `.gitignore` |

**Pinned commits, not branches.** Each clone is a shallow fetch of a
recorded SHA — a third party's push must never change what our CI tests.
Bumping a pin is a deliberate, reviewable diff.

**CI-only, by design.** The local suite is provably offline (spec 01's
network scenario) and stays that way; cloning real repositories is network
work and lives in a CI job beside the other E2E jobs.

**Rejected alternatives.** Vendoring snapshots of the three projects into
this repository: no network flake, but tens of thousands of foreign lines
in-tree, a license-notice burden, and pins that rot invisibly. Running the
projects' own test suites after install: proves their code, not our
installer, and costs minutes per project.

---

## Acceptance Tests

### Scenario: The installer lands cleanly on a real library crate

```
Given a shallow clone of a pinned real-world library crate
When  Keeler is installed into it with --no-tools
Then  the installer exits zero
And   every file the completeness guard tracks exists in the project
```

### Scenario: The installer lands cleanly on a real workspace

```
Given a shallow clone of a pinned real-world workspace
When  Keeler is installed into it with --no-tools
Then  the installer exits zero
And   it reports the workspace-root manifest as the project's own to manage
```

### Scenario: A lived-in project keeps every byte of its own content

```
Given a shallow clone of a pinned real-world project with its own
      workflows, .gitignore and documentation
When  Keeler is installed into it
Then  no pre-existing file's content changes except the documented
      append-only edits (CLAUDE.md import, .gitignore entries, Cargo.toml
      sections)
And   every conflicting file is named and lands alongside as <name>.keeler
```

### Scenario: A second run over a real project changes nothing

```
Given a pinned real-world project that Keeler was just installed into
When  the installer runs again
Then  the working tree is byte-identical to its state after the first run
```

### Scenario: The pins are exact

```
Given the integration job's project list
When  a clone is made
Then  it is fetched at the recorded commit SHA, never a branch head
```

### Scenario: The local suite stays offline

```
Given the integration checks
When  the harness test suite runs locally
Then  no integration clone is attempted
And   the network guarantee of spec 01 still holds
```

---

## Tasks

The contract checker is the deliverable; the CI job is the thing that
points it at real repositories. Splitting them that way is what makes T1–T4
testable locally and offline: the checker runs the installer over a
directory that already exists, so the harness can hand it a generated
shape and CI can hand it a clone. A `KEELER_INSTALL_SH` override lets a
test point the checker at a deliberately defective installer — an
assertion that cannot fail is not an assertion, and each invariant below
is pinned by both a passing and a failing case.

- [ ] **T1 — The contract checker, and the completeness invariant.**
      Scenarios: _The installer lands cleanly on a real library crate_.
      Tests: acceptance — `scripts/integration-check.sh <dir>` over a
      generated single-crate library exits zero and reports what it
      checked; against a defective installer that skips one install-set
      file it exits non-zero naming the missing path. Deliverable:
      `scripts/integration-check.sh` (snapshot → install → assert exit
      zero + every tracked file present). Deps: none.
- [ ] **T2 — The checker recognizes a workspace root.**
      Scenarios: _The installer lands cleanly on a real workspace_.
      Tests: acceptance — over a generated workspace-root project
      (`[workspace]`, no `[package]`) the checker passes and asserts the
      installer named the root manifest as the project's own to manage;
      with that report suppressed it fails, naming what it expected.
      Deps: T1.
- [ ] **T3 — Nothing of theirs changes, and every conflict is named.**
      Scenarios: _A lived-in project keeps every byte of its own content_.
      Tests: acceptance — over a generated lived-in shape (own workflows,
      own docs, a `.gitignore` with no final newline, own copies of
      install-set files) the checker passes, and the conflicts it reports
      are exactly the `.keeler` files on disk. Property — no silent pass:
      for any pre-existing file a defective installer clobbers, the
      checker fails and names that file; the three documented append
      targets (`CLAUDE.md`, `.gitignore`, `Cargo.toml`) are the only
      permitted diffs. Deps: T1.
- [ ] **T4 — A second run must change nothing.**
      Scenarios: _A second run over a real project changes nothing_.
      Tests: acceptance — the checker installs twice and compares tree
      snapshots, passing on a byte-identical second run; against a
      non-idempotent installer (one that appends on every run) it fails
      naming the drifting path. Deps: T1.
- [ ] **T5 — The real-world job, on exact pins.**
      Scenarios: _The pins are exact_. Tests: acceptance (static, over
      `ci.yml`) — an `installer-real-world` job whose matrix carries the
      three projects, each ref a 40-character lowercase hex SHA, fetched
      shallow at that SHA with no branch name anywhere in the clone step,
      each entry running the checker; the pin-bump comment is present.
      The end-to-end contract over real repositories is observable only in
      CI — like spec 02's release job, its first real run is its
      integration test. Deps: T1, T2, T3, T4.
- [ ] **T6 — The local suite stays offline.**
      Scenarios: _The local suite stays offline_. Tests: acceptance — the
      checker contains no `clone`, `fetch` or `ls-remote` (cloning is the
      workflow's job, not the script's), and a full harness run leaves the
      network-call log untouched, so spec 01's network guarantee still
      holds with the checker in the suite. Deps: T1.

Not a task: the new script joins the shell gate for free — `just lint`
already shellchecks `scripts/*.sh`, and spec 01's gate test covers it.

---

## Implementation Notes

**Shape.** One CI job (`installer-real-world`) with a matrix over
`(repo, sha, shape)`. The per-project checks live in
`scripts/integration-check.sh` — thin, shellcheck-gated like the other
scripts: shallow-fetch the SHA, snapshot the tree (`git add -A` +
`git status` hash, the same trick the idempotency E2E step uses), run
`install.sh --no-tools`, assert the invariants, run it again, compare
snapshots.

**What is asserted, per shape.** Exit zero; the completeness-guard file
list present; pre-existing files unchanged except the three documented
append targets; conflict report names ⊆ actual `.keeler` files; second
run: zero tree delta. The workspace clone additionally asserts the
workspace-root note. No `just dev`, no gates on their code.

**Pin discipline.** SHAs recorded in the workflow matrix, one line per
project. Dependabot cannot bump these; a comment says how (deliberate PR).

### Non-goals

- Running the pinned projects' own tests or Keeler's gates on their code.
- Vendoring project snapshots into this repository.
- Windows shapes.
- More than three projects — this is a diversity sample, not a census.
