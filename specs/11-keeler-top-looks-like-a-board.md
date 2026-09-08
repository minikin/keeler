# Spec 11 — keeler-top looks like a board

**Status:** Approved
**Effort:** Large
**Module:** `keeler-top/src/frame.rs`, `keeler-top/src/theme.rs` (new), `keeler-top/src/board.rs` (row order, new facts), `keeler-top/src/graph.rs` (task titles), `keeler-top/src/run.rs` (percentage as a number, spawn time), `keeler-top/src/app.rs` (compact toggle, scroll), `keeler-top/src/dispatch.rs` (review record and exit file reads), `keeler-top/src/main.rs` (theme from the environment), `keeler-top/tests/top.rs`, `crap-baseline.json` (refreshed at fan-in)

## Context

Spec 10 built the board's substance: what a row knows, where it reads
it, and the three levers. What it did not build is a board anyone
wants to look at. The first live frame was a bare grid — unstyled
text, a white reverse-video bar across the whole selected row, nine
identical `done` lines padded with dashes to the right edge, and a
one-line detail pane reading `T1 — done`. The user's verdict was a
single word.

The reference is the terminal monitors the user already runs beside
Keeler (abtop, btop): dark ground, a monospace face, panels drawn as
bordered boxes with a title on the frame, colour that means something,
and rows that carry a second line for the thing happening right now. A
canvas was drawn first and reviewed by a designer before this text was
written; the scenarios below stand on their own, and the canvas is the
reference they were drawn from, not a thing a test can open:

> https://claude.ai/code/artifact/41097262-5cc5-4132-8fad-cee8cdcf1eb2

**What a board is for.** A watcher of a wave asks, in this order:
*is anything stuck? what is running, and how far along? what is left?*
So the board leads with what needs a human — failed, died, incomplete,
paused — puts running work next, and closes with what waits. The
header's first line is the healthy counts; its right half names the
exceptions, and is empty when there are none, which is itself the
signal.

**What changes.** Three bordered panels with titles on the frame
(`wave`, `tasks`, the selected task) and a one-line message footer.
Rows carry a state glyph and its word, a stage, a model, an eight-cell
context bar with the percentage beside it, tokens, the commit facts,
and the task's title from the spec. A live task shows a second line
under its row — `└─ Tool: command`, elapsed at the right edge — while
closed and waiting tasks take one line and never pad with dashes. The
selected row is a marker in the row's own colour and a lighter
background, never a full-width reverse bar. When the rows outgrow the
panel, live rows collapse to one line except the selected one, and the
panel scrolls to keep the selection visible.

**What does not change.** Spec 10's data contract is kept and
extended, never replaced: the stage table, the two reads and their
cadences, the levers. `--once` keeps its plain-text table, its spec
order and its column words exactly — it is the script and test
surface, and a script that parsed it yesterday parses it tomorrow.

**New reads this spec adds**, each with its source:

| Fact | Where it comes from |
| --- | --- |
| task titles | the spec copy the board already fetches from the ref: the text between `**Tn — ` and the `.**` that closes it |
| review verdict | `reviews/<slug>/<tid>.md` on the task branch while it exists, on the ref once landed — the `Verdict:` line |
| exit code | `.keeler/runs/<slug>/<tid>.exit` under the project root the recipe passed; the board composes this path itself for a task whose status line carries none |
| log path | `.keeler/runs/<slug>/<tid>.log`, composed the same way |
| time since spawn | the timestamp of the stream's first `init` record |

**The state vocabulary, all of it.** `keeler-status` prints `running`,
`passed`, `incomplete (<reason>)`, `failed (exit N)`, `died`, `paused`,
`done` and `not spawned`; the graph adds `ready` and `blocked ← …`. The
board holds no second opinion about these words: the theme classifies a
state by its leading word and passes the rest through untouched.

| State | Glyph | ASCII | Colour | Group |
| --- | --- | --- | --- | --- |
| failed (exit N) | `✗` | `x` | red | 0 needs you |
| died | `⊘` | `X` | red | 0 |
| incomplete (…) | `◔` | `o` | yellow | 0 |
| paused | `‖` | `=` | violet | 1 |
| running | `●` | `*` | orange | 2 |
| passed | `◐` | `+` | green | 3 |
| ready | `◇` | `<` | blue | 4 |
| blocked ← … | `○` | `-` | dim | 5 |
| not spawned | `·` | `.` | dim | 6 |
| done | `✓` | `v` | dim in a live view, green in a finished view | 7 |
| anything else | `?` | `?` | plain | 2 |

