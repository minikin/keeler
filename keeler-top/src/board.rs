//! One tick's board: a row per task, and the streams behind them.
//!
//! Three readings meet here and none of them is re-derived. `keeler-status`
//! says what a task's state is — eight specs of git queries decided that,
//! and a second answer would be a second truth. `scripts/keeler-graph.sh`
//! supplies the one word that vocabulary has not got: the recipe prints
//! `not spawned` both for a task nobody has got to and for one the graph is
//! holding, which are different answers to the question a watcher is
//! asking. And the run's own `.stream` says what the agent is doing right
//! now.
//!
//! What this module does is put the three in one row, and answer for the
//! cells where none of them has anything to say. That answer is the dash,
//! never a blank: a run that has not started and a board that failed to
//! read one must not look alike.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::clock::Timestamp;
use crate::git::{BranchFacts, branch_facts};
use crate::graph::{Graph, GraphLine, GraphState};
use crate::run::{DASH, RunView, fold};
use crate::status::{Status, StatusLine};
use crate::stream::StreamReader;
use crate::theme::Theme;

/// The recipe's word for a task nobody has spawned — and the one word of
/// its vocabulary the graph can improve on.
const NOT_SPAWNED: &str = "not spawned";

/// The recipe's word for a task whose session is up.
///
/// The one state the board's levers divide on: `p` and `Enter` need a
/// session, and `R` is for a task that has none.
pub const RUNNING: &str = "running";

/// The recipe's word for a task somebody stopped on purpose. Live, like
/// `running`, in the one sense the frame cares about: the row carries a
/// second line, because there is something to say under it.
pub const PAUSED: &str = "paused";

/// The recipe's word for a task that landed. A board of nothing else is a
/// finished view, which is a different board.
pub const DONE: &str = "done";

/// One task, as the board shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The task id, as the spec spells it.
    pub id: String,
    /// The state column: `keeler-status`'s word, or the graph's for the one
    /// it has none for.
    pub state: String,
    /// The run's log, as the report named it — the path every other file of
    /// the run is found beside.
    pub log: Option<PathBuf>,
    /// What the run's stream says, for a task that has one.
    pub run: Option<RunView>,
    /// What the task's branch and worktree say, while they are there.
    pub branch: Option<BranchFacts>,
    /// What the spec's Tasks section calls this task, for the rows that
    /// have room for it — and nothing for a task line that does not give
    /// one, which is a blank column rather than a board complaining about
    /// somebody's prose.
    pub title: Option<String>,
}

impl Row {
    /// Whether the run's session is up.
    #[must_use]
    pub fn running(&self) -> bool {
        self.state == RUNNING
    }

    /// Whether the row carries a second line: a run that is happening now,
    /// or one somebody stopped and can start again.
    ///
    /// Every other state is closed or waiting, and a connector under one of
    /// those would be a line saying nothing is happening.
    #[must_use]
    pub fn live(&self) -> bool {
        self.running() || self.state == PAUSED
    }

    /// The tmux session the run is in: `keeler-<slug>-<tid>`, the name
    /// `keeler-spawn` gave it, with the id lowercased on the way in as
    /// every name in graph mode is.
    #[must_use]
    pub fn session(&self, slug: &str) -> String {
        format!("keeler-{slug}-{}", self.id.to_lowercase())
    }

    /// The marker the board writes when it has stopped a run.
    ///
    /// Beside the log the report named, rather than under a run directory
    /// composed here: `keeler-status` prints the log it reads the marker
    /// beside, and the board is launched with the working directory it
    /// happened to be started in — which is not the repository root
    /// whenever somebody opens the board from a subdirectory.
    #[must_use]
    pub fn marker(&self) -> Option<PathBuf> {
        self.log.as_ref().map(|log| log.with_extension("paused"))
    }

    /// The stage column: how far the run has got.
    #[must_use]
    pub fn stage_column(&self) -> String {
        self.run
            .as_ref()
            .map_or_else(|| DASH.to_string(), |run| run.stage.to_string())
    }

    /// The tool column: the last call the main session made.
    #[must_use]
    pub fn tool_column(&self) -> String {
        self.run
            .as_ref()
            .map_or_else(|| DASH.to_string(), RunView::tool_column)
    }

