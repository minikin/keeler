//! The decision: ticked × records × backlog → what is missing.
//!
//! Pure by design — no git, no filesystem, no clock — so the rule that
//! gates a release can be unit- and mutation-tested without a repository.
//! The parsers that produce these inputs live beside it, not inside it.

use std::collections::BTreeSet;

/// One task, addressed the way every artifact addresses it:
/// `<spec-slug>/<task-id>`. The branch, the worktree, the review record
/// and the backlog all spell a task this way; so does the gate's output.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TaskId {
    spec: String,
    task: String,
}

impl TaskId {
    /// The task id is lowercased on the way in, as it is on the way into
    /// every path: the spec's checkbox says `T1`, the record lives at
    /// `t1.md`, and they must be the same address here or a reviewed task
    /// reads as missing.
    #[must_use]
    pub fn new(spec: impl Into<String>, task: impl Into<String>) -> Self {
        Self {
            spec: spec.into(),
            task: task.into().to_lowercase(),
        }
    }
}

impl std::fmt::Display for TaskId {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(out, "{}/{}", self.spec, self.task)
    }
}

/// What a review record concluded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Pass,
    Fail,
}

/// A review record, as far as the decision cares: whose it is and what it
/// says. Shape and parsing belong to the record grammar, not here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub task: TaskId,
    pub verdict: Verdict,
}

/// Why a ticked task is not accounted for. Two ticks can fail for reasons
/// that would read alike and are not alike: one has no evidence at all, the
/// other has evidence that says no. Kept apart, the first sends the reader
/// to write a review and the second to read the one already written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    /// Ticked, with neither a review record nor a backlog line.
    Unreviewed,
    /// Ticked, and its own review record's verdict is `fail`.
    ReviewFailed,
}

/// A ticked task the evidence does not account for, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Uncovered {
    pub task: TaskId,
    pub why: Why,
}

impl std::fmt::Display for Uncovered {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.why {
            Why::Unreviewed => write!(
                out,
                "{} is ticked, but no review record and no backlog line account for it",
                self.task,
            ),
            // The record's path is the task's address under the directory
            // and extension the workflow fixes — `reviews/<spec>/<task>.md`,
            // the one place /keeler:review writes. Naming it is the whole
            // difference between "go review this" and "go read the review
            // you already wrote".
            Why::ReviewFailed => write!(
                out,
                "{task} is ticked, but reviews/{task}.md says `Verdict: fail`",
                task = self.task,
            ),
        }
    }
}

/// What the gate concluded: a count of what was accounted for, or what is
/// missing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Every ticked task is accounted for — this many of them.
    AllAccounted { ticked: usize },
    /// These ticked tasks are not, in address order: each was ticked
    /// without the evidence the tick claims exists, and each says which
    /// lack it is.
    Missing(Vec<Uncovered>),
}

/// The gate's one rule. A ticked task is covered by a record whose verdict
/// is `pass`, or by the backlog when no record exists at all. A record
/// outranks the backlog either way: debt cannot shelter a known-bad review.
#[must_use]
pub fn decide(ticked: &[TaskId], records: &[Record], backlog: &[TaskId]) -> Decision {
    // A BTreeSet, not the slice as it came: the same task ticked twice is
    // one task to account for, and the missing list comes out in address
    // order — the same order however the specs were read.
    let distinct: BTreeSet<&TaskId> = ticked.iter().collect();
    let missing: Vec<Uncovered> = distinct
        .iter()
        .filter_map(|task| {
            uncovered(task, records, backlog).map(|why| Uncovered {
                task: (*task).clone(),
                why,
            })
        })
        .collect();
    if missing.is_empty() {
        Decision::AllAccounted {
            ticked: distinct.len(),
        }
    } else {
        Decision::Missing(missing)
    }
}

