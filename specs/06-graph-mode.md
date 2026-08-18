# Spec 06 — Graph mode: parallel agents over a task DAG

**Status:** Approved
**Effort:** Large
**Module:** `.claude/commands/keeler/` (`graph.md` new; `tasks.md`, `review.md`, `mutants.md`, `feature.md` amended), `.claude/keeler.md`, `templates/keeler.yml`, `Justfile`, `scripts/keeler-graph.sh`, `install.sh`, `specs/TEMPLATE.md`, `tests/graph.rs`

## Context

Keeler today is a linear pipeline per feature: spec → tasks → tdd → qa →
review → mutants. One agent, one branch, stages in sequence. That shape is
right for the quality axis — but it leaves the throughput axis on the
table. The emerging practice around Claude Code is **graph engineering**:
decompose a feature into a DAG (Directed Acyclic Graph) of tasks, fan
independent tasks out to parallel headless agents in isolated worktrees,
and fan back in at merge points. Most people doing this have the
parallelism and nothing else — no shared contract between agents, no
machine-verifiable definition of done, review by vibes. Keeler has exactly
the missing pieces: the approved spec is the contract every spawned agent
reads, and the gates make each branch's "done" checkable without a human
watching it happen.

What changes: the Tasks section of a spec stops being a flat checklist and
becomes a dependency graph. A new command reads that graph and tells you
what is unblocked. A `Justfile` recipe turns "run task T3" into a worktree,
a branch, and a headless Claude Code session pointed at the spec. The
diff-based gates — already the branch-friendly ones — become the per-branch
contract; whole-repo state (`crap-baseline.json`, the coverage ratchet)
moves explicitly to a merge-time concern on the integration branch. And
review, the one stage that leaves no evidence, starts leaving some: a
committed review file per branch, because at fan-in of five branches
"nothing noticed review was skipped" stops being a footnote and becomes the
failure mode.

**Why the spec stays the single source of truth.** The alternative — a
separate `graph.yml` next to the spec — was rejected: two files describing
one feature drift, and the human approves the spec, not a sidecar. The
dependency annotations live in the Tasks section the human already reads
and approves. Approving the spec *is* approving the graph.

**Why worktrees, not clones.** Clones re-download and lose the shared
object store; worktrees are instant, share `.git`, and make branch cleanup
one command. The cost — one checkout per task on disk — is the point:
agents must not share a working directory.

**Rejected alternatives.** An orchestrator daemon that schedules the whole
graph automatically: rejected for now — the human choosing when to fan out
and when to merge is a feature, not a gap, and a scheduler is a place for
bugs to hide from the gates. Parallelism inside one session via subagents:
useful for exploration, but subagents share a working tree and cannot each
run the full gate suite in isolation.

---

## Acceptance Tests

### Scenario: The tasks stage emits a dependency graph

```
Given a spec whose Tasks section was produced by /keeler:tasks
When  the Tasks section is read
Then  every task carries an id, a needs list (possibly empty), and its
      scenarios and test types as before
And   the needs lists reference only task ids defined in the same spec
And   the graph they form is acyclic
```

### Scenario: A malformed Tasks section is refused naming the line

```
Given a spec whose Tasks section has a Needs: naming an id no task
      defines, or two items opening with the same id, or one item carrying
      two Needs: lines
When  `just keeler-graph` runs against it
Then  it fails naming the line and what is wrong with it
And   nothing is reported as ready
```

### Scenario: Graph status names what is unblocked

```
Given a spec with tasks T1..T4 where T2 and T3 need T1, and T1 is checked off
When  /keeler:graph runs against the spec
Then  it reports T2 and T3 as ready and T4 as blocked with its unmet needs
And   it reports nothing as ready when the graph is complete
```

### Scenario: A cycle is refused loudly

```
Given a spec whose Tasks section contains T1 needing T2 and T2 needing T1
When  /keeler:graph runs against the spec
Then  it fails naming the cycle
And   no task is reported as ready
```

### Scenario: Spawning a task creates an isolated agent