    /// The elapsed column: how long that call has been running.
    ///
    /// Empty rather than dashed, as the run's own column is: a task with no
    /// run is not waiting on anything, which is the same thing every other
    /// empty elapsed column says.
    #[must_use]
    pub fn elapsed_column(&self, now: Timestamp) -> String {
        self.run
            .as_ref()
            .map_or_else(String::new, |run| run.elapsed_column(now))
    }

    /// The context column: the share of its window the run has used.
    #[must_use]
    pub fn context_column(&self) -> String {
        self.run
            .as_ref()
            .map_or_else(|| DASH.to_string(), RunView::context_column)
    }

    /// The tokens column: everything the run has written.
    #[must_use]
    pub fn tokens_column(&self) -> String {
        self.run
            .as_ref()
            .map_or_else(|| DASH.to_string(), RunView::tokens_column)
    }

    /// The model column: what the run was started with.
    #[must_use]
    pub fn model_column(&self) -> String {
        self.run
            .as_ref()
            .map_or_else(|| DASH.to_string(), RunView::model_column)
    }

    /// The commit column: the branch's head, how far ahead of the feature
    /// branch it is, and what is not committed yet.
    ///
    /// The dirty count appears only when there is one. A `0 dirty` on every
    /// clean row would spend the column's width saying nothing, and the
    /// number is read for exactly one decision — whether a branch is safe
    /// to remove — which a zero never enters into.
    #[must_use]
    pub fn commit_column(&self) -> String {
        let Some(branch) = &self.branch else {
            return DASH.to_string();
        };
        let dirty = if branch.dirty > 0 {
            format!("  {} dirty", branch.dirty)
        } else {
            String::new()
        };
        format!("{} +{}{dirty}", branch.head, branch.ahead)
    }
}

/// The state column: `keeler-status`'s word, except for the one it has not
/// got.
///
/// `not spawned` is the recipe's answer for a task nobody has started, and
/// the graph knows why: because its turn has not come, or because nobody
/// has taken it. Every other state the recipe prints is passed through
/// whole — `incomplete (no review record, box not ticked)` names the two
/// things still missing, and those are the words that say what to do.
#[must_use]
pub fn state_column(state: &str, graph: Option<&GraphLine>) -> String {
    if state != NOT_SPAWNED {
        return state.to_string();
    }
    match graph {
        Some(line) if line.state == GraphState::Blocked => {
            format!("blocked ← {}", line.needs.join(", "))
        }
        Some(line) if line.state == GraphState::Ready => "ready".to_string(),
        _ => state.to_string(),
    }
}

/// One task's stream and what has been folded from it.
#[derive(Debug)]
struct Run {
    reader: StreamReader,
    view: RunView,
}

/// The streams the board is reading, one per task, kept between ticks.
///
/// The readers are the state that has to survive a tick: each holds how far
/// into its file it has got, so a refresh reads the bytes that arrived and
/// none of the earlier ones. A board that built these afresh every second
/// would re-parse a megabyte a second, and the run this spec was written
/// against had streams that size.
#[derive(Debug, Default)]
pub struct Runs(HashMap<String, Run>);

impl Runs {
    /// Reads whatever has arrived in one task's stream, or nothing when
    /// there is no stream to read.
    ///
    /// Nothing is the honest answer for three tasks at once: one never
    /// spawned, one whose worktree `keeler-land` has removed, and one whose
    /// session ended before `tee` ever created the file. The board shows
    /// the state alone for all three.
    pub fn refresh(&mut self, task: &StatusLine) -> Option<RunView> {
        let log = task.log.as_ref()?;
        let path = log.with_extension("stream");
        if !path.exists() {
            return None;
        }
        let run = self.0.entry(task.id.clone()).or_insert_with(|| Run {
            reader: StreamReader::new(path),
            view: RunView::default(),
        });
        let batch = run.reader.poll();
        if batch.restarted {
            run.view = RunView::default();
        }
        // The prefix that tells this task's own work from a scratch file
        // somewhere else. A report line that carries a log but no worktree
        // must not fall back to the empty path: `strip_prefix("")` succeeds
        // on every absolute path there is, so every edit anywhere would
        // read as this task's tdd. The log's own path stands in — it names
        // a file, so no edit can be inside it, and the stage stays where
        // the rest of the stream puts it.
        let worktree = task.worktree.clone().unwrap_or_else(|| log.clone());
        for record in batch.records {
            fold(&mut run.view, record, &worktree);
        }
        // Not a stream signal: the runner writes the exit file after the
        // stream is closed, so the only place it can be seen is here.
        if log.with_extension("exit").exists() {
            run.view.ended();
        }
        Some(run.view.clone())
    }
}

