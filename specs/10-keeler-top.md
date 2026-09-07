# Spec 10 — keeler-top: the live board

**Status:** Approved
**Effort:** Large
**Module:** `keeler-top/` (new workspace crate, with `keeler-top/tests/top.rs`), `Cargo.toml` (`members`), `Justfile` (`keeler-top`, `keeler-status`, `keeler-resume`), `CLAUDE.md` (the gates paragraph), `crap-baseline.json` (refreshed at fan-in), `tests/top_recipes.rs`

## Context

`keeler-status <spec>` answers one question per task: running, passed,
incomplete, failed, died, done or never spawned. That is the right
answer for a script and the wrong one for a human watching a wave.
During spec 09's first wave the question actually asked, three times in
a row, was "which stage is T10 on, what is it running, how long has it
been running it, and is it about to run out of context?" — and each
time the answer came from grepping `.keeler/runs/<slug>/<tid>.log` by
hand. The data is there; nothing reads it.

`keeler-top` is a terminal board that reads it. One row per task, a
detail pane for the selected one, refreshed every second, with the three
levers a watcher actually reaches for: attach to the task's tmux session,
pause it, resume it. It refreshes continuously, on the two cadences
described below. It is a ratatui binary — the first place in Keeler
where a TUI earns its keep, and one that sits beside the pipeline rather
than inside it, so spec 05's decision (the pipeline stays shell) is
untouched.