```
Given a spec with an unblocked task T3, and tmux on PATH
When  `just keeler-spawn <spec> T3` runs
Then  a worktree exists outside the repository root on branch keeler/<spec-slug>/t3
And   a detached tmux session named keeler-<spec-slug>-t3 is running a
      Claude Code session in it, whose prompt names the spec file, the
      task id, and the whole per-task pipeline — tdd, qa, review, mutants,
      with `just keeler-branch` as its gate
And   the recipe returns at once, printing how to attach
And   the main working tree is untouched
```

### Scenario: A spawned agent commits on its branch, and nowhere else

```
Given a task spawned by `just keeler-spawn`
When  its pipeline finishes a stage
Then  the work is committed on keeler/<spec-slug>/<task-id>, in that
      worktree, so the review record has a commit to name and the
      worktree is clean when the task lands
And   nothing is pushed: the branch reaches the remote, and a pull
      request, only by the human's hand
And   the shipped rules say this is the one place an agent commits
      without asking, and why the spawn was the asking
```

### Scenario: A finished agent leaves a verdict the gate decided, and a log

```
Given a spawned session that has exited
When  its tmux session is gone
Then  .keeler/runs/<spec-slug>/<task-id>.exit holds the exit code of
      `just keeler-branch`, run after the agent finished — not the agent's
      own, which is zero for a turn that ended in FAIL
And   .keeler/runs/<spec-slug>/<task-id>.log holds everything the session
      printed, so the run can be read after the window is gone
And   `just keeler-status <spec>` lists each task as running, passed or
      failed, deciding "running" by asking tmux and not by the absence of
      a file
```

### Scenario: A dead session is resumable, and says so

```
Given a spawned session that died before its pipeline finished — a usage
      limit, a killed terminal, a reboot
When  `just keeler-status <spec>` runs
Then  it distinguishes a task that died mid-pipeline from one that failed
      its gate: the first has no .exit at all, the second has a non-zero
      one
And   it names the log and the worktree, which together are what a resume
      reads — the commits already on the branch say how far the pipeline
      got
```

### Scenario: Spawning without tmux is refused, and says how to get it

```
Given a machine without tmux
When  `just keeler-spawn <spec> T3` runs
Then  it refuses before creating anything, naming tmux and how to install it
```

### Scenario: Spawning from an uncommitted or unapproved spec is refused

```
Given a spec whose working-tree copy differs from HEAD, or whose Status:
      is not Approved
When  `just keeler-spawn <spec> T3` runs
Then  it refuses before creating anything, saying which — the worktree is
      cut from HEAD and would not see an uncommitted graph, and an
      unapproved spec is not a contract any agent should build from
```

### Scenario: Spawning from anywhere but the feature's branch is refused

```
Given a checkout on any branch other than feat/<spec-slug> for the spec
      being spawned from
When  `just keeler-spawn <spec> T3` runs
Then  it refuses before creating anything, naming the branch it expected
And   no worktree, branch or run directory is created
```

### Scenario: Spawning a task that is already spawned is refused

```
Given a worktree for T3 already exists, whatever state it is in
When  `just keeler-spawn <spec> T3` runs again
Then  it refuses naming the path, and creates nothing
```

### Scenario: Landing cleans up only what is clean

```
Given a landed task whose worktree has no uncommitted changes
When  `just keeler-land` finishes
Then  the worktree and the branch are removed
And   a worktree with uncommitted changes is left in place and named,
      for the human to look at first
```

### Scenario: Spawning a blocked task is refused

```
Given a spec where T4 needs the unfinished T1
When  `just keeler-spawn <spec> T4` runs
Then  it refuses, naming the unmet dependency
And   no worktree or branch is created
```

### Scenario: Branch gates are diff-based only

```
Given a task branch with changes, and the bytes of crap-baseline.json
      and the Justfile's cov recipe as they were on main
When  `just keeler-branch` runs on that branch
Then  the full local gate runs, then crap-delta, then mutants-diff
And   afterwards crap-baseline.json and the cov recipe are byte-identical
      to what they were — a branch measures against the baseline and
      never moves it
```