Rows are ordered by group, and within a group in the order the status
report gave them, which is the spec's order. **Finished view** means
every row's state is `done`; it swaps the live columns for the title
alone and paints done green.

**Assumptions carried into approval.**

- Implemented after spec 10 lands on main, on a branch cut from that
  main.
- The theme is a value built once from the environment in `main` and
  passed to the renderer, so a test renders the same board through two
  themes. `NO_COLOR` set means every style is the default and the glyphs
  alone carry state, which is why every state's glyph is distinct.
  `KEELER_TOP_ASCII=1` (or a locale that is not UTF-8) swaps every
  non-ASCII glyph for the ASCII column above, and the box borders for
  `+-|`.
- Colours, from the canvas after the contrast pass (all ≥ 4.5:1 on the
  ground `#14161f`, dim included):
  text `#c0caf5`, dim `#787f9e`, chrome `#565f89` (rules and the empty
  bar track only), orange `#ff9e64`, green `#9ece6a`, red `#f7768e`,
  yellow `#e0af68`, violet `#bb9af7`, blue `#7aa2f7`, cyan `#7dcfff`,
  selection background `#2a2f45`, panel borders `#3b4261` with the
  title in the panel's colour: wave blue, tasks orange, detail green.
- Glyphs that terminals render at ambiguous width (`● ◐ ◔ ○ ‖ ⊘ ◇ ▸
  └ ─ ← …`) are accepted as the price of a btop-style board; the
  ASCII fallback is the way out for a terminal that doubles them. The
  marker is `▸` (U+25B8), never `▶`, which some platforms draw as an
  emoji.
- Columns at full width, each followed by one space; the marker column
  carries its own trailing space; offsets are from the panel's inner
  left edge:

  | Column | Width | Starts at | Content |
  | --- | --- | --- | --- |
  | MARK | 2 | 0 | `▸ ` on the selected row, two spaces otherwise |
  | TASK | 4 | 2 | the id |
  | STATE | 17, or the widest state present if wider | 7 | glyph, space, the state's word and its reason, never cut |
  | STAGE | 7 | 25 | |
  | MODEL | 9 | 33 | |
  | CONTEXT | 14 | 43 | 8 bar cells, space, the percentage right-aligned in 4, a 1-cell flag slot holding `!` from 80% |
  | TOKENS | 6 | 58 | right-aligned, header right-aligned too |
  | COMMIT | 13 | 65 | `<hash> +<ahead> ~<dirty>`, `~N` only when dirty; the header reads `COMMIT +~` |
  | TITLE | the rest | 79 | the task's title, dim, cut with `…` |

  Offsets are inside the panel: the usable width is the terminal's
  minus 2 for the borders. When STATE is wider than 17 every later
  column moves right by the difference. The bar fills `round(percent /
  12.5)` cells, half up. The **finished view** has its own row: MARK 2,
  TASK 4, STATE 6 (`✓ done`), LANDED the rest, so LANDED starts at 14
  and its header reads `  TASK STATE  LANDED`.
- Width bands follow from the table, and they are ordered and
  cumulative: a column dropped at a wider band stays dropped, and a
  column that no longer fits is dropped whole, never clipped. With
  STATE at 17 the row without a title is 78 cells wide (0..77), so:
  the title needs 12 cells and is drawn from 93 columns; from 80 the
  row is whole without a title; from 71 the bar collapses to ` NN% `
  (5 cells, header `CTX`); from 54 MODEL and TOKENS are gone; below
  that columns keep dropping right to left — CTX, then COMMIT — and
  STATE, STAGE and the id are the last to go; a row that still does not
  fit overflows rather than showing a clipped hash. A wider STATE
  shifts every threshold by the difference. The detail panel goes below 100
  columns; the key hints go when strip + hints do not fit the second
  header line; when the first header line does not fit, the counts are
  cut from the left and the status age is never dropped. Height: the
  detail panel goes below 24 rows; the wave panel collapses to its
  first line below 16 rows (the hints go with it); the tasks panel
  never gets fewer than 5 rows. A panel's row count includes its
  column-header line. When the selection falls below the window the
  panel scrolls so the selected task's last line is its last row; when
  it falls above, so the task's first line is its first row.
