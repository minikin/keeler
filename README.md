# Keeler

[![CI](https://github.com/minikin/keeler/actions/workflows/ci.yml/badge.svg)](https://github.com/minikin/keeler/actions/workflows/ci.yml)
[![Latest release](https://img.shields.io/github/v/release/minikin/keeler)](https://github.com/minikin/keeler/releases/latest)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

Keeler is a quality workflow for **Rust** projects built with
**[Claude Code](https://claude.com/claude-code)**. No code before a spec
you approved, no bugfix without a failing test first, and no change is done
until it passes a set of gates that check each other. It doesn't ask the
agent to be careful — it makes carelessness fail the build.

> Named after [Leonarde Keeler](https://en.wikipedia.org/wiki/Leonarde_Keeler),
> who built the first practical polygraph. A polygraph does not detect lies;
> it records several independent channels at once, on the theory that you can
> fool one but not all of them.

**[KEELER.md](KEELER.md)** explains why the workflow is shaped this way. The
installer ships it into your project, so the reasoning arrives with the rules.

## Install

**You need** a Rust toolchain, `bash`, `curl` and `tar` (plus `git` — the
workflow lives on branches); graph mode also needs `tmux`. macOS and Linux —
on Windows, WSL or Git Bash. The gate tools themselves the installer puts in
for you, unless you pass `--no-tools`.

From inside any Rust project — existing or freshly `cargo new`-ed:

```bash
# pin a release
KEELER_REF=v0.4.0 curl -fsSL https://raw.githubusercontent.com/minikin/keeler/main/install.sh | bash -s .

# or take main
curl -fsSL https://raw.githubusercontent.com/minikin/keeler/main/install.sh | bash -s .
```

Prefer to verify before running? Every release ships `install.sh` with its
SHA256:

```bash
gh release download v0.4.0 --repo minikin/keeler --pattern 'install.sh*'
sha256sum -c install.sh.sha256    # shasum -a 256 -c on macOS
bash install.sh .
```

|              | What lands in your project                                                                                                 |
| ------------ | -------------------------------------------------------------------------------------------------------------------------- |
| **Commands** | the nine slash commands under `.claude/commands/keeler/` — `/keeler:feature`, `:spec`, `:tasks`, `:tdd`, `:qa`, `:review`, `:mutants`, `:fix`, `:graph` — plus two skills and `specs/TEMPLATE.md` |
| **Gates**    | a `Justfile` and the configs behind it: `just dev`, `test`, `lint`, `cov`, `crap`, `crap-baseline`, `crap-delta`, `mutants-diff` — the linear road, and what CI runs |
| **Graph mode** | in the same `Justfile`, the recipes for the parallel road — `keeler-feature-branch`, `keeler-graph`, `keeler-fan-out`, `keeler-spawn`, `keeler-status`, `keeler-resume`, `keeler-branch`, `keeler-land` — plus `scripts/keeler-graph.sh`. Opt-in: unused, they cost nothing |
| **Rules**    | `.claude/keeler.md`, imported by `CLAUDE.md` with one line — your `CLAUDE.md` is otherwise untouched                       |
| **Guide**    | `KEELER.md` — the same workflow explained for humans: why each gate exists, and the graph-mode day start to finish          |
| **CI**       | `.github/workflows/keeler.yml` — the gates, plus two checks that fire only on `keeler/*` pull requests: the baseline was not moved, the review record names a commit of the branch |
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

### Adopting it in an existing codebase

**Legacy code will not pass on day one — that's expected.** Run
`just crap-baseline` and **commit `crap-baseline.json`**: from then on
`just crap-delta` fails only on *regressions*, and CI enforces the same on
every pull request. Use `just mutants-diff` — changed lines only — never
full mutation testing on a legacy tree. Set the coverage threshold in the
`cov` recipe to today's number and ratchet it up. The gates guard the delta,
not the past.

## Graph mode

A feature's tasks can run in parallel — one agent per task, each on its own
branch in its own worktree, each through the whole pipeline. The graph lives
in the spec: `/keeler:tasks` writes `Needs: T1.` on each task, so approving
the spec is approving the graph.

```
$ just keeler-fan-out specs/07-fan-out.md
keeler-fan-out: specs/07-fan-out.md on feat/07-fan-out
  T1 done
  T2 ready
  T3 ready
  T4 blocked (waiting on T2)
wave: T2 T3
spawn T2 T3? [yes/no] yes
```

One yes spawns them all, into one tmux window with a pane per run.
`just keeler-status <spec>` is the board afterwards; you merge the finished
branches into the feature branch, and `just keeler-land` runs the gates and
clears the landed worktrees.

It is opt-in and changes nothing on the linear road: a project that never
runs these recipes never meets them. Its one extra requirement is **tmux**.
[KEELER.md](KEELER.md#graph-mode-the-same-pipeline-in-parallel) has the day,
start to finish.

## What Keeler is not

- **Not for other agents.** It installs Claude Code's slash commands and
  skills. The *method* is portable and the gates are plain `cargo` and
  `just`, which anything can run — but the pipeline assumes Claude Code.
- **Not a substitute for review.** Every gate leaves evidence except
  review, so nothing notices when review is skipped. The commands lead from
  each stage to the next; following them is on you.
- **Not able to write its own instructions.** A spawned agent may not edit
  files under `.claude/` — a headless session has nobody to ask for that
  consent — so a task whose deliverable is a command file or the rules file
  is the human's to do. Found the hard way, by a spawned agent that reported
  it rather than routing around it.
- **Not Windows-native.** The installer is a shell script — WSL or Git
  Bash — and the shipped gates have no Windows CI job behind them.

## Toolchain

- [just](https://github.com/casey/just) — task runner
- [cargo-nextest](https://nexte.st) — test runner
- [cargo-llvm-cov](https://github.com/taiki-e/cargo-llvm-cov) — coverage
- [cargo-crap](https://github.com/minikin/cargo-crap) — CRAP score gate (complexity × uncoverage)
- [cargo-mutants](https://mutants.rs) — mutation testing
- [proptest](https://proptest-rs.github.io/proptest/) — property-based tests

## Documentation

Each file says one thing once, and points at the others rather than
repeating them:

| File                                       | Written for                       | What it answers                                                                       |
| ------------------------------------------ | --------------------------------- | ------------------------------------------------------------------------------------- |
| **README.md**                              | you, right now                    | What is this, how do I install it, what do I type first                               |
| **[KEELER.md](KEELER.md)**                 | your team                         | *Why* each stage and gate exists, what AI failure mode each one catches, the graph-mode day start to finish |
| **[.claude/keeler.md](.claude/keeler.md)** | the agent                         | The workflow as law: the pipeline, the change classes, the commit rule, the gate table |
| **[CONTRIBUTING.md](CONTRIBUTING.md)**     | contributors to Keeler itself     | Which road a change takes here, the release checklist, the standing review debt        |
| **[SECURITY.md](SECURITY.md)**             | anyone about to pipe curl to bash | What the installer trusts, what it never overwrites, how to report a hole              |
| **[CHANGELOG.md](CHANGELOG.md)**           | upgraders                         | What each release changed, and what it broke                                           |

`KEELER.md` and `.claude/keeler.md` are installed into your project — the
reasoning and the rules travel with the workflow. The rest stay here.

## This repository

Both the product and its test bench:

- `install.sh` — the deliverable
- `templates/keeler.yml` — the CI workflow adopters receive
- `.claude/` — the commands, the skills and the rules file, exactly as installed
- `specs/` — the Gherkin specs this repository is built from; `TEMPLATE.md` starts yours
- `xtask/` — the release tooling: `cargo xtask release-guard`, `release-notes`, `checksum`
- `scripts/integration-check.sh` — the contract checker CI runs against pinned clones of anyhow, serde and ripgrep
- `tests/` — the harness that drives `install.sh` against generated projects, offline

## License

MIT — see [LICENSE](LICENSE). Contributions follow Keeler's own workflow;
[CONTRIBUTING.md](CONTRIBUTING.md) says which road yours takes.
