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
just dev    # fmt, clippy (pedantic), shellcheck, tests, coverage, CRAP
```

The deliverable is `install.sh` and the files it ships — its tests live in
`tests/installer.rs` and drive the installer against generated projects,
offline. If you change installer behavior, add a test there; if you add a
workflow file (a command, a skill), `install.sh` must ship it — a test will
remind you.

## Docs that have to keep up

Each doc has one job, and a change that outgrows its doc is not finished:

- **`CHANGELOG.md`** — every user-visible change, under `[Unreleased]`.
- **`README.md`** — only if the change alters what the installer puts in a
  project, what you need to run it, or the first commands you type.
- **`KEELER.md`** — only if the *reasoning* changed: a new stage, a new gate,
  a new failure mode one of them defends against.
- **`.claude/keeler.md`** — only if the rules the agent obeys changed. It
  carries the version marker, so a release bumps it either way.
- **`SECURITY.md`** — if the change moves what the installer trusts.

Several of these are gate-checked: tests assert that the README explains
verification, that the rules and `KEELER.md` both walk through a graph-mode
day, and that `VERSION`, the rules marker and `CHANGELOG.md` agree.

## Writing for the agent

The files under `.claude/commands/keeler/` and `.claude/skills/` are not
documentation about the workflow — they *are* the workflow, executed by an
agent that reads them fresh every run. Prose quality there is behavior.
When you touch one, measure it against three levers (the framing comes
from mattpocock/skills' `writing-for-agents`; the bars are ours):

- **The description is the trigger.** A skill fires, or doesn't, on its
  `description:` line alone — the body is never seen until it has fired.
  Lead with the words that decide invocation, name the distinct cases that
  should reach it, and cut everything the body already says. A right body
  behind a weak description is a coin-flip, and the fix is the wording,
  not more wording.
- **Every step ends on a checkable bound.** "The interview is done when
  the frontier is empty" can be verified; "until understanding is reached"
  invites declaring victory early. The bound also sets the workload:
  "every scenario maps to a test named after it" forces legwork that
  "add tests" does not. If a step's done-condition can't be told from its
  not-done, sharpen it before shipping.
- **One word, used the same way everywhere, beats a clause.** *Tight*,
  *frontier*, *load-bearing*, *oracle* — each names a discipline the
  model already knows, so repeating the word re-invokes the discipline at
  the cost of a token. Spelling the definition out at every site spends
  tokens teaching what a shared term recruits for free. When the same
  idea appears in three files under three phrasings, that is a rename
  waiting to happen.

The same rule as code review applies: a step that restates what the agent
would do anyway is noise — delete it. What earns its place is the
instruction the agent would *not* derive on its own.

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
3. Bump the pinned tag in the worked examples — README's install section,
   `install.sh`'s usage text, the `keeler-upgrade` comment in the
   `Justfile`. Nothing mechanical checks these; `grep -rn vX.Y.Z` for the
   old tag is the check.
4. `cargo check` to refresh `Cargo.lock`, which CI verifies is in step.
5. Open the release PR, merge it, then `git tag vX.Y.Z && git push origin
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

## Dependencies

**Nothing bumps a dependency but a hand.** There is no Dependabot here: a
bot that raises a version it cannot make compile leaves the repository
red and someone else's afternoon to it — which is what `sha2 0.11` did,
its digest having dropped `LowerHex`. Check for updates when you have
reason to (`cargo update --dry-run`, a release you are preparing, a
security advisory), take them deliberately, and fix what they break in
the same commit. The same goes for the action pins in `ci.yml` and in
`templates/keeler.yml` — the second was always a hand bump, because
Dependabot never scanned it.

## Conventions

- **Commit messages follow [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/).**
  `<type>(<scope>): <subject>`, imperative mood, no trailing period. Types
  in use: `feat`, `fix`, `docs`, `chore`, `test`, `refactor`. The scope is
  optional and, when it is a spec, is the spec's number — `feat(spec-04):`,
  `fix(spec-05):`. A breaking change carries `!` after the type and a
  `BREAKING CHANGE:` footer. The body says *why*, not what — the diff
  already says what.
- Never weaken a test to make code pass — strengthen the test or fix the
  code.
- `specs/` files are contracts: propose changes in the PR, don't edit
  approved specs silently.
