---
description: Bugfix pipeline — reproduce with a failing test first, then fix minimally
argument-hint: <bug description or failing behavior>
---

Bug to fix: $ARGUMENTS

You are on the **bugfix path** (see .claude/keeler.md § Change classes). No spec is drafted for a bug — but the discipline is stricter in one way: **no fix may be written before the bug is reproduced by a failing test.**

1. **Understand & reproduce.** Restate the bug: expected vs actual behavior. Write a regression test that fails on the current code — named after the bug's behavior, placed where it belongs (unit test next to the code, or acceptance test if the bug is user-observable). The test must be **tight**: it fails on the user's exact symptom — not a different failure that happens to be nearby — gives the same verdict every run (a flaky bug: a pinned, high reproduction rate — see below), and finishes in seconds. Reproduction is claimed only by showing it: the `cargo nextest run --workspace -E 'test(<name>)'` invocation and its red output, actually run, **red for the right reason** — the asserted symptom, not a compile error or a broken fixture — because a test written but not run, or red for any other reason, is not a reproduction. A flaky bug is reproduced by raising its rate until it is debuggable — a proptest with enough cases to hit it reliably, its found case committed as a seed file (where those land is in .claude/keeler.md § Testing conventions) — never by rerunning until it happens to fail. If you cannot reproduce, stop and report — never "fix" what you can't observe.
2. **Minimise.** Shrink the repro to the smallest scenario that still goes red: cut inputs, setup and data one at a time, re-running the test after each cut. Done when every remaining element is load-bearing — removing any one turns the test green. Minimisation shrinks the scenario, never the seam: the test stays where step 1 placed it, so a user-observable bug keeps its acceptance test. The minimal repro is the regression test worth keeping, and what is left standing points at the cause.
3. **Check the spec.** Find the spec covering this behavior:
   - If the bug **contradicts an approved scenario** — it's a plain defect; the scenario is your oracle.
   - If the spec is **silent** (the bug lives in unspecified behavior) — propose a one-scenario spec amendment and wait for approval before fixing; the fix would otherwise encode an undecided requirement.
4. **Hypothesise before touching code.** Rank 3–5 falsifiable hypotheses, each stating its prediction: "if X is the cause, then changing Y makes the test pass." A hypothesis with no prediction is a hunch — discard or sharpen it. Show the ranked list to the user, who often knows what re-ranks it ("we changed that yesterday"), but proceed with your ranking rather than blocking on an answer. A single hypothesis is the failure this step prevents: anchoring on the first plausible idea. When minimisation has already cornered the cause — one load-bearing element, one candidate — say so and move on; the ranking is for bugs whose cause is still open.
5. **Fix minimally.** The smallest change that makes the regression test pass without breaking others. Resist drive-by refactoring — if you see unrelated debt, note it in the summary instead.
6. **Gates.** Run `just dev`, then `just mutants-diff`. Surviving mutants on the fixed lines mean the regression test is weaker than it looks — strengthen it (see /keeler:mutants).
7. **Report** (English, per .claude/keeler.md): the reproduction, root cause, the fix, gate results, and anything discovered along the way. Do not commit without confirmation.

<!-- Steps 1, 2 and 4 adapt mattpocock/skills' diagnosing-bugs (MIT, (c) Matt Pocock). -->
