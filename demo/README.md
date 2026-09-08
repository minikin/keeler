# The demo board

`board.py` builds a throwaway repository whose board shows every state
at once — two tasks running, one failed, one incomplete, one paused,
one ready, three blocked, two done — so `keeler-top` can be filmed, or
looked at after a change to the frame, without spending an agent on a
wave.

```
python3 demo/board.py            # build it; it prints the two commands
python3 demo/board.py --clean    # remove the repository, its worktrees and its tmux sessions
```

It is repository machinery: nothing here is installed into a project,
and no recipe reaches it.

## What it fabricates, and what is real

Real: a git repository at `/tmp/keeler-demo` (or the path given as the
first argument), a feature branch carrying an Approved spec with ten
tasks and a dependency graph, a task branch per spawned task with its
commits, linked worktrees for the live ones (one of them dirty),
review records, `.keeler/runs/<slug>/<tid>.{stream,log,exit}` in
Claude's stream-json, a `.paused` marker, and a detached tmux session
per running task. The board reads all of that the way it reads a real
wave — `keeler-status`, the graph parser, the streams and `git`.

Invented: the work. The tasks are an espresso machine, the commits
touch files nobody compiles, and the streams describe stages that never
ran. Nothing in the fixture is a fixture *of the board*; the board is
never told it is a demo.

## What each row is there to show

| Task | State | Shows |
| --- | --- | --- |
| T3 | running | the stage from a `Skill code-review`, a live `Bash` with its elapsed, a dirty worktree |
| T5 | running | the gate stage, a context bar over the 80% mark, a bigger model window |
| T4 | failed (exit 2) | the exception half of the header, and a red row |
| T6 | incomplete | a green gate that is not a finished task |
| T8 | paused | the marker, the violet row, `resume: R` under it |
| T10 | ready | the one state a human can act on |
| T7, T9 | blocked | what they wait on |
| T1, T2 | done | the quiet end of the list |

Re-running rebuilds it from scratch with fresh timestamps, so the
elapsed and "since spawn" figures are always plausible.
