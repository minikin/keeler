# Keeler

[![CI](https://github.com/minikin/keeler/actions/workflows/ci.yml/badge.svg)](https://github.com/minikin/keeler/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

Keeler is a quality workflow for **Rust** projects built with
**[Claude Code](https://claude.com/claude-code)**. No code before a spec
you approved, no bugfix without a failing test first, and no change is done
until it passes a set of gates that check each other. It doesn't ask the
agent to be careful — it makes carelessness fail the build.

>It is named after [Leonarde Keeler](https://en.wikipedia.org/wiki/Leonarde_Keeler),
builder of the first practical polygraph. 

**[KEELER.md](KEELER.md)** explains
why the workflow is shaped the way it is.

## Install

From inside any Rust project — existing or freshly `cargo new`-ed:

```bash
# pin a release
KEELER_REF=v0.3.0 curl -fsSL https://raw.githubusercontent.com/minikin/keeler/main/install.sh | bash -s .

# or take main
curl -fsSL https://raw.githubusercontent.com/minikin/keeler/main/install.sh | bash -s .
```

Prefer to verify before running? Every release ships `install.sh` with its
SHA256:

```bash
gh release download v0.3.0 --repo minikin/keeler --pattern 'install.sh*'
sha256sum -c install.sh.sha256    # shasum -a 256 -c on macOS
bash install.sh .
```

|              | What lands in your project                                                                                                 |
| ------------ | -------------------------------------------------------------------------------------------------------------------------- |
| **Workflow** | slash commands under `.claude/commands/keeler/`, two skills, a spec template, `Justfile`, gate configs                     |
| **Rules**    | `.claude/keeler.md`, imported by `CLAUDE.md` with one line — your `CLAUDE.md` is otherwise untouched                       |
| **CI**       | `.github/workflows/keeler.yml` — the gates and nothing else, beside your own workflows                                     |
| **Manifest** | `proptest` as a dev-dependency, `[profile.mutants]`, `[lints.clippy]` — each only if missing                               |
| **Ignores**  | the gates' artifacts, appended to `.gitignore`; equivalent spellings are not duplicated                                    |
| **Tools**    | `cargo-nextest`, `cargo-llvm-cov`, `cargo-mutants`, `cargo-crap`, `just` — only the ones you lack; `--no-tools` skips this |

The installer keeps its hands off your files. Anything you already had is
never overwritten: if it differs from what Keeler ships, the new version
lands alongside as `<name>.keeler` and the run names it. The one exception
is `.claude/keeler.md`, which Keeler owns — an upgrade replaces it and keeps
your previous copy as `.claude/keeler.md.bak`. Running it twice changes
nothing. `just keeler-upgrade` re-runs it later; the version you have is
recorded at the top of `.claude/keeler.md`.

## The first day

Three roads, by weight of the change:

- **Feature** — `/keeler:feature <problem>` runs the whole pipeline: a spec
  you approve, then tests first, then the gates. Or stage by stage:
  `/keeler:spec → /keeler:tasks → /keeler:tdd → /keeler:qa → /keeler:review → /keeler:mutants`.
- **Bugfix** — `/keeler:fix`: a failing regression test first, then the
  minimal fix.
- **Trivial** — docs, comments, config: `just lint` and done.

`just dev` runs every gate locally; `just` lists the rest.

**Legacy code will not pass on day one — that's expected.** Run
`just crap-baseline` and **commit `crap-baseline.json`**: from then on
`just crap-delta` fails only on *regressions*, and CI enforces the same on
every pull request. Use `just mutants-diff` — changed lines only — never
full mutation testing on a legacy tree. Set the coverage threshold in the
`cov` recipe to today's number and ratchet it up. The gates guard the delta,
not the past.

## What Keeler is not

- **Not for other agents.** It installs Claude Code's slash commands and
  skills. The *method* is portable and the gates are plain `cargo` and
  `just`, which anything can run — but the pipeline assumes Claude Code.
- **Not a substitute for review.** Every gate leaves evidence except
  review, so nothing notices when review is skipped. The commands lead from
  each stage to the next; following them is on you.
- **Not Windows-native.** The installer is a shell script — WSL or Git
  Bash — and the shipped gates have no Windows CI job behind them.

## Toolchain

- [just](https://github.com/casey/just) — task runner
- [cargo-nextest](https://nexte.st) — test runner
- [cargo-llvm-cov](https://github.com/taiki-e/cargo-llvm-cov) — coverage
- [cargo-crap](https://github.com/minikin/cargo-crap) — CRAP score gate (complexity × uncoverage)
- [cargo-mutants](https://mutants.rs) — mutation testing
- [proptest](https://proptest-rs.github.io/proptest/) — property-based tests

## This repository

Both the product and its test bench:

- `install.sh` — the deliverable
- `templates/keeler.yml` — the CI workflow adopters receive
- `.claude/` — the commands, the skills and the rules file, exactly as installed
- `specs/` — the Gherkin specs this repository is built from; `TEMPLATE.md` starts yours
- `xtask/` — the release tooling: `cargo xtask release-guard`, `release-notes`, `checksum`
- `scripts/integration-check.sh` — the contract checker CI runs against pinned clones of anyhow, serde and ripgrep
- `tests/` — the harness that drives `install.sh` against generated projects, offline
- [CONTRIBUTING.md](CONTRIBUTING.md) — the roads, the release checklist, and the review debt