/// The board as one tick left it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Board {
    /// The spec, as the report's header names it.
    pub rel: String,
    /// The ref that report answered from — `feat/<slug>`, or HEAD once a
    /// landed feature's branch is gone.
    pub git_ref: String,
    /// When the report was answered. The header shows its age rather than
    /// the answer alone: the status read runs on its own cadence, and a
    /// board whose slow read has stalled must not look like a live one.
    pub answered: Timestamp,
    /// One row per task, in the order the spec lists them.
    pub rows: Vec<Row>,
    /// Which row the detail pane is about.
    pub selected: usize,
    /// Whether the live rows are drawn on one line: what the watcher last
    /// asked of them with `z`, and `None` for the automatic answer — which
    /// only the frame can give, since it is the one that knows how tall the
    /// panel is.
    ///
    /// Here beside `selected` and for the same reason that one is here: the
    /// renderer is handed a board and nothing else, so a view decision it
    /// cannot read is a view decision it cannot draw.
    pub compact: Option<bool>,
    /// The line under the table: what the last keypress did, or why it did
    /// nothing.
    pub message: String,
}

impl Board {
    /// One tick's board, from the two reads and the streams.
    #[must_use]
    pub fn assemble(status: &Status, graph: &Graph, runs: &mut Runs, answered: Timestamp) -> Self {
        let rows = status
            .tasks
            .iter()
            .map(|task| Row {
                id: task.id.clone(),
                state: state_column(&task.state, graph.line(&task.id)),
                log: task.log.clone(),
                run: runs.refresh(task),
                title: graph.title(&task.id),
                // The distance is measured from the ref the report
                // answered about, so the commit column and the state column
                // are about one graph and not two.
                branch: task
                    .worktree
                    .as_deref()
                    .and_then(|worktree| branch_facts(worktree, &status.git_ref)),
            })
            .collect();
        Self {
            rel: status.rel.clone(),
            git_ref: status.git_ref.clone(),
            answered,
            rows,
            selected: 0,
            compact: None,
            message: String::new(),
        }
    }

    /// The spec's slug: its file name without `.md`.
    ///
    /// Every name in graph mode is derived from it — the branch, the
    /// worktree, the run directory and the tmux session — and this is the
    /// derivation `keeler-status` makes, from the path in the report's own
    /// header rather than from anything the board was launched with.
    #[must_use]
    pub fn slug(&self) -> &str {
        std::path::Path::new(&self.rel)
            .file_stem()
            .and_then(std::ffi::OsStr::to_str)
            .unwrap_or_default()
    }

    /// How old `keeler-status`'s answer is.
    ///
    /// Shown rather than the answer's own time: the slow read runs on its
    /// own cadence, and a board whose recipe has stalled must not look like
    /// a live one. Which spec and which ref the answer was about are the
    /// wave panel's title; this is the one thing about it that moves.
    #[must_use]
    pub fn age(&self, now: Timestamp) -> String {
        format!("status {}s ago", now.seconds_since(self.answered))
    }

    /// The row the detail pane is about, or nothing on a board with no
    /// tasks.
    #[must_use]
    pub fn selected_row(&self) -> Option<&Row> {
        self.rows.get(self.selected)
    }

    /// Whether every task has landed, which is a different board: the live
    /// columns have nothing left to say, so the row is the id, the word and
    /// what landed, and `done` stops being the background and becomes the
    /// answer.
    ///
    /// A board with no rows at all is not finished. A spec whose tasks are
    /// still to be written has nothing to have finished, and the header
    /// congratulating somebody on it would be the board's own arithmetic
    /// talking.
    #[must_use]
    pub fn finished(&self) -> bool {
        !self.rows.is_empty() && self.rows.iter().all(|row| row.state == DONE)
    }

