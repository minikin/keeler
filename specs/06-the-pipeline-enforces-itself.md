# Spec 06 — The pipeline enforces itself

**Status:** Draft
**Effort:** Medium
**Module:** `keeler/`, `xtask/`, `.claude/commands/keeler/`, `.claude/keeler.md`

## Context

Every stage of this pipeline leaves evidence except one. TDD leaves tests.
QA leaves a gate result. Mutants leave a number. **Review leaves nothing** —
and what leaves nothing can be skipped without anyone noticing.

It was. Twenty-odd tasks across specs 02, 03, 04 and 05 went
`tdd → qa → mutants`, with the review stage cut out entirely, and the gap
surfaced only because someone asked. Not one gate, test or report was
missing afterwards; the specs read as complete.

The cause is structural, not personal, which is why discipline is the wrong
fix:

- **The routing sends you past it.** `.claude/commands/keeler/tdd.md` step 6
  says to report *"which task is next"*. It names the next **task**, not
  the next **stage**. Working task by task — the normal way — the path from
  TDD never passes review. Only `/keeler:feature` lists all six stages, and
  nothing forces its use.
- **The bookkeeping lies early.** `tdd.md` step 5 ticks the task's
  checkbox. The spec records "done" when one stage of four has run, so an
  unreviewed task is indistinguishable from a finished one.
- **Nothing is missing when review is skipped.** There is no artifact whose
  absence could fail anything.

This is a defect in the deliverable, not only in this repository's habits:
the same commands install into every adopting project, so the same hole is
shipped with them.

**What changes.** Review produces a record, tied to the commit it examined.
A gate refuses any task ticked without one. The checkbox moves to the last
stage, so an unticked box means unfinished rather than unstarted, and each
command names the stage that follows it.

**What this can and cannot guarantee.** It can guarantee that no task is
marked done, no spec marked implemented, and no release cut without a
review record naming a real commit in this history. It **cannot** guarantee
the review was any good — a record can be written carelessly. Enforcing
that a step happened is mechanical; enforcing that it was done well is not,
and claiming otherwise would be the same kind of comfortable lie this spec
exists to remove.

**The debt is real and must be visible.** Four specs are already Implemented
with tasks that were never reviewed. Backfilling honest reviews for all of
them is worth doing but is not this spec's job; hiding them is not an
option either. They are recorded as accepted debt in a committed list —
the same idiom as `crap-baseline.json`: a shared reference that the gate
measures against, where every addition is a visible, reviewable diff rather
than a silent exemption.

**Rejected alternatives.** *Trusting the rules file*: that is exactly what
failed. *Folding review into QA*: the two ask different questions — QA asks
whether the gates are green, review asks whether the code answers the spec
— and merging them would lose the second. *A pre-commit hook*: local, easy
to bypass, invisible in CI, and not something adopters inherit. *Reviewing
in CI automatically*: a review nobody reads is theatre, and it would remove
the human judgement the stage exists for.

---

## Acceptance Tests

### Scenario: A task ticked without a review is caught

```
Given a spec whose task is ticked
And   no review record for that task
When  the pipeline gate runs
Then  it fails, naming the spec and the task
```

### Scenario: A review record must name a commit that exists

```
Given a review record whose commit is not in this repository's history
When  the pipeline gate runs
Then  it fails, naming the record and the commit it could not find
```

### Scenario: A reviewed task passes

```
Given a ticked task with a review record naming a commit in this history
When  the pipeline gate runs
Then  it passes, and says how many tasks it accounted for
```

### Scenario: Accepted debt is explicit, and adding to it is a diff

```
Given tasks ticked before this gate existed
When  they are listed in the committed backlog
Then  the gate accepts exactly those and no others
And   a task not on the list is still refused
```

### Scenario: A spec cannot be implemented with an unreviewed task

```
Given a spec marked Implemented
When  the pipeline gate runs
Then  every one of its tasks has a review record or the gate fails
```

### Scenario: A release refuses a pipeline that was skipped

```
Given a repository whose pipeline gate fails
When  the release guard runs
Then  it refuses, naming the unreviewed task
```

### Scenario: The checkbox means the whole pipeline ran

```
Given the shipped pipeline commands
When  they are read
Then  the task's checkbox is ticked by the last stage, not the first
And   no stage tells the reader to move to the next task before the
      pipeline for the current one has finished
```

### Scenario: Every stage names the one that follows it

```
Given each shipped pipeline command
When  its closing instruction is read
Then  it names the next stage of the pipeline
And   the last stage is the only one that names the next task
```

---

## Tasks

_Empty until /keeler:tasks runs against an approved spec._

---

## Implementation Notes

**The record.** One file per reviewed task, `reviews/NN-TX.md`, carrying the
spec, the task, the commit reviewed, and the findings — including an
explicit "none", because a review that found nothing still happened. The
commit is what makes the record hard to fake in passing: it must be an
ancestor of `HEAD`, so a copied or invented record fails.

**The gate.** A function over three inputs — the specs' ticked tasks, the
review records, and the accepted-debt list — returning what is missing.
Pure, so it is unit-testable and mutation-testable without a repository;
the git lookup that decides whether a commit exists is the only impure
part and lives at the edge.

**Where it lives.** In the `keeler` library, because adopters need it as
much as this repository does. This repository reaches it through
`cargo xtask` until spec 05 gives the binary a command line; adopters get
it when they get the binary. Until then their pipeline is documented but
unenforced — worth stating plainly rather than implying otherwise.

**The routing fixes.** `tdd.md` step 6 names `/keeler:qa` as the next
stage. The checkbox moves from `tdd.md` to `mutants.md`, which already owns
the spec's `Status:` line, so both marks of completion are made by the
stage that can honestly make them. `.claude/keeler.md` says so too, since
it currently states the opposite.

**Invariants worth property tests.** Gate soundness: for any set of ticked
tasks and records, the gate passes exactly when every ticked task has a
record or is on the debt list — no false pass, no false failure. Debt
monotonicity: removing an entry from the debt list can only turn a pass
into a failure, never the reverse.

### Non-goals

- Judging review quality. The gate proves a review happened, not that it
  was good.
- Reviewing automatically. The stage exists for human judgement; a
  generated review nobody reads is worse than none, because it looks like
  coverage.
- Backfilling reviews for specs 01–04 here. The debt is recorded so it is
  visible and can be worked off deliberately.
- Enforcing the other stages. TDD, QA and mutants already leave evidence
  whose absence is loud.
