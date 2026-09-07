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

use crate::clock::Timestamp;
use crate::git::{BranchFacts, branch_facts};
use crate::graph::{GraphLine, GraphState};
use crate::run::{DASH, RunView, fold};
use crate::status::{Status, StatusLine};
use crate::stream::StreamReader;

/// The recipe's word for a task nobody has spawned — and the one word of
/// its vocabulary the graph can improve on.
const NOT_SPAWNED: &str = "not spawned";

/// One task, as the board shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The task id, as the spec spells it.
    pub id: String,
    /// The state column: `keeler-status`'s word, or the graph's for the one
    /// it has none for.
    pub state: String,
    /// What the run's stream says, for a task that has one.
    pub run: Option<RunView>,
    /// What the task's branch and worktree say, while they are there.
    pub branch: Option<BranchFacts>,
}

impl Row {
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
    /// The line under the table: what the last keypress did, or why it did
    /// nothing.
    pub message: String,
}

impl Board {
    /// One tick's board, from the two reads and the streams.
    #[must_use]
    pub fn assemble(
        status: &Status,
        graph: &[GraphLine],
        runs: &mut Runs,
        answered: Timestamp,
    ) -> Self {
        let rows = status
            .tasks
            .iter()
            .map(|task| Row {
                id: task.id.clone(),
                state: state_column(&task.state, graph.iter().find(|line| line.id == task.id)),
                run: runs.refresh(task),
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
            message: String::new(),
        }
    }

    /// The header line: which spec, read from which ref, and how long ago.
    #[must_use]
    pub fn header(&self, now: Timestamp) -> String {
        format!(
            "{} on {}   status {}s ago",
            self.rel,
            self.git_ref,
            now.seconds_since(self.answered)
        )
    }

    /// The row the detail pane is about, or nothing on a board with no
    /// tasks.
    #[must_use]
    pub fn selected_row(&self) -> Option<&Row> {
        self.rows.get(self.selected)
    }
}

#[cfg(test)]
mod tests {
    use super::{Board, Row, Runs, state_column};
    use crate::clock::Timestamp;
    use crate::git::{BranchFacts, Commit};
    use crate::graph::{GraphLine, GraphState};
    use crate::run::RunView;
    use crate::status::{StatusLine, parse};

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
            run,
            branch,
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
            let dir =
                std::env::temp_dir().join(format!("keeler-top-unit-{name}-{}", std::process::id()));
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
    fn the_header_names_the_spec_the_ref_and_the_age_of_the_answer() {
        let status = parse("graph: specs/01-foo.md on feat/01-foo\n").expect("a report");
        let answered = Timestamp::from_epoch_seconds(1_000);
        let board = Board::assemble(&status, &[], &mut Runs::default(), answered);

        assert_eq!(
            board.header(Timestamp::from_epoch_seconds(1_004)),
            "specs/01-foo.md on feat/01-foo   status 4s ago",
        );
        // A board with no tasks has no row to be about, and the pane asks
        // rather than indexing.
        assert_eq!(board.selected_row(), None);
    }
}