- The message footer is the last line of the frame, below every panel.
- The header's counts cover running, passed, ready, blocked, not
  spawned and done; the four needs-you states appear in the right half
  only, named in the order of the state table and, within one state, in
  the report's order.
- The second line of a live row is `    └─ <Tool>: <detail>` with the
  elapsed right-aligned to the panel's inner right edge; for a paused
  task it reads `    └─ resume: R`.

---

## Acceptance Tests

### Panels

### Scenario: the frame is three bordered panels and a footer

```
Given a running wave and a 120x40 terminal
When  the board renders
Then  three boxes are drawn with box-drawing borders, titled "wave  <spec> on <ref>", "tasks" and the selected task's id on their top borders
And   the last line of the frame is reserved for messages
```

### Scenario: panel borders are neutral and titles carry the colour

```
Given the board renders
Then  every border cell is drawn in the chrome border colour
And   "wave" is blue, "tasks" is orange and the detail title is green
```

### Scenario: the message footer shows the last lever's refusal

```
Given the user pressed p on T2, which is not running
When  the board renders
Then  the footer reads "keeler-top: T2 is not running — there is nothing to pause." in yellow
And   the footer is empty again after the next keypress
```

### Header

### Scenario: the wave title names the spec and the ref

```
Given keeler-status answered "graph: specs/01-foo.md on feat/01-foo"
When  the board renders
Then  the wave panel's title reads "wave  specs/01-foo.md on feat/01-foo"
```

### Scenario: a landed feature's board names HEAD as the ref

```
Given feat/01-foo is gone and keeler-status answered from HEAD
When  the board renders
Then  the wave panel's title reads "wave  specs/01-foo.md on HEAD"
```

### Scenario: the first header line is the healthy counts and the status age

```
Given tasks running ×2, passed ×1, done ×3, and keeler-status answered 1 s ago
When  the board renders
Then  the wave panel's first line begins "● 2 running   ◐ 1 passed   ✓ 3 done"
And   ends with "status 1s ago"
And   a state with no task is absent from the counts
```

### Scenario: the first line's right half names what needs a human

```
Given T9 failed (exit 2), T8 paused, T4 died, T2 incomplete (no review record)
When  the board renders
Then  the first line's right half reads "✗ T9 exit 2 · ⊘ T4 died · ◔ T2 incomplete · ‖ T8 paused" before "status 1s ago"
And   each item is in its state's colour
```

### Scenario: with nothing to act on the right half is empty

```
Given every task is running, passed, blocked or done
When  the board renders
Then  nothing stands between the counts and "status 1s ago"
```

### Scenario: the second line is an outcome strip, one glyph per task, in fives

```
Given tasks T1 done, T2 done, T3 running, T4 died, T5 passed, T6 paused, T7 blocked, T8 not spawned, T9 ready, T10 failed
When  the board renders
Then  the second line begins "✓✓●⊘◐ ‖○·◇✗" with a space after every fifth glyph and none after the last
And   each glyph is in its state's colour
```

### Scenario: the key hints sit at the right of the second line with bright keys

```
Given a 120-column terminal with nine tasks
When  the board renders
Then  the second line ends with "j/k move · Enter attach · p pause · R resume · r refresh · z compact · q quit"
And   the key names are in the text colour and the words after them dim
```

### Scenario: hints go before the strip is cut

```
Given 30 tasks in an 80-column terminal
When  the board renders
Then  the second line holds the whole 30-glyph strip and no hints
```

### Scenario: a finished spec's header says so

```
Given every task is done
When  the board renders
Then  the first line begins "✓ 9 done   the feature is finished here — land it on main"
And   the hints read "j/k move · r refresh · q quit"
```

### Rows

### Scenario: a running task's row

