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

Releases are cut by machine (`.github/workflows/release.yml`); the human
part is the prep commit:

1. Fold `[Unreleased]` into a `## [X.Y.Z] — YYYY-MM-DD` section in
   `CHANGELOG.md` and update the compare links.
2. Set `VERSION` and the `keeler-version` marker in `.claude/keeler.md` to
   `X.Y.Z` (the version CI job holds these three in agreement).
3. Merge, then tag: `git tag vX.Y.Z && git push origin vX.Y.Z`.

The workflow re-checks that the tag tells the truth, runs the gates,
extracts that section as the release notes, and attaches `install.sh` plus
its SHA256. A tag that disagrees with `VERSION` fails the workflow and
publishes nothing. Reruns never overwrite a published release.

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
| `xtask` — the release tooling (specs 02, 04) | in progress | — |
| `scripts/integration-check.sh` (spec 03) | in progress | — |

Two findings are open on the first area, both low: no integrity check on
the fetched tarball, and a piped run from inside a Keeler clone can
silently ignore `KEELER_REF`.

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
