# Spec 08 — The pipeline enforces itself

**Status:** Approved
**Effort:** Medium
**Module:** `xtask/`, `.github/workflows/ci.yml`, `Justfile`, `reviews/BACKLOG.md`, `.claude/keeler.md`

## Context

Every stage of this pipeline leaves evidence whose absence is loud —
except one, and it is enforced in only half the territory. Graph mode
gave review a record: `/keeler:review` writes
`reviews/<spec-slug>/<task-id>.md`, and the shipped workflow fails a
`keeler/*` pull request whose record is missing or names a commit the
branch did not make. But that check guards only spawned task branches,
and — as writing this spec revealed — it does not run in Keeler's own
repository at all: the job lives in `templates/keeler.yml` and was never
adopted into `.github/workflows/ci.yml`. On the linear road and on main
there is nothing: a task ticked without a review is indistinguishable
from a finished one. Spec 06's T1 and T2 are ticked on main today with
no record, and nothing noticed — the exact failure, again, that the
review stage's own history warned about.

A previous attempt at this gate was built and parked
(`feat/pipeline-enforces-itself`, spec then numbered 06) with three
blocking defects: it panicked on a duplicate backlog line, it could not
run under CI's shallow clone, and the record format it demanded was
documented nowhere. Two of the three have since been dissolved by other
work — the record format is documented in `/keeler:review` and in use
seventeen times over, and the shallow-clone problem disappears entirely
once the gate stops checking ancestry (below). What remains is smaller
than what was parked.

**Why the gate on main cannot check ancestry — and does not need to.**
Pull requests land on main by squash, which rewrites SHAs: fifteen of
the seventeen records on main name a `Commit:` that is not in main's
history, and a fresh clone may not hold those objects at all. Ancestry
is therefore a pull-request-time property, and the shipped `review-record`
job already checks it there, under `fetch-depth: 0`. The main-side gate
checks what main can see: every ticked task has a well-formed record
whose verdict is `pass`, or a line in the accepted-debt list. No git
history is consulted, so the gate runs identically in a worktree, a
shallow clone, and CI.

**The debt is real and is recorded, not forgiven.** Thirty-seven tasks
across specs 01–04 were ticked before any record existed, and spec 06's
T1 and T2 joined them. All thirty-nine go into `reviews/BACKLOG.md` —
the `crap-baseline.json` idiom: a committed reference the gate measures
against, where every addition is a reviewable diff and removing a line
is the goal. The three areas of specs 01–04 have since been reviewed by
area (CONTRIBUTING.md records the outcomes), so the historical lines are
debt of *record*, not of *review*; 06-T1 and 06-T2 are debt of both and
are worked off first.

**What this can and cannot guarantee.** It guarantees that no task is
ticked, no spec marked Implemented, and no release cut without a review
record or an explicit, diff-visible debt entry. It cannot guarantee the
review was any good — a record can be written carelessly. Enforcing that
a step happened is mechanical; enforcing that it was done well is not,
and claiming otherwise would be the comfortable lie this spec exists to
remove.

**Rejected alternatives.** *Trusting the rules file*: that is what
failed, twice now. *Checking ancestry on main*: impossible under squash
merges, see above; the alternative of banning squash merges taxes every
merge for one check's benefit. *Shipping the gate to adopters*: it lives
in `xtask`, repository machinery adopters do not have; their linear road
stays documented and unenforced, and the rules must say so plainly
rather than imply otherwise.

---

## Acceptance Tests

### Scenario: A task ticked without a review is caught

```
Given a spec whose task is ticked
And   no review record for that task
And   no line for it in the accepted-debt list
When  `cargo xtask pipeline-check` runs
Then  it fails, naming the spec and the task
```

### Scenario: A reviewed task passes, and the gate says what it counted

```
Given ticked tasks each holding a record whose verdict is pass
When  the gate runs
Then  it passes, and says how many ticked tasks it accounted for
```

### Scenario: A fail verdict contradicts a ticked box