    /// The rows in the order the board draws them, each with where the
    /// report put it.
    #[must_use]
    pub fn ordered(&self) -> Vec<(usize, &Row)> {
        ordered(&self.rows)
    }

    /// Where the selection lands when it moves one row down the board, or
    /// up it — as the board is drawn, and not as the report listed them.
    ///
    /// The two orders stopped being the same when the rows began leading
    /// with what needs a human: a selection that walked the report's would
    /// jump about the screen, and nothing on the board would say why. The
    /// answer is still a report index, because that is what the levers and
    /// the detail pane read.
    #[must_use]
    pub fn moved(&self, down: bool) -> usize {
        let order = self.ordered();
        let last = order.len().saturating_sub(1);
        let here = order
            .iter()
            .position(|(index, _)| *index == self.selected)
            .unwrap_or_default();
        let there = if down {
            here.saturating_add(1).min(last)
        } else {
            here.saturating_sub(1)
        };
        order.get(there).map_or(self.selected, |(index, _)| *index)
    }
}

/// The rows in the order a watcher asks for them — what needs a human
/// first, then what is running, then what is left — each paired with where
/// the report put it, which is what the selection is counted in.
///
/// The group is the state table's, and within a group the report's order
/// holds — which is the spec's order, and the one order the person reading
/// the board already has in their head. Stable rather than sorted by id:
/// `T10` sorts before `T2` as text and after it as a number, and the report
/// has already answered the question.
#[must_use]
pub fn ordered(rows: &[Row]) -> Vec<(usize, &Row)> {
    let mut ordered: Vec<(usize, &Row)> = rows.iter().enumerate().collect();
    ordered.sort_by_key(|(_, row)| Theme::group(&row.state));
    ordered
}

/// The same order, as the report's own indices — which is what a test about
/// the order itself reads.
#[must_use]
pub fn order(rows: &[Row]) -> Vec<usize> {
    ordered(rows).into_iter().map(|(index, _)| index).collect()
}

#[cfg(test)]
mod tests {
    use super::{Board, Row, Runs, state_column};
    use crate::clock::Timestamp;
    use crate::git::{BranchFacts, Commit};
    use crate::graph::{GraphLine, GraphState};
    use crate::run::RunView;
    use crate::status::{StatusLine, parse};
    use crate::theme::Theme;

    fn graph(state: GraphState, needs: &[&str]) -> GraphLine {
        GraphLine {
            id: "T3".to_string(),
            state,
            needs: needs.iter().map(|need| (*need).to_string()).collect(),
        }
    }

    fn branch(head: &str, ahead: usize, dirty: usize) -> BranchFacts {
        BranchFacts {
            head: head.to_string(),
            ahead,
            dirty,
            commits: vec![Commit {
                hash: head.to_string(),
                subject: "feat(01-foo): T1 green".to_string(),
            }],
        }
    }

    fn row(branch: Option<BranchFacts>, run: Option<RunView>) -> Row {
        Row {
            id: "T1".to_string(),
            state: "running".to_string(),
            log: Some(std::path::PathBuf::from("/r/.keeler/runs/01-foo/t1.log")),
            run,
            branch,
            title: None,
        }
    }

    #[test]
    fn only_not_spawned_is_the_graphs_to_answer() {
        // Every other word the recipe prints was decided by git queries the
        // board has no second opinion about.
        for state in [
            "running",
            "died",
            "paused",
            "passed",
            "done",
            "incomplete (no review record, box not ticked)",
            "failed (exit 1)",
        ] {
            assert_eq!(
                state_column(state, Some(&graph(GraphState::Blocked, &["T1"]))),
                state,
                "the graph overwrote {state}",
            );
        }
    }

    #[test]
    fn a_task_the_graph_has_nothing_to_say_about_keeps_the_recipes_word() {
        // A graph line missing altogether — the two reads are on different
        // cadences, so a spec that gained a task between them is exactly
        // this — and a task the graph calls done while the recipe calls it
        // unspawned, which is a tick landed since the last status read.
        assert_eq!(state_column("not spawned", None), "not spawned");
        assert_eq!(
            state_column("not spawned", Some(&graph(GraphState::Done, &[]))),
            "not spawned",
        );
    }