**Who gets it.** Adopters, not only this repository. After spec 09 the
repository root is the plugin, so a workspace crate at `keeler-top/`
ships with it, and the front door is the one 09 built: `keeler
keeler-top <spec>`, through `bin/keeler`, which runs the plugin's
Justfile with the project as working directory. The recipe builds the
crate from the plugin's own tree with `cargo run --release
--manifest-path`, and passes the binary three things: the spec, the
project root, and the plugin root — the last is how the binary shells
back to the same Justfile and to `scripts/keeler-graph.sh`. Flags after
the spec reach the binary, so `keeler keeler-top --once <spec>` works
from a script. The first launch after an install or a `/plugin update`
compiles ratatui and its tree, which is minutes, and the plugin's
`rust-toolchain.toml` may pull a toolchain first; the recipe says so
before cargo starts, probing `<plugin root>/target/release/keeler-top`
— a workspace member's artifacts live in the workspace's `target/`, not
its own. Every launch after that is instant until the plugin's cache
path changes. It is not `xtask`: xtask is repository machinery that never reaches an
adopter, and it carries one dependency on purpose.

**What the stream actually contains.** Three streams from spec 09's
first wave were read before this was written, and one thing they do not
contain is a `Skill` call named `keeler:tdd`, `keeler:qa`,
`keeler:review` or `keeler:mutants` — those are slash commands in the
init record, and the only `Skill` calls in all three runs are
`code-review`, from the second half of `/keeler:review`. So the stage is
read from what the agent demonstrably does, in this order of evidence:

| Signal in the stream                                                          | Stage     |
| ----------------------------------------------------------------------------- | --------- |
| nothing yet                                                                   | `reading` |
| any `Write`/`Edit` tool_use on a path inside the worktree, other than under `reviews/` | `tdd`     |
| a recipe call whose recipe is `dev`, `ci`, `cov`, `crap`, `crap-baseline`, `crap-delta`, `lint` or `test` | `qa`      |
| a `Skill` call `code-review`, or a `Write`/`Edit` on `reviews/<slug>/<tid>.md` | `review`  |
| a recipe call whose recipe is `mutants`, `mutants-all` or `mutants-diff`      | `mutants` |
| a recipe call whose recipe is `keeler-branch`                                 | `gate`    |
| `.exit` file present, whatever it holds                                       | `ended`   |

One matching rule for every Bash row: the *recipe* is the first word
after `just` or `keeler` at the start of the command, exact — so `cat
mutants.out` and `grep mutants` match nothing, and `just crap-delta`
(half of `keeler-branch`) is qa. Paths in `Write`/`Edit` inputs are
absolute; they are matched after stripping the worktree prefix
`keeler-status` reports, so a `/tmp/x/src/y` edit is not tdd and an
edit to the worktree's `Justfile` is — t10's whole red-green cycle was
edits to the Justfile, and a table listing only `src/` and `tests/`
would have shown `reading` through the very run this spec was written
against. `ended` rather than `done` for the last row because a `failed
(exit 1)` task has an exit file too, and its stage is over rather than
finished. A `Skill` call `keeler:tdd|qa|review|mutants` counts as the
same signal as the row it names, so if the plugin ever surfaces the
commands as skills the board reads them too. **The stage never moves backwards**: the
fold keeps the maximum, so `just dev` run again during review does not
demote the row, and a re-run of an earlier stage shows as the later one.
An agent that does a stage by hand in some other way leaves the board on
the previous stage; that is the honest answer.

**Only the main session counts.** A run spawns subagents (`Task`), and
their records share the stream with `parent_tool_use_id` set: t10's
stream had 121 `assistant` records, of which 35 were a subagent's. The
row's stage, tool, texts and context come from records whose
`parent_tool_use_id` is null. And one message arrives as one record per
content block, each carrying the whole message's `usage`: 121 records,
70 distinct `message.id`. Output tokens are summed once per id.

**Two reads, one truth.** `keeler-status` decides what running, died,
passed and incomplete mean, with git queries against the task branches
that took eight specs to get right, and `keeler-top` does not re-derive
any of it. But a `just` recipe costs about 2.4 s to start on this
repository, measured, and `keeler-status` has no word for *blocked*: it
prints `not spawned` for every task the graph is holding. So the board
makes two reads on two cadences. `scripts/keeler-graph.sh` runs on every
tick (it costs nothing) and supplies the graph's own word — `blocked ←
…` or `ready` — for tasks `keeler-status` calls `not spawned`. It runs
against the spec **as the ref holds it**, never the working tree: the
board does the same `git show <graph_ref>:<rel>` into a temporary file
that `keeler-status` and `keeler-graph` do, with `graph_ref` the
`feat/<slug>`-or-HEAD the status header names, so the two reads answer
about the same graph. `keeler-graph`'s own comment says why: answering
from the working tree would make this the one place in graph mode where
an uncommitted tick counts. `keeler-status` runs in the background every
five seconds, and on `r`, and the header shows how old its last answer
is. Neither read is a re-derivation: the script is the graph parser
every recipe already uses, and the recipe is the board.

**The stream is replaced on resume.** The runner writes `.stream` with
`tee`, not `tee -a`, and `keeler-resume` regenerates the runner with the
same path: every resume truncates the file and starts over, while `.log`
keeps growing across runs. The incremental reader therefore treats a
file shorter than its offset, or a second `init` record, as a new run
and parses from the start — the second signal covers a resume that has
already written past the old offset before the board looked.

**Paused is not died.** A session killed by hand and a session that
crashed look identical today: no tmux session, no `.exit` file. The
board's `p` key kills the session and then writes a marker,
`.keeler/runs/<slug>/<tid>.paused`, which `keeler-resume` removes. Kill
first, then mark: a marker written before a kill that fails would be a
claim the board cannot back. Between the kill and the next status read
the row may say `died` for one refresh; the marker settles it on the
read after. A kill that bypassed the board stays `died`, as now. A
marker nobody resumes — the human removed the worktree by hand — would
say `paused` for ever, so `keeler-status` prints beside a `paused` line
the same escape hatch it prints for `failed`: the `rm` that clears it.

**A keypress is the human's lever.** The rules reserve spawning,
watching and landing for the human because an agent cannot consent for
one. `p` and `R` are pressed by the human in an interactive board, which
is the same word `keeler-spawn` takes at the command line, and `R` runs
`keeler-resume` and nothing else — a task the board can resume is one the
human already spawned. The board never spawns a new task, fans out or
lands.

**Assumptions carried into approval.**

- This spec is implemented after spec 09 lands on main. Both edit the
  Justfile, and 09's T9 rewrites it wholesale.
- The context window is inferred from the model name in the init
  record: a `[1m]` suffix means 1,000,000 tokens, anything else
  200,000. The init record's model carries the suffix; the assistant
  records' does not.
- Percentages are rounded half up to the nearest whole number, and a
  context of 80% or more carries a trailing `!`. Token counts print as
  `999`, `1.0k` from 1,000, `1.0M` from 1,000,000, one decimal; a value
  that would round to `1000.0k` prints as `1.0M`. Elapsed prints
  `MM:SS` under an hour and `H:MM:SS` at or above it.
- `--once` is the plain-text frame for scripts and tests. Rendering goes
  through ratatui's `TestBackend` in tests; no live agent is needed.

---

## Acceptance Tests

### Invocation

### Scenario: the recipe builds the crate from the plugin tree and passes it three paths

```
Given a project with specs/01-foo.md committed on feat/01-foo
When  the user runs `keeler keeler-top specs/01-foo.md`
Then  the recipe runs cargo with --manifest-path pointing at keeler-top/Cargo.toml under the Justfile's own directory
And   the binary receives the spec path, the project root and the plugin root
And   flags given after the spec reach the binary
```

### Scenario: --once through the front door prints the table on stdout

```
Given a project with specs/01-foo.md committed on feat/01-foo
When  the user runs `keeler keeler-top --once specs/01-foo.md`
Then  stdout holds only the table
And   cargo's build output, if any, went to stderr
```

### Scenario: the first launch says it is building before cargo starts

```
Given <plugin root>/target/release/keeler-top does not exist
When  the user runs `keeler keeler-top specs/01-foo.md`
Then  stderr says keeler-top is being built for the first time, before cargo's own output
And   once the binary exists the notice is not printed again
```

### Scenario: the graph is read from the ref, not the working tree

```
Given T1's box is ticked in the working tree but not committed on feat/01-foo
When  the board renders
Then  T1 is not done, and T2 (Needs: T1) reads "blocked ← T1"
```

### Scenario: the board shells back through the Justfile it was launched from

```
Given the binary was launched with plugin root P
When  it refreshes state
Then  it runs `just --justfile P/Justfile --working-directory <root> keeler-status <spec>`
And   `bash P/scripts/keeler-graph.sh` for the graph
```

### Scenario: outside a git repository the board refuses with keeler-status's words

```
Given a directory that is not a git repository
When  the user runs `keeler-top specs/01-foo.md`
Then  it exits 1
And   stderr carries the reason keeler-status gave
```

### Scenario: a spec not committed on the feature branch is refused

```
Given specs/01-foo.md exists in the working tree but is not committed on feat/01-foo or HEAD
When  the user runs `keeler-top specs/01-foo.md`
Then  it exits 1
And   stderr says the spec is not committed on the ref the board reads
```

### Scenario: --once prints one frame as a table and exits

```
Given a spec whose T1 is running and T2 is not spawned
When  the user runs `keeler-top --once specs/01-foo.md`
Then  stdout is a plain table with a header line and one row per task
And   the process exits 0 without entering the alternate screen
```

### Scenario: without a terminal the board refuses and names --once

```
Given stdout is a pipe and --once was not given
When  the user runs `keeler-top specs/01-foo.md`
Then  it exits 1
And   stderr says the board needs a terminal, and that --once prints one frame
```

### Scenario: the header names the ref keeler-status answered from and the age of that answer

```
Given keeler-status printed "graph: specs/01-foo.md on feat/01-foo" four seconds ago
When  the board renders
Then  the header reads "specs/01-foo.md on feat/01-foo" and "status 4s ago"
```

### Board rows

### Scenario: every task in the graph has a row, in spec order

```
Given a spec with tasks T1..T5
When  the board renders
Then  it shows five rows, T1 first and T5 last
```

### Scenario: the state column is keeler-status's word for the task

```
Given keeler-status reports T1 running, T2 died, T3 passed, T4 incomplete (no review record), T5 failed (exit 1)
When  the board renders
Then  each row's state column shows exactly that word, with incomplete's reason and failed's exit code kept
```

### Scenario: a not-spawned task the graph holds reads blocked with its needs

```
Given keeler-status reports T3 not spawned and keeler-graph.sh reports "T3 blocked T1 T2"
When  the board renders
Then  T3's state column reads "blocked ← T1, T2"
```

### Scenario: a not-spawned task the graph calls ready reads ready

```
Given keeler-status reports T1 not spawned and keeler-graph.sh reports "T1 ready"
When  the board renders
Then  T1's state column reads "ready"
```

### Scenario: a done task with no worktree shows dashes for the live columns

```
Given T1 is done and ../<repo>-01-foo-t1 no longer exists
When  the board renders
Then  T1's state is done
And   its stage, tool, context, tokens and commit columns show "—"
```

### Scenario: a landed feature whose branch is gone still renders

```
Given feat/01-foo no longer exists and keeler-status answers from HEAD
When  the board renders
Then  every row is shown and the commit column reads "—" for each
And   the header names HEAD as the ref
```

### Stage

### Scenario: before any signal the stage is reading

```
Given T1's stream holds an init record and Read tool calls only
When  the board renders
Then  T1's stage is "reading"
```

### Scenario: an edit under tests/ moves the stage to tdd

```
Given T1's stream holds an Edit tool_use on tests/top.rs
When  the board renders
Then  T1's stage is "tdd"
```

### Scenario: a gate recipe moves the stage to qa

```
Given T1's stream holds a Bash tool_use with command "just dev 2>&1 | tail -35"
When  the board renders
Then  T1's stage is "qa"
```

### Scenario: the code-review skill or a write to the review record means review

```
Given T1's stream holds a Skill tool_use with skill "code-review"
When  the board renders
Then  T1's stage is "review"
And   a Write tool_use on reviews/01-foo/t1.md alone gives the same answer
```

### Scenario: a mutants command means mutants

```
Given T1's stream holds a Bash tool_use with command "just mutants-diff main"
When  the board renders
Then  T1's stage is "mutants"
```

### Scenario: a keeler-branch command sets the stage to gate

```
Given T1's stream holds a Bash tool_use whose command starts with "just keeler-branch"
When  the board renders
Then  T1's stage is "gate"
```

### Scenario: the stage does not move backwards

```
Given T1's stream holds a Skill code-review followed by a Bash "just dev"
When  the board renders
Then  T1's stage is still "review"
```

### Scenario: a keeler skill call, if one appears, names its stage

```
Given T1's stream holds a Skill tool_use with skill "keeler:mutants"
When  the board renders
Then  T1's stage is "mutants"
```

### Scenario: a subagent's tool calls do not move the stage

```
Given T1's stream holds a Bash "just dev" whose record has parent_tool_use_id set, and nothing else
When  the board renders
Then  T1's stage is "reading"
```

### Scenario: a task with an exit file shows ended as its stage

```
Given T1's .exit file holds 0 and keeler-status says passed
And   T2's .exit file holds 1 and keeler-status says failed (exit 1)
When  the board renders
Then  both stages read "ended"
```

### Scenario: an edit outside the worktree is not tdd

```
Given T1's stream holds a Write tool_use on /tmp/probe/src/lib.rs and nothing else
When  the board renders
Then  T1's stage is "reading"
```

### Scenario: an edit to the worktree's Justfile is tdd

```
Given T1's stream holds an Edit tool_use on <worktree>/Justfile
When  the board renders
Then  T1's stage is "tdd"
```

### Scenario: a command that merely mentions mutants is not the mutants stage

```
Given T1's stream holds a Bash tool_use with command "grep -c mutants mutants.out"
When  the board renders
Then  T1's stage is "reading"
```

### Scenario: crap-delta is qa

```
Given T1's stream holds a Bash tool_use with command "keeler crap-delta"
When  the board renders
Then  T1's stage is "qa"
```

### Last tool and elapsed time

### Scenario: the tool column shows the last main-session tool call and its command

```
Given T1's stream's last main-session tool_use is Bash with command "just dev 2>&1 | tail -35"
When  the board renders
Then  T1's tool column reads "Bash: just dev 2>&1 | tail -35"
```

### Scenario: a subagent's tool call does not become the row's tool

```
Given T1's main session's last tool_use is Task, and a later tool_use with parent_tool_use_id set is Bash "cargo test"
When  the board renders
Then  T1's tool column reads "Task" and its description
```

### Scenario: a long command is cut to the column with an ellipsis

```
Given the last command is longer than the tool column
When  the board renders
Then  the column shows its head followed by "…"
And   the detail pane shows it whole
```

### Scenario: elapsed counts from the last tool call's timestamp

```
Given T1's last tool_use assistant record is stamped 02:14 before now
And   no tool_result for its tool_use_id has arrived
When  the board renders
Then  T1's elapsed column reads "02:14"
```

### Scenario: elapsed over an hour shows hours

```
Given T1's last tool_use is stamped 1 hour 2 minutes 5 seconds before now and has not returned
When  the board renders
Then  T1's elapsed column reads "1:02:05"
```

### Scenario: a tool that returned shows no elapsed time

```
Given T1's last tool_use has a tool_result carrying its tool_use_id later in the stream
When  the board renders
Then  T1's elapsed column is empty
```

### Scenario: a skill call shows as Skill and the skill's name

```
Given the last tool_use is Skill with skill "code-review"
When  the board renders
Then  the tool column reads "Skill: code-review"
```

### Context, tokens and model

### Scenario: the model comes from the init record

```
Given T1's stream's init record says model "claude-opus-5[1m]"
When  the board renders
Then  T1's model column reads "opus5[1m]"
```

### Scenario: context is the last main-session assistant record's input over the model's window

```
Given the model is "claude-opus-5[1m]"
And   the last main-session assistant record's usage has input_tokens 2, cache_read_input_tokens 121691 and cache_creation_input_tokens 270
When  the board renders
Then  T1's context column reads "12%"
```

### Scenario: a half is rounded up

```
Given the model is "claude-opus-5[1m]" and the usage sums to 115003
When  the board renders
Then  T1's context column reads "12%"
And   a sum of 114999 reads "11%"
```

### Scenario: a subagent's usage is not the row's context

```
Given the last assistant record in the stream has parent_tool_use_id set and a usage sum of 900000
And   the last main-session assistant record sums to 120000, window 1,000,000
When  the board renders
Then  T1's context column reads "12%"
```

### Scenario: a model without the [1m] suffix has a 200k window

```
Given the model is "claude-sonnet-5"
And   the last main-session assistant usage sums to 100000 input-side tokens
When  the board renders
Then  T1's context column reads "50%"
```

### Scenario: context at 80% or more is marked

```
Given the last main-session assistant usage puts context at 79% of the window
When  the board renders
Then  T1's context column reads "79%"
And   at 80% it reads "80%!"
```

### Scenario: tokens is the run's output summed once per message

```
Given T1's stream holds three records of one message id with output_tokens 300, and one record of another with 1200
When  the board renders
Then  T1's tokens column reads "1.5k"
```

### Scenario: token counts print in the stated form

```
Given output sums of 999, 1000, 12100, 999999 and 12100000
When  each is formatted
Then  they read "999", "1.0k", "12.1k", "1.0M" and "12.1M"
```

### Scenario: a stream with no assistant record yet shows dashes

```
Given T1's stream holds only the init record
When  the board renders
Then  T1's context and tokens columns show "—"
And   its model column shows the init record's model
```

### Branch facts

### Scenario: the commit column is the branch's head and its distance from the feature branch

```
Given keeler/01-foo/t1 is three commits ahead of feat/01-foo, head f5064b1
When  the board renders
Then  T1's commit column reads "f5064b1 +3"
```

### Scenario: uncommitted changes in the worktree are counted

```
Given T1's worktree has two modified files and one untracked
When  the board renders
Then  T1's commit column ends with "3 dirty"
```

### Scenario: a clean worktree on the base commit shows the base

```
Given keeler/01-foo/t1 equals feat/01-foo and the worktree is clean
When  the board renders
Then  T1's commit column reads the base's short hash and "+0"
```

### Pause and resume

### Scenario: p kills the selected running task, then marks it

```
Given T1 is running in tmux session keeler-01-foo-t1 and its row is selected
When  the user presses p
Then  `tmux kill-session -t =keeler-01-foo-t1` runs
And   .keeler/runs/01-foo/t1.paused is written after it succeeds
And   T1's row reads "paused" on the next refresh
```

### Scenario: a kill that fails writes no marker

```
Given T1's row is selected and the tmux stub fails on kill-session
When  the user presses p
Then  no marker is written and the status line carries tmux's reason
```

### Scenario: p on a task that is not running does nothing

```
Given T2's row is selected and T2 is not spawned
When  the user presses p
Then  no file is written and the status line says T2 is not running
```

### Scenario: keeler-status reports paused when the marker is there

```
Given T1's tmux session is gone, no .exit file, and .keeler/runs/01-foo/t1.paused exists
When  the user runs `keeler keeler-status specs/01-foo.md`
Then  T1's line reads "paused" with the same log, worktree and resume hint died carries
And   a second hint names the rm that clears a marker nobody will resume
```

### Scenario: a paused task is not re-spawned by the wave

```
Given T1 is paused with its marker on disk
When  the user runs `keeler keeler-fan-out specs/01-foo.md`
Then  T1 is not in the wave it names
```

### Scenario: a session killed without the marker is still died

```
Given T1's tmux session is gone, no .exit file and no .paused marker
When  the board renders
Then  T1's state is "died"
```

### Scenario: R resumes a paused or died task

```
Given T1 is paused with its marker on disk and its row selected
When  the user presses R
Then  `keeler-resume specs/01-foo.md T1` runs through the plugin's Justfile
And   T1's row reads "running" on the next refresh
```

### Scenario: keeler-resume removes the marker once the session is started

```
Given .keeler/runs/01-foo/t1.paused exists and T1 is resumable
When  the user runs `keeler keeler-resume specs/01-foo.md T1`
Then  the marker is gone after `tmux new-session` succeeds
And   it is still there if tmux failed
```

### Scenario: R on a task that is not resumable shows keeler-resume's refusal

```
Given T1 is running and its row selected
When  the user presses R
Then  the status line shows keeler-resume's reason and nothing is spawned
```

### Scenario: a resumed task's stream is read from the start

```
Given the board has parsed 1 MB of T1's stream
When  the file is replaced by one 4 kB long
Then  the board discards what it parsed and reads the new file from offset 0
And   the row shows the new run's stage and tool
```

### Attach

### Scenario: Enter attaches to the selected running task and returns on detach

```
Given T1 is running, its row selected, and $TMUX is unset
When  the user presses Enter
Then  the board leaves the alternate screen and runs `tmux attach -t =keeler-01-foo-t1`
And   when tmux exits the board redraws in the alternate screen with its state refreshed
```

### Scenario: inside tmux Enter switches the client instead

```
Given $TMUX is set and T1 is running with its row selected
When  the user presses Enter
Then  the board runs `tmux switch-client -t =keeler-01-foo-t1` and stays up
```

### Scenario: Enter without tmux says so in the status line

```
Given tmux is not on PATH and T1's row is selected
When  the user presses Enter
Then  the status line says tmux is not installed and the board stays up
```

### Scenario: Enter on a task with no session says so

```
Given T2 is not spawned and its row selected
When  the user presses Enter
Then  the status line says T2 has no session to attach
```

### Navigation and refresh

### Scenario: j and k move the selection and the detail pane follows

```
Given the board shows T1..T3 with T1 selected
When  the user presses j twice then k once
Then  T2 is selected and the detail pane shows T2
```

### Scenario: the stream is re-read once a second without input

```
Given T1's stream gains a new tool_use record after the board is up
When  one second passes
Then  the tool column shows the new command
```

### Scenario: keeler-status is re-read every five seconds and on r

```
Given keeler-status's last answer is two seconds old
When  the user presses r
Then  keeler-status runs again and the header's age resets
And   without r it runs again when the answer is five seconds old
```

### Scenario: a slow keeler-status does not stall the board

```
Given keeler-status takes three seconds to answer
When  the board is up
Then  the stream columns keep refreshing every second while it runs
```

### Scenario: only the stream's new bytes are read on refresh

```
Given T1's stream is 1 MB and the board has already parsed it
When  200 bytes are appended
Then  the refresh reads those bytes and no earlier ones
```

### Scenario: q quits and restores the terminal

```
Given the board is up
When  the user presses q
Then  the process exits 0 and the terminal is restored to the screen it was on
```

### Scenario: a panic restores the terminal before the message is printed

```
Given the board is up in the alternate screen
When  the process panics
Then  the terminal leaves raw mode and the alternate screen
And   the panic message is printed on the restored screen
```

### Detail pane

### Scenario: the detail pane shows the selected task's last five texts and its last command

```
Given T1's stream holds seven main-session assistant text blocks and the last tool_use is Bash "just dev"
When  T1 is selected
Then  the pane shows the last five texts, oldest first, and the command in full
```

### Scenario: the detail pane lists the branch's commits since the feature branch

```
Given keeler/01-foo/t1 holds commits "test(01-foo): T1 red" and "feat(01-foo): T1 green" on top of feat/01-foo
When  T1 is selected
Then  the pane lists both subjects with their short hashes, newest first
```

### Robustness

### Scenario: a malformed stream line is skipped

```
Given T1's stream holds a line that is not JSON between two valid records
When  the board renders
Then  the two valid records are counted and the board shows no error
```

### Scenario: a half-written last line waits for its rest

```
Given T1's stream ends mid-record without a newline
When  the board renders
Then  the partial line is not parsed
And   it is parsed on the refresh after the newline arrives
```

### Scenario: a missing stream file shows the state alone

```
Given keeler-status says T1 died but .keeler/runs/01-foo/t1.stream does not exist
When  the board renders
Then  T1's state is died and its live columns show "—"
```

### Scenario: a narrow terminal drops the detail pane before it drops columns

```
Given a terminal 100 columns wide and 12 lines tall
When  the board renders
Then  every task row is shown with state, stage and tool
And   the detail pane is absent
```

### Properties

### Scenario: any record sequence folds without panic and in pieces as in one

```
Given any sequence of stream records, valid or malformed
When  they are folded all at once, and separately in two halves at any split
Then  neither run panics and both produce the same view
```

### Scenario: any byte stream reads the same at any split

```
Given any byte sequence of newline-separated records
When  it is fed to the reader whole, and again in two pieces split at any byte
Then  the records produced are identical
```

### Scenario: any usage yields a context percentage within bounds and monotone

```
Given any two usages a and b with a's sum no greater than b's, and any window
When  each is turned into a percentage
Then  both lie in 0..=100 and a's is no greater than b's
```

### Scenario: any stage sequence never moves backwards

```
Given any sequence of stage signals
When  they are folded
Then  every intermediate stage is no earlier than the one before it
```

### Scenario: any token count formats within the stated shape

```
Given any count
When  it is formatted
Then  it is at most six characters
And   the number it shows is within half a unit of its last digit from the count
```

---

## Tasks

Each task lists its scenarios, the test types that pin it, and — when it
depends on earlier tasks — a `Needs:` naming them. The crate is one
Cargo package; tasks touching `fold` are chained rather than parallel
because they edit one function. Every task that adds tests to a shared
file adds them under its own `// ── Tn` heading, never at the end.