```
Given a ticked task whose review record says `Verdict: fail`
When  the gate runs
Then  it fails, naming the record and the contradiction
```

### Scenario: A malformed record is refused, not misread

```
Given a review record missing one of its four header lines
When  the gate runs
Then  it fails, naming the file and the line it lacks
```

### Scenario: The gate needs no git history

```
Given a record whose `Commit:` names a SHA absent from this clone
When  the gate runs
Then  the record still counts — ancestry belongs to the pull-request
      check, and the gate consults no history at all
```

### Scenario: Accepted debt is explicit, and adding to it is a diff

```
Given tasks ticked before this gate existed, listed in the committed
      backlog
When  the gate runs
Then  it accepts exactly those and no others
And   a ticked task on neither the backlog nor the records is refused
```

### Scenario: A duplicate backlog line is refused, naming the line

```
Given a backlog holding the same task twice
When  the gate runs
Then  it fails, naming the duplicated line — it does not panic
```

### Scenario: An Implemented spec vouches for every task

```
Given a spec marked Implemented with a task unticked or unreviewed
When  the gate runs
Then  it fails, naming the spec and the task
```

### Scenario: A release refuses a pipeline that was skipped

```
Given a repository where the gate fails
When  the release guard runs
Then  it refuses, and its message names the failing task
```

### Scenario: The gate is one recipe away, everywhere the gates live

```
Given the Justfile and the CI workflow of this repository
When  `just dev` runs, and when CI runs on a push or pull request
Then  each runs `cargo xtask pipeline-check`
```

### Scenario: This repository runs the check it ships