```
Given T3 is running, stage review, model "claude-opus-5[1m]", context 41%, 12.1M output tokens, head b33e05f four ahead with two dirty files, titled "The fold reads the tool, the clock, the usage and the model", selected
When  the board renders at 120 columns
Then  T3's row reads "▸ T3   ● running         review  opus5[1m] ███░░░░░  41%   12.1M b33e05f +4 ~2 The fold reads the tool, the clock, th…"
And   it is exactly 118 cells wide
```

### Scenario: the running task's second line

```
Given T3's last tool is Bash "just dev 2>&1 | tail -35", started 02:14 ago
When  the board renders
Then  the line under T3's row begins "    └─ Bash: just dev 2>&1 | tail -35"
And   "02:14" ends at the panel's inner right edge
And   "Bash" is cyan, the command plain, the "└─" dim
```

### Scenario: a paused task's second line says how to resume

```
Given T8 is paused
When  the board renders
Then  the line under T8's row reads "    └─ resume: R"
```

### Scenario: closed and waiting tasks take one line

```
Given T6 passed, T9 failed (exit 2), T4 died, T2 incomplete (no review record), T7 blocked ← T6, T5 ready, T1 done, T10 not spawned
When  the board renders
Then  each of those rows is one line and no line stands under it
```

### Scenario: a state's reason is never cut

```
Given T2 is incomplete (no review record, box not ticked) on a 140-column terminal
When  the board renders
Then  T2's STATE column reads "◔ incomplete (no review record, box not ticked)" whole
And   every later column in every row starts 30 cells further right than the table says
```

### Scenario: a done row in a live view is the id, the word and the title

```
Given T1 is done, titled "The crate exists and reads a stream incrementally", and T3 is running
When  the board renders
Then  T1's row reads "  T1   ✓ done" followed by spaces and then the title at cell 79
And   no "—" appears in the row
```

### Scenario: a row with no run shows empty live columns

```
Given T5 is ready
When  the board renders
Then  T5's STAGE, MODEL, CONTEXT, TOKENS and COMMIT columns are blank
```

### Scenario: a state the board does not recognise still gets a row

```
Given keeler-status printed a word the theme has no entry for
When  the board renders
Then  the row shows "?" and the word, in the text colour, in the running group
```

### Scenario: rows are ordered by what needs a human first

```
Given T1 done, T2 incomplete (no review record), T3 running, T4 died, T5 ready, T6 passed, T7 blocked ← T6, T8 paused, T9 failed (exit 2), T10 not spawned
When  the board renders
Then  the rows appear in the order T2, T4, T9, T8, T3, T6, T5, T7, T10, T1
```

### Scenario: within a group the report's order holds

```
Given the report lists T5 before T3 and both are running
When  the board renders
Then  T5's row is above T3's
```

### Scenario: --once is untouched

```
Given the ten tasks above
When  the user runs `keeler-top --once specs/01-foo.md`
Then  stdout lists T1..T10 in the report's order in spec 10's plain table
And   its commit column still reads "b33e05f +4  2 dirty" and its dash cells are still "—"
And   no box-drawing, colour, bar glyph or second line appears
```

### Scenario: the finished view lists each task's title

```
Given every task is done and the spec's Tasks section names T1 "The crate exists and reads a stream incrementally"
When  the board renders
Then  the tasks header reads "  TASK STATE  LANDED"
And   T1's row reads "  T1   ✓ done The crate exists and reads a stream incrementally" with "✓ done" green
```

### Scenario: a title the spec does not give is blank

```
Given T4's task line lacks the "**T4 — …**" form
When  the board renders
Then  T4's TITLE column is blank and the board shows no error
```

### Selection

### Scenario: the selected row carries a marker in its own colour and a lighter background

```
Given T3 (running) is selected
When  the board renders
Then  T3's row begins with "▸" in orange
And   both of T3's lines have the selection background
And   no other row has it, and no cell uses reverse video
```

### Scenario: the marker takes the row's colour

```
Given T1 (done) is selected in a live view
When  the board renders
Then  the marker is dim
```

### Scenario: j and k move over tasks, not lines

```
Given T3 (two lines) is selected and T5 (two lines) is next
When  the user presses j
Then  T5 is selected and its marker is on its first line
```

### Colour

### Scenario: each state has its glyph and colour

