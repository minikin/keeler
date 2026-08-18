# Spec 07 — Fan-out: one answer, one wave

**Status:** Draft
**Effort:** Medium
**Module:** `.claude/commands/keeler/spec.md`, `.claude/commands/keeler/tasks.md`, `Justfile`, `.claude/keeler.md`, `KEELER.md`, `tests/fan_out.rs`

## Context

Graph mode works, and its first real use showed where the human's time
goes: not into deciding, but into typing. After a spec is approved the
road forks — linear or graph — and nothing asks; the human remembers to
cut `feat/<spec-slug>`, commit, run `/keeler:tasks`, commit again, read
the graph, and then run `just keeler-spawn` once per ready task, each a
command typed by hand, each opening a tmux session the human then has to
find. Spec 06 was right to leave all of that to the human: it was
building the parts, and a scheduler is a place for bugs to hide from the
gates. But four spawns typed one after another are not four decisions.
They are one decision — *this wave* — spelled out four times.

This spec makes the fork explicit and the wave one command. After
approval, `/keeler:spec` asks which road; choosing the graph does the
mechanical steps itself and hands off to `/keeler:tasks`. Then
`just keeler-fan-out <spec>` reads the graph, names every ready task, and
on one **yes** spawns them all — and opens a single tmux window with a
pane per run, so the human sees the wave, not a list of session names.

**What is kept from spec 06, deliberately.** The human still names what
spawns — one wave at a time rather than one task at a time, but nothing
starts that the human did not say yes to. Each spawn is still
`keeler-spawn`, with every refusal it has: `fan-out` is a loop over it,
not a second implementation. Nothing pushes. `Status:` is still main's.

**Rejected alternatives.** Spawning without asking — the fork was the
answer, so a second question is redundant: rejected, because the fork was
answered before `/keeler:tasks` drew the graph, and the wave is the first
time the human sees what the graph actually contains. A watcher that
spawns dependents as their needs land — that is the scheduler spec 06
named as a spec of its own, and it stays one; see Non-goals. One tmux
session per task, as spawn does today, plus a helper to attach to each —
rejected: the point is to see the wave together, and tmux panes in one
window are exactly that.

---

## Acceptance Tests

### Scenario: Approval asks which road

```
Given a spec the user has just approved
When  /keeler:spec sets Status: Approved
Then  it asks whether to develop the feature linearly or as a graph, and
      waits for the answer
And   "linearly" hands off to /keeler:tasks with nothing else done
```

### Scenario: Choosing the graph does the mechanical steps

```
Given the user answered "graph"
When  the command continues
Then  it creates the branch feat/<spec-slug> from the current one, checks
      it out, and commits the approved spec there
And   it hands off to /keeler:tasks, which writes Needs: into the tasks
And   it says the branch it made and that the next step is
      `just keeler-fan-out <spec>`
```

### Scenario: A feature branch that already exists is used, not remade

```
Given feat/<spec-slug> already exists
When  the user answers "graph"
Then  the command checks it out rather than failing on the name, and
      says so
```

### Scenario: Fan-out names the wave and waits for yes

```
Given feat/<spec-slug> is checked out and the graph has T2, T3 and T5
      ready and T4 blocked on T2
When  `just keeler-fan-out <spec>` runs
Then  it prints the ready tasks — T2, T3, T5 — and the blocked one with
      what it waits on, and asks for a yes before anything is spawned
And   an answer other than yes spawns nothing and creates nothing
```

### Scenario: Yes spawns the whole wave through keeler-spawn

```
Given the wave above and the answer yes
When  fan-out continues
Then  it runs `just keeler-spawn <spec> T2`, T3 and T5, in that order,
      through the same recipe a hand would use
And   every refusal keeler-spawn has still applies per task — a task
      that has become spawned meanwhile is refused and named, and the
      rest of the wave still spawns
And   it reports each task's outcome, and exits non-zero if any spawn
      refused
```

### Scenario: The wave is one tmux window with a pane per run

```
Given a wave of three tasks spawned by fan-out
When  the spawns have started
Then  a tmux window named keeler-<spec-slug> holds one pane per task,
      each attached to that task's session, laid out so all are visible
And  `tmux attach -t keeler-<spec-slug>` shows the wave, and the run
      prints that command
And   the per-task sessions keeler-<spec-slug>-<task> still exist, so
      `keeler-status` and a single attach work exactly as before
```

### Scenario: An empty wave says so

```
Given a graph with nothing ready — every task done, or every remaining
      task blocked
When  `just keeler-fan-out <spec>` runs
Then  it says nothing is ready and why — done, or blocked on what — and
      exits zero, spawning nothing
```

### Scenario: Fan-out refuses where spawn would

```
Given a checkout that is not feat/<spec-slug>, or a spec that differs
      from HEAD, or one that is not Approved
When  `just keeler-fan-out <spec>` runs
Then  it refuses before printing a wave, with the same message
      keeler-spawn gives, and asks nothing
```