- [x] **T1 — The crate exists and reads a stream incrementally.** Creates `keeler-top/` (lib + bin, `Cargo.toml`, `keeler-top/tests/top.rs`), adds it to the workspace `members`, rewrites CLAUDE.md's "the gates measure `xtask/`" paragraph. `StreamReader::poll()` with offset, trailing-partial buffer, shrink-or-second-init reset, and `Record` as a tagged enum. Scenarios: _a malformed stream line is skipped; a half-written last line waits for its rest; only the stream's new bytes are read on refresh; a resumed task's stream is read from the start; any byte stream reads the same at any split_. Tests: unit + property (split-invariance of the reader) + acceptance.
- [x] **T2 — The fold reads the stage.** Needs: T1. `Stage` (Ord), `stage_of(&ToolCall, worktree)`, the recipe-word rule, worktree-relative paths, main-session-only records, monotone fold. Scenarios: _before any signal the stage is reading; an edit under tests/ moves the stage to tdd; a gate recipe moves the stage to qa; the code-review skill or a write to the review record means review; a mutants command means mutants; a keeler-branch command sets the stage to gate; the stage does not move backwards; a keeler skill call, if one appears, names its stage; a subagent's tool calls do not move the stage; a task with an exit file shows ended as its stage; an edit outside the worktree is not tdd; an edit to the worktree's Justfile is tdd; a command that merely mentions mutants is not the mutants stage; crap-delta is qa; any stage sequence never moves backwards_. Tests: unit + property (stage monotone under any signal sequence).
- [x] **T3 — The fold reads the tool, the clock, the usage and the model.** Needs: T2. `ToolCall` with timestamp and returned flag, elapsed formatting, `window_for`, `percent`, per-message output dedupe, `format_tokens`, the five-text ring, `[1m]` from the init record. Scenarios: _the tool column shows the last main-session tool call and its command; a subagent's tool call does not become the row's tool; elapsed counts from the last tool call's timestamp; elapsed over an hour shows hours; a tool that returned shows no elapsed time; a skill call shows as Skill and the skill's name; the model comes from the init record; context is the last main-session assistant record's input over the model's window; a half is rounded up; a subagent's usage is not the row's context; a model without the [1m] suffix has a 200k window; context at 80% or more is marked; tokens is the run's output summed once per message; token counts print in the stated form; a stream with no assistant record yet shows dashes; any record sequence folds without panic and in pieces as in one; any usage yields a context percentage within bounds and monotone; any token count formats within the stated shape_. Tests: unit + property (fold associativity, percentage bounds and monotonicity, token format shape).
- [x] **T4 — The board's two reads and the branch facts.** Needs: T1. The `keeler-status` parser (header `graph:` line, path-less `done`/`not spawned`, skipped hint lines), the `keeler-graph.sh` parser, the `git show <ref>:<rel>` temp-file read, `BranchFacts` from git, and the `Dispatch` trait whose real implementation shells through the plugin's Justfile. Scenarios: _the graph is read from the ref, not the working tree; the board shells back through the Justfile it was launched from; the commit column is the branch's head and its distance from the feature branch; uncommitted changes in the worktree are counted; a clean worktree on the base commit shows the base_. Tests: unit + acceptance (synthetic repository with PATH stubs for `just`).
- [x] **T5 — The frame: rows, header, detail pane, --once.** Needs: T3, T4. `Row` assembly, `layout`, rendering through `TestBackend`, `--once` as plain text, the non-tty refusal, `keeler-status`'s refusals relayed. Scenarios: _outside a git repository the board refuses with keeler-status's words; a spec not committed on the feature branch is refused; --once prints one frame as a table and exits; without a terminal the board refuses and names --once; the header names the ref keeler-status answered from and the age of that answer; every task in the graph has a row, in spec order; the state column is keeler-status's word for the task; a not-spawned task the graph holds reads blocked with its needs; a not-spawned task the graph calls ready reads ready; a done task with no worktree shows dashes for the live columns; a landed feature whose branch is gone still renders; a long command is cut to the column with an ellipsis; the detail pane shows the selected task's last five texts and its last command; the detail pane lists the branch's commits since the feature branch; a missing stream file shows the state alone; a narrow terminal drops the detail pane before it drops columns_. Tests: unit + acceptance (TestBackend buffers, `--once` stdout).
- [x] **T6 — The loop: keys, cadences, terminal guard.** Needs: T5. `on_key` as a pure function, the one-second tick, the five-second status thread with `r`, the raw-mode/alternate-screen guard with `Drop` and the panic hook. Scenarios: _j and k move the selection and the detail pane follows; the stream is re-read once a second without input; keeler-status is re-read every five seconds and on r; a slow keeler-status does not stall the board; q quits and restores the terminal; a panic restores the terminal before the message is printed_. Tests: unit + acceptance.
- [ ] **T7 — The levers: pause, resume, attach.** Needs: T6. `Pause`, `Resume`, `Attach` actions through `Dispatch`; kill-then-mark; `switch-client` inside tmux; the guard dropped and re-entered around attach. Scenarios: _p kills the selected running task, then marks it; a kill that fails writes no marker; p on a task that is not running does nothing; a session killed without the marker is still died; R resumes a paused or died task; R on a task that is not resumable shows keeler-resume's refusal; Enter attaches to the selected running task and returns on detach; inside tmux Enter switches the client instead; Enter without tmux says so in the status line; Enter on a task with no session says so_. Tests: unit (recording `Dispatch` double) + acceptance (tmux and just stubs).
- [x] **T8 — keeler-status and keeler-resume learn the marker.** `paused` with both hints in `keeler-status`, marker removal after `tmux new-session` in `keeler-resume`, `tests/top_recipes.rs` created, `tests/spawn.rs` updated where the new word appears. Justfile regions: the `died` branch of `keeler-status` and the tail of `keeler-resume`. Scenarios: _keeler-status reports paused when the marker is there; a paused task is not re-spawned by the wave; keeler-resume removes the marker once the session is started_. Tests: acceptance.
- [ ] **T9 — The keeler-top recipe.** Needs: T5, T8. `keeler-top *ARGS` under its own heading in the Justfile: the first-build notice against `{{justfile_directory()}}/target/release/keeler-top`, `cargo run --release --manifest-path`, the three paths and pass-through flags; tests added to `tests/top_recipes.rs` under a `// ── T9` heading. Scenarios: _the recipe builds the crate from the plugin tree and passes it three paths; --once through the front door prints the table on stdout; the first launch says it is building before cargo starts_. Tests: acceptance (cargo stub on PATH).