### Scenario: Baseline updates happen at fan-in, on main

```
Given a task branch that passed its gates and was merged
When  `just keeler-land` runs on main
Then  `just dev` runs on main
And   crap-baseline.json is regenerated and staged, not committed, and the
      run says to review the diff and commit
And   the working tree holds exactly that staged change and nothing else
```

### Scenario: keeler-land refuses to run anywhere but main

```
Given the current branch is keeler/06-graph-mode/t3
When  `just keeler-land` runs
Then  it refuses before running any gate, naming the branch it is on
And   crap-baseline.json and the spec are untouched
```

### Scenario: A branch that moved the baseline is refused by CI

```
Given a keeler/* branch whose diff against main touches crap-baseline.json
      or the Justfile's cov recipe
When  the shipped CI workflow runs on its pull request
Then  it fails naming the file — baselines move only at land time, on main
```

### Scenario: A branch that was green alone can still redden main

```
Given two task branches that each passed their gates
When  the second is merged and `just keeler-land` runs on main
Then  `just dev` runs on main before anything else
And   if it fails, the baseline is left exactly as it was, nothing is
      staged, and the run exits non-zero saying main is red after fan-in
```

### Scenario: A branch ticks its task and nothing else

```
Given a task branch keeler/06-graph-mode/t2 whose pipeline reached
      /keeler:mutants with zero survivors
When  the stage finishes
Then  T2's checkbox is ticked in the spec on that branch
And   the spec's Status: line is unchanged
And   `just keeler-graph` on feat/06-graph-mode still reports T2 as not
      done, because readiness is read from the feature branch and not
      from an unlanded task branch
```

### Scenario: Landing the last task marks the spec implemented

```
Given a spec on main whose every task is ticked
When  `just keeler-land` runs and the gates are green
Then  the spec's Status: is set to Implemented and staged, alongside the
      baseline, for the same human commit
And   a spec with any task unticked is left as it was
```

### Scenario: Review leaves evidence

```
Given a task branch ready for fan-in
When  the review stage's command is read
Then  it instructs writing reviews/<spec-slug>/<task-id>.md with a header
      of Spec:, Task:, Commit: and Verdict:, followed by the findings
And   Commit: is the SHA the review examined
```

### Scenario: A review record must name a commit on its own branch

```
Given a keeler/* branch pull request
When  the shipped CI workflow runs on it
Then  it fails if reviews/<spec-slug>/<task-id>.md is missing
And   it fails if the record's Commit: is not an ancestor of the branch
      head, or is an ancestor of main — a record copied from another task
      names a commit that is not here, and one that names the merge base
      names work the branch has not done
And   a record whose Commit: is a commit the branch itself made passes
```

### Scenario: Adopters opt in, not out

```
Given a fresh Rust project
When  Keeler is installed into it
Then  the graph command, the spawn, status, branch and land recipes, and
      the review-evidence check land alongside the existing pipeline
And   a spec written in the old format — no Needs: on any task — is read
      by `just keeler-graph` with every task reported ready
And   `just dev` is the recipe it was, and /keeler:feature routes as it did
```

---

## Tasks

The reading side comes first: the graph exists on paper before anything
spawns from it. Spawning builds on reading; the branch gate, the land
recipe and the review evidence are independent of each other once the
format exists; the installer wiring lands last, when there is something
real to ship. T2, T3, T4, T5 and T7 need only T1 — five branches from one
root, the fan-out of this very spec. The land recipe was one task in the
draft and is two here: gates-then-baseline is one red-green cycle, and
finishing a spec and removing worktrees is another, so `/keeler:tasks`
split it rather than ship a task eight scenarios wide.

