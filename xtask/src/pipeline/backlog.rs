//! The backlog: explicit debt, refused when it lies.
//!
//! `reviews/BACKLOG.md` is the accepted-debt list — the `crap-baseline.json`
//! idiom: a committed reference the gate measures against, where every
//! addition is a reviewable diff and removing a line is the goal. One task
//! per line, spelled `<spec-slug>/<task-id>` like every other artifact.
//! A duplicate or unparseable line is a refusal naming the line, never a
//! panic — the parked gate's first blocker, now a scenario.

use super::decision::TaskId;

/// Why the backlog was refused. Each variant names the line — its number
/// and its text — because "the backlog is malformed" sends the reader
/// hunting and "line 12" sends them to line 12.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// The line is not `<spec-slug>/<task-id>`.
    Unparseable { line: usize, text: String },
    /// The line names a task an earlier line already named.
    Duplicate { line: usize, text: String },
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unparseable { line, text } => write!(
                out,
                "line {line} is not a backlog entry: `{text}` — expected <spec-slug>/<task-id>",
            ),
            Self::Duplicate { line, text } => {
                write!(out, "line {line} duplicates an earlier entry: `{text}`")
            }
        }
    }
}

impl std::error::Error for Refusal {}

/// Parses the accepted-debt list: one task per line, blank lines allowed,
/// anything else refused. Order is the file's, so what the gate accepts is
/// exactly what a reader of the diff saw added.
///
/// # Errors
///
/// Refuses the first line that is not a task address, and the first line
/// that names a task an earlier line already named — each refusal carrying
/// the line's number and text.
pub fn parse(text: &str) -> Result<Vec<TaskId>, Refusal> {
    let mut debts: Vec<TaskId> = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let text = raw.trim();
        if text.is_empty() {
            continue;
        }
        let Some(task) = entry(text) else {
            return Err(Refusal::Unparseable {
                line: index + 1,
                text: text.to_string(),
            });
        };
        if debts.contains(&task) {
            return Err(Refusal::Duplicate {
                line: index + 1,
                text: text.to_string(),
            });
        }
        debts.push(task);
    }
    Ok(debts)
}

/// One line as a task address, or nothing. Exactly one `/`, both halves
/// non-empty and free of whitespace — slugs and task ids never hold any,
/// so whitespace means prose that wandered into the debt list.
fn entry(line: &str) -> Option<TaskId> {
    let (spec, task) = line.split_once('/')?;
    let half =
        |text: &str| !text.is_empty() && !text.contains(|c: char| c == '/' || c.is_whitespace());
    (half(spec) && half(task)).then(|| TaskId::new(spec, task))
}

#[cfg(test)]
mod tests {
    use super::super::decision::{Decision, Record, TaskId, Verdict, decide};
    use super::{Refusal, parse};

    fn task(spec: &str, id: &str) -> TaskId {
        TaskId::new(spec, id)
    }

    #[test]
    fn the_backlog_parses_one_task_per_line_in_file_order() {
        let text = "01-install/t1\n\n02-release/t3\n01-install/t2\n";
        assert_eq!(
            parse(text).unwrap(),
            vec![
                task("01-install", "t1"),
                task("02-release", "t3"),
                task("01-install", "t2"),
            ],
        );
    }

    #[test]
    fn an_empty_backlog_is_no_debt_at_all() {
        assert_eq!(parse("").unwrap(), vec![]);
        assert_eq!(parse("\n\n").unwrap(), vec![]);
    }

    #[test]
    fn a_duplicate_backlog_line_is_refused_naming_the_line() {
        // Given a backlog holding the same task twice
        let text = "06-graph-mode/t1\n06-graph-mode/t2\n06-graph-mode/t1\n";
        // When the gate runs (the parse is where the refusal lives)
        let refusal = parse(text).unwrap_err();
        // Then it fails, naming the duplicated line — it does not panic
        assert_eq!(
            refusal,
            Refusal::Duplicate {
                line: 3,
                text: "06-graph-mode/t1".into(),
            },
        );
        let message = refusal.to_string();
        assert!(
            message.contains("line 3") && message.contains("06-graph-mode/t1"),
            "the refusal does not name the line: {message}",
        );
    }

    #[test]
    fn a_duplicate_differing_only_in_case_is_still_a_duplicate() {
        // `T1` and `t1` are one address — the same rule that lets a
        // record at `t1.md` cover a checkbox that says `T1`. Two spellings
        // of one task are one debt line twice, not two debts.
        let refusal = parse("06-graph-mode/T1\n06-graph-mode/t1\n").unwrap_err();
        assert_eq!(
            refusal,
            Refusal::Duplicate {
                line: 2,
                text: "06-graph-mode/t1".into(),
            },
        );
    }