---

## Implementation Notes

**Crate.** `keeler-top/` in the workspace: a library holding every decision and a thin binary, as `xtask` is laid out, so `keeler-top/tests/` can reach the core. Dependencies:
`ratatui`, `crossterm`, `serde`, `serde_json`. `Cargo.toml`'s `members`
gains it. The gate recipes already run `--workspace`, so they reach the
new member without a change; what changes is CLAUDE.md's "the gates
measure `xtask/`, and only it" paragraph, and `crap-baseline.json`, which
is refreshed on main at fan-in as the rules require — the new crate's
functions are absent from it until then.

**Reaching the coverage bar.** `just cov` fails under 90% lines across
the workspace and `just crap` at 15, and a TUI's terminal setup, event
loop and shell-outs are the classic uncoverable surface. The crate is
split so that surface is thin: a pure core (`fold`, `window_for`,
`percent`, `format_tokens`, `stage_of`, `on_key`, `layout`, the
`keeler-status` and `keeler-graph.sh` parsers) is unit- and
property-tested with no terminal; rendering runs through
`ratatui::backend::TestBackend`; the shell-outs (`Attach`, `Pause`,
`Resume`, `Status`) are one `Dispatch` trait with a recording test
double, and the real one is exercised by the acceptance tests against
PATH stubs for `tmux` and `just`, the way `tests/spawn.rs` does. The
functions expected to sit near the threshold are `main` and the
terminal guard; they hold no decisions.

