# Keeler — graph mode

The chapter the rules point at. It is here rather than in `keeler.md`
because the rules reach the agent through a `SessionStart` hook whose
output is capped, and graph mode is a third of their length — paid for in
every session of every project, when most days never spawn anything.

Read it on instruction: `/keeler:graph` before answering, `/keeler:spec`
at the road question, `/keeler:tasks` when it writes `Needs:`, and the
prompt `keeler-spawn` hands its agent.

## Graph mode: the same pipeline, in parallel

The road in `keeler.md` is one agent, one branch, stages in sequence. **Graph mode is that same pipeline run by several agents at once** — one per task, each on its own branch in its own worktree, each running `tdd → qa → review → mutants` for the one task it was given. It is opt-in and adds nothing to the linear road: `/keeler:feature` routes exactly as it always did, and a project that never runs the recipes below never notices they are there.

The approved spec is what every spawned agent reads, so the graph lives in the spec and nowhere else. `/keeler:tasks` writes each task's dependencies into the Tasks section as `Needs: T1, T2.` — approving the spec is approving the graph. A task with no `Needs:` is a root, so a spec written before graph mode existed reads as a graph whose every task is ready.

The stages are slash commands; these recipes deliberately are not —
spawning, watching and landing are the human's levers, and the consent
each one grants cannot be given by the agent to itself. When the user asks
in-session ("fan out the spec", "show the board", "land it"), run the
matching recipe below; what you may not do is run `keeler-spawn`,
`keeler-fan-out` or `keeler-land` unasked.

| Command                                       | What it does                                                                                                                                                                                                          |
| --------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `just keeler-feature-branch <spec>`           | Cuts `feat/<spec-slug>` from main, checks it out and commits the approved spec there — the "graph" answer's one mechanical step, and the same by hand. Reuses the branch when it already exists. |
| `/keeler:graph` — `just keeler-graph <spec>`  | Reads the graph: which tasks are ready, blocked (naming what they wait on) or done. A cycle or a `Needs:` naming no task is refused by the parser, naming the line. Readiness is read from the spec on the feature's own branch, **`feat/<spec-slug>`** — a tick on a task branch unblocks nothing until it lands there. It opens with the ref it answered from, and falls back to HEAD once a landed feature's branch is gone. |
| `just keeler-fan-out <spec>`                  | The wave: names every ready, unspawned task and asks once; one **yes** spawns them all through `keeler-spawn`, into one tmux window with a pane per run — nothing starts that was not said yes to. |
| `just keeler-spawn <spec> <task>`             | Cuts the worktree and branch `keeler/<spec-slug>/<task-id>` and hands the task to a headless agent in a detached tmux session, returning at once. Runs only from the feature's branch, **`feat/<spec-slug>`**, and refuses anywhere else. Also refuses a blocked, done or already-spawned task, a spec that differs from HEAD or is not `Approved`, and a machine without tmux. |
| `just keeler-status <spec>`                   | The board: running, passed, `incomplete` (naming which of the three a task still lacks), failed, died mid-pipeline, `paused` (a death someone meant — the marker `keeler-top`'s `p` leaves), done, or never spawned — with each run's log and worktree, which are what a resume reads.                                                                             |
| `just keeler-resume <spec> <task>`            | Re-runs a task whose session ended before its gate, in the worktree and branch it already has — the commits on it are how far the pipeline got. Refuses a task still running, one that reached its gate, one that is done, and one never spawned. |
| `just keeler-branch`                          | The gate a task branch runs in place of `just dev`: `dev`, then `crap-delta`, then `mutants-diff` — diff-based by construction.                                                                                        |
| `just keeler-land`                            | Fan-in, at two levels the branch name decides. On the feature branch `feat/<spec-slug>`: `just dev`, then each **closed** task's clean worktree and branch are removed — closed meaning the gate was green, the review record is there and the box is ticked, since a tick alone is the cheapest of the three to produce. On main: `just dev`, then the baseline is regenerated and **staged, never committed**, and a spec whose every box is ticked gets `Status: Implemented` staged beside it. Anywhere else it refuses. |

Three rules keep parallel branches from lying to each other:

- **A branch measures the shared reference; it never moves it.** `crap-baseline.json` settles at fan-in, on main, and CI refuses a `keeler/*` pull request whose diff touched it. The bars left the project with the recipes: `KEELER_COV_MIN` and `KEELER_CRAP_MAX` live in the workflow now, beside the pin, and CI guards nothing else.
- **A task branch ticks its own task and leaves `Status:` alone.** `Status:` is the one line no task branch may write; `just keeler-land` sets it on main once every box is ticked.
- **Review leaves a record.** /keeler:review writes `reviews/<spec-slug>/<task-id>.md`, and CI on a `keeler/*` pull request fails when it is missing or names a commit the branch did not make.

**A feature, start to finish.** The commands above are the parts; this is the day.

1. `/keeler:spec` on any branch, iterate to approval — as always. On approval it asks which road; **"graph"** is the answer that starts this one, and it runs step 2 for you.
2. `just keeler-feature-branch <spec>` — cuts `feat/<spec-slug>` from main, checks it out and commits the approved spec there, or reuses the branch when it already exists. Answering **graph** is the consent for that one commit, and the question says so before it is answered — as spawning a task is the consent for the commits on its branch. `/keeler:spec` runs this for you on the **graph** answer; run it by hand if you answered linearly and changed your mind. The name is the spec's file name without `.md`, and `keeler-spawn` refuses to run anywhere else.
3. `/keeler:tasks` — writes `Needs:` into every task. Then **commit the graph**: the wave and every spawn read the committed spec, never the working tree.
4. `just keeler-fan-out <spec>` — names every ready task and, on one yes, spawns the wave into a tmux window with a pane per run. `just keeler-graph <spec>` answers the same question without acting on it.
5. Or `just keeler-spawn <spec> T1` one task at a time, when you want to name each. Each returns at once; the agent runs the whole per-task pipeline in its tmux session and ticks its box at the end.
6. `just keeler-status <spec>` for the board; `tmux attach -t keeler-<spec-slug>-t1` to watch, `Ctrl-b d` to leave. `died` means the session ended before its gate ran — its commits are on the branch and its log is under `.keeler/runs/`, which is what a resume starts from.
7. Merge each finished task branch into `feat/<spec-slug>` and run `just keeler-land` there: gates, then the landed worktrees go. The tick has arrived, so its dependents are ready — back to step 5, until `keeler-land` says the feature is finished.
8. Pull request from `feat/<spec-slug>` to main; merge; on main, `just keeler-land` stages the baseline and `Status: Implemented`; you commit.

Three things no step may do for you: an agent never pushes; only `keeler-land` on main writes `Status:`; `keeler-land` runs on the feature branch and on main and refuses everywhere else.

`<spec-slug>` is the spec's file name without `.md`, and the task id is lowercased on the way into every path: `specs/01-login.md` T3 gives the branch `keeler/01-login/t3`, the worktree `../<repo>-01-login-t3`, the tmux session `keeler-01-login-t3` and the record `reviews/01-login/t3.md`. tmux is graph mode's one extra requirement; `just` and the cargo tools are the same ones the linear road uses.
