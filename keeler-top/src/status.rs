//! Reading what `keeler-status` printed.
//!
//! The board does not decide what running, died, passed or incomplete
//! mean. Eight specs of git queries against the task branches decided
//! that, inside the recipe, and a second implementation here would give
//! the project two answers that could disagree — about the one question
//! graph mode exists to answer. So the recipe runs and this reads its
//! report.
//!
//! The report is `printf`'s, and three things about that shape matter:
//!
//! ```text
//! graph: specs/01-foo.md on feat/01-foo
//! T1     running          log /r/.keeler/runs/01-foo/t1.log  worktree /r-01-foo-t1
//!        resume with: keeler keeler-resume specs/01-foo.md T1
//! T2     not spawned
//! T3     done
//! ```
//!
//! A task line begins at the left margin and every hint under one is
//! indented, so the hints are skipped by that indentation rather than by
//! matching what they say — a hint added tomorrow is skipped too. `done`
//! and `not spawned` carry no paths at all. And the state is padded to a
//! column but not bounded by it: `incomplete (no review record, box not
//! ticked)` runs well past, so the state ends where the ` log ` marker
//! begins and not at any width.

use std::path::PathBuf;

/// The marker between a task's state and its log path.
const LOG: &str = " log ";
/// The marker between the log path and the worktree path. Two spaces: the
/// recipe's own separator, and what keeps a log path with a space in it
/// from ending the field early.
const WORKTREE: &str = "  worktree ";

/// One task's line of the report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusLine {
    /// The task id, as the spec spells it — `T1`, not `t1`.
    pub id: String,
    /// The recipe's word for what the task is doing, whole: `running`,
    /// `died`, `paused`, `passed`, `done`, `not spawned`, and the two that
    /// carry a reason with them — `incomplete (no review record)` and
    /// `failed (exit 1)`.
    pub state: String,
    /// The run's log, for the states that have one.
    pub log: Option<PathBuf>,
    /// The worktree the run is in, for the states that have one.
    pub worktree: Option<PathBuf>,
}

/// The whole report: the header the recipe opens with, and a line per task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    /// The spec, as a path relative to the repository root.
    pub rel: String,
    /// The ref the recipe read the graph from — `feat/<slug>`, or `HEAD`
    /// once a landed feature's branch is gone. The board reads its own
    /// graph from this same ref, so the two answer about one graph.
    pub git_ref: String,
    /// One line per task, in the order the spec lists them.
    pub tasks: Vec<StatusLine>,
}

/// Reads a report, or nothing if this is not one.
///
/// Nothing rather than an empty report: the recipe prints its header
/// before anything can go wrong, so a stdout without one is not a board
/// with no tasks — it is some other command's output, or a `just` that
/// never reached the recipe, and the board must not draw a graph of
/// nothing over the answer it already had.
#[must_use]
pub fn parse(report: &str) -> Option<Status> {
    let mut lines = report.lines();
    let header = lines.by_ref().find_map(header_of)?;
    let (rel, git_ref) = header;
    let tasks = lines
        .filter(|line| !line.starts_with(char::is_whitespace))
        .filter_map(task_of)
        .collect();
    Some(Status {
        rel,
        git_ref,
        tasks,
    })
}

/// `graph: <rel> on <ref>` into its two halves.
///
/// Split from the right: a ref is one word, while a spec's path is
/// whatever the file is called and may hold spaces.
fn header_of(line: &str) -> Option<(String, String)> {
    let rest = line.strip_prefix("graph: ")?;
    let (rel, git_ref) = rest.rsplit_once(" on ")?;
    Some((rel.to_string(), git_ref.to_string()))
}