/// Whether one ticked task is accounted for, and if not, which lack it is.
/// Records are consulted first and alone when any exist — which is what
/// lets a backlog line cover only the unrecorded, and a fail verdict fail
/// its task whatever else holds.
fn uncovered(task: &TaskId, records: &[Record], backlog: &[TaskId]) -> Option<Why> {
    let mut verdicts = records
        .iter()
        .filter(|record| record.task == *task)
        .map(|record| record.verdict)
        .peekable();
    if verdicts.peek().is_none() {
        return if backlog.contains(task) {
            None
        } else {
            Some(Why::Unreviewed)
        };
    }
    if verdicts.all(|verdict| verdict == Verdict::Pass) {
        None
    } else {
        Some(Why::ReviewFailed)
    }
}

#[cfg(test)]
mod tests {
    use super::{Decision, Record, TaskId, Uncovered, Verdict, Why, decide};

    fn task(spec: &str, id: &str) -> TaskId {
        TaskId::new(spec, id)
    }

    fn unreviewed(spec: &str, id: &str) -> Uncovered {
        Uncovered {
            task: task(spec, id),
            why: Why::Unreviewed,
        }
    }

    fn review_failed(spec: &str, id: &str) -> Uncovered {
        Uncovered {
            task: task(spec, id),
            why: Why::ReviewFailed,
        }
    }

    fn pass(spec: &str, id: &str) -> Record {
        Record {
            task: task(spec, id),
            verdict: Verdict::Pass,
        }
    }

    fn fail(spec: &str, id: &str) -> Record {
        Record {
            task: task(spec, id),
            verdict: Verdict::Fail,
        }
    }

    #[test]
    fn a_task_ticked_without_a_review_is_caught() {
        // Given a spec whose task is ticked, no review record for that
        // task, and no line for it in the accepted-debt list
        let ticked = [task("08-pipeline", "t1")];
        // When the gate decides
        let decision = decide(&ticked, &[], &[]);
        // Then it fails, naming the spec and the task
        assert_eq!(
            decision,
            Decision::Missing(vec![unreviewed("08-pipeline", "t1")])
        );
        // The address the failure prints spells out both halves.
        assert_eq!(task("08-pipeline", "t1").to_string(), "08-pipeline/t1");
        assert!(
            unreviewed("08-pipeline", "t1")
                .to_string()
                .contains("08-pipeline/t1"),
            "the failure does not name the spec and the task",
        );
    }

    #[test]
    fn a_missing_review_and_a_failed_one_do_not_read_alike() {
        // Two ticks fail for reasons that are not the same: one has no
        // evidence, the other has evidence that says no. Reported alike,
        // the second sends the reader to write a review that is already
        // written — so each says which it is, and the failed one names
        // the record.
        let absent = unreviewed("03-wild", "t2").to_string();
        let failed = review_failed("03-wild", "t2").to_string();
        assert_ne!(absent, failed);
        assert!(
            absent.contains("no review record") && absent.contains("no backlog line"),
            "the unreviewed failure does not say what is absent: {absent}",
        );
        assert!(
            failed.contains("reviews/03-wild/t2.md") && failed.contains("Verdict: fail"),
            "the failed review does not name the record and the contradiction: {failed}",
        );
    }

    #[test]
    fn a_reviewed_task_passes_and_the_gate_says_what_it_counted() {
        // Given ticked tasks each holding a record whose verdict is pass
        let ticked = [task("01-install", "t1"), task("01-install", "t2")];
        let records = [pass("01-install", "t1"), pass("01-install", "t2")];
        // When the gate decides
        let decision = decide(&ticked, &records, &[]);
        // Then it passes, and says how many ticked tasks it accounted for
        assert_eq!(decision, Decision::AllAccounted { ticked: 2 });
    }

    #[test]
    fn a_task_id_is_one_address_whatever_the_case() {
        // The spec's checkbox says `T1`; the record lives at
        // `reviews/<slug>/t1.md`. One task, one address — otherwise a
        // reviewed task reads as missing because two parsers disagreed
        // on a letter's case.
        assert_eq!(task("08-pipeline", "T1"), task("08-pipeline", "t1"));
        assert_eq!(task("08-pipeline", "T1").to_string(), "08-pipeline/t1");
    }