```
Given one task in each of the ten states
When  the board renders
Then  failed is "✗" red, died "⊘" red, incomplete "◔" yellow, paused "‖" violet, running "●" orange, passed "◐" green, ready "◇" blue, blocked "○" dim, not spawned "·" dim, done "✓" dim
```

### Scenario: a finished view paints done green

```
Given every task is done
When  the board renders
Then  every "✓ done" is green
```

### Scenario: the context bar fills cells and colours by threshold

```
Given context at 41%, 65% and 84%
When  the board renders
Then  the bars read "███░░░░░", "█████░░░" and "███████░"
And   their filled cells are blue, yellow and red, the empty cells chrome
And   the percentages read "  41% ", "  65% " and "  84%!"
```

### Scenario: the thresholds are at 60 and 80

```
Given context at 59%, 60%, 79% and 80%
When  the board renders
Then  59% is blue, 60% and 79% yellow, 80% red with "!"
```

### Scenario: the percentage and its flag never move a neighbour

```
Given context at 9%, 41% and 100%
When  the board renders
Then  the CONTEXT column reads "█░░░░░░░   9% ", "███░░░░░  41% " and "████████ 100%!"
And   each is 14 cells
```

### Scenario: the commit column colours the hash and the dirty count

```
Given T3's commit facts are hash b33e05f, 4 ahead, 2 dirty
When  the board renders
Then  "b33e05f" is yellow, "+4" plain, "~2" red
And   a clean worktree shows "+4" and no "~"
```

### Scenario: NO_COLOR draws the same characters in default colours

```
Given NO_COLOR is set
When  the board renders
Then  every cell's foreground and background are the terminal defaults
And   the characters in every cell equal those of the coloured frame
And   the selected row is marked by "▸" alone
```

### Scenario: the ASCII theme replaces every non-ASCII glyph

```
Given KEELER_TOP_ASCII=1
When  the board renders
Then  the states read x X o = * + < - . v, the fallback "?", the bar "###-----", the marker ">", the connector "`-" and the borders "+-|"
And   "←", "·", "…" and "→" are drawn as "<-", "-", "..." and "->"
And   no glyph the theme owns is above U+007F
```

### Compact mode and scrolling

### Scenario: rows that outgrow the panel collapse to one line except the selected

```
Given 14 running tasks, T3 selected, and a tasks panel 20 rows tall
When  the board renders
Then  T3 takes two lines and the other running tasks one
And   each collapsed row shows its tool in the TITLE column as "Bash: just dev…"
```

### Scenario: z toggles compact by hand

```
Given nine tasks that fit two-line
When  the user presses z
Then  every live row is one line and the hints read "z expand"
And   pressing z again restores the second lines
```

### Scenario: a short panel scrolls to keep the selection visible

```
Given 12 tasks, all running, and a tasks panel 8 rows tall
When  the user presses j until T12 is selected
Then  T12's row is drawn inside the tasks panel
And   the first row drawn is T7
```

### Detail pane

### Scenario: the live pane opens with a fact block

```
Given T3 is running, stage gate, in its tool for 07:52, spawned 41 minutes ago, log at .keeler/runs/10-keeler-top/t3.log, review record with Verdict: pass on its branch
When  T3 is selected
Then  the pane's first lines read
      "state   ● running · gate · 07:52 in this tool · 41m since spawn"
      "paths   keeler/10-keeler-top/t3 · ../keeler-10-keeler-top-t3 · tmux keeler-10-keeler-top-t3"
      "run     .keeler/runs/10-keeler-top/t3.log"
      "review  reviews/10-keeler-top/t3.md   Verdict: pass"
