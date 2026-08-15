# Spec 02 — Releases are cut by machine, verified by hash

**Status:** Implemented
**Effort:** Small
**Module:** `.github/workflows/release.yml`, `scripts/`, `Justfile`, `README.md`, `SECURITY.md`

## Context

Installations are designed to pin releases — `KEELER_REF=v0.1.0` resolves
through codeload, the CHANGELOG keeps per-version sections with compare
links, and the version-consistency CI job enforces `VERSION` ↔ rules-file
marker ↔ CHANGELOG agreement on every push. But the release end of that
contract does not exist: the repository has **no tags and no releases** —
nothing for `KEELER_REF` to actually pin, no notes, no assets, and nothing
for a cautious adopter to verify `install.sh` against before piping it into
bash. Cutting a release today means remembering an unwritten checklist, and
nothing stops a tag whose name disagrees with `VERSION` from shipping.

Who hits this: adopters who want to pin *and verify* what they run
(`SECURITY.md` currently answers `curl | bash` concerns only with "read it
first"), and the maintainer, for whom every release is a manual sequence
with silent-failure modes.

What changes: pushing a tag `vX.Y.Z` produces the release mechanically —
but only after the repository proves the tag honest. The workflow re-checks
version consistency, runs the fast gates, extracts that version's CHANGELOG
section as the release notes, and attaches `install.sh` plus its SHA256 so
the two-step verify is possible and documented. A tag that lies about its
version, or points at a CHANGELOG with no matching section, produces a
failed workflow and **no release** — never a wrong one.

The changelog-extraction and checksum logic live in small shell scripts,
not inline YAML, for the same reason spec 01 moved gates onto the
deliverable: scripts can be driven by the harness offline and gated by
shellcheck; workflow YAML cannot.

**Constraint:** the end-to-end act of publishing (tag push → release
appears) is only observable on GitHub. The spec therefore pins every piece
the harness *can* observe — extraction, checksum round-trip, refusal
conditions, workflow shape — and leaves the final integration to the first
real tag, `v0.1.0`.

**Rejected alternatives.** Release-on-every-merge (release-please style):
wrong cadence for a workflow tool people pin; versions here are deliberate.
Signing (GPG / Sigstore attestation): valuable, but a separate trust
decision with key-management questions — the SHA256 answers "did my bytes
match the release?" today and signing can layer on later. Publishing to
crates.io: the deliverable is not a crate.

---

## Acceptance Tests

### Scenario: A pushed tag that matches VERSION produces the release

```
Given VERSION reads X.Y.Z, the rules-file marker agrees, and CHANGELOG.md
      has a section for X.Y.Z
When  the tag vX.Y.Z is pushed
Then  a GitHub release named vX.Y.Z exists
And   its notes are the CHANGELOG section for X.Y.Z
And   its assets include install.sh and a SHA256 checksum file for it
```

### Scenario: A crate version that disagrees with VERSION is refused

```
Given a repository whose VERSION and Cargo.toml say different versions
When  the guard runs
Then  it refuses, naming the manifest and both versions
And   a repository whose manifests all agree passes
```

### Scenario: A tag that disagrees with VERSION is refused

```
Given VERSION reads X.Y.Z
When  a tag naming any other version is pushed
Then  the release workflow fails
And   no release is created for that tag
```

### Scenario: The release notes are exactly the version's CHANGELOG section

```
Given a CHANGELOG with sections for several versions and an Unreleased
      heading
When  the notes for version X.Y.Z are extracted
Then  the output is the body of the X.Y.Z section
And   it contains no heading, entry, or link line from any other section
```

### Scenario: Extracting notes for an absent version fails loudly

```
Given a CHANGELOG with no section for version X.Y.Z
When  the notes for X.Y.Z are extracted
Then  the extraction exits non-zero
And   it names the version it could not find
```

### Scenario: The published checksum verifies the script

```
Given the checksum file produced for install.sh
When  sha256sum -c runs against it in a directory holding that install.sh
Then  verification succeeds
And   verification fails for an install.sh whose bytes differ
```

### Scenario: The gates run before anything is published

```
Given a tag whose lint or test gate would fail
When  the release workflow runs
Then  it fails before creating the release
```

### Scenario: A release is never overwritten

```
Given a GitHub release already exists for vX.Y.Z
When  the release workflow runs again for that tag
Then  it fails
And   the existing release is unchanged
```

### Scenario: Release scripts are gated like the installer

```
Given a release script under scripts/ containing a statically detectable
      shell defect
When  the lint gate runs in this repository
Then  it fails naming the file and line
```

### Scenario: The verification story is documented where adopters look

```
Given the README install section and SECURITY.md
When  an adopter follows them
Then  both name the pin-and-verify path: download install.sh and its
      checksum from the release, sha256sum -c, then run
```

### Scenario: The release workflow is not shipped to adopters

```
Given a fresh Rust project
When  Keeler is installed into it
Then  no release workflow lands in the project
```

---

## Tasks

- [x] **T1 — CHANGELOG section extraction, exact and loud.**
      Scenarios: _The release notes are exactly the version's CHANGELOG
      section_, _Extracting notes for an absent version fails loudly_.
      Tests: acceptance on fixture changelogs; property — extraction
      totality: over a generated changelog with sections S₁…Sₙ, each
      extraction is disjoint from every other section's entries and
      concatenating all extractions plus headings/links loses nothing but
      blank lines. Deliverable: `scripts/release-notes.sh`. Deps: none.
- [x] **T2 — A checksum that round-trips.**
      Scenarios: _The published checksum verifies the script_. Tests:
      acceptance — `sha256sum -c` succeeds against the produced file for
      identical bytes and fails for differing bytes. Deliverable:
      `scripts/checksum.sh` (sha256sum/shasum portability). Deps: none.
- [x] **T3 — Release scripts join the shell gate.**
      Scenarios: _Release scripts are gated like the installer_. Tests:
      acceptance — a defective script under `scripts/` fails `just lint`
      naming file and line; the shipped recipe stays repo-keyed so
      adopters are untouched. Deps: T1 (first script to gate).
- [x] **T4 — The tag must tell the truth.**
      Scenarios: _A tag that disagrees with VERSION is refused_. Tests:
      acceptance — the guard script exits non-zero naming both versions
      when tag ≠ `v$(cat VERSION)`, zero on agreement. Deliverable:
      `scripts/release-guard.sh` (tag/VERSION/marker/CHANGELOG agreement).
      Deps: none.
- [x] **T5 — The release workflow, assembled.**
      Scenarios: _A pushed tag that matches VERSION produces the release_
      (the end-to-end contract — observable only on GitHub, like the
      bootstrap jobs; the first real tag is its integration test), _The
      gates run before anything is published_, _A release is never
      overwritten_, _The release workflow is not shipped to adopters_.
      Tests: static acceptance on `release.yml` — tag trigger, guard +
      `just ci` ordered before `gh release create`, `create` (never
      `edit`/`--clobber`), notes and assets from the T1/T2 scripts; and a
      harness assertion that no release workflow lands in an installed
      project. Deps: T1, T2, T4.
- [x] **T6 — The verify story, documented.**
      Scenarios: _The verification story is documented where adopters
      look_. Tests: acceptance — README's install section and SECURITY.md
      both name the pin-and-verify path (`sha256sum -c` against the
      release assets); CONTRIBUTING.md carries the release checklist.
      Deps: T5.
- [x] **T7 — The manifests must agree too.**
      Scenarios: _A crate version that disagrees with VERSION is refused_.
      Tests: unit — a manifest whose `[package] version` differs from
      VERSION is named with both versions; every workspace member is
      checked, not only the root; a member that inherits its version from
      the workspace is not mistaken for a disagreement. Property — the
      guard's diagnosis still names every mismatched pair, manifests
      included. Deps: T4.


---

## Implementation Notes

**Shape.** `.github/workflows/release.yml`, triggered by `push` on tags
`v*`. Steps: checkout → assert tag == `v$(cat VERSION)` → re-run the
version-consistency checks → `just ci` (lint + test; the installer E2E jobs
stay on the PR/push pipeline — the tag commit already passed them) →
`scripts/release-notes.sh $VERSION CHANGELOG.md > notes.md` →
`scripts/checksum.sh install.sh > install.sh.sha256` → `gh release create
"v$VERSION" --notes-file notes.md install.sh install.sh.sha256` (create,
never edit — rerunning fails on the existing release).

**Scripts, not YAML.** `scripts/release-notes.sh` — awk/sed section
extraction between `## [X.Y.Z]` and the next `## [` or the link block;
`scripts/checksum.sh` — `shasum -a 256`/`sha256sum` portability wrapper
emitting the standard `<hash>  install.sh` line. Both are harness-testable
offline against fixture files and join `install.sh` under shellcheck (the
`lint` recipe's repo-keyed line widens to `install.sh scripts/*.sh`).

**Invariant worth a property test** — extraction totality: for a generated
CHANGELOG containing sections `S₁…Sₙ`, extracting each version returns
text disjoint from every other section's entries, and concatenating all
extractions plus headings/links loses nothing but blank lines. (Cheap to
generate; pins the off-by-one-heading class of awk bugs.)

**Docs.** README install section gains the pinned verify variant;
SECURITY.md's `curl | bash` paragraph points at it. `CONTRIBUTING.md` gains
the release checklist: move Unreleased → `[X.Y.Z] — date` with compare
links, bump `VERSION` + marker, merge, tag, push tag.

**v0.1.0.** First consumer of the machinery, cut after this spec ships —
the version everything already claims (`VERSION`, the rules-file marker,
the CHANGELOG section) but that never got a tag or release. Release prep
folds today's Unreleased entries (spec 01, the installer fixes, the quick
wins) into the existing `[0.1.0]` section, refreshes its date, and the tag
push does the rest.

### Non-goals

- Signing or provenance attestation (Sigstore, GPG) — future layer.
- Publishing to crates.io, Homebrew, or any registry.
- Automated version bumping or release-PR generation.
- Changing what adopters receive — the release workflow is repo-only.
