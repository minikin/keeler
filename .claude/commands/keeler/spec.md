---
description: Analyze a problem and draft a Gherkin spec for approval
argument-hint: <problem description>
---

Problem to analyze: $ARGUMENTS

You are in the **spec stage** of the workflow (see .claude/keeler.md). Your job is analysis and specification only — **do not write any implementation code, do not create tasks yet**.

1. **Analyze the problem.** Restate it in your own words: who has this problem, what triggers it, what the desired outcome is. List the assumptions you're making and the constraints you see (performance, error handling, edge cases).
2. **Interview until nothing is silently assumed.** The open questions form a tree — settling one unblocks the ones that hang off it. Work it in rounds:
   - The **frontier** is every question whose prerequisites are already settled. Ask the whole frontier in one round — numbered, each with your recommended answer attached, because the user corrects a proposal faster than they draft an answer from nothing. A question that depends on one still open in this round belongs to a later round.
   - The tree is seeded from the axes the spec must settle: scope, inputs, failure behavior, non-goals. Non-goals are not scenarios — ask about them directly.
   - **Facts are yours to find** — the code, the existing specs, the git history. The user is asked only for decisions — and anything that changes the shape of the solution is a decision, however much it looks like a fact.
   - Phrase each question as the scenario it will become — "what observably happens when …?" — so the settled frontier reads straight into the Acceptance Tests.
   - Recompute the frontier after each round of answers. The interview is done when it is empty: every branch visited, nothing assumed. A problem small enough to have no open questions is done after zero rounds — this step sizes itself.
   - If the user says to proceed, or stops answering, proceed on your recommended answers — recording every question that never settled as an explicit assumption in the spec's Context, where approval will confirm or overturn it.
3. **Draft the spec.** Copy `specs/TEMPLATE.md` to `specs/NN-<slug>.md` (next free number, kebab-case slug). Fill in:
   - **Context** — the problem analysis from step 1.
   - **Acceptance Tests** — Gherkin scenarios (Given/When/Then) covering the happy path, each edge case, and each failure mode. Scenarios must describe observable behavior, not implementation.
   - **Implementation Notes** — approach sketch, key types, invariants worth a property test, and explicit **Non-goals**.
   - Leave the **Tasks** section empty — that's for /keeler:tasks after approval.
   - Set **Status: Draft**.
4. **Present the spec** to the user: summarize it briefly and point out the decisions you made and the ones you left open.
5. **Stop and wait for approval.** Iterate on feedback until the user approves. On approval, set **Status: Approved**. Only then may /keeler:tasks be run.
6. **Ask which road, and wait for the answer.** With the spec approved, ask one question — develop this feature **linearly**, or **as a graph**? — and say what each answer does before it is answered. Stop there: nothing below happens until the user has answered.
   - **linearly** — one agent, one branch, the stages in sequence. Hand off to /keeler:tasks and do nothing else: no branch, no commit.
   - **graph** — one agent per task, in parallel (see the graph-mode section of .claude/keeler.md). This answer creates `feat/<spec-slug>` and **commits the approved spec** there, so the answer is the consent for that commit; on any other answer, create nothing and commit nothing.

     Run `just keeler-feature-branch <spec>` — it cuts the branch from main, checks it out and commits the spec, or reuses the branch if it is already there. Then invoke /keeler:tasks, and when it has written the graph, hand back with the next steps in order: **commit the graph** /keeler:tasks wrote (fan-out and spawn read the committed spec, never the working tree), then `just keeler-fan-out <spec>`.

<!-- Step 2's interview mechanic adapts mattpocock/skills' grilling (MIT, (c) Matt Pocock). -->