```

### Scenario: a review record not yet written says so

```
Given T3 has no reviews/<slug>/t3.md on its branch
When  T3 is selected
Then  the review line reads "review  — not written yet"
```

### Scenario: a failed task's state line carries the exit code

```
Given T9 failed (exit 2) and its turn ended 04:11 ago
When  T9 is selected
Then  the state line reads "state   ✗ failed · exit 2 · ended 04:11 ago"
```

### Scenario: the pane's sections are ruled and ordered agent, commits, last command

```
Given T3 has seven texts, four commits since the feature branch and a last command
When  T3 is selected
Then  after the fact block come "── agent ──" and the last five texts, oldest first, without a prefix
And   then "── commits ──" and one commit per line, newest first, the first hash equal to the row's COMMIT hash
And   then "── last command ──" and the command in full
And   every rule spans the pane's inner width
```

### Scenario: commits that do not fit end with a count

```
Given T3 has 9 commits since the feature branch and the pane holds 3 commit lines
When  T3 is selected
Then  the third line reads "… +7 more" in dim
```

### Scenario: a done task's pane shows its record, its run and the next step

```
Given T1 is done and its worktree is gone
When  T1 is selected
Then  the pane's title reads "T1"
And   its lines read "state   ✓ done · landed, worktree and branch removed by keeler-land", "review  reviews/<slug>/t1.md   Verdict: pass", "run     .keeler/runs/<slug>/t1.log · exit 0", "next    keeler keeler-land on main → baseline staged, Status: Implemented"
```

### Scenario: a done task whose run files are gone shows dashes for them

```
Given T1 is done and .keeler/runs/<slug>/t1.exit does not exist
When  T1 is selected
Then  the run line reads "run     —"
```

### Narrow and short terminals

### Scenario: at 100 columns the title is drawn cut

```
Given a 100-column terminal
When  the board renders
Then  T3's title occupies cells 79..97 and ends with "…"
```

### Scenario: below 93 columns the title goes and the row stays whole

```
Given a 90-column terminal
When  the board renders
Then  no title is drawn
And   MODEL, TOKENS and the full eight-cell bar are still drawn
```

### Scenario: below 81 columns the bar collapses to its percentage

```
Given a 78-column terminal
When  the board renders
Then  the CONTEXT column reads " 41% " and its header "CTX"
And   MODEL and TOKENS are still drawn
```

### Scenario: below 72 columns MODEL and TOKENS go

```
Given a 68-column terminal
When  the board renders
Then  the tasks header reads "  TASK STATE             STAGE   CTX   COMMIT +~"
```

### Scenario: a column that does not fit is dropped whole

```
Given a 60-column terminal and T2 incomplete (no review record)
When  the board renders
Then  T2's row shows "◔ incomplete (no review record)" whole and its stage
And   COMMIT is absent from every row rather than half drawn
```

### Scenario: the detail panel goes below 100 columns or 24 rows

```
Given a 96x40 terminal, and separately a 120x22 terminal
When  the board renders
Then  in both the wave and tasks panels are drawn and the detail panel is absent
```

### Scenario: the wave panel collapses to one line below 16 rows

```
Given a 120x14 terminal
When  the board renders
Then  the wave panel holds only its first line
And   the tasks panel has at least 5 rows
```

### Properties

### Scenario: any set of tasks orders by group then report order

```
Given any list of tasks with any states in any report order
When  the rows are ordered
Then  every task in a lower-numbered group precedes every task in a higher one
And   within a group the report's order holds
And   ordering twice gives the same result
```

### Scenario: any percentage fills a monotone number of cells

```
Given any two percentages a ≤ b
When  their bars are drawn
Then  a's filled cells ≤ b's, both in 0..=8, and 100% fills all eight
```

### Scenario: every state maps to a distinct glyph in both glyph sets

```
Given the ten states and the fallback
When  each is themed in the Unicode set and in the ASCII set
Then  no two share a glyph in either set
```

### Scenario: any set of states is never cut

```
Given any set of state words of any length
When  the columns are computed
Then  every state's word is drawn whole and every later column starts after the widest
```

### Scenario: the plain theme carries no colour

```
Given any state and any percentage
When  the NO_COLOR theme styles them
Then  every Style is the default
```

---

## What this supersedes in spec 10

Spec 10's acceptance tests stay as written for `--once`; for the TUI
frame these six change wording, and their tests move to the `--once`
path or are rewritten under this spec:

- *every task in the graph has a row, in spec order* — holds for `--once`; the frame orders by group.
- *a done task with no worktree shows dashes for the live columns* — holds for `--once`; the frame leaves them blank.
- *a landed feature whose branch is gone still renders* — holds; the frame no longer ends rows with `—`.
- *a narrow terminal drops the detail pane before it drops columns* — holds; the tool now sits on the second line or in the TITLE column, not in the row.
- *the detail pane shows the selected task's last five texts and its last command* — the texts lose their prefix and sit under `── agent ──`.
- *the commit column …* scenarios — `N dirty` stays in `--once`; the frame writes `~N`.

---

## Tasks

Each task lists its scenarios, the test types that pin it, and — when it
depends on earlier tasks — a `Needs:` naming them. `frame.rs` is the hot
file: tasks that touch it are chained, or told which new file to put
their part in, so no two branches edit one region. Tests added to
`keeler-top/tests/top.rs` go under a `// ── Tn` heading, never at the
end.