**Data flow.** Every second:

1. `bash <plugin>/scripts/keeler-graph.sh <spec>` → ready/blocked/done per task, ~0 ms.
2. For each task with a stream: `StreamReader::poll()` compares the file's length to its offset (shorter → reset to 0 and clear the view), reads from the offset, splits on newline, keeps the trailing partial, parses each line with `serde_json::from_str::<Record>` into a tagged enum (`Init`, `Assistant`, `ToolResult`, `Other`) and folds it into `RunView`; records with `parent_tool_use_id` set fold into nothing.
3. `git` per worktree, when the feature branch exists: `rev-parse --short HEAD`, `rev-list --count feat/<slug>..HEAD`, `status --porcelain`, `log --format=%h %s feat/<slug>..HEAD`.
4. Render.

Every five seconds and on `r`, on a thread: `just --justfile
<plugin>/Justfile --working-directory <root> keeler-status <spec>`,
parsed into the header (`graph: <rel> on <ref>`) and one `StatusLine
{ id, state, log: Option, worktree: Option }` per task — `done` and `not
spawned` lines carry no paths, and `died`/`failed` are followed by a hint
line that is skipped.

**Key types.**

```rust
enum Stage { Reading, Tdd, Qa, Review, Mutants, Gate, Ended }  // Ord
struct RunView { model: Option<String>, stage: Stage, last_tool: Option<ToolCall>, last_tool_returned: bool, context_used: Option<u64>, output_by_message: HashMap<String, u64>, texts: VecDeque<String> /* ≤5 */ }
struct ToolCall { name: String, detail: String, at: Timestamp }
struct Row { id: String, state: State, run: Option<RunView>, branch: Option<BranchFacts> }
fn window_for(model: &str) -> u64        // "[1m]" → 1_000_000, else 200_000
fn percent(used: u64, window: u64) -> u8 // round half up, clamp 100
fn stage_of(call: &ToolCall) -> Option<Stage>
fn fold(view: &mut RunView, record: Record)
fn on_key(app: &mut App, key: KeyEvent) -> Action
```