- [x] **T1 — Task lines grow ids and needs.** Scenarios: _The tasks stage
      emits a dependency graph_, _A malformed Tasks section is refused
      naming the line_. Tests: acceptance — the harness drives
      `scripts/keeler-graph.sh` as a subprocess against fixture specs (the
      pattern `tests/installer.rs` uses for `install.sh`): the extended
      task line is read, a wrapped one is joined, a malformed one is
      refused naming the line, and `TEMPLATE.md` and `tasks.md` carry the
      format; this spec's own Tasks section parses to the graph it draws;
      and `tasks.md` carries the same-region rule and the hot-file list, so
      the graph it emits is one whose branches can merge. Property — over
      generated acyclic graphs, every task is reported exactly once as
      ready, blocked or done. Deliverable: the Tasks format in
      `TEMPLATE.md`, the `/keeler:tasks` command emitting it, and
      `scripts/keeler-graph.sh`, which `just keeler-graph` and
      `keeler-spawn` both call.
- [x] **T2 — /keeler:graph reads readiness.** Needs: T1. Scenarios:
      _Graph status names what is unblocked_, _A cycle is refused loudly_.
      Tests: acceptance — `just keeler-graph` against fixture specs, and
      the command file instructs running it rather than reading the spec
      itself. Cycle detection is the script's, so the refusal is machine
      checkable and not an agent's opinion.
- [x] **T3 — just keeler-spawn.** Needs: T1. Scenarios: _Spawning a task
      creates an isolated agent_, _A spawned agent commits on its branch,
      and nowhere else_, _A finished agent leaves a verdict the gate
      decided, and a log_, _A dead session is resumable, and says so_,
      _Spawning without tmux is refused, and says
      how to get it_, _Spawning from an uncommitted or unapproved spec is
      refused_, _Spawning a task that is already spawned is refused_,
      _Spawning a blocked task is refused_. Tests:
      acceptance — harness drives the recipe against a generated project
      with stub `claude` and `tmux` on PATH, offline; the stubs record what
      they were asked to run, so the prompt, the permission flags and the
      session name are all asserted; `.claude/keeler.md` carries the
      carve-out and the prompt carries the instruction. Deliverable:
      `keeler-spawn`, `keeler-status`, the tmux check in `install.sh`, and
      the commit carve-out in the rules.