/// One task line into its fields, or nothing if the line is not one.
fn task_of(line: &str) -> Option<StatusLine> {
    let (id, rest) = line.split_once(' ')?;
    let rest = rest.trim_start();
    let Some((state, paths)) = rest.split_once(LOG) else {
        return Some(StatusLine {
            id: id.to_string(),
            state: rest.trim_end().to_string(),
            log: None,
            worktree: None,
        });
    };
    let (log, worktree) = match paths.split_once(WORKTREE) {
        Some((log, worktree)) => (log, Some(PathBuf::from(worktree))),
        None => (paths, None),
    };
    Some(StatusLine {
        id: id.to_string(),
        state: state.trim_end().to_string(),
        log: Some(PathBuf::from(log)),
        worktree,
    })
}

#[cfg(test)]
mod tests {
    use super::{Status, StatusLine, parse};
    use std::path::PathBuf;

    /// The board's own repository, reported the way the recipe reports it —
    /// every state that has a shape of its own, and every hint that follows
    /// one.
    const REPORT: &str = "\
graph: specs/01-foo.md on feat/01-foo
T1     running          log /r/.keeler/runs/01-foo/t1.log  worktree /r-01-foo-t1
T2     died             log /r/.keeler/runs/01-foo/t2.log  worktree /r-01-foo-t2
       resume with: keeler keeler-resume specs/01-foo.md T2
T3     passed           log /r/.keeler/runs/01-foo/t3.log  worktree /r-01-foo-t3
T4     incomplete (no review record, box not ticked) log /r/.keeler/runs/01-foo/t4.log  worktree /r-01-foo-t4
T5     failed (exit 1)  log /r/.keeler/runs/01-foo/t5.log  worktree /r-01-foo-t5
       verdict from an earlier run? rm /r/.keeler/runs/01-foo/t5.exit to make it resumable
T6     done
T7     not spawned
";

    fn states(report: &str) -> Vec<String> {
        parse(report)
            .expect("the fixture opens with a header")
            .tasks
            .into_iter()
            .map(|task| format!("{} {}", task.id, task.state))
            .collect()
    }

    #[test]
    fn the_header_names_the_spec_and_the_ref_it_was_read_from() {
        let status = parse(REPORT).expect("the fixture opens with a header");

        assert_eq!(status.rel, "specs/01-foo.md");
        assert_eq!(status.git_ref, "feat/01-foo");
    }

    #[test]
    fn a_landed_features_report_names_head() {
        let status = parse("graph: specs/01-foo.md on HEAD\nT1     done\n").expect("a report");

        assert_eq!(status.git_ref, "HEAD");
    }

    #[test]
    fn a_spec_whose_name_holds_a_space_keeps_it() {
        let status = parse("graph: specs/01 on the road.md on feat/01\n").expect("a report");

        assert_eq!(status.rel, "specs/01 on the road.md");
        assert_eq!(status.git_ref, "feat/01");
    }

    #[test]
    fn every_state_is_kept_whole_and_the_hints_between_them_are_skipped() {
        assert_eq!(
            states(REPORT),
            [
                "T1 running",
                "T2 died",
                "T3 passed",
                "T4 incomplete (no review record, box not ticked)",
                "T5 failed (exit 1)",
                "T6 done",
                "T7 not spawned",
            ],
        );
    }

    #[test]
    fn a_state_that_carries_paths_carries_both_of_them() {
        let running = parse(REPORT).expect("a report").tasks.remove(0);

        assert_eq!(running.id, "T1");
        assert_eq!(
            running.log,
            Some(PathBuf::from("/r/.keeler/runs/01-foo/t1.log")),
        );
        assert_eq!(running.worktree, Some(PathBuf::from("/r-01-foo-t1")));
    }

    #[test]
    fn done_and_not_spawned_carry_no_paths() {
        let tasks = parse(REPORT).expect("a report").tasks;

        for task in tasks
            .iter()
            .filter(|task| task.id == "T6" || task.id == "T7")
        {
            assert_eq!(task.log, None, "{} was given a log it has not got", task.id);
            assert_eq!(task.worktree, None, "{} was given a worktree", task.id);
        }
    }

    #[test]
    fn a_stdout_with_no_header_is_not_a_report() {
        assert_eq!(parse(""), None);
        assert_eq!(parse("error: Justfile does not contain recipe\n"), None);
        assert_eq!(parse("graph: specs/01-foo.md\n"), None);
    }

