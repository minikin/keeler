//! The record grammar: four headers, a verdict, or a refusal naming the file.
//!
//! A record is what `/keeler:review` writes at `reviews/<slug>/<task>.md`:
//! `Spec:`, `Task:`, `Commit:` and `Verdict:` on its first four lines, then
//! the findings. The gate validates shape and verdict and nothing else —
//! `Commit:` must be present and non-empty but is never resolved, because
//! ancestry belongs to the pull-request check and this gate consults no
//! history at all. A record the grammar cannot read is refused, never
//! guessed at: a misread record could vouch for the wrong task.

use super::decision::{Record, TaskId, Verdict};

/// Why a record was refused, phrased for the person who has to open the
/// file: which file, and which line it lacks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    file: String,
    complaint: String,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(out, "{}: {}", self.file, self.complaint)
    }
}

impl std::error::Error for Refusal {}

/// Reads one review record. `file` is only a name for the refusal to carry —
/// the content has already been read, so the parser stays pure.
///
/// # Errors
///
/// Refuses a record that lacks one of its four header lines, one whose
/// header names nothing, and a verdict that is neither `pass` nor `fail` —
/// naming the file and the line each time.
pub fn parse(file: &str, content: &str) -> Result<Record, Refusal> {
    record(content).map_err(|complaint| Refusal {
        file: file.to_string(),
        complaint,
    })
}

/// The grammar itself, with the file name out of the way: four headers in
/// `/keeler:review`'s order, then findings the gate never reads.
fn record(content: &str) -> Result<Record, String> {
    let mut lines = content.lines();
    let spec = header(lines.next(), 1, "Spec")?;
    let task = header(lines.next(), 2, "Task")?;
    // Present and non-empty, never resolved: ancestry belongs to the
    // pull-request check, and this gate consults no history at all.
    header(lines.next(), 3, "Commit")?;
    let verdict = match header(lines.next(), 4, "Verdict")? {
        "pass" => Verdict::Pass,
        "fail" => Verdict::Fail,
        other => {
            return Err(format!(
                "`Verdict:` must be `pass` or `fail`, not `{other}`"
            ));
        }
    };
    Ok(Record {
        task: TaskId::new(spec, task),
        verdict,
    })
}