### Scenario: The next wave is a re-run

```
Given a wave that landed — its ticks merged into feat/<spec-slug> and
      `just keeler-land` run there
When  `just keeler-fan-out <spec>` runs again
Then  the tasks the landing unblocked are the new wave, and the tasks
      already done are listed as done, not offered
```

### Scenario: The linear road is untouched

```
Given a user who answered "linearly", or a project that never runs
      fan-out
When  the pipeline runs
Then  /keeler:tasks, /keeler:tdd and the rest behave exactly as before,
      and no branch is created and no tmux is required
```

---

## Tasks

The fork comes first because it is small and every later step assumes
it. Fan-out's reading side before its spawning side; the tmux window
last, because it is the only piece that cannot be tested with the stub
tmux the harness already has.

- [ ] **T1 — Approval asks which road, and the graph answer does the
      steps.** Scenarios: _Approval asks which road_, _Choosing the graph
      does the mechanical steps_, _A feature branch that already exists is
      used, not remade_, _The linear road is untouched_. Tests: acceptance
      — `spec.md` carries the question and both branches of the answer;
      the branch-and-commit step is a recipe, `just keeler-feature-branch
      <spec>`, the harness drives against a fixture repository with and
      without the branch existing. Deliverable: the question in `spec.md`,
      the recipe, and the hand-off text.
- [ ] **T2 — Fan-out reads and asks.** Needs: T1. Scenarios: _Fan-out
      names the wave and waits for yes_, _An empty wave says so_,
      _Fan-out refuses where spawn would_. Tests: acceptance — the harness
      runs the recipe with a piped answer against fixture graphs; property
      — over generated graphs, the wave printed is exactly the set
      `keeler-graph` reports ready. Deliverable: `just keeler-fan-out
      <spec>` up to and including the yes.
- [ ] **T3 — Yes spawns the wave.** Needs: T2. Scenarios: _Yes spawns the
      whole wave through keeler-spawn_, _The next wave is a re-run_.
      Tests: acceptance — with the stub `claude` and `tmux` from
      `tests/spawn.rs`, every task in the wave is handed to
      `keeler-spawn` in order, a mid-wave refusal is reported and does not
      stop the rest, and a second run after a landing offers the newly
      ready tasks. Deliverable: the spawning half of the recipe.
- [ ] **T4 — The wave is one window.** Needs: T3. Scenarios: _The wave is
      one tmux window with a pane per run_. Tests: acceptance — the stub
      tmux records the window and pane commands it was asked for, and the
      per-task sessions are still created; one test marked
      `#[ignore]`-unless-real-tmux runs the true thing. Deliverable: the
      window, and the attach line the run prints.

---

## Implementation Notes

`keeler-fan-out` is shell in the `Justfile`, beside the recipes it drives,
and it drives them: `just keeler-graph` for the wave, `just keeler-spawn`
per task. It owns no logic those two already have — a refusal in
`keeler-spawn` is reported, not reimplemented, and readiness is never
computed here. That is what keeps it small, and what keeps the human's
one yes meaning what it did for a single spawn: the same recipe runs, the
same guards fire.

The tmux window is built after the spawns, from the sessions they made:
`tmux new-window -n keeler-<spec-slug>` in a session of the same name,
then one `split-window` per task running `tmux attach -t
keeler-<spec-slug>-<task>` — nested attach, which tmux allows with
`TMUX=` cleared for the inner one — and `select-layout tiled`. The
per-task sessions are the source of truth and outlive the window; the
window is a view. Closing it kills nothing.

The yes is read from stdin so a test can pipe it, and so a human at a
terminal is asked in the ordinary way. `KEELER_FAN_OUT_YES=1` in the
environment answers yes without asking, for the caller who has already
decided — a later scheduler, if one is ever built, would be that caller.

`/keeler:spec` gains one question after it sets Approved, and its graph
answer runs `just keeler-feature-branch <spec>` — the branch, the
checkout, the commit — then invokes `/keeler:tasks`. That recipe is a
recipe and not three lines in the command file because it must behave
the same when a human runs it by hand after answering "linearly" and
changing their mind.

### Non-goals

- No watcher and no scheduler: nothing spawns without a yes, and nothing
  runs `fan-out` again on its own when a wave lands. The loop from step
  7 back to step 5 is the human's, and a spec that automates it is a
  spec about what happens when a wave fails half-way, when a landing is
  red, and when a merge conflicts — none of which this spec answers.
- No merge automation: fan-out spawns; it does not merge task branches
  into the feature branch. That is `keeler-land`'s caller — the human.
- No change to what a spawned agent does, or may do. Same prompt, same
  permissions, same "never pushes".
- No fan-out from main or from a task branch: it runs where
  `keeler-spawn` runs, and refuses where it refuses.
