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

## Conventions

- Commit messages: imperative, `fix:`/`feat:`/`chore:` prefixes.
- Never weaken a test to make code pass — strengthen the test or fix the
  code.
- `specs/` files are contracts: propose changes in the PR, don't edit
  approved specs silently.
