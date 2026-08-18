# Spec 07 — Fan-out: one answer, one wave

**Status:** Approved
**Effort:** Medium
**Module:** `.claude/commands/keeler/spec.md`, `.claude/commands/keeler/tasks.md`, `.claude/commands/keeler/feature.md`, `Justfile`, `.claude/keeler.md`, `KEELER.md`, `tests/fan_out.rs`

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
Given the spec stage's command file
When  it is read
Then  after setting Status: Approved it instructs asking whether to
      develop the feature linearly or as a graph, and waiting for the
      answer
And   the question says that "graph" creates feat/<spec-slug> and commits
      the approved spec there — the answer is the consent for that commit
And   "linearly" hands off to /keeler:tasks with nothing else done
```

### Scenario: The graph answer cuts the branch and commits the spec

```
Given specs/07-fan-out.md Approved in the working tree of main, and an
      unrelated uncommitted change beside it
When  `just keeler-feature-branch specs/07-fan-out.md` runs
Then  feat/07-fan-out exists and is checked out
And   its HEAD commit touches specs/07-fan-out.md and nothing else — the
      unrelated change is still uncommitted, as it was
And   the command file instructs running that recipe on "graph", then
      /keeler:tasks, then says the next steps in order: commit the graph
      /keeler:tasks wrote, then `just keeler-fan-out <spec>`
```

### Scenario: A feature branch that already exists is used, not remade

```
Given feat/07-fan-out already exists holding an earlier copy of the spec,
      and the working tree holds the newly approved one
When  `just keeler-feature-branch specs/07-fan-out.md` runs
Then  it checks the branch out, carries the working-tree spec across, and
      commits it there because it differs — saying which of the three it
      did
And   a working tree whose spec equals the branch's is checked out and
      nothing is committed
```

### Scenario: A feature branch is cut from main and nowhere else

```
Given a checkout on another spec's feat/* branch, or on a keeler/* task
      branch
When  `just keeler-feature-branch specs/07-fan-out.md` runs
Then  it refuses before creating anything, saying that a feature branch
      is cut from main and naming the branch it is on
```

### Scenario: Fan-out names the wave and waits for yes

```
Given feat/<spec-slug> is checked out and the graph has T2, T3 and T5
      ready and T4 blocked on T2
When  `just keeler-fan-out <spec>` runs
Then  it prints the ready tasks — T2, T3, T5 — and the blocked one with
      what it waits on, and asks for a yes before anything is spawned
And   a ready task the board already knows — running, died, passed but
      not yet landed — is listed in the board's own words and not
      offered: the wave is what is ready and not yet spawned
And   the answer is yes when it reads `yes` or `y`, case aside; anything
      else spawns nothing, creates nothing, and exits non-zero
And   when there is nobody to ask — stdin is not a terminal and
      KEELER_FAN_OUT_YES is unset — it refuses naming the variable rather
      than hanging or exiting in silence
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
Then  a tmux session named keeler-<spec-slug>-wave holds one pane per
      task that spawned, each attached to that task's session, laid out
      so all are visible
And   `tmux attach -t =keeler-<spec-slug>-wave` shows the wave, and the
      run prints that command — or `switch-client` when it is already
      inside tmux
And   the per-task sessions keeler-<spec-slug>-<task> still exist, so
      `keeler-status` and a single attach work exactly as before
```

### Scenario: An empty wave says so

```
Given a graph with nothing to offer — every task done, or every
      remaining task blocked, or every ready task already spawned
When  `just keeler-fan-out <spec>` runs
Then  it says nothing is ready and why — done, blocked on what, or
      already spawned and in what state — and exits zero, spawning
      nothing
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

### Scenario: The rules describe the fork and the wave

```
Given the shipped rules and KEELER.md
When  they are read
Then  the graph-mode day names `just keeler-feature-branch` and
      `just keeler-fan-out` in the steps a human takes
And   they say the graph answer commits the spec, and that answering
      "graph" is the human's consent for that one commit
And   they say the graph is committed before fan-out reads it
```

### Scenario: The linear road is untouched

```
Given the command files of the linear road
When  they are read
Then  /keeler:tasks, /keeler:tdd, /keeler:qa, /keeler:review and
      /keeler:mutants carry no new instruction for the "linearly" answer,
      and none of them names a branch to create or tmux to require
And   /keeler:feature, whose step 1 is /keeler:spec, says that on the
      "graph" answer it stops after /keeler:tasks with the same hand-off
      — commit the graph, then `just keeler-fan-out` — because its
      per-task loop is the linear road's and the wave is not
```

---

## Tasks

The fork and the fan-out are independent — the recipe that cuts a branch
and the recipe that reads a wave share nothing but the Justfile — so T1
and T2 are both roots, and this spec is itself a wave of two. Fan-out's
reading side before its spawning side; the tmux view last, because it is
the one piece the stub tmux cannot stand in for.

