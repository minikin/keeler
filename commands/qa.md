---
description: Run the full quality gate — fmt, lint, tests, coverage, CRAP
---

Read `${CLAUDE_PLUGIN_ROOT}/keeler.md` before anything else — the workflow rules this command is a stage of. Then read `${CLAUDE_PLUGIN_ROOT}/gates.md`: it holds the gate table, the bar each gate is held to, and the baseline discipline.

You are in the **QA stage**. Run the full fast gate and report the results honestly — never hide or hand-wave a failure.

1. Run `keeler dev` (fmt → lint → nextest + doc tests → coverage → cargo-crap).
2. If any step fails, show the actual output, diagnose, fix, and re-run from the failed step. Coverage/CRAP failures are usually missing tests, not broken code — prefer adding tests over restructuring.
3. Run `keeler cov` and inspect the summary: changed code must have no uncovered lines. If lines in this change are uncovered, write tests for them (test-first) and re-run.
4. If `crap-baseline.json` exists, run `keeler crap-delta` and include the per-function delta (regressions / improvements / new) in the report. A regression fails the gate: decompose the offending function or cover it better, then re-run.
5. Report a compact results table:

| Gate                           | Result                                   |
| ------------------------------ | ---------------------------------------- |
| fmt                            | ✅ / ❌                                    |
| clippy (-D warnings, pedantic) | ✅ / ❌                                    |
| tests (nextest + doc)          | ✅ / ❌ (N passed)                         |
| coverage                       | % lines, uncovered lines in changed code |
| CRAP                           | worst function + score                   |
| CRAP delta (if baseline)       | regressed / improved / new counts        |

6. Close with the status the rules' § Reporting defines: **PASS** if every gate is green; **FAIL + gate name** otherwise; **FLAKY** if a signal was unstable.

**Next stage: `/keeler:review`.** Green gates say the code does what it does; they say nothing about whether that is what the spec asked for. Only review reads the two against each other.
