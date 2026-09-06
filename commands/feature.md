---
description: Run the full feature pipeline — spec → tasks → TDD → QA → review → mutants
argument-hint: <problem description>
---

Read `${CLAUDE_PLUGIN_ROOT}/keeler.md` before anything else — the workflow rules this command runs end to end.

Problem: $ARGUMENTS

Orchestrate that workflow for this problem, stage by stage. Each stage follows its own command's instructions exactly — invoke `/keeler:<stage>` rather than working from memory of what it says:

0. **Baseline** — run `keeler crap-baseline` to snapshot the current CRAP scores; the final QA will diff against it.
1. **Spec** — follow /keeler:spec: analyze the problem, interview in rounds until the frontier is empty, draft `specs/NN-<slug>.md`. **Hard stop: wait for the user to approve the spec.** Do not proceed on your own. /keeler:spec then asks which road: on **"graph"** this orchestration stops after /keeler:tasks and hands back — commit the graph, then `keeler keeler-fan-out <spec>` — because the loop below is the linear road's, one task at a time in order, and a wave is not that.
2. **Tasks** — follow /keeler:tasks: break the approved spec into TDD tasks with scenario mapping.
3. **TDD** — follow /keeler:tdd for each task in order: red → green → refactor, one task at a time. Give a one-line progress update per completed task.
4. **QA** — follow /keeler:qa: `keeler dev`, coverage inspection, results table.
5. **Review** — follow /keeler:review: spec conformance, correctness, test quality, simplification. Present findings to the user; apply agreed fixes via the /keeler:tdd discipline and re-run /keeler:qa.
6. **Mutants** — follow /keeler:mutants: mutation tests on changed files; strengthen tests and loop (tests → /keeler:qa → mutants) until zero survivors.

Finish with a summary: spec file, tasks completed, tests added (unit/property/acceptance counts), coverage, worst CRAP score, the CRAP delta vs the stage-0 baseline (regressed/improved/new), mutation results per round, and review findings with their resolutions.