    #[test]
    fn a_line_that_is_no_task_address_is_refused_naming_the_line() {
        let text = "01-install/t1\n\nthirty-seven tasks, reviewed by area\n";
        assert_eq!(
            parse(text).unwrap_err(),
            Refusal::Unparseable {
                line: 3,
                text: "thirty-seven tasks, reviewed by area".into(),
            },
        );
    }

    #[test]
    fn a_half_empty_or_overfull_address_is_refused() {
        // Each of these has a `/` and still names no task: an empty half,
        // a second slash, whitespace where a slug never has any.
        for bad in ["/t1", "01-install/", "01-install/t1/extra", "01 install/t1"] {
            assert_eq!(
                parse(bad).unwrap_err(),
                Refusal::Unparseable {
                    line: 1,
                    text: bad.into(),
                },
                "`{bad}` should not parse as a task address",
            );
        }
    }

    #[test]
    fn accepted_debt_is_explicit_and_adding_to_it_is_a_diff() {
        // Given tasks ticked before this gate existed, listed in the
        // committed backlog
        let backlog = parse("01-install/t1\n02-release/t3\n").unwrap();
        let listed = [task("01-install", "t1"), task("02-release", "t3")];
        // When the gate runs
        // Then it accepts exactly those and no others
        assert_eq!(
            decide(&listed, &[], &backlog),
            Decision::AllAccounted { ticked: 2 },
        );
        // And a ticked task on neither the backlog nor the records is
        // refused
        let ticked = [
            task("01-install", "t1"),
            task("02-release", "t3"),
            task("03-wild", "t2"),
        ];
        assert_eq!(
            decide(&ticked, &[], &backlog),
            Decision::Missing(vec![task("03-wild", "t2")]),
        );
    }

    use proptest::prelude::Strategy;

    /// The same small universe the decision's tests use, and for the same
    /// reason: ticks, records and backlog lines must collide often enough
    /// to exercise every combination.
    fn any_task() -> impl Strategy<Value = TaskId> {
        (0..2usize, 0..3usize).prop_map(|(spec, task)| {
            TaskId::new(["01-install", "02-release"][spec], ["t1", "t2", "t3"][task])
        })
    }

    fn any_record() -> impl Strategy<Value = Record> {
        (any_task(), proptest::bool::ANY).prop_map(|(task, passed)| Record {
            task,
            verdict: if passed { Verdict::Pass } else { Verdict::Fail },
        })
    }

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig {
            cases: 512,
            failure_persistence: Some(Box::new(
                proptest::test_runner::FileFailurePersistence::WithSource("proptest-regressions"),
            )),
            ..proptest::prelude::ProptestConfig::default()
        })]

        /// What lands in the committed file is exactly what the gate will
        /// accept: rendering distinct tasks one per line and parsing them
        /// back is the identity, order and all.
        #[test]
        fn a_committed_backlog_round_trips_through_the_parser(
            tasks in proptest::collection::btree_set(any_task(), 0..6),
        ) {
            let tasks: Vec<TaskId> = tasks.into_iter().collect();
            let text = tasks.iter().fold(String::new(), |mut file, task| {
                use std::fmt::Write;
                let _ = writeln!(file, "{task}");
                file
            });
            proptest::prop_assert_eq!(parse(&text), Ok(tasks));
        }

        /// The parked gate's first blocker: it panicked on a duplicate
        /// line. Whatever the file holds, the answer is a parse or a
        /// refusal — and a parse never smuggles a duplicate through.
        #[test]
        fn any_input_is_parsed_or_refused_never_panicked_on(
            text in proptest::string::string_regex(".{0,80}").unwrap(),
        ) {
            if let Ok(tasks) = parse(&text) {
                let distinct: std::collections::BTreeSet<&TaskId> = tasks.iter().collect();
                proptest::prop_assert_eq!(distinct.len(), tasks.len());
            }
        }

        /// Debt monotonicity: removing a backlog line never turns a
        /// failure into a pass — working debt off can only tighten the
        /// gate, so no removal needs re-arguing the whole file.
        #[test]
        fn removing_a_backlog_line_never_turns_a_failure_into_a_pass(
            ticked in proptest::collection::vec(any_task(), 0..6),
            records in proptest::collection::vec(any_record(), 0..6),
            backlog in proptest::collection::vec(any_task(), 1..6),
            removed in 0..6usize,
        ) {
            let mut shrunk = backlog.clone();
            shrunk.remove(removed % backlog.len());
            if let Decision::Missing(missing) = decide(&ticked, &records, &backlog) {
                let after = decide(&ticked, &records, &shrunk);
                proptest::prop_assert!(
                    matches!(after, Decision::Missing(_)),
                    "removing a line pardoned {missing:?}: {after:?}",
                );
            }
        }
    }
}