    #[test]
    fn a_blocked_task_names_everything_it_waits_on() {
        assert_eq!(
            state_column("not spawned", Some(&graph(GraphState::Blocked, &["T1"]))),
            "blocked ← T1",
        );
        assert_eq!(
            state_column(
                "not spawned",
                Some(&graph(GraphState::Blocked, &["T1", "T2", "T3"])),
            ),
            "blocked ← T1, T2, T3",
        );
        assert_eq!(
            state_column("not spawned", Some(&graph(GraphState::Ready, &[]))),
            "ready",
        );
    }

    #[test]
    fn the_commit_column_names_the_dirt_only_when_there_is_some() {
        assert_eq!(
            row(Some(branch("f5064b1", 3, 0)), None).commit_column(),
            "f5064b1 +3"
        );
        assert_eq!(
            row(Some(branch("f5064b1", 3, 2)), None).commit_column(),
            "f5064b1 +3  2 dirty",
        );
        assert_eq!(row(None, None).commit_column(), "—");
    }

    #[test]
    fn a_row_with_no_run_dashes_every_column_the_run_would_have_filled() {
        let row = row(None, None);

        assert_eq!(row.stage_column(), "—");
        assert_eq!(row.tool_column(), "—");
        assert_eq!(row.context_column(), "—");
        assert_eq!(row.tokens_column(), "—");
        assert_eq!(row.model_column(), "—");
        // The elapsed column is the one that stays empty: a row with no run
        // is not waiting on a tool, which is what every other empty elapsed
        // column already means.
        assert_eq!(row.elapsed_column(Timestamp::now()), "");
    }

    #[test]
    fn a_row_whose_tool_has_not_come_back_counts_from_when_it_was_called() {
        let row = row(
            None,
            Some(RunView {
                last_tool: Some(crate::run::ToolCall {
                    id: "toolu_1".to_string(),
                    name: "Bash".to_string(),
                    detail: "just dev".to_string(),
                    at: Some(Timestamp::from_epoch_seconds(1_000)),
                }),
                ..RunView::default()
            }),
        );

        assert_eq!(
            row.elapsed_column(Timestamp::from_epoch_seconds(1_134)),
            "02:14",
        );
        assert_eq!(row.tool_column(), "Bash: just dev");
    }

    #[test]
    fn a_row_with_a_run_answers_from_it() {
        let mut view = RunView {
            model: Some("claude-opus-5[1m]".to_string()),
            context_used: Some(500_000),
            ..RunView::default()
        };
        view.ended();
        let row = row(None, Some(view));

        assert_eq!(row.stage_column(), "ended");
        assert_eq!(row.context_column(), "50%");
        assert_eq!(row.model_column(), "opus5[1m]");
    }

    #[test]
    fn a_task_with_no_log_has_no_stream_to_look_for() {
        // `done` and `not spawned` carry no paths at all, so there is
        // nothing to open and nothing to report about not opening it.
        let task = StatusLine {
            id: "T1".to_string(),
            state: "done".to_string(),
            log: None,
            worktree: None,
        };

        assert_eq!(Runs::default().refresh(&task), None);
    }

    /// A run directory of its own, removed on drop, with the log path a
    /// report would name — the board finds the stream and the exit file
    /// from that path, so the fixture is built the same way round.
    struct Runfiles(std::path::PathBuf);

    impl Runfiles {
        fn new(name: &str) -> Self {
            let dir = crate::fixture_dir(name);
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn task(&self) -> StatusLine {
            StatusLine {
                id: "T1".to_string(),
                state: "running".to_string(),
                log: Some(self.0.join("t1.log")),
                worktree: Some(std::path::PathBuf::from("/w/repo-01-foo-t1")),
            }
        }

        fn write(&self, name: &str, body: &str) {
            std::fs::write(self.0.join(name), body).unwrap();
        }
    }

    impl Drop for Runfiles {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// One assistant record carrying one tool call, in the shape the CLI
    /// writes it.
    fn edit(path: &str) -> String {
        serde_json::json!({
            "type": "assistant",
            "parent_tool_use_id": null,
            "message": { "id": "m1", "content": [
                { "type": "tool_use", "id": "toolu_1", "name": "Edit", "input": { "file_path": path } },
            ]},
        })
        .to_string()
    }

