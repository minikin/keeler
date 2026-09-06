# Keeler — quality gates

The chapter the rules point at: the table, the bars, and what does and
does not enforce the review stage. It is here rather than in `keeler.md`
so the rules fit the `SessionStart` hook's ceiling whole; the rules keep
the one line that says every gate below must be green.

Read it on instruction: `/keeler:qa`, `/keeler:mutants` and
`/keeler:review` each open by reading this file.

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

**The review stage is the one that can be skipped quietly.** Every other
gate in this table fails loudly when it is not met. Review does not: a
skipped review leaves no artifact whose absence anything trips over, and
in the repository that ships Keeler it was skipped for twenty tasks
before anyone noticed. That repository now catches it — `cargo xtask
pipeline-check` refuses a ticked task holding neither a
`reviews/<spec-slug>/<task-id>.md` record whose `Verdict:` is `pass` nor
a line of accepted debt, and `just dev`, CI and the release guard all run
it. **Your project does not get that gate.** It is repository machinery
Keeler does not install, so on the linear road your review stage stays
documented rather than enforced; in graph mode the `keeler/*`
pull-request check in `.github/workflows/keeler.yml` is the part of it
you do have. Everywhere else the mechanism is the pipeline itself, whose
commands lead from each stage to the next, and the discipline to follow
them.

**Baseline discipline:** `crap-baseline.json` is **committed to the repository** — it is the shared reference every developer and CI measures against, so the ratchet works for the whole team, not just one machine. `just crap-delta` shows per-function before/after and fails on any regression: the "did this change make the codebase worse?" gate. Refresh the baseline (`just crap-baseline`) only deliberately, in its own commit — a moved baseline is a visible decision, reviewable like any other diff.
