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
    /// The line names a task the line at `first` already named. Both
    /// numbers are carried because the two lines need not read alike:
    /// `06-graph-mode/T1` and `06-graph-mode/t1` are one address, so a
    /// message quoting only the second sends a reader grepping for text
    /// the first line does not contain.
    Duplicate {
        line: usize,
        first: usize,
        text: String,
    },
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unparseable { line, text } => write!(
                out,
                "line {line} is not a backlog entry: `{text}` — expected <spec-slug>/<task-id>",
            ),
            Self::Duplicate { line, first, text } => {
                write!(out, "line {line} duplicates line {first}: `{text}`")
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
    let mut debts: Vec<(usize, TaskId)> = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let text = raw.trim();
        if text.is_empty() {
            continue;
        }
        let line = index + 1;
        let Some(task) = entry(text) else {
            return Err(Refusal::Unparseable {
                line,
                text: text.to_string(),
            });
        };
        if let Some((first, _)) = debts.iter().find(|(_, seen)| *seen == task) {
            return Err(Refusal::Duplicate {
                line,
                first: *first,
                text: text.to_string(),
            });
        }
        debts.push((line, task));
    }
    Ok(debts.into_iter().map(|(_, task)| task).collect())
}

/// One line as a task address, or nothing. Exactly one `/`, and each half
/// non-empty and spelled the way a slug and a task id are spelled: ASCII
/// letters, digits, `-` and `_`. Anything else is prose that wandered into
/// the debt list — a bullet's dash and space, a trailing comma, backticks
/// — and accepting it would mint an address no ticked task can equal, so
/// the gate would report a task missing that the file plainly lists.
fn entry(line: &str) -> Option<TaskId> {
    let (spec, task) = line.split_once('/')?;
    let half = |text: &str| {
        !text.is_empty()
            && text
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    };
    (half(spec) && half(task)).then(|| TaskId::new(spec, task))
}

#[cfg(test)]
mod tests {
    use super::super::decision::{Decision, Record, TaskId, Uncovered, Verdict, Why, decide};
    use super::{Refusal, entry, parse};

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
                first: 1,
                text: "06-graph-mode/t1".into(),
            },
        );
        let message = refusal.to_string();
        assert!(
            message.contains("line 3")
                && message.contains("line 1")
                && message.contains("06-graph-mode/t1"),
            "the refusal does not name both lines: {message}",
        );
    }

    #[test]
    fn a_duplicate_differing_only_in_case_names_the_line_it_duplicates() {
        // `T1` and `t1` are one address — the same rule that lets a
        // record at `t1.md` cover a checkbox that says `T1`. Two spellings
        // of one task are one debt line twice, not two debts.
        //
        // And this is why the refusal carries the earlier line's number
        // rather than trusting the reader to search for its text: the two
        // lines are the same address spelled differently, so grepping what
        // the message quotes finds only the second of them.
        let refusal = parse("06-graph-mode/T1\n06-graph-mode/t1\n").unwrap_err();
        assert_eq!(
            refusal,
            Refusal::Duplicate {
                line: 2,
                first: 1,
                text: "06-graph-mode/t1".into(),
            },
        );
    }

    #[test]
    fn a_line_that_is_no_task_address_is_refused_naming_the_line() {
        let text = "01-install/t1\n\nthirty-seven tasks, reviewed by area\n";
        let refusal = parse(text).unwrap_err();
        assert_eq!(
            refusal,
            Refusal::Unparseable {
                line: 3,
                text: "thirty-seven tasks, reviewed by area".into(),
            },
        );
        let message = refusal.to_string();
        assert!(
            message.contains("line 3") && message.contains("thirty-seven tasks, reviewed by area"),
            "the refusal does not name the line: {message}",
        );
    }

    #[test]
    fn an_address_wearing_prose_is_refused_rather_than_silently_accepted() {
        // A hand-edited debt list grows Markdown: a bullet, a trailing
        // comma, backticks, a sentence's full stop. Each of these holds a
        // `/` with non-empty halves, so a laxer rule would mint a task
        // address no ticked task can ever equal — and the gate would then
        // report `01-install/t1` missing while the file plainly lists it,
        // with nothing pointing at the line that lied.
        for bad in [
            "01-install/t1,",
            "01-install/t1.",
            "`01-install/t1`",
            "[01-install/t1]",
            "- 01-install/t1",
        ] {
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
            Decision::Missing(vec![Uncovered {
                task: task("03-wild", "t2"),
                why: Why::Unreviewed,
            }]),
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

    /// One line of a plausible backlog: an address, the same address in
    /// another case, or the junk a hand-edited file collects. Drawn from
    /// the same tiny universe as `any_task` so that repeats — which is
    /// what the duplicate refusal exists for — actually happen.
    fn any_line() -> impl Strategy<Value = String> {
        proptest::prop_oneof![
            any_task().prop_map(|task| task.to_string()),
            any_task().prop_map(|task| task.to_string().to_uppercase()),
            proptest::string::string_regex("[a-z0-9/ ,-]{0,12}").unwrap(),
        ]
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
        /// refusal — and each answer is checked against the file it came
        /// from: a parse smuggles no duplicate through, and a refusal
        /// names a line that exists, quotes what that line says, and — for
        /// a duplicate — points at the earlier line, which holds the same
        /// address. A refusal that named the wrong line would send the
        /// reader hunting, which is the whole failure this module exists
        /// to prevent.
        #[test]
        fn every_answer_is_checked_against_the_file_it_came_from(
            lines in proptest::collection::vec(any_line(), 0..8),
        ) {
            let file = lines.join("\n");
            let at = |line: usize| lines[line - 1].trim().to_string();
            match parse(&file) {
                Ok(tasks) => {
                    let distinct: std::collections::BTreeSet<&TaskId> = tasks.iter().collect();
                    proptest::prop_assert_eq!(distinct.len(), tasks.len());
                    let addresses = lines.iter().filter(|line| !line.trim().is_empty()).count();
                    proptest::prop_assert_eq!(tasks.len(), addresses);
                }
                Err(Refusal::Unparseable { line, text }) => {
                    proptest::prop_assert!((1..=lines.len()).contains(&line));
                    proptest::prop_assert_eq!(&at(line), &text);
                    proptest::prop_assert!(entry(&text).is_none());
                }
                Err(Refusal::Duplicate { line, first, text }) => {
                    proptest::prop_assert!((1..=lines.len()).contains(&line));
                    proptest::prop_assert!(first < line);
                    proptest::prop_assert_eq!(&at(line), &text);
                    proptest::prop_assert_eq!(entry(&at(first)), entry(&text));
                }
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
