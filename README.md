# Keeler

[![CI](https://github.com/minikin/keeler/actions/workflows/ci.yml/badge.svg)](https://github.com/minikin/keeler/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

> Named after [Leonarde Keeler](https://en.wikipedia.org/wiki/Leonarde_Keeler), builder of the first practical polygraph.

Keeler is a quality-assurance workflow for AI-assisted **Rust** development:
every change starts as a human-approved spec, is built test-first, and must
clear independent gates — unit and property tests, coverage, CRAP score,
review, and mutation testing that verifies the tests themselves. A defect
may slip past one gate, but rarely past all of them.

The method is language-agnostic; this implementation is wired to the Rust
toolchain (`cargo nextest`, `cargo llvm-cov`, `cargo mutants`, `cargo crap`,
`proptest`, `just`). Porting it means swapping the `Justfile` recipes and the
tool names in the commands — the pipeline and its rules stay as they are.

## How it works

Every feature begins as a Gherkin specification that a human reviews and
approves before any code exists — that approval is the only moment
requirements get decided. Implementation is strictly test-driven: the
failing test is shown first, then the minimal code, then the refactor.

The result must clear a series of independent verification gates: the test
suite, mechanical line-coverage and complexity-vs-coverage (CRAP) thresholds,
a spec-conformance review, and finally mutation testing — deliberately
planted bugs that the tests are required to catch. A surviving mutant means
the tests are too weak, and the rule is absolute: strengthen the test, never
bend the code to satisfy the tool. A per-function baseline recorded before
each feature guarantees the codebase never gets quietly worse.

Bug fixes and trivial edits take lighter roads. See
[KEELER.md](KEELER.md) for the full picture with diagrams, and
[.claude/keeler.md](.claude/keeler.md) for the rules the AI operates under.

## Quick start

- **Feature:** `/keeler:spec → approve → /keeler:tasks → /keeler:tdd → /keeler:qa → /keeler:review → /keeler:mutants` —
  or `/keeler:feature <problem>` to run the whole examination end to end.
- **Bugfix:** `/keeler:fix` — reproduce with a failing regression test first, then
  fix minimally.
- **Trivial** (docs, comments, config): fast path — `just lint`, no spec,
  no ceremony.

See *Change classes* in [.claude/keeler.md](.claude/keeler.md) for how to pick the right road.


## Install

From inside any Rust project — existing or freshly `cargo new`-ed:

```bash
curl -fsSL https://raw.githubusercontent.com/minikin/keeler/main/install.sh | bash -s .
```

Prefix it with `KEELER_REF=v0.1.0` to pin a release instead of tracking
`main`. Later, `just keeler-upgrade` re-runs the installer in place; the
version you have is recorded at the top of `.claude/keeler.md`.

|              | What it installs                                                                                       |
| ------------ | ------------------------------------------------------------------------------------------------------ |
| **Tools**    | `cargo-nextest`, `cargo-llvm-cov`, `cargo-mutants`, `cargo-crap`, `just` — only the ones you're missing |
| **Workflow** | slash commands, skills, spec template, `Justfile`, gate configs, and a `keeler.yml` CI workflow         |
| **Manifest** | `proptest` as a dev-dependency, plus the `[profile.mutants]` and `[lints.clippy]` sections              |
| **Ignores**  | the generated artifacts, appended to `.gitignore`                                                       |

- Re-running is safe — nothing is duplicated and nothing of yours is
  overwritten; a conflicting file is copied alongside as `<name>.keeler`.
- An existing `CLAUDE.md` keeps every word: the rules install as
  `.claude/keeler.md` and a single `@.claude/keeler.md` import is appended.
- `--no-tools` skips the tool installs. Prefer to read the script first?
  `git clone https://github.com/minikin/keeler && ./keeler/install.sh <project>`
  does exactly the same.

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
- [cargo-mutants](https://mutants.rs) — mutation testing (the control questions)
- [proptest](https://proptest-rs.github.io/proptest/) — property-based tests

## Layout

- `specs/` — Gherkin specs (Given/When/Then), one per feature; `TEMPLATE.md` is the starting point
- `src/` — implementation with unit + property tests inline
- `tests/acceptance.rs` — one acceptance test per spec scenario
- `.claude/keeler.md` — the workflow rules, imported by `CLAUDE.md` so your own instructions stay untouched
- `.claude/commands/keeler/` — the workflow slash commands, invoked as `/keeler:spec`, `/keeler:fix`, …
- `.claude/skills/` — self-triggering knowledge: property-testing, gherkin-specs
- `.github/workflows/ci.yml` — the same gates as physics, not discipline (installs as `keeler.yml` in your project)