    #[test]
    fn a_line_that_is_not_a_task_line_is_no_task() {
        // The report's own trailing blank lines, and a word on a line of
        // its own — neither is a task, and neither is worth an error.
        let status =
            parse("graph: specs/01-foo.md on HEAD\n\nT1     done\nnothing\n").expect("a report");

        assert_eq!(
            status
                .tasks
                .iter()
                .map(|task| task.id.as_str())
                .collect::<Vec<_>>(),
            ["T1"],
        );
    }

    #[test]
    fn a_line_that_lost_its_worktree_still_gives_its_log() {
        let status = parse("graph: s.md on HEAD\nT1     died             log /r/t1.log\n")
            .expect("a report");

        assert_eq!(
            status.tasks,
            [StatusLine {
                id: "T1".to_string(),
                state: "died".to_string(),
                log: Some(PathBuf::from("/r/t1.log")),
                worktree: None,
            }],
        );
    }

    #[test]
    fn an_empty_graph_is_a_report_with_no_tasks() {
        assert_eq!(
            parse("graph: specs/01-foo.md on feat/01-foo\n"),
            Some(Status {
                rel: "specs/01-foo.md".to_string(),
                git_ref: "feat/01-foo".to_string(),
                tasks: Vec::new(),
            }),
        );
    }

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig {
            failure_persistence: Some(Box::new(
                proptest::test_runner::FileFailurePersistence::WithSource("proptest-regressions"),
            )),
            ..proptest::prelude::ProptestConfig::default()
        })]

        /// Whatever the recipe's `printf` can produce, the parser reads
        /// back — the two are one format, and this is the only place they
        /// are checked against each other rather than against an example.
        /// Paths are generated with spaces in them: the markers, not the
        /// columns, are what tells the fields apart, and a repository
        /// under `~/My Projects` is nobody's mistake.
        #[test]
        fn a_line_the_recipe_printed_reads_back_as_it_was_written(
            id in "T[0-9]{1,3}",
            state in recipe_state(),
            log in path(),
            worktree in path(),
        ) {
            let printed = format!("{id:<6} {state:<16} log {log}  worktree {worktree}");

            proptest::prop_assert_eq!(
                super::task_of(&printed),
                Some(StatusLine {
                    id,
                    state,
                    log: Some(PathBuf::from(log)),
                    worktree: Some(PathBuf::from(worktree)),
                }),
            );
        }

        /// And the shorter form, which is the same `printf` with two
        /// fields fewer.
        #[test]
        fn a_path_less_line_reads_back_as_it_was_written(
            id in "T[0-9]{1,3}",
            state in "(done|not spawned)",
        ) {
            let printed = format!("{id:<6} {state}");

            proptest::prop_assert_eq!(
                super::task_of(&printed),
                Some(StatusLine { id, state, log: None, worktree: None }),
            );
        }
    }

    /// Every word the recipe has for a task that carries paths, and the
    /// two that carry a reason inside them. Drawn from the recipe rather
    /// than from an alphabet: a state is not free text, and a generator
    /// that invented one holding ` log ` would be asking the format to
    /// express something it cannot.
    fn recipe_state() -> impl proptest::prelude::Strategy<Value = String> {
        use proptest::prelude::{Just, prop_oneof};
        prop_oneof![
            Just("running".to_string()),
            Just("died".to_string()),
            Just("paused".to_string()),
            Just("passed".to_string()),
            "failed \\(exit [1-9][0-9]?\\)",
            "incomplete \\((no review record|box not ticked|no review record, box not ticked)\\)",
        ]
    }

    /// A path with room for a space in it, and no room for the two-space
    /// separator that ends the log field.
    fn path() -> impl proptest::prelude::Strategy<Value = String> {
        "/[a-z]{1,5}( [a-z]{1,5})?/[a-z]{1,5}"
    }
}