- [x] **T4 — The branch gate, and the tick that stays on the branch.**
      Needs: T1. Scenarios: _Branch gates are diff-based only_, _A branch
      that moved the baseline is refused by CI_, _A branch ticks its task
      and nothing else_. Deliverable: `just keeler-branch`; the CI job in
      `templates/keeler.yml` that diffs `crap-baseline.json` and the `cov`
      recipe on keeler/* pull requests; the one-line change to
      `mutants.md` — on a keeler/* branch it ticks the task and leaves
      `Status:` alone. Tests: acceptance — the harness runs the recipe on a
      fixture branch and asserts the baseline and the recipe are byte-
      identical after; the CI check driven against fixture diffs;
      `mutants.md` text carries the branch condition.
- [x] **T5 — keeler-land: gates first, baseline second, staged not
      committed.** Needs: T1. Scenarios: _Baseline updates happen at
      fan-in, on main_, _A branch that was green alone can still redden
      main_, _keeler-land refuses to run anywhere but main_. Deliverable:
      `just keeler-land`, and the shared main-resolution helper it and
      `mutants-diff` both call. Tests: acceptance — the harness runs the
      recipe on a fixture main and inspects the index; a fixture whose
      merged tree fails `just dev` proves the baseline untouched and the
      exit non-zero; run on a fixture branch it refuses naming it.
- [x] **T6 — keeler-land finishes a spec and cleans up after it.**
      Needs: T5. Scenarios: _Landing the last task marks the spec
      implemented_, _Landing cleans up only what is clean_. Deliverable:
      the `Status:` write and the worktree/branch removal in
      `keeler-land`. Tests: acceptance — a fixture with every box ticked
      gets `Implemented` staged and one with a box unticked does not; a
      clean worktree is removed and a dirty one is left and named.
- [x] **T7 — Review writes a file and CI wants it.** Needs: T1. Scenarios:
      _Review leaves evidence_, _A review record must name a commit on its
      own branch_. Tests: acceptance — `review.md` carries the instruction
      and the header format; the shipped workflow's check, driven against
      fixture repositories, fails on a missing file, fails on a Commit:
      that is not an ancestor of HEAD, and passes on one that is.
      **This was built once and parked** — `feat/pipeline-enforces-itself`
      holds a working gate whose own review found three blockers, and T5
      starts by reading them: the check ran in a shallow checkout and could
      not see the commits it verified (`fetch-depth: 0` in the shipped
      workflow is a hard requirement); nothing told `/keeler:review` to
      write the file, so following the pipeline exactly produced a gate
      nobody could satisfy (`review.md` writes it, and the format lives
      there, not in a spec); and a duplicated backlog line panicked the
      gate. The tests on that branch passed their own review and are the
      starting point.
- [x] **T8 — Installer ships graph mode.** Needs: T2, T3, T4, T6, T7.
      Scenarios: _Adopters opt in, not out_. Tests: acceptance — the
      install harness verifies the new file set lands, that a spec in the
      old format — no dependency annotations at all — goes through `just
      keeler-graph` with every task ready, and that the `dev` recipe is byte-identical to before. Not
      "the linear road is byte-for-byte unaffected": T1 rewrites
      `TEMPLATE.md` and `tasks.md`, T5 rewrites `review.md` and the
      shipped workflow, and all of them are on the linear road. What is
      unaffected is its behaviour, and that is what the test asserts.

- [x] **T9 — The graph is read from the feature's branch.** Needs: T8.
      Scenarios: _Spawning from anywhere but the feature's branch is
      refused_, _A branch ticks its task and nothing else_. Deliverable:
      `keeler-spawn` and `keeler-status` read the spec from
      `feat/<spec-slug>` rather than main, and `keeler-spawn` refuses to
      run anywhere else. Tests: acceptance — the harness spawns from the
      feature branch, from main and from an unrelated branch, and asserts
      the refusal names what it expected; a dependency ticked on the
      feature branch unblocks its dependent, and one ticked on a task
      branch does not.
- [ ] **T10 — Landing happens twice.** Needs: T9. Scenarios: _Landing the
      last task marks the spec implemented_, _Baseline updates happen at
      fan-in, on main_. Deliverable: `keeler-land` split by level — on the
      feature branch it ticks and cleans up; on main it sets `Status:` and
      moves the baseline, and each refuses the other's work. Tests:
      acceptance — the harness runs it at both levels and inspects what
      changed and what did not; running it on a task branch is refused as
      before.

---

## Implementation Notes

The task line format extends the existing one minimally:

```
- [ ] **T3 — just keeler-spawn.** Needs: T1. Scenarios: <...>. Tests: <...>
```

`Needs:` is optional and empty means root.

**The grammar the parser reads, so that it and a human agree:**

- The Tasks section runs from the `## Tasks` heading to the next `## `
  heading. Nothing outside it is a task — the example item quoted in these
  notes is deliberately outside, and a parser that reads it has read the
  wrong region.
- A task item runs from a line beginning `- [ ]` or `- [x]` to the next
  such line or the end of the section, however many physical lines it
  wraps across. Continuation lines are indented; the parser joins them.
- The item opens with `**Tn — `, and `n` is one or more digits. `Tn` is
  the id; the em dash and title follow.
- `Needs: Ta, Tb.` may appear anywhere in the item, once — and "once"
  counts the token, so an item's prose may not *mention* `Needs:` either.
  This spec's own T8 did, and was refused by its own parser: the fixture
  bent, not the rule. Ids in it must be defined in the same spec. An item
  without it is a root. A second
  `Needs:`, an id defined twice, or a need naming no task are each a
  refusal that names the line — silently taking the first, or the last,
  would be a graph nobody wrote.
- The checkbox is the only completion signal. Prose elsewhere may mention
  `T1` freely; only the opening `**Tn — ` of an item defines one.

The spec that describes this is its own fixture: `specs/06-graph-mode.md`
goes through the parser and must yield exactly the graph its Tasks
section draws — eight tasks: T2, T3, T4, T5 and T7 needing T1; T6 needing
T5; T8 needing T2, T3, T4, T6 and T7.

**The parser is forty lines of shell, and where it lives is a decision.**
`scripts/keeler-graph.sh` reads a spec's Tasks section and prints each
task as ready, blocked (with unmet needs) or done; `just keeler-graph`
shows that, `keeler-spawn` refuses on it, and `/keeler:graph` is a thin
command that runs the recipe rather than reading the spec itself — so a
cycle is refused by a program, not judged by an agent. The harness tests
it as a subprocess, the way it tests `install.sh`.

Two alternatives were weighed. A `cargo xtask` subcommand would be Rust
under the mutation gate — but xtask is this repository's machinery and
never reaches an adopter, and the parser is needed *in the adopter's
project*, where the specs live and `keeler-spawn` runs. Shipping xtask to
adopters is a new runtime and most of the parked Rust installer over
again (`HANDOVER.md` on `feat/installer-in-rust`); keeping xtask here and
shell there is two parsers of one format, drifting. So: shell, beside the
two shell files already there and gated the same way. If the format ever
outgrows sixty lines of shell, that is the signal to ship a binary — and
that is its own spec about delivering one, not a side effect of this.

**The review record has a header, and CI reads it.** Four lines —
`Spec:`, `Task:`, `Commit:`, `Verdict:` — then the findings, or an
explicit "none", because a review that found nothing still happened. CI
on a keeler/* pull request checks two things: the file exists, and its
`Commit:` is on the branch — an ancestor of the head and *not* an
ancestor of main. Existence alone is a gate `touch` satisfies. Ancestry
of the head alone is not enough either: the merge base is an ancestor of
every branch, so a record naming it would pass while covering none of the
branch's work. Both together mean the SHA is one this branch made, which
is what "reviewed this branch" should mean. A branch with no commits of
its own therefore cannot carry a record — correctly, since there is
nothing to review. `/keeler:review` writes the file itself, at the end of the stage;
this is the second of the parked gate's blockers, and it is why the format
lives in `review.md` and not in a spec nobody reads at review time. The
check needs `fetch-depth: 0` in the shipped workflow, or a shallow
checkout sees no ancestors and every honest record fails — the first of
those blockers.

**`<spec-slug>` is the spec's file name without `.md`** — `06-graph-mode`
for `specs/06-graph-mode.md`. Nothing is derived from the title: the file
name is already unique by construction, and a second identity for the
same spec is a second thing that can collide. So the branch is
`keeler/06-graph-mode/t3`, the tmux session `keeler-06-graph-mode-t3`,
the worktree `../<repo>-06-graph-mode-t3`, and the record
`reviews/06-graph-mode/t3.md` — one name, four places, no slugging. The
task id is `T3` in the spec and `t3` in every path: lowercased once, on
the way out of the parser, because Linux CI is case-sensitive and a
branch named for `T3` and a record named for `t3` would never meet.

Branch naming `keeler/<spec-slug>/<task-id>` is the invariant everything
else hangs on: the CI review check triggers on it, `keeler-land` cleans it
up, humans can read it in `git branch`.

Worktrees land as siblings of the repository root
(`../<repo>-<spec-slug>-<task-id>`), never inside it — inside would be
visible to every other agent's globbing, and the spec slug in the path is
what stops two in-flight specs' T3s from colliding.

**`keeler-spawn` reads the spec as committed, because that is what the
worktree will see.** `/keeler:tasks` writes the graph into the working
tree and, by the commit law, leaves it uncommitted; a spawn straight after
would compute readiness from a file the new worktree does not have. So the
recipe refuses when the spec differs from HEAD, and refuses when its
`Status:` is not `Approved` — the same requirement `tdd.md` already puts
on the linear road, carried to this one.

**The spawned agent runs the whole per-task pipeline**, not one stage:
tdd, then qa, review and mutants, with `just keeler-branch` as its gate.
The point of a spawn is a branch that finishes unattended, and a branch
that stops after tdd never produces the review file, never earns its
tick, and breaks the pipeline law by construction. The prompt is built
from four things — the spec path, the task id, that instruction, and the
commit carve-out below; anything more is context the spec should have
contained.

**The spawn is the commit consent.** The rules say no commit without the
human's word, and an unattended branch cannot ask. So the word is given in
advance and narrowly: on a `keeler/*` branch created by `keeler-spawn`,
the agent commits on that branch, in that worktree, as each stage
finishes — which is what gives the review record a `Commit:` to name and
leaves the worktree clean for `keeler-land` to remove. Nowhere else, and
never on main; the rules file says so in as many words. The human named
the task and ran the spawn, and that is the confirmation, given before
rather than after. What the agent never does is push: the branch reaches
the remote, and a pull request, only when the human decides.

**Permissions are the spec's decision, not T3's.** The session runs with
`--permission-mode acceptEdits` and an explicit `--allowedTools` covering
`cargo`, `just` and `git` — enough to edit, test and commit inside its
worktree — with `git push` explicitly disallowed, because "nothing is
pushed" must be a permission and not a sentence in a prompt: an agent
that can push is one push away from breaking the promise, however
carefully it is asked not to. Not `bypassPermissions`: a headless agent
with unrestricted shell is a decision nobody should make by default in a
recipe.

**The session lives in tmux, and that is what makes it both parallel and
visible.** `keeler-spawn` creates a detached session named
`keeler-<spec-slug>-<task-id>` and returns at once; three spawns are three
sessions and the human is back at their prompt. `tmux ls` is the status
board and `tmux attach` the live view — a view only: `claude -p` is not
interactive, so attaching shows the run and does not join it. The session runs two things in
order: the agent, then `just keeler-branch` — and it is the *gate's* exit
code that goes to `.keeler/runs/<slug>/<task>.exit`, because `claude -p`
exits zero for any finished turn, including one that ended in FAIL. The
verdict is the machine's, as everywhere else in this pipeline. Everything
printed is teed to `<task>.log` beside it, so a run can be read after its
window is gone; `keeler-status` reads both, and asks `tmux has-session`
for "running" rather than inferring it from a missing file. The three
states are not two: a session that is gone with no `.exit` died before
its gate ever ran, which is a different thing from a gate that failed,
and a resume starts from the commits already on the branch — this was
learned the hard way when both agents of the manual dry run died on a
usage limit and had to be restored by reading their commits by hand. The paths are
absolute and under the main checkout, and `.keeler/` is added to the
ignore list the installer already writes. Nothing here decides *what* to spawn:
every session is one the human named, so the no-scheduler line holds. The
alternatives — plain background processes with log files and PID
tracking, or N terminals by hand — are respectively half a scheduler and
no parallelism; tmux is the thing that is neither. It becomes a
requirement, checked by `install.sh` the way `just` is.

**Readiness is read from the spec on the feature's branch,
`feat/<spec-slug>`.** A feature gets one branch and its tasks fan out from
it; `keeler-spawn` refuses to run anywhere else, so which branch holds the
graph is a name a machine checks rather than a convention someone
remembers. A tick on a *task* branch unblocks nothing — that is what keeps
parallel branches from racing each other's dependencies — while a tick on
the feature branch does, because arriving there **is** the landing. The
tick still happens on the task branch, which is how anyone sees the work
was finished, and `Status:` remains the one line no task branch may write.

This is the amendment the first real use of graph mode earned. Reading
readiness from main is right only when every task lands on main directly;
with a branch per feature it is wrong, and wrong in the silent direction —
a task whose dependency had landed on the feature branch read as blocked,
so the graph refused work that was ready on disk. The invariant survives
at the level where it belongs.

**Landing happens twice, and the two are not the same.** A task lands into
the feature branch: its box is ticked there and its worktree goes. The
feature lands into main: that is where `Status:` becomes `Implemented`,
and where the baseline moves — the baseline is the whole team's reference,
and a moved one must be visible in one place rather than in every
feature's branch.

`keeler-land` runs in one order and refuses to run in any other:
`just dev` on main first, the baseline second, and only if the first was
green. Not `dev-full` — hours of mutants on every fan-in is a cost nobody
would pay, and `mutants-diff` on a freshly merged main has nothing to
diff. A red main after fan-in means two branches that were each right
alone are wrong together, and the human decides whether to fix or revert;
what `keeler-land` guarantees is that the baseline never records a broken
main as the new normal.

**Two tasks that must edit the same region of one file are not
independent.** That is a rule for `/keeler:tasks`, written into the
command by T1: when two tasks would change the same lines, one gets a
`Needs:` on the other, because parallel edits to one region are a merge
conflict by construction and the graph is where that is prevented, not at
fan-in. The rule is about regions, not files, because some files every
branch adds to and git merges them cleanly *if each task adds in its own
place*: a recipe in the `Justfile`, a test function in `tests/graph.rs`, a
job in the workflow — additive files, each task appending under its own
heading rather than at the end. Those, and the ones every branch touches
regardless, are the named hot files, each with its answer: the spec — one
line, its own tick, the human's to merge; `Cargo.lock` — regenerated on
merge, `cargo` resolves it again; `proptest-regressions/` — append-only
seed files; `crap-baseline.json` — never on a branch, settled above; the
`Justfile`, `tests/graph.rs` and `templates/keeler.yml` — additive, own
place each.

This spec is its own first test of the rule. T2, T3 and T4 all add
recipes to the `Justfile`; T4 and T5 both add a job to the workflow; every
task adds tests. Under a per-file rule they would have to be chained and
the fan-out would be gone. Under the per-region rule they stay parallel,
and the fan-in works only if each adds where the rule says. If that fails
in practice, the rule was wrong and the spec should say so.

**"Main" is one thing, decided once.** `keeler-land` refuses to run off
it, and finds it exactly the way `mutants-diff` already does — the first
of `origin/main`, `origin/master`, `main`, `master` that exists — through
one shared helper, so no two recipes can disagree about where main is.
`crap-baseline` is *not* changed: `/keeler:feature` runs it at step 0 on
whatever branch the human is on, and a recipe that refused there would
break the linear road at its first step. What keeps a branch from moving
the baseline is CI (the scenario above), not a refusal in the recipe.

**`keeler-land` stages; it never commits.** The rules say no commit
without the human's word, and that a moved baseline is a decision worth a
diff someone reads. So the recipe regenerates `crap-baseline.json`, sets
`Status: Implemented` when every box is ticked, stages both, and prints
"review the diff and commit". A `just` recipe that authored a commit
would have an agent committing without asking the moment it ran one.

The branch-side gate is one new recipe, `just keeler-branch`: `dev`, then
`crap-delta`, then `mutants-diff`. `just dev` is untouched — an adopter on
the linear road never sees a change — and the branch gate is what a
spawned agent is told to run. The rest of the split is CI: it diffs
`crap-baseline.json` and the `cov` recipe on keeler/* PRs and fails if a
branch touched them.

### Non-goals

- No scheduler *in this spec*: nothing spawns a task the human did not
  name. A `keeler-fan-out` that spawns everything `keeler-graph` reports
  ready is a natural next step and a spec of its own — one that builds on
  tmux sessions that already work, rather than inventing its own
  background story.
- No pushing: a spawned agent commits on its branch and stops there.
  Nothing leaves the machine — no push, no pull request — without the
  human's hand.
- No merge automation: `keeler-land` verifies and updates baselines; it
  does not decide merge order or resolve conflicts. The spec file turned
  out not to need it: a task item spans several lines, so two branches
  ticking two boxes never edit within git's context window and merge
  cleanly — measured on T2 and T7, and on a deliberate probe of adjacent
  T3 and T4. The file is hot only when two branches edit the *same* item,
  which the same-region rule already forbids.
- No cross-spec graphs: a DAG spans one spec. Two features are two graphs.
- No Windows story beyond what the installer already claims.
- No portability to other agents in this spec; the spawn recipe assumes
  `claude -p`, as the pipeline already assumes Claude Code.
