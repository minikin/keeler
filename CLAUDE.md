# CLAUDE.md

This file provides guidance to Claude Code when working with code in this repository.

## Workflow (THE LAW)

Every feature follows this pipeline. Do not skip stages or reorder them.

```
problem ──▶ /spec ──▶ user approves spec ──▶ /tasks ──▶ /tdd (per task)
                ▲                                            │
                └── back-and-forth until approved            ▼
                                                           /qa  (fmt, lint, tests, coverage, CRAP)
                                                             │
                                                             ▼
                                                          /review
                                                             │
                                                             ▼
                                                         /mutants ──▶ survivors? ──▶ strengthen tests,
                                                             │            │          then /qa + /mutants again
                                                             ▼            ◀──────────────┘
                                                           done (all gates green)
```

1. **/spec** — analyze the problem, draft a Gherkin spec in `specs/`, iterate with the user until they approve it. **Never write implementation code at this stage.**
2. **/tasks** — break the approved spec into ordered tasks, each mapped to its scenarios and test types.
3. **/tdd** — implement one task at a time, strictly test-first (red → green → refactor). Unit tests + property tests (proptest) + acceptance tests per scenario.
4. **/qa** — run the full quality gate: `just dev` (fmt, clippy, nextest, doc tests, coverage, cargo-crap).
5. **/review** — spec-conformance check (scenario→test mapping, scope creep, invariant coverage) done in-project, then the built-in `code-review` skill for generic correctness/simplification — don't hand-roll what it already verifies.
6. **/mutants** — mutation tests on changed files. Surviving mutants mean the tests are weak: strengthen the tests (never weaken the code to satisfy a mutant), then re-run /qa and /mutants until zero survivors.

## Change classes

Not every change is a feature. Pick the lightest class that honestly fits — and never downgrade a change mid-flight to dodge a failing gate:

| Class       | When                                                 | Pipeline                                                                                 |
| ----------- | ---------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| **Feature** | New behavior, changed behavior, new API              | Full: `/spec → approve → /tasks → /tdd → /qa → /review → /mutants`                       |
| **Bugfix**  | Existing behavior is wrong                           | `/fix`: failing regression test first → minimal fix → `just dev` + `just mutants-diff`   |
| **Trivial** | Docs, comments, config, renames — no behavior change | Fast path: `just lint` (plus `just test` if code was touched at all); no spec, no review |

Rule of thumb: if you're debating whether it changes behavior, it's not trivial. If a "trivial" change makes any test fail, it wasn't trivial — reclassify.

## Commits

**Never commit without explicit user confirmation.** Finish the work, run the gates, then ask — the user decides when a commit happens and may want to review the diff first. This applies to every stage, including "obvious" checkpoints like a finished task or a green pipeline.

## Reporting

**Every finished task ends with a summary in English**, regardless of the conversation language. The summary must cover:

- what was done (tests written, code changed, gates run — with real numbers);
- **problems discovered along the way**: failing gates, surviving mutants, spec gaps, pipeline holes, ambiguities — and how each was resolved or why it was left open;
- open questions that need a user decision (e.g. proposed spec amendments);
- what the next step is;
- a closing one-line status: **PASS** (all gates green), **FAIL** (name the gate that failed), or **FLAKY** (unstable signal — rerun before trusting it).

Never bury a discovered problem in the middle of a transcript — it must reappear in the summary even if it was already mentioned when found.

## Specs (THE LAW)

All feature specs live in `specs/`, written Gherkin style (Given/When/Then). Copy `specs/TEMPLATE.md` for new ones; number them `NN-slug.md`.

- **Never modify a spec file without explicit permission from the user.** The one exception is the `Status:` line and task checkboxes — pipeline bookkeeping: /spec sets Approved on the user's approval, /tdd ticks tasks, /mutants sets Implemented when the final gate is clean.
- A spec's Acceptance Tests section IS the acceptance criteria — every scenario must map to at least one test named after it.
- When a spec needs to change (scope change, new edge case), propose the change and wait for approval before editing the file.

## Quality gates

All of these must be green before a feature is considered done:

| Gate       | Command                                               | Bar                                            |
| ---------- | ----------------------------------------------------- | ---------------------------------------------- |
| Format     | `cargo fmt --all -- --check`                          | clean                                          |
| Lints      | `cargo clippy --all-targets -- -D warnings`           | zero warnings (pedantic is on)                 |
| Tests      | `cargo nextest run --all-targets && cargo test --doc` | all pass                                       |
| Coverage   | `just cov`                                            | no uncovered lines in changed code             |
| CRAP       | `just crap`                                           | no function above threshold 15                 |
| Mutation   | `just mutants-diff`                                   | zero surviving mutants in changed files        |
| CRAP delta | `just crap-delta`                                     | no function's CRAP score regressed vs baseline |

**Baseline discipline:** before starting a feature, record a CRAP baseline with `just crap-baseline` (JSON, stable `--sort file`). After the feature, `just crap-delta` shows per-function before/after and fails on any regression — this is the "did the change make the codebase worse?" gate.

## Commands

```bash
just            # list recipes
just test       # nextest + doc tests
just lint       # fmt --check + clippy -D warnings
just ci         # lint + test
just cov        # coverage summary (cargo-llvm-cov)
just crap       # coverage + CRAP gate (cargo-crap, threshold 15)
just crap-baseline  # record CRAP baseline before a feature
just crap-delta     # CRAP before/after vs baseline; fails on regression
just dev        # fmt, lint, test, crap — the full fast gate
just mutants src/lib.rs   # mutation tests for one file
just mutants-diff         # mutation tests on files changed vs HEAD
just dev-full   # dev + all mutants (slow)
```

## Skills

Project skills live in `.claude/skills/<name>/SKILL.md` and load automatically when their description matches the work at hand — no invocation needed:

- **property-testing** — invariant catalog and proptest patterns; fires during /tdd and when a surviving mutant points at a missing law rather than a missing example.
- **gherkin-specs** — how to write observable, testable Given/When/Then scenarios; fires during /spec and the conformance half of /review.

Add new skills the same way: a folder under `.claude/skills/`, frontmatter with `name` and a `description` that says *when* to use it — the description is the trigger.

**Recommended user-level skills** for a Rust project on this workflow (installed globally, not shipped with the repo — install your own equivalents if missing):

- **rust-best-practices** — idiomatic Rust: borrowing vs cloning, `Result` error handling, API design; consult while writing or refactoring any Rust code.
- **clean-code** — naming, function size, structure; the go-to during the REFACTOR step of /tdd.
- **rust-async-patterns** — Tokio, async traits, concurrency; the moment the project grows async code.

## Testing conventions

- Unit tests live in `#[cfg(test)]` blocks next to the code they test.
- Property tests use `proptest` and live in the same blocks — reach for one whenever the code has an invariant (ordering, idempotence, round-trip, saturation, bounds).
- Acceptance tests live in `tests/acceptance.rs` — one test per spec scenario, named after the scenario, structured as Given/When/Then comments.
- Tests are the spec's enforcement arm: when a mutant survives, the fix is a better test, not a code tweak.
- When proptest finds a counterexample it writes a seed file under `proptest-regressions/` — **commit those files**; they are regression tests that pin the found case forever.