/// One header line: `<name>: <value>`, the value non-empty once trimmed.
fn header<'line>(
    line: Option<&'line str>,
    number: usize,
    name: &str,
) -> Result<&'line str, String> {
    let Some(line) = line else {
        return Err(format!(
            "the record ends before its `{name}:` header (line {number})"
        ));
    };
    let Some(value) = line
        .strip_prefix(name)
        .and_then(|rest| rest.strip_prefix(':'))
    else {
        return Err(format!(
            "line {number} should be the `{name}:` header, not `{line}`"
        ));
    };
    let value = value.trim();
    if value.is_empty() {
        return Err(format!(
            "the `{name}:` header (line {number}) names nothing"
        ));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::super::decision::{Decision, Record, TaskId, Verdict, decide};
    use super::parse;

    /// A record as `/keeler:review` writes one: four headers, a blank
    /// line, then the findings.
    fn well_formed(spec: &str, task: &str, commit: &str, verdict: &str) -> String {
        format!(
            "Spec: {spec}\nTask: {task}\nCommit: {commit}\nVerdict: {verdict}\n\n\
             ## Findings\n\nnone\n"
        )
    }

    #[test]
    fn a_malformed_record_is_refused_not_misread() {
        // Given a review record missing one of its four header lines
        let full = well_formed("06-graph-mode", "t1", "f0b1548", "pass");
        for name in ["Spec", "Task", "Commit", "Verdict"] {
            let missing_one = full
                .lines()
                .filter(|line| !line.starts_with(name))
                .collect::<Vec<_>>()
                .join("\n");
            // When the gate parses it
            let Err(refusal) = parse("reviews/06-graph-mode/t1.md", &missing_one) else {
                panic!("a record missing `{name}:` was accepted");
            };
            // Then it fails, naming the file and the line it lacks
            let message = refusal.to_string();
            assert!(
                message.contains("reviews/06-graph-mode/t1.md"),
                "the refusal does not name the file: {message}",
            );
            assert!(
                message.contains(&format!("`{name}:`")),
                "the refusal does not name the `{name}:` line it lacks: {message}",
            );
        }
    }

    #[test]
    fn a_record_that_ends_early_is_refused_at_the_header_it_lacks() {
        // A missing line and a file that simply stops are different reads
        // of the same lack; both must name the header that never came.
        let full = well_formed("06-graph-mode", "t1", "f0b1548", "pass");
        for (kept, name) in ["Spec", "Task", "Commit", "Verdict"]
            .into_iter()
            .enumerate()
        {
            let truncated = full.lines().take(kept).collect::<Vec<_>>().join("\n");
            let Err(refusal) = parse("reviews/06-graph-mode/t1.md", &truncated) else {
                panic!("a record cut off before `{name}:` was accepted");
            };
            let message = refusal.to_string();
            assert!(
                message.contains(&format!("`{name}:`")),
                "the refusal does not name the `{name}:` line it lacks: {message}",
            );
        }
    }

    #[test]
    fn a_fail_verdict_contradicts_a_ticked_box() {
        // Given a ticked task whose review record says `Verdict: fail` —
        // and a backlog line for it too, since debt cannot shelter a
        // review that concluded the work is bad
        let record = parse(
            "reviews/03-wild/t2.md",
            &well_formed("03-wild", "t2", "abc1234", "fail"),
        )
        .expect("a well-formed fail record was refused");
        let ticked = [TaskId::new("03-wild", "t2")];
        let backlog = [TaskId::new("03-wild", "t2")];
        // When the gate runs
        let decision = decide(&ticked, &[record], &backlog);
        // Then it fails, naming the record and the contradiction: the
        // missing task's address is the record's address.
        assert_eq!(
            decision,
            Decision::Missing(vec![TaskId::new("03-wild", "t2")]),
        );
    }

    #[test]
    fn a_well_formed_record_names_its_task_and_verdict() {
        // The spec's checkbox says `T2`; the record's `Task:` header may
        // spell it either way and still be the same address.
        let record = parse(
            "reviews/08-pipeline/t2.md",
            &well_formed("08-pipeline", "T2", "f0b1548", "pass"),
        )
        .expect("a well-formed record was refused");
        assert_eq!(
            record,
            Record {
                task: TaskId::new("08-pipeline", "t2"),
                verdict: Verdict::Pass,
            },
        );
    }

    #[test]
    fn a_header_that_names_nothing_is_refused() {
        // `Commit:` must be present and non-empty — a header with nothing
        // after the colon is the lack the grammar exists to catch, one
        // whitespace variant at a time.
        for commit_line in ["Commit:", "Commit: ", "Commit:   "] {
            let content = format!("Spec: 03-wild\nTask: t2\n{commit_line}\nVerdict: pass\n");
            let Err(refusal) = parse("reviews/03-wild/t2.md", &content) else {
                panic!("an empty `{commit_line}` header was accepted");
            };
            let message = refusal.to_string();
            assert!(
                message.contains("`Commit:`"),
                "the refusal does not name the empty header: {message}",
            );
        }
    }

    #[test]
    fn a_verdict_that_is_neither_pass_nor_fail_is_refused() {
        let content = well_formed("03-wild", "t2", "abc1234", "maybe");
        let Err(refusal) = parse("reviews/03-wild/t2.md", &content) else {
            panic!("`Verdict: maybe` was accepted");
        };
        let message = refusal.to_string();
        assert!(
            message.contains("maybe") && message.contains("reviews/03-wild/t2.md"),
            "the refusal names neither the verdict nor the file: {message}",
        );
    }

    #[test]
    fn the_findings_are_free_form_not_more_headers() {
        // A finding may quote `Verdict: fail` — seventeen records on main
        // quote headers freely. Only the first four lines are grammar; a
        // parser that scanned the whole file would let the findings
        // overturn the verdict.
        let content = format!(
            "{}\nA quoted counter-example:\nVerdict: fail\nCommit:\n",
            well_formed("03-wild", "t2", "abc1234", "pass"),
        );
        let record =
            parse("reviews/03-wild/t2.md", &content).expect("free-form findings were refused");
        assert_eq!(record.verdict, Verdict::Pass);
    }

    use proptest::prelude::Strategy;

    /// The same small universe as the decision's tests, so records,
    /// ticks and backlog lines collide often enough to matter.
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

        /// The grammar round-trips: whatever `/keeler:review` writes, the
        /// parser reads back as the record that was meant — same address,
        /// same verdict, whatever the findings say.
        #[test]
        fn any_record_the_review_stage_writes_is_read_back_exactly(
            spec in "[a-z][a-z0-9-]{0,15}",
            task in "[Tt][0-9]{1,2}",
            commit in "[a-f0-9]{7,40}",
            passed in proptest::bool::ANY,
            findings in "[ -~]{0,60}",
        ) {
            let verdict = if passed { "pass" } else { "fail" };
            let content = format!(
                "Spec: {spec}\nTask: {task}\nCommit: {commit}\nVerdict: {verdict}\n\n{findings}\n"
            );
            let read = parse("reviews/any.md", &content);
            proptest::prop_assert_eq!(read, Ok(Record {
                task: TaskId::new(&spec, &task),
                verdict: if passed { Verdict::Pass } else { Verdict::Fail },
            }));
        }

        /// Record dominance, first half: adding a `pass` record never
        /// breaks a passing gate — not for a task the backlog covered,
        /// not for one already recorded, not for one nobody ticked.
        #[test]
        fn adding_a_pass_record_never_breaks_a_passing_gate(
            ticked in proptest::collection::vec(any_task(), 0..6),
            records in proptest::collection::vec(any_record(), 0..6),
            backlog in proptest::collection::vec(any_task(), 0..6),
            extra in any_task(),
        ) {
            if let Decision::AllAccounted { ticked: counted } = decide(&ticked, &records, &backlog) {
                let mut with_extra = records;
                with_extra.push(Record { task: extra, verdict: Verdict::Pass });
                proptest::prop_assert_eq!(
                    decide(&ticked, &with_extra, &backlog),
                    Decision::AllAccounted { ticked: counted },
                );
            }
        }

        /// Record dominance, second half: a `fail` record fails its task
        /// whatever the backlog says and whatever else was recorded.
        #[test]
        fn a_fail_record_fails_its_task_whatever_the_backlog_says(
            ticked in proptest::collection::vec(any_task(), 1..6),
            records in proptest::collection::vec(any_record(), 0..6),
            backlog in proptest::collection::vec(any_task(), 0..6),
            which in 0..6usize,
        ) {
            let target = ticked[which % ticked.len()].clone();
            let mut with_fail = records;
            with_fail.push(Record { task: target.clone(), verdict: Verdict::Fail });
            match decide(&ticked, &with_fail, &backlog) {
                Decision::Missing(missing) => proptest::prop_assert!(
                    missing.contains(&target),
                    "the gate failed, but not on {target}: {missing:?}",
                ),
                Decision::AllAccounted { .. } => proptest::prop_assert!(
                    false,
                    "a fail record on {target} did not fail the gate",
                ),
            }
        }
    }
}
