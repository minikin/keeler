# Spec 03 — The installer against the wild

**Status:** Draft
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

_Empty until /keeler:tasks runs against an approved spec._

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