```
Given the `review-record` job that `templates/keeler.yml` ships
When  Keeler's own CI workflow is read
Then  it carries the same job, guarding `keeler/*` pull requests here
```

### Scenario: The rules stop claiming nothing can notice

```
Given the shipped workflow rules' review-stage warning
When  the rules are read
Then  they say the gate runs in the repository that ships Keeler
And   that an adopting project's review stage remains unenforced
```

---

## Tasks

Each task lists its scenarios, the test types that pin it, and — when it
depends on earlier tasks — a `Needs:` naming them. The core splits into
one module per input (`xtask/src/pipeline/` — decision, records,
backlog, specs) so the three parsers can be built in parallel without
touching one region. Acceptance tests land in `tests/pipeline.rs`, each
task under its own test functions; the two CI tasks each add their own
job block to `.github/workflows/ci.yml` under its own job name.

- [x] **T1 — The decision: ticked × records × backlog → what is
      missing.** The pure core and its types; coverage means a pass
      record or an unrecorded backlog line. Scenarios: _A task ticked
      without a review is caught_, _A reviewed task passes, and the gate
      says what it counted_. Tests: unit + property (gate soundness: no
      false pass, no false failure).
- [ ] **T2 — The record grammar: four headers, a verdict, or a refusal
      naming the file.** Parsing and validation of
      `reviews/<slug>/<task>.md`; a fail verdict fails its task whatever
      else holds. Needs: T1. Scenarios: _A malformed record is refused,
      not misread_, _A fail verdict contradicts a ticked box_. Tests:
      unit + property (record dominance).
- [ ] **T3 — The backlog: explicit debt, refused when it lies.**
      Parsing `reviews/BACKLOG.md`; duplicates and unparseable lines are
      refusals naming the line. Needs: T1. Scenarios: _Accepted debt is
      explicit, and adding to it is a diff_, _A duplicate backlog line
      is refused, naming the line_. Tests: unit + property (debt
      monotonicity).
- [x] **T4 — The spec reader, and the promise Implemented makes.**
      Ticked tasks from every `specs/*.md` but the template; an
      Implemented spec with an unticked or uncovered task is named.
      Needs: T1. Scenarios: _An Implemented spec vouches for every
      task_. Tests: unit + acceptance.
- [ ] **T5 — The command: `cargo xtask pipeline-check`, and the debt on
      the books.** The impure shell over the core; seeds
      `reviews/BACKLOG.md` with the thirty-nine so this repository
      passes its own gate. Needs: T2, T3, T4. Scenarios: _The gate needs
      no git history_. Tests: acceptance.
- [ ] **T6 — The release guard refuses a skipped pipeline.** The guard
      runs the gate before anything is published. Needs: T5. Scenarios:
      _A release refuses a pipeline that was skipped_. Tests: unit +
      acceptance.
- [ ] **T7 — Wired where the gates live.** `just dev` and a CI job of
      its own (`pipeline-check:`) run the command. Needs: T5. Scenarios:
      _The gate is one recipe away, everywhere the gates live_. Tests:
      acceptance.
- [ ] **T8 — This repository runs the check it ships.** The
      `review-record:` job copied verbatim from `templates/keeler.yml`
      into our own workflow. Scenarios: _This repository runs the check
      it ships_. Tests: acceptance.
- [ ] **T9 — The rules stop claiming nothing can notice.**
      `.claude/keeler.md`'s review-stage warning and CONTRIBUTING's
      "Nothing enforces this" both redrawn: enforced here, documented
      for adopters. Needs: T7. Scenarios: _The rules stop claiming
      nothing can notice_. Tests: acceptance.

---

## Implementation Notes

**The gate is a pure function at the core.** Three inputs — the ticked
tasks read from every `specs/*.md` except the template, the parsed
records under `reviews/`, and the backlog — and one output: what is
missing, or a count of what was accounted for. No git, no network, no
clock. The impure shell around it reads files and prints; the decision
is unit- and mutation-testable without a repository.

**A ticked task passes** exactly when it holds a well-formed record with
`Verdict: pass`, or sits on the backlog with no record at all. A record
always outranks the backlog: a `fail` record on a backlogged task still
fails, so debt cannot shelter a known-bad review.

**The record grammar is `/keeler:review`'s**, four header lines — Spec,
Task, Commit, Verdict — then findings. The gate validates shape and
verdict; `Commit:` must be present and non-empty but is never resolved.

**The backlog** is `reviews/BACKLOG.md`, one task per line as
`<spec-slug>/<task-id>`, seeded with the thirty-nine: specs 01–04's
thirty-seven and spec 06's t1 and t2. A duplicate or unparseable line is
a refusal naming the line, never a panic — the parked gate's first
blocker, now a scenario.

**Where it is wired.** `just dev` (after the existing gates — it is
milliseconds), a CI job on push and pull request, and `release-guard`,
which refuses a tag over a failing pipeline. The `review-record` job is
copied from `templates/keeler.yml` into `.github/workflows/ci.yml`
verbatim, `fetch-depth: 0` and all.

**Doc sync.** `.claude/keeler.md`'s review-stage paragraph currently
says "there is no mechanism that will catch this for you"; once the gate
exists that sentence is false in this repository and must instead draw
the boundary: enforced here, documented-only for adopters. CONTRIBUTING's
"Nothing enforces this" paragraph goes the same way.

**Invariants worth property tests.**

- *Gate soundness*: for any set of ticked tasks, records and backlog
  lines, the gate passes exactly when every ticked task is covered —
  no false pass, no false failure.
- *Debt monotonicity*: removing a backlog line never turns a failure
  into a pass.
- *Record dominance*: adding a `pass` record never breaks a passing
  gate, and a `fail` record fails its task whatever the backlog says.

### Non-goals

- Judging review quality. The gate proves a review happened, not that
  it was good.
- Reviewing automatically. The stage exists for human judgement.
- Shipping the gate to adopters. It is `xtask` machinery; the shipped
  rules say plainly that adopting projects get the documented pipeline
  and the `keeler/*` CI check, not this gate.
- Extending the pull-request ancestry check beyond `keeler/*` branches.
- Working off the historical debt here. The backlog makes it visible;
  removing lines is its own deliberate work, 06-t1 and 06-t2 first.