- [x] **T1 — The theme.** `keeler-top/src/theme.rs` (new) and `main.rs` wiring: `Theme::new`/`from_env`, the ten-state table with glyphs, ASCII glyphs, colours and groups, `bar()` with round-half-up cells and the three colours, the flag slot, `NO_COLOR` and `KEELER_TOP_ASCII`; `run.rs` exposes the percentage as a number. Scenarios: _the thresholds are at 60 and 80; the percentage and its flag never move a neighbour; any percentage fills a monotone number of cells; every state maps to a distinct glyph in both glyph sets; the plain theme carries no colour_. Tests: unit + property (bar monotone and bounded, glyphs injective in both sets, plain theme all-default).
- [ ] **T2 — Rows as lines, in order.** Needs: T1. `Columns` from the width and the widest state present (in a new `keeler-top/src/layout.rs`), `frame::lines()` producing one or two styled lines per task, `board::order()` by group then report order, `graph::titles()` and `Row.title`, the finished-view row, `--once` untouched. Scenarios: _a running task's row; the running task's second line; a paused task's second line says how to resume; closed and waiting tasks take one line; a state's reason is never cut; a done row in a live view is the id, the word and the title; a row with no run shows empty live columns; a state the board does not recognise still gets a row; rows are ordered by what needs a human first; within a group the report's order holds; --once is untouched; the finished view lists each task's title; a title the spec does not give is blank; any set of tasks orders by group then report order; any set of states is never cut_. Tests: unit + property (ordering total and stable, no state cut) + acceptance (TestBackend).
- [ ] **T3 — Colour in the frame.** Needs: T2. Every span carries its theme style; `NO_COLOR` and ASCII rendered end to end. Scenarios: _each state has its glyph and colour; a finished view paints done green; the context bar fills cells and colours by threshold; the commit column colours the hash and the dirty count; NO_COLOR draws the same characters in default colours; the ASCII theme replaces every non-ASCII glyph_. Tests: acceptance (cell style assertions).
- [ ] **T4 — Panels, header and footer.** Needs: T2. `render()` restructured into three bordered panels and the footer, with the wave panel's two lines (counts, exceptions, status age; outcome strip in fives, key hints) in a new `keeler-top/src/panels.rs`; the message footer drawn from `App.message`. Scenarios: _the frame is three bordered panels and a footer; panel borders are neutral and titles carry the colour; the message footer shows the last lever's refusal; the wave title names the spec and the ref; a landed feature's board names HEAD as the ref; the first header line is the healthy counts and the status age; the first line's right half names what needs a human; with nothing to act on the right half is empty; the second line is an outcome strip, one glyph per task, in fives; the key hints sit at the right of the second line with bright keys; hints go before the strip is cut; a finished spec's header says so_. Tests: unit (strip and counts builders) + acceptance.
- [ ] **T5 — Selection, compact mode, scrolling.** Needs: T4. The marker in the row's colour and the selection background, `j`/`k` over tasks, automatic collapse when rows outgrow the panel, `z`, the scroll anchor (`app.rs` and the tasks panel in `panels.rs`). Scenarios: _the selected row carries a marker in its own colour and a lighter background; the marker takes the row's colour; j and k move over tasks, not lines; rows that outgrow the panel collapse to one line except the selected; z toggles compact by hand; a short panel scrolls to keep the selection visible_. Tests: unit (`on_key`, scroll arithmetic) + acceptance.
- [ ] **T6 — The detail pane and its new reads.** Needs: T4. `Row.verdict`, `Row.exit`, `Row.spawned_at` through `Dispatch` and the stream's first `init`; the fact block, the three ruled sections, the commit count, the done task's pane (new `keeler-top/src/detail.rs`). Scenarios: _the live pane opens with a fact block; a review record not yet written says so; a failed task's state line carries the exit code; the pane's sections are ruled and ordered agent, commits, last command; commits that do not fit end with a count; a done task's pane shows its record, its run and the next step; a done task whose run files are gone shows dashes for them_. Tests: unit + acceptance (stubs for the record and exit reads).
- [ ] **T7 — The bands applied.** Needs: T5, T6. `layout::bands()` for width and height, cumulative and drop-whole, driving `Columns`, the panels and the hints. Scenarios: _at 100 columns the title is drawn cut; below 93 columns the title goes and the row stays whole; below 81 columns the bar collapses to its percentage; below 72 columns MODEL and TOKENS go; a column that does not fit is dropped whole; the detail panel goes below 100 columns or 24 rows; the wave panel collapses to one line below 16 rows_. Tests: unit (band boundaries) + acceptance.

