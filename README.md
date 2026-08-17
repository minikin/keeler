# Keeler

[![CI](https://github.com/minikin/keeler/actions/workflows/ci.yml/badge.svg)](https://github.com/minikin/keeler/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

> Named after [Leonarde Keeler](https://en.wikipedia.org/wiki/Leonarde_Keeler), builder of the first practical polygraph.

Keeler is a quality workflow for **Rust** projects built with
[Claude Code](https://claude.com/claude-code).

It installs Claude Code's own furniture — slash commands under
`.claude/commands/keeler/`, skills under `.claude/skills/`, and a rules
file imported by `CLAUDE.md`. Another coding agent will not find them, so
Keeler is not a general AI tool and does not pretend to be one. The
*method* is portable — spec first, test first, gates that check each other
— and the gates themselves are plain `cargo` and `just`, which any agent
or none can run. Only the commands assume Claude Code.

The rules are simple: no code before a spec you approved, no bugfix without
a failing test first, and no change is done until it passes a set of gates
that check each other — tests, coverage, complexity, review, and mutation
testing that checks the tests themselves. Any single gate can be fooled.
All of them together — rarely.

Keeler doesn't ask the agent to be careful. It makes carelessness fail the
build.

## How it works

**1. Spec first.** Every feature starts as a Gherkin spec (Given/When/Then)
in `specs/`. A human reads it and approves it — that is the only moment
requirements get decided. No code exists yet.

**2. Tests first.** Implementation is strict TDD: the failing test is shown
first, then the minimal code, then the refactor. Property tests
([proptest](https://proptest-rs.github.io/proptest/)) pin the invariants
examples can't.

**3. Gates.** The result must pass every gate, and each catches what the
others miss:

| Gate       | What it catches                                              |
| ---------- | ------------------------------------------------------------ |
| Format     | style drift (`rustfmt`)                                      |
| Lints      | warnings and footguns (`clippy`, pedantic, zero tolerance)   |
| Tests      | broken behavior (`nextest` + doc tests)                      |
| Coverage   | untested lines in changed code                               |
| CRAP       | complex code hiding behind missing tests                     |
| Review     | scope creep and spec mismatches                              |
| Mutation   | weak tests — bugs planted on purpose must be caught          |
| CRAP delta | any function getting worse than the committed baseline       |

One rule is absolute: **a surviving mutant means the test is weak.
Strengthen the test — never bend the code to satisfy the tool.**

The method is language-agnostic; this implementation is wired to the Rust
toolchain. Porting it means swapping the `Justfile` recipes and tool names —
the pipeline and its rules stay the same.

## Quick start

Three roads, by weight of the change:

- **Feature:** `/keeler:spec → approve → /keeler:tasks → /keeler:tdd →
  /keeler:qa → /keeler:review → /keeler:mutants` — or just
  `/keeler:feature <problem>` to run the whole pipeline end to end.
- **Bugfix:** `/keeler:fix` — reproduce with a failing regression test
  first, then fix minimally.
- **Trivial** (docs, comments, config): `just lint` and done — no spec, no
  ceremony.

See [KEELER.md](KEELER.md) for the full picture with diagrams, and
[.claude/keeler.md](.claude/keeler.md) for the rules Claude Code operates under.

## Install

From inside any Rust project — existing or freshly `cargo new`-ed:

```bash
# latest from main
curl -fsSL https://raw.githubusercontent.com/minikin/keeler/main/install.sh | bash -s .

# or pin a release
KEELER_REF=v0.3.0 curl -fsSL https://raw.githubusercontent.com/minikin/keeler/main/install.sh | bash -s .
```

`just keeler-upgrade` re-runs the installer later; the version you have is
recorded at the top of `.claude/keeler.md`.

|              | What it installs                                                                                        |
| ------------ | ------------------------------------------------------------------------------------------------------- |
| **Tools**    | `cargo-nextest`, `cargo-llvm-cov`, `cargo-mutants`, `cargo-crap`, `just` — only the ones you're missing  |
| **Workflow** | slash commands, skills, spec template, `Justfile`, gate configs                                          |
| **CI**       | `.github/workflows/keeler.yml` — the gates and nothing else, kept separate from your own workflows       |
| **Manifest** | `proptest` as a dev-dependency, plus the `[profile.mutants]` and `[lints.clippy]` sections               |
| **Ignores**  | the generated artifacts, appended to `.gitignore` — duplicates and equivalent patterns are skipped       |

The installer is safe to re-run, and it keeps its hands off your files:

- **Your files are never overwritten.** If a file you changed differs from
  what Keeler ships, the new version lands alongside it as `<name>.keeler`,
  and the run names every such file so you can merge on your own terms.
- **One exception:** `.claude/keeler.md`, the rules file Keeler owns. An
  upgrade replaces it — that is how rule changes reach you — and your
  previous copy is kept as `.claude/keeler.md.bak`. Project-specific
  instructions belong in `CLAUDE.md`, which is never rewritten: an existing
  `CLAUDE.md` keeps every word and gains a single `@.claude/keeler.md`
  import line.
- **Nothing is duplicated.** Manifest sections and `.gitignore` entries are
  added only when missing — a second run has nothing to do.
- `--no-tools` skips the tool installs. Prefer to read the script first?
  `git clone https://github.com/minikin/keeler && ./keeler/install.sh <project>`
  installs whatever the clone has checked out — `main`, unless you check out
  a tag first.
- Prefer to **verify** before running? Every release ships `install.sh`
  with its SHA256:

  ```bash
  gh release download v0.3.0 --repo minikin/keeler --pattern 'install.sh*'
  sha256sum -c install.sh.sha256    # shasum -a 256 -c on macOS
  bash install.sh .
  ```
- On Windows, run it from WSL or Git Bash — the installer is a shell script.

## Adopting it in an existing codebase

**Legacy code will not pass the gates on day one — that's expected.** Adopt
them incrementally:

1. `just crap-baseline`, then **commit `crap-baseline.json`** — it freezes
   today's scores as the shared reference. From now on `just crap-delta`
   fails only on *regressions*: legacy debt is grandfathered, new debt is
   not. CI enforces the same gate on every pull request.
2. Use `just mutants-diff` (changed lines only) — never run full mutation
   testing on a legacy codebase.
3. Set the coverage threshold in the `cov` recipe to today's number, and
   ratchet it up as tested code grows. Same for the CRAP `--threshold`.
4. Write specs for new features only; legacy behavior earns specs
   opportunistically, one `/keeler:fix` at a time.

The principle: **the gates guard the delta, not the past.** Every change
must leave the codebase better; nobody is asked to repay ten years of debt
up front.

## Skills

Keeler installs two project skills in `.claude/skills/` that load
themselves when relevant: **property-testing** (an invariant catalog for
proptest — round-trips, idempotence, shape checks) and **gherkin-specs**
(how to write scenarios that actually gate code).

Recommended companions, installed globally on your machine (not part of the
repo): **[rust-best-practices](https://github.com/apollographql/rust-best-practices)**
for idiomatic Rust (built on Apollo's handbook), **[clean-code](https://github.com/jackjin1997/ClawForge)**
for the refactor step of TDD (from the ClawForge collection), and
**[rust-async-patterns](https://github.com/search?q=rust-async-patterns+SKILL.md&type=code)**
once the project grows async code.

## Toolchain

- [just](https://github.com/casey/just) — task runner (`just` lists recipes)
- [cargo-nextest](https://nexte.st) — test runner
- [cargo-llvm-cov](https://github.com/taiki-e/cargo-llvm-cov) — coverage
- [cargo-crap](https://github.com/minikin/cargo-crap) — CRAP score gate (complexity × uncoverage)
- [cargo-mutants](https://mutants.rs) — mutation testing
- [proptest](https://proptest-rs.github.io/proptest/) — property-based tests

## This repository

What you're looking at is both the product and its test bench:

- `install.sh` — the deliverable: the script that sets everything up in
  your project
- `templates/keeler.yml` — the CI workflow adopters receive
- `specs/` — Gherkin specs; `TEMPLATE.md` is the starting point for yours
- `.claude/keeler.md` — the workflow rules, imported by `CLAUDE.md`
- `.claude/commands/keeler/` — the slash commands (`/keeler:spec`,
  `/keeler:fix`, …)
- `.claude/skills/` — self-triggering knowledge: property-testing,
  gherkin-specs
- `tests/installer.rs` — the harness: tests that drive `install.sh` against
  generated projects, offline — the gates here measure the deliverable, not
  a placeholder
- `.github/workflows/ci.yml` — this repository's CI: lints + shellcheck,
  the harness suite, and end-to-end installer tests on Linux and macOS
