# Contributing

Keeler runs on its own workflow — contributions follow the same rules the
tool asks of its adopters. The short version:

## Pick the right road

| Change                                    | Road                                                                                                     |
| ----------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| New behavior, changed behavior            | Spec first: draft a Gherkin spec in `specs/` (copy `TEMPLATE.md`), get it approved in the PR discussion, then implement test-first |
| A bug                                     | Failing regression test first, then the minimal fix                                                       |
| Docs, comments, config — no behavior      | Just open the PR; run `just lint`                                                                         |

The full rules live in [.claude/keeler.md](.claude/keeler.md) and
[KEELER.md](KEELER.md).

## Before you push

```bash
just dev    # fmt, clippy (pedantic), shellcheck, the test suite
```

The deliverable is `install.sh` and the files it ships — its tests live in
`tests/installer.rs` and drive the installer against generated projects,
offline. If you change installer behavior, add a test there; if you add a
workflow file (a command, a skill), `install.sh` must ship it — a test will
remind you.

## What CI checks

Everything `just dev` does, plus end-to-end installer runs on Linux and
macOS, a from-scratch bootstrap (the installer installing its own tools,
including the forced source-compile fallback), and version consistency
(`VERSION` ↔ the marker in `.claude/keeler.md` ↔ a `CHANGELOG.md` entry).

## Cutting a release

1. Move the `[Unreleased]` entries into a new `## [X.Y.Z] — date` section.
   An empty section is refused: the notes are that section verbatim, and a
   release must not ship blank ones.
2. Bump `VERSION`, the `<!-- keeler-version: -->` marker in
   `.claude/keeler.md`, and the `version` in every manifest — the root
   `Cargo.toml` and each workspace member. `cargo xtask release-guard`
   holds all of them in agreement and names every one that disagrees.
3. `cargo check` to refresh `Cargo.lock`, which CI verifies is in step.
4. Open the release PR, merge it, then `git tag vX.Y.Z && git push origin
   vX.Y.Z`.

The tag push is what cuts the release: the guard runs, then `just ci`, then
the notes and the checksum, then `gh release create`. A published release
is never overwritten, so a bad one has to be deleted rather than fixed —
which is why the guard refuses before anything is published.

## Review debt

The review stage was skipped for every task in specs 01 through 04 —
twenty-odd of them — and nothing noticed. Each had green gates, and a
skipped review leaves no artifact whose absence could fail anything.

It is being worked off by **area**, not by task. Half those tasks describe
code that was later rewritten, so reading their diffs would be archaeology;
what matters is the code as it stands. Three areas, and the state of each:

| Area | Reviewed | Outcome |
| --- | --- | --- |
| `install.sh` and its harness (spec 01) | yes | 13 defects found, 11 fixed. Two shipped ones destroyed data: a manifest with inherited `[lints]` was left unparseable, and a dangling symlink was written through, outside the project. |
| `xtask` — the release tooling (specs 02, 04) | yes | 12 defects found. Three high: an empty CHANGELOG section publishes a release with blank notes and the `create`-only policy cannot repair it; the guard checks zero manifests when versions are workspace-inherited and prints that as success; nothing ties a tag to a commit that passed the full CI while the release assumes it did. |
| `scripts/integration-check.sh` (spec 03) | yes | 12 defects found. Two critical: the three append targets are exempt from every comparison, so an installer that wipes `.gitignore` passes; and the manifest is never parsed, so the whole of the installer's Cargo.toml work is unverified — which is how a manifest cargo refuses to read once shipped. |

All three areas are reviewed and every finding is either fixed or
recorded. The one deliberately left open is the fetched tarball's lack of
a checksum, which SECURITY.md states plainly rather than implying
otherwise: closing it means publishing per-release archive digests, which
is its own decision.

**Nothing enforces this.** A gate was built and its own review found three
blocking defects, so it is parked on `feat/pipeline-enforces-itself`. Until
something replaces it, the pipeline's commands lead from each stage to the
next and the discipline is the only mechanism there is. When a review
happens, add a row here.

## Conventions

- Commit messages: imperative, `fix:`/`feat:`/`chore:` prefixes.
- Never weaken a test to make code pass — strengthen the test or fix the
  code.
- `specs/` files are contracts: propose changes in the PR, don't edit
  approved specs silently.