**Elapsed.** From the assistant record's `timestamp` against the clock,
not from `tool_progress`: the heartbeats arrive every 30 s and only
while a tool runs, so the timestamp is both finer and present for every
call.

**Justfile.** Three touches: a new `keeler-top *ARGS` recipe (prints
the first-build notice when `{{justfile_directory()}}/target/release/keeler-top`
is absent, then `cargo run --release --manifest-path … -- --plugin-root
{{justfile_directory()}} --root <cwd> ARGS`); `keeler-status` prints
`paused` where it prints `died` today when the marker exists, with the
resume hint and an `rm` hint; `keeler-resume` deletes the marker after
`tmux new-session` returns zero. `keeler-fan-out` needs nothing: it
selects `not spawned` lines and `paused` is not one.

**Tests.** The crate's own logic and rendering live in
`keeler-top/tests/top.rs` (the binary is reachable there as
`CARGO_BIN_EXE_keeler-top`; it is not from the root harness). The three
recipe scenarios — the `keeler-top` recipe, `paused` in `keeler-status`,
the marker in `keeler-resume` — live in `tests/top_recipes.rs` in the
root harness beside the other recipe suites; `tests/spawn.rs` asserts
on `keeler-status` output today and is updated where the new word
appears.

**Terminal guard.** Raw mode and the alternate screen are entered by a
guard whose `Drop` restores both, and a panic hook restores them before
the default hook prints. `Attach` drops the guard, runs tmux, and
re-enters on return. `--once` and a non-tty stdout never touch the
terminal.

### Non-goals

- Replacing `keeler-status`; it stays the script-facing board and the one place state is decided.
- Spawning a new task, fanning out or landing from the TUI. Those remain shell recipes the human runs by name.
- Cost in dollars, rate-limit windows, or anything from `rate_limit_event`.
- Watching more than one spec at once.
- Any agent other than Claude Code, or any run not started by `keeler-spawn` / `keeler-resume`.
- Shipping a prebuilt binary; the plugin builds from source on first use.
- Reading `.log`: it is the append-only record across runs; the board reads the current run's `.stream`.
- Subagent activity as its own rows or metrics.