    #[test]
    fn a_stream_the_runner_replaced_takes_the_view_with_it() {
        // Every resume truncates the file — the runner tees, it does not
        // append — and a board that folded the new run onto the old one
        // would show a stage the new run has not reached.
        let runfiles = Runfiles::new("replaced");
        let task = runfiles.task();
        runfiles.write(
            "t1.stream",
            &format!("{}\n", edit("/w/repo-01-foo-t1/src/x.rs")),
        );
        let mut runs = Runs::default();
        assert_eq!(
            runs.refresh(&task).expect("the stream is there").stage,
            crate::run::Stage::Tdd,
        );

        runfiles.write("t1.stream", "{\"type\":\"result\"}\n");

        assert_eq!(
            runs.refresh(&task).expect("the stream is there").stage,
            crate::run::Stage::Reading,
            "the run that ended is still in the view",
        );
    }

    #[test]
    fn the_exit_file_beside_the_log_ends_the_run() {
        // The runner writes it after the stream is closed, so it reaches
        // the view from here and from nowhere else.
        let runfiles = Runfiles::new("exit-file");
        let task = runfiles.task();
        runfiles.write(
            "t1.stream",
            &format!("{}\n", edit("/w/repo-01-foo-t1/src/x.rs")),
        );
        assert_eq!(
            Runs::default().refresh(&task).expect("a stream").stage,
            crate::run::Stage::Tdd,
        );

        runfiles.write("t1.exit", "1\n");

        assert_eq!(
            Runs::default().refresh(&task).expect("a stream").stage,
            crate::run::Stage::Ended,
        );
    }

    #[test]
    fn a_report_line_that_lost_its_worktree_reads_no_edit_as_this_tasks_work() {
        // The empty path is a prefix of every absolute path there is —
        // `strip_prefix("")` succeeds on all of them — so a board that fell
        // back to it would read a scratch file in /tmp as this task's tdd.
        let runfiles = Runfiles::new("no-worktree");
        let task = StatusLine {
            worktree: None,
            ..runfiles.task()
        };
        runfiles.write("t1.stream", &format!("{}\n", edit("/tmp/probe/src/lib.rs")));

        let view = Runs::default().refresh(&task).expect("the stream is there");

        assert_eq!(view.stage, crate::run::Stage::Reading);
        // And the rest of the stream is read as it always is.
        assert_eq!(view.tool_column(), "Edit: /tmp/probe/src/lib.rs");
    }

    #[test]
    fn a_rows_session_and_marker_are_the_names_every_other_recipe_composes() {
        // `keeler-spawn` named the session `keeler-<slug>-<tid>` and put the
        // log in `.keeler/runs/<slug>/`; `keeler-status` reads the marker
        // beside that log and `keeler-resume` removes it there. All three
        // are the same two derivations, and this row makes neither of them
        // up.
        let row = row(None, None);

        assert_eq!(row.session("01-foo"), "keeler-01-foo-t1");
        assert_eq!(
            row.marker(),
            Some(std::path::PathBuf::from("/r/.keeler/runs/01-foo/t1.paused")),
        );
        assert!(row.running());
    }

    #[test]
    fn a_task_whose_line_carries_no_paths_has_no_marker_to_write() {
        let row = Row {
            state: "done".to_string(),
            log: None,
            ..row(None, None)
        };

        assert_eq!(row.marker(), None);
        assert!(!row.running());
    }

    #[test]
    fn the_slug_is_the_specs_file_name_without_its_suffix() {
        let slug = |rel: &str| {
            Board::assemble(
                &parse(&format!("graph: {rel} on HEAD\n")).expect("a report"),
                &crate::graph::Graph::default(),
                &mut Runs::default(),
                Timestamp::default(),
            )
            .slug()
            .to_string()
        };

        assert_eq!(slug("specs/01-foo.md"), "01-foo");
        // Named from wherever the reader stood: `keeler-status` prints the
        // path relative to the repository root, and a board launched from a
        // subdirectory would otherwise compose a session name of its own.
        assert_eq!(slug("01-foo.md"), "01-foo");
        assert_eq!(slug("specs/01 on the road.md"), "01 on the road");
        assert_eq!(slug(""), "");
    }

    // ── 11-T2