- [ ] **T1 — Approval asks which road, and the graph answer does the
      steps.** Scenarios: _Approval asks which road_, _The graph answer
      cuts the branch and commits the spec_, _A feature branch that already exists is
      used, not remade_, _A feature branch is cut from main and nowhere
      else_, _The rules describe the fork and the wave_, _The
      linear road is untouched_. Tests: acceptance
      — `spec.md` carries the question and both branches of the answer;
      the branch-and-commit step is a recipe, `just keeler-feature-branch
      <spec>`, the harness drives against a fixture repository with and
      without the branch existing. Deliverable: the question in `spec.md`,
      the recipe, and the hand-off text.
- [ ] **T2 — Fan-out reads and asks.** Scenarios: _Fan-out
      names the wave and waits for yes_, _An empty wave says so_,
      _Fan-out refuses where spawn would_. Tests: acceptance — the harness
      runs the recipe with a piped answer against fixture graphs; property
      — over generated graphs, the wave printed is exactly the set
      `keeler-graph` reports ready; a text assertion that no file under
      `.claude/commands/` sets KEELER_FAN_OUT_YES. Deliverable: `_spawn-preflight <spec>`,
      lifted out of `keeler-spawn` with `tests/spawn.rs` green throughout,
      and `just keeler-fan-out <spec>` up to and including the yes.
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
and it drives them: `just keeler-graph` for readiness, `just keeler-status`
for what is already spawned — the wave is the first minus the second —
and `just keeler-spawn` per task. It owns no logic those two already have — a refusal in
`keeler-spawn` is reported, not reimplemented, and readiness is never
computed here. That is what keeps it small, and what keeps the human's
one yes meaning what it did for a single spawn: the same recipe runs, the
same guards fire.

**The guards fire once, in one place.** `keeler-spawn` checks tmux, the
file, that it is in the repository, that the spec equals HEAD and is
Approved, and that the branch is `feat/<spec-slug>` — and it does all of
that on its way to cutting a worktree. `fan-out` must refuse on the same
grounds *before* it prints a wave, and the only way to do that without
writing the checks twice is to move them out of `keeler-spawn` into a
private recipe both call, `_spawn-preflight <spec>`, the way `_main-ref`
already holds the one answer to where main is. This touches spec 06's
recipe and its tests; it is a refactor and not a change of behaviour,
and `tests/spawn.rs` stays green through it — that is the test of it
being one.

The view is built after the spawns, from the sessions they made, and
**every tmux target fan-out uses is exact** — `=name` — because tmux
matches a bare name as a prefix and `keeler-07-fan-out` is a prefix of
`keeler-07-fan-out-t2`: spec 06 met this in `keeler-status`, and a probe
here showed `kill-session -t keeler-07-fan-out` killing t2's session. The
view session is named `keeler-<spec-slug>-wave` so that it is not a
prefix of anything either. It is created detached with `-x`/`-y` large
enough for the wave, then one `split-window` per spawned task, each
followed by `select-layout tiled` — a detached 80x24 session runs out of
room at the fourth pane otherwise. Each pane runs an attach to its task's
session with `TMUX=` cleared, written into a runner under
`.keeler/runs/<slug>/wave.sh` the way spawn writes its own — not inline,
because the pane runs under the user's shell, and in zsh `=name` is
command lookup, not a tmux target. The per-task sessions are the source
of truth and outlive the view; when a run ends its pane closes, and when
the last does the view session goes with it. Closing the view kills
nothing. From inside tmux the run prints `switch-client`, since a nested
attach fails at once.

The yes is read from stdin so a test can pipe it, and so a human at a
terminal is asked in the ordinary way; `yes` or `y`, case aside, and
nothing else. When stdin is not a terminal and no answer was given in
advance, the recipe refuses and names `KEELER_FAN_OUT_YES` — `read` at
EOF under `set -e` would otherwise end the recipe with a bare 1, and a
pipe that never closes would hang it. `KEELER_FAN_OUT_YES=1` answers yes
without asking, for the caller who has already decided: a human in a
script, or a later scheduler if one is ever built. It is the zero-yes
path this spec otherwise refuses, so it is fenced: **no command file sets
it**, and a test says so — an agent that could set it for itself would
have found the way around the one question that is the human's.

`/keeler:spec` gains one question after it sets Approved, and its graph
answer runs `just keeler-feature-branch <spec>` — the branch, the
checkout, the commit — then invokes `/keeler:tasks`. The recipe cuts
from main only, found through `_main-ref` as every other recipe finds it:
a feature cut from another feature's branch, or from a task branch, is a
feature nested where nothing else in graph mode expects one. When the
branch already exists — a spec re-approved after an amendment, which is
what happened to spec 06 — the recipe carries the working-tree spec
across, since that is the copy the human just approved, and commits it
there only if it differs. That recipe is a
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