    #[test]
    fn an_unrecorded_backlog_line_covers_its_task() {
        let ticked = [task("02-release", "t3")];
        let backlog = [task("02-release", "t3")];
        assert_eq!(
            decide(&ticked, &[], &backlog),
            Decision::AllAccounted { ticked: 1 },
        );
    }

    #[test]
    fn a_fail_record_does_not_cover_its_task() {
        let ticked = [task("02-release", "t3")];
        let records = [fail("02-release", "t3")];
        assert_eq!(
            decide(&ticked, &records, &[]),
            Decision::Missing(vec![review_failed("02-release", "t3")]),
        );
    }

    #[test]
    fn a_record_outranks_the_backlog() {
        // Coverage means a pass record or an *unrecorded* backlog line: a
        // fail record on a backlogged task still fails, so debt cannot
        // shelter a review that concluded the work is bad.
        let ticked = [task("03-wild", "t2")];
        let records = [fail("03-wild", "t2")];
        let backlog = [task("03-wild", "t2")];
        assert_eq!(
            decide(&ticked, &records, &backlog),
            Decision::Missing(vec![review_failed("03-wild", "t2")]),
        );
    }

    #[test]
    fn the_count_is_of_ticked_tasks_not_of_records() {
        // A record for an unticked task is fine — reviewed, pipeline not
        // finished — but it is not one of the tasks the gate accounts for.
        let ticked = [task("01-install", "t1")];
        let records = [pass("01-install", "t1"), pass("01-install", "t9")];
        assert_eq!(
            decide(&ticked, &records, &[]),
            Decision::AllAccounted { ticked: 1 },
        );
    }

    use proptest::prelude::Strategy;

    /// A small universe, so ticks, records and backlog lines collide often
    /// enough that every combination of the three is actually exercised.
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

        /// Gate soundness: for any ticked tasks, records and backlog, the
        /// gate passes exactly when every ticked task is covered — no
        /// false pass, no false failure — and what it reports is exactly
        /// the uncovered set, or the count of distinct ticked tasks.
        #[test]
        fn the_gate_passes_exactly_when_every_ticked_task_is_covered(
            ticked in proptest::collection::vec(any_task(), 0..6),
            records in proptest::collection::vec(any_record(), 0..6),
            backlog in proptest::collection::vec(any_task(), 0..6),
        ) {
            // The model restates the rule from the spec's words: a pass
            // record with no fail beside it, or a backlog line with no
            // record at all. It models the reason too — a tick with no
            // evidence and a tick its record contradicts are different
            // failures, and the gate must not blur them.
            let distinct: std::collections::BTreeSet<TaskId> = ticked.iter().cloned().collect();
            let uncovered: Vec<Uncovered> = distinct
                .iter()
                .filter_map(|task| {
                    let has_pass = records
                        .iter()
                        .any(|r| r.task == *task && r.verdict == Verdict::Pass);
                    let has_fail = records
                        .iter()
                        .any(|r| r.task == *task && r.verdict == Verdict::Fail);
                    let has_record = has_pass || has_fail;
                    if (has_pass && !has_fail) || (!has_record && backlog.contains(task)) {
                        return None;
                    }
                    Some(Uncovered {
                        task: task.clone(),
                        why: if has_record { Why::ReviewFailed } else { Why::Unreviewed },
                    })
                })
                .collect();

            match decide(&ticked, &records, &backlog) {
                Decision::AllAccounted { ticked: counted } => {
                    proptest::prop_assert!(
                        uncovered.is_empty(),
                        "false pass: {uncovered:?} are ticked and uncovered",
                    );
                    proptest::prop_assert_eq!(counted, distinct.len());
                }
                Decision::Missing(missing) => {
                    proptest::prop_assert!(
                        !uncovered.is_empty(),
                        "false failure: every ticked task is covered, yet {missing:?}",
                    );
                    proptest::prop_assert_eq!(missing, uncovered);
                }
            }
        }
    }
}