    /// A board of nothing but states, in the report's order.
    fn rows_of(states: &[(&str, &str)]) -> Vec<Row> {
        states
            .iter()
            .map(|(id, state)| Row {
                id: (*id).to_string(),
                state: (*state).to_string(),
                ..row(None, None)
            })
            .collect()
    }

    /// The ids in the order the board draws them.
    fn ordered(rows: &[Row]) -> Vec<&str> {
        super::order(rows)
            .into_iter()
            .map(|index| rows[index].id.as_str())
            .collect()
    }

    #[test]
    fn rows_are_ordered_by_what_needs_a_human_first() {
        // Given one task in each of the ten states, in the report's order
        let rows = rows_of(&[
            ("T1", "done"),
            ("T2", "incomplete (no review record)"),
            ("T3", "running"),
            ("T4", "died"),
            ("T5", "ready"),
            ("T6", "passed"),
            ("T7", "blocked ← T6"),
            ("T8", "paused"),
            ("T9", "failed (exit 2)"),
            ("T10", "not spawned"),
        ]);

        // When the rows are ordered
        // Then they appear in the order T2, T4, T9, T8, T3, T6, T5, T7, T10, T1
        assert_eq!(
            ordered(&rows),
            ["T2", "T4", "T9", "T8", "T3", "T6", "T5", "T7", "T10", "T1"],
        );
    }

    #[test]
    fn within_a_group_the_reports_order_holds() {
        // Given the report lists T5 before T3 and both are running
        let rows = rows_of(&[("T5", "running"), ("T3", "running")]);

        // When the board renders
        // Then T5's row is above T3's — and the ids are not what decides
        // it: sorted as text, T10 would come before T2.
        assert_eq!(ordered(&rows), ["T5", "T3"]);
        assert_eq!(
            ordered(&rows_of(&[("T10", "running"), ("T2", "running")])),
            ["T10", "T2"],
        );
    }

    /// A board over a report of nothing but states.
    fn board_of(states: &[(&str, &str)]) -> Board {
        let report: String = std::iter::once("graph: s.md on HEAD\n".to_string())
            .chain(
                states
                    .iter()
                    .map(|(id, state)| format!("{id:<6} {state}\n")),
            )
            .collect();
        Board::assemble(
            &parse(&report).expect("a report"),
            &crate::graph::Graph::default(),
            &mut Runs::default(),
            Timestamp::default(),
        )
    }

    #[test]
    fn a_board_with_no_rows_has_nothing_to_order_and_has_not_finished() {
        assert_eq!(super::order(&[]), Vec::<usize>::new());
        // And a spec whose tasks are still to be written has not finished
        // anything: the finished view's congratulation would be the board's
        // own arithmetic talking.
        let board = board_of(&[]);
        assert!(!board.finished());
        assert!(board.ordered().is_empty());
    }

    #[test]
    fn a_finished_board_is_one_where_every_task_has_landed() {
        assert!(board_of(&[("T1", "done"), ("T2", "done")]).finished());
        assert!(!board_of(&[("T1", "done"), ("T2", "running")]).finished());
        // The word and nothing near it: a task nobody has spawned has not
        // landed, whatever group the two share.
        assert!(!board_of(&[("T1", "done"), ("T2", "not spawned")]).finished());
        // And the order the board draws them in is the rows themselves.
        assert_eq!(
            board_of(&[("T1", "done"), ("T2", "running")])
                .ordered()
                .into_iter()
                .map(|(_, row)| row.id.as_str())
                .collect::<Vec<_>>(),
            ["T2", "T1"],
        );
    }

    #[test]
    fn the_selection_moves_down_the_board_as_it_is_drawn() {
        // The rows are drawn by group and the report lists them in the
        // spec's order, so the two orders are different — and the one the
        // selection walks has to be the one the eye follows. Counted in the
        // report's indices, because that is what every lever reads.
        let board = board_of(&[("T1", "done"), ("T2", "running"), ("T3", "passed")]);
        assert_eq!(
            board
                .ordered()
                .into_iter()
                .map(|(_, row)| row.id.as_str())
                .collect::<Vec<_>>(),
            ["T2", "T3", "T1"],
        );

        // T1 is selected, and it is the last row on the board.
        assert_eq!(board.moved(true), 0, "the selection left the board");
        assert_eq!(board.moved(false), 2, "up from the last row is T3's");

        let middle = Board {
            selected: 2,
            ..board.clone()
        };
        assert_eq!(middle.moved(true), 0, "down from T3 is T1, the last row");
        assert_eq!(middle.moved(false), 1, "up from T3 is T2, the first");

        // And a board with no rows has nowhere to move to.
        assert_eq!(board_of(&[]).moved(true), 0);
    }

