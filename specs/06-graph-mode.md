# Spec 06 — Graph mode: parallel agents over a task DAG

**Status:** Draft
**Effort:** Large
**Module:** `.claude/commands/keeler/`, `templates/`, `Justfile`, `specs/TEMPLATE.md`, `tests/graph.rs`

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
Given a spec with an unblocked task T3
When  `just keeler-spawn <spec> T3` runs
Then  a worktree exists outside the repository root on branch keeler/<spec-slug>/t3
And   a headless Claude Code session starts in it with a prompt naming the
      spec file, the task id, and the tdd stage
And   the main working tree is untouched
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
Given a task branch with changes
When  `just dev` runs on that branch
Then  crap-delta and mutants-diff run against the branch's merge base
And   neither crap-baseline.json nor the coverage threshold is modified
      on the branch
```

### Scenario: Baseline updates happen at fan-in, on main

```
Given a task branch that passed its gates and was merged
When  `just keeler-land` runs on main
Then  the full gate suite runs on main
And   crap-baseline.json is regenerated and the change, if any, is committed
And   a branch that regenerated the baseline itself fails the CI check
```

### Scenario: A branch that was green alone can still redden main

```
Given two task branches that each passed their gates
When  the second is merged and `just keeler-land` runs on main
Then  the full gate suite runs on main before anything else
And   if it fails, the baseline is left exactly as it was and the run
      exits non-zero, saying main is red after fan-in
And   the baseline is only ever regenerated over a green main
```

### Scenario: Review leaves evidence

```
Given a task branch ready for fan-in
When  /keeler:review completes
Then  a review file exists under reviews/<spec-slug>/<task-id>.md on the
      branch, recording findings and a verdict
And   the keeler CI workflow fails a keeler/* branch PR that lacks it
```

### Scenario: Adopters opt in, not out

```
Given a fresh Rust project
When  Keeler is installed into it
Then  the graph command, spawn and land recipes, and the review-evidence
      check land alongside the existing pipeline
And   the linear road — /keeler:feature straight through — still works
      unchanged with no graph annotations required
```

---

## Tasks

The reading side comes first: the graph exists on paper before anything
spawns from it. Spawning builds on reading; the gate split and review
evidence are independent of both once the branch naming exists; the
installer wiring lands last, when there is something real to ship. T3, T4
and T5 need only T1's format — they are the fan-out of this very spec.

- [ ] **T1 — Task lines grow ids and needs.** Scenarios: _The tasks stage
      emits a dependency graph_. Tests: unit + property — parser accepts
      the extended task line, property test that parse ∘ render is
      identity, acyclicity check on generated graphs. Deliverable: the
      Tasks format in `TEMPLATE.md`, the `/keeler:tasks` command emitting
      it, and the parser both commands share.
- [ ] **T2 — /keeler:graph reads readiness.** Needs: T1. Scenarios:
      _Graph status names what is unblocked_, _A cycle is refused loudly_.
      Tests: unit + acceptance against fixture specs.
- [ ] **T3 — just keeler-spawn.** Needs: T1. Scenarios: _Spawning a task
      creates an isolated agent_, _Spawning a blocked task is refused_.
      Tests: acceptance — harness drives the recipe against a generated
      project with a stub `claude` on PATH, offline.
- [ ] **T4 — Gates split into branch-side and land-side.** Needs: T1.
      Scenarios: _Branch gates are diff-based only_, _Baseline updates
      happen at fan-in, on main_, _A branch that was green alone can still
      redden main_. Tests: acceptance — the harness runs both recipes and
      inspects what changed; a fixture where the merged tree fails `just
      dev` proves the baseline is untouched and the exit is non-zero; CI
      check unit-tested.
- [ ] **T5 — Review writes a file and CI wants it.** Needs: T1. Scenarios:
      _Review leaves evidence_. Tests: acceptance — a keeler/* branch
      without the file fails the shipped workflow, one with it passes.
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
- [ ] **T6 — Installer ships graph mode.** Needs: T2, T3, T4, T5.
      Scenarios: _Adopters opt in, not out_. Tests: acceptance — the
      install harness verifies the new file set and that the linear road
      is byte-for-byte unaffected.

---

## Implementation Notes

The task line format extends the existing one minimally:

```
- [ ] **T3 — just keeler-spawn.** Needs: T1. Scenarios: <...>. Tests: <...>
```

`Needs:` is optional and empty means root.

**There is no shared parser, and that is a decision, not a gap.**
`/keeler:graph` is an instruction to Claude Code, not a program: it reads
the spec the way it reads everything else, and asking it to shell out to a
parser for a line it can read is ceremony. The one place a machine has to
read the format is `just keeler-spawn`, which must refuse a blocked task
without an agent in the loop — and there the format is simple enough that
twenty lines of `awk` in the recipe cover it. The two alternatives both
hand adopters a new runtime: a `keeler-graph` binary is the Rust installer
this project parked (`HANDOVER.md` on `feat/installer-in-rust`), and an
xtask subcommand never reaches adopters at all. Two places knowing the
format is the cost; a new artifact to ship, version and verify would have
been the larger one.

Branch naming `keeler/<spec-slug>/<task-id>` is the invariant everything
else hangs on: the CI review check triggers on it, `keeler-land` cleans it
up, humans can read it in `git branch`. Property test worth having:
slugging is stable and collision-free across the spec titles in `specs/`.

Worktrees land as siblings of the repository root (`../<repo>-t3`), never
inside it — inside would be visible to every other agent's globbing.
`keeler-spawn` passes the headless session a prompt built from three
things only: the spec path, the task id, and the stage to run. Anything
more is context the spec should have contained.

`keeler-land` runs in one order and refuses to run in any other: the full
gate suite on main first, the baseline second, and only if the first was
green. A red main after fan-in means two branches that were each right
alone are wrong together, and the human decides whether to fix or revert;
what `keeler-land` guarantees is that the baseline never records a broken
main as the new normal.

The land-side/branch-side gate split is configuration, not new tooling:
`just dev` on a branch already runs the diff gates; the change is that the
baseline-touching recipes refuse to run off main, and CI diffs
`crap-baseline.json` and the `cov` threshold on keeler/* PRs and fails if
a branch touched them.

### Non-goals

- No scheduler: nothing spawns a task the human did not name.
- No merge automation: `keeler-land` verifies and updates baselines; it
  does not decide merge order or resolve conflicts.
- No cross-spec graphs: a DAG spans one spec. Two features are two graphs.
- No Windows story beyond what the installer already claims.
- No portability to other agents in this spec; the spawn recipe assumes
  `claude -p`, as the pipeline already assumes Claude Code.