---

## Implementation Notes

**Theme.** `keeler-top/src/theme.rs` owns every colour and glyph and is
a value, not a global:

```rust
pub struct Theme { colour: bool, ascii: bool }
impl Theme {
    pub fn new(colour: bool, ascii: bool) -> Self
    pub fn from_env() -> Self            // NO_COLOR, KEELER_TOP_ASCII, LANG/LC_ALL
    pub fn look(&self, state: &str, finished: bool) -> Look   // by leading word; "?" for the rest
    pub fn bar(&self, percent: u8) -> (String, Style)         // 8 cells, round half up
    pub fn group(state: &str) -> u8                            // 0..=7
    pub fn border(&self) -> BorderSet
}
pub struct Look { pub glyph: &'static str, pub style: Style }
```

Constants: `TEXT`, `DIM`, `CHROME`, `ORANGE`, `GREEN`, `RED`, `YELLOW`,
`VIOLET`, `BLUE`, `CYAN`, `SELECTED_BG`, `BORDER` with the values in the
assumptions.

**Rows as lines.** `frame.rs` grows a second path beside `cells()`: `fn
lines(row: &Row, cols: &Columns, theme: &Theme, selected: bool,
compact: bool) -> Vec<Line>` producing one or two `Line`s of `Span`s.
`cells()` and `once()` do not change. `Columns` is computed once per
frame from the width and the widest state present; the band rules are
a function `fn bands(width: u16, height: u16, widest_state: u16, tasks:
usize) -> Layout` with its own unit tests.

**Order and scroll.** `board.rs`: `fn order(rows: &[Row]) -> Vec<usize>`
stable by `Theme::group`. `app.rs` keeps `selected: usize` over tasks
and `compact: Option<bool>` (`None` = automatic); the scroll offset is
derived per frame: below the window, the selected task's last line is
the panel's last row; above it, its first line is the first row — no
stored offset.

**New facts.** `Row` gains `title: Option<String>`, `verdict:
Option<String>`, `exit: Option<i32>`, `spawned_at: Option<Timestamp>`.
Titles come from `graph::titles(spec_text)`, so `graph::read` returns
the spec text alongside the states. Verdict and exit go through
`Dispatch` (a `git show <ref>:reviews/…` and a file read) so the
acceptance tests keep stubbing them.

**Message footer.** The `App.message` spec 10 already sets is drawn on
the footer line in yellow (refusals) or red (errors) and cleared on the
next key.

**Testing.** Rendering through `ratatui::backend::TestBackend`; a helper
`cell(buf, x, y) -> (String, Color, Color, Modifier)`; the theme is
constructed explicitly per test (`Theme::new(false, false)`), never from
the process environment. `z` is one new key in `on_key`; spec 10's key
scenarios are unaffected, and `z` still works in a finished view even
though its hint is not shown.

### Non-goals

- Styling `--once`; it is the plain-text surface for scripts and tests.
- Mouse support, panel-jump keys, configurable palettes or a light-terminal palette.
- A quota or tokens-per-minute panel like the reference's top row; the board shows a spec, not a machine.
- Collapsing done rows into one summary line; scrolling and compact mode cover a 30-task wave.