    #[test]
    fn a_row_carries_the_title_the_spec_gives_its_task_and_no_other() {
        let status = parse("graph: s.md on HEAD\nT1     done\nT2     done\n").expect("a report");
        let graph = crate::graph::Graph {
            lines: Vec::new(),
            titles: crate::graph::titles("- [x] **T1 — The theme.** x\n"),
        };

        let board = Board::assemble(&status, &graph, &mut Runs::default(), Timestamp::default());

        assert_eq!(board.rows[0].title.as_deref(), Some("The theme"));
        assert_eq!(board.rows[1].title, None, "a title nobody wrote was found");
    }

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig {
            failure_persistence: Some(Box::new(
                proptest::test_runner::FileFailurePersistence::WithSource("proptest-regressions"),
            )),
            ..proptest::prelude::ProptestConfig::default()
        })]

        /// Given any list of tasks with any states in any report order:
        /// every task in a lower-numbered group precedes every task in a
        /// higher one, within a group the report's order holds, and
        /// ordering twice gives the same result.
        #[test]
        fn any_set_of_tasks_orders_by_group_then_report_order(
            states in proptest::collection::vec(state(), 0..12),
        ) {
            let rows: Vec<Row> = states
                .iter()
                .enumerate()
                .map(|(index, state)| Row {
                    id: format!("T{index}"),
                    state: state.clone(),
                    ..row(None, None)
                })
                .collect();

            let order = super::order(&rows);

            // Every row, exactly once: a board that dropped one would be a
            // task nobody is watching.
            let mut seen = order.clone();
            seen.sort_unstable();
            proptest::prop_assert_eq!(seen, (0..rows.len()).collect::<Vec<_>>());
            for pair in order.windows(2) {
                let (first, next) = (&rows[pair[0]], &rows[pair[1]]);
                let (low, high) = (Theme::group(&first.state), Theme::group(&next.state));
                proptest::prop_assert!(low <= high);
                if low == high {
                    proptest::prop_assert!(pair[0] < pair[1], "the report's order was lost");
                }
            }
            // Ordering the ordered board again moves nothing.
            let again: Vec<Row> = order.iter().map(|index| rows[*index].clone()).collect();
            proptest::prop_assert_eq!(
                super::order(&again),
                (0..again.len()).collect::<Vec<_>>(),
            );
        }
    }

    /// A state as the board is shown one: the recipe's vocabulary, the
    /// graph's two words, and one the theme has no row for.
    fn state() -> impl proptest::prelude::Strategy<Value = String> {
        proptest::prelude::Strategy::prop_map(
            proptest::sample::select(vec![
                "failed (exit 2)",
                "died",
                "incomplete (no review record)",
                "paused",
                "running",
                "passed",
                "ready",
                "blocked ← T1",
                "not spawned",
                "done",
                "sulking",
            ]),
            str::to_string,
        )
    }

    #[test]
    fn the_board_names_the_spec_the_ref_and_the_age_of_the_answer() {
        let status = parse("graph: specs/01-foo.md on feat/01-foo\n").expect("a report");
        let answered = Timestamp::from_epoch_seconds(1_000);
        let board = Board::assemble(
            &status,
            &crate::graph::Graph::default(),
            &mut Runs::default(),
            answered,
        );

        assert_eq!(board.rel, "specs/01-foo.md");
        assert_eq!(board.git_ref, "feat/01-foo");
        assert_eq!(
            board.age(Timestamp::from_epoch_seconds(1_004)),
            "status 4s ago",
        );
        // Counted from the answer and not from the board's own clock: a
        // read that never came back must not read as one a second old.
        assert_eq!(board.age(answered), "status 0s ago");
        // A board with no tasks has no row to be about, and the pane asks
        // rather than indexing.
        assert_eq!(board.selected_row(), None);
    }
}
