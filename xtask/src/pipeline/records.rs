//! The record grammar: four headers, a verdict, or a refusal naming the file.
//!
//! A record is what `/keeler:review` writes at `reviews/<slug>/<task>.md`:
//! `Spec:`, `Task:`, `Commit:` and `Verdict:` on its first four lines, then
//! the findings. The gate validates shape and verdict and nothing else —
//! `Commit:` must be present and non-empty but is never resolved, because
//! ancestry belongs to the pull-request check and this gate consults no
//! history at all. A record the grammar cannot read is refused, never
//! guessed at: a misread record could vouch for the wrong task. Which is
//! also why the record's address is read off its path and its headers must
//! agree — see [`filed_as`].

use std::path::{Path, PathBuf};

use super::decision::{Record, TaskId, Verdict};
use crate::Failure;

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

/// Reads one review record. `file` is the path it was found at — the
/// content has already been read, so the parser stays pure, but the path is
/// the address, not decoration.
///
/// # Errors
///
/// Refuses a record that lacks one of its four header lines, one whose
/// header names nothing, a verdict that is neither `pass` nor `fail`, and
/// one whose headers name a task other than the one it is filed as —
/// naming the file and the lack each time.
pub fn parse(file: &str, content: &str) -> Result<Record, Refusal> {
    record(file, content).map_err(|complaint| Refusal {
        file: file.to_string(),
        complaint,
    })
}

/// The grammar itself: four headers in `/keeler:review`'s order, then
/// findings the gate never reads.
fn record(file: &str, content: &str) -> Result<Record, String> {
    let mut lines = content.lines();
    let spec = header(lines.next(), 1, "Spec")?;
    let task = header(lines.next(), 2, "Task")?;
    let task = filed_as(file, &TaskId::new(spec, task))?;
    // Present and non-empty, never resolved: ancestry belongs to the
    // pull-request check, and this gate consults no history at all.
    header(lines.next(), 3, "Commit")?;
    let verdict = verdict(header(lines.next(), 4, "Verdict")?)?;
    Ok(Record { task, verdict })
}

/// The task the record's path says it holds — `<spec-slug>/<task-id>` read
/// off `…/<spec-slug>/<task-id>.md` — checked against the one its headers
/// name.
///
/// The path decides, because the path is where the rest of the workflow
/// agrees a task's address lives: the branch, the worktree and the CI check
/// all spell it that way, while a header is a line someone can carry over
/// from the neighbouring record without noticing. Disagreement is refused
/// rather than resolved: one of the two is wrong, and a gate that picked
/// for you would let a misfiled record vouch for a task nobody reviewed.
fn filed_as(file: &str, named: &TaskId) -> Result<TaskId, String> {
    let Some(held) = address(file) else {
        return Err("is not `<spec-slug>/<task-id>.md`, so it addresses no task".to_string());
    };
    if held == *named {
        return Ok(held);
    }
    Err(format!(
        "its headers name {named}, but it is filed as {held} — one of the two is wrong"
    ))
}

/// The last two components of a record's path, or nothing when the path has
/// no such shape.
fn address(file: &str) -> Option<TaskId> {
    let (directories, name) = file.rsplit_once('/')?;
    let task = name.strip_suffix(".md")?;
    let spec = directories
        .rsplit_once('/')
        .map_or(directories, |(_, last)| last);
    if spec.is_empty() {
        return None;
    }
    Some(TaskId::new(spec, task))
}

/// The one header whose value the gate reads rather than merely requires.
fn verdict(value: &str) -> Result<Verdict, String> {
    match value {
        "pass" => Ok(Verdict::Pass),
        "fail" => Ok(Verdict::Fail),
        other => Err(format!(
            "`Verdict:` must be `pass` or `fail`, not `{other}`"
        )),
    }
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

/// Reads every record under `dir` — `reviews/<spec-slug>/<task-id>.md`, one
/// directory down, which is the only shape that addresses a task.
///
/// A file at the top level is not a record: `reviews/BACKLOG.md` lives
/// there, and reading the debt list as a review would refuse it as
/// malformed — the gate failing over the file that exists to make it pass.
///
/// Inside a record directory the rule is the other way round, and
/// deliberately so: every `*.md` there is held to the grammar, while a
/// `.DS_Store` or a `.txt` is passed over. Litter the operating system
/// leaves is not a claim about a review; a Markdown file sitting where
/// records live is, and a directory that holds records holds nothing else.
/// Refusing it names the file and the header it lacks, which is how a
/// reader learns the rule; passing over it would let a record misnamed by
/// one letter vanish, and its task would read as unreviewed while the file
/// sat right there.
///
/// # Errors
///
/// Names the directory it cannot read, the file it cannot read, and the
/// record the grammar refuses. An absent `reviews/` is no records at all —
/// a repository that has reviewed nothing, which fails the moment anything
/// is ticked.
pub fn read_from(dir: &Path) -> Result<Vec<Record>, Failure> {
    let mut records = Vec::new();
    for spec in sorted(dir)? {
        if !spec.is_dir() {
            continue;
        }
        for file in sorted(&spec)? {
            if file.extension().is_some_and(|kind| kind == "md") {
                let file = file.display().to_string();
                records.push(parse(&file, &crate::read(&file)?)?);
            }
        }
    }
    Ok(records)
}

/// What `dir` holds, in file-name order — the gate's output must not depend
/// on the filesystem's mood — or nothing at all when `dir` is not there.
fn sorted(dir: &Path) -> Result<Vec<PathBuf>, Failure> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(why) if why.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(why) => return Err(format!("cannot read {}: {why}", dir.display()).into()),
    };
    let mut paths: Vec<PathBuf> = entries
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<_, _>>()
        .map_err(|why| format!("cannot read {}: {why}", dir.display()))?;
    paths.sort();
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::super::decision::{Decision, Record, TaskId, Uncovered, Verdict, Why, decide};
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
        // Then it fails, naming the record and the contradiction — not
        // the same word it uses for a task nobody reviewed at all.
        let Decision::Missing(missing) = decision else {
            panic!("a ticked task with a fail record was accounted for");
        };
        assert_eq!(
            missing,
            vec![Uncovered {
                task: TaskId::new("03-wild", "t2"),
                why: Why::ReviewFailed,
            }],
        );
        let complaint = missing[0].to_string();
        assert!(
            complaint.contains("reviews/03-wild/t2.md") && complaint.contains("Verdict: fail"),
            "the failure names neither the record nor the contradiction: {complaint}",
        );
    }

    #[test]
    fn a_record_filed_under_the_wrong_task_is_refused() {
        // A record copied from its neighbour and not re-headed vouches for
        // the task it was copied from: that task reads as reviewed when
        // nobody reviewed it, and this one reads as having no record while
        // the file sits right there. The path and the headers must agree,
        // and the gate names both rather than guessing which is right.
        let content = well_formed("06-graph-mode", "t3", "1ad1a50", "pass");
        let Err(refusal) = parse("reviews/06-graph-mode/t4.md", &content) else {
            panic!("a record filed under another task's name was accepted");
        };
        let message = refusal.to_string();
        assert!(
            message.contains("06-graph-mode/t3") && message.contains("06-graph-mode/t4"),
            "the refusal does not name both addresses: {message}",
        );

        // A `Spec:` header differing only in case is the same misfiling.
        // One task, one address, one spelling — a slug normalised here
        // would silently address a spec that may not be this one.
        let miscased = well_formed("06-Graph-Mode", "t3", "1ad1a50", "pass");
        let Err(refusal) = parse("reviews/06-graph-mode/t3.md", &miscased) else {
            panic!("a record whose `Spec:` header is miscased was accepted");
        };
        assert!(
            refusal.to_string().contains("06-Graph-Mode/t3"),
            "the refusal does not name the address the headers spell: {refusal}",
        );
    }

    #[test]
    fn the_last_two_components_are_the_address_however_the_path_reaches_them() {
        // T5 may reach a record by an absolute path or by one relative to
        // wherever it walked from; both name the same record, and neither
        // may change what it vouches for.
        for file in [
            "reviews/03-wild/t2.md",
            "/Users/someone/keeler/reviews/03-wild/t2.md",
            "03-wild/t2.md",
        ] {
            let record = parse(file, &well_formed("03-wild", "t2", "abc1234", "pass"))
                .unwrap_or_else(|why| panic!("`{file}` was refused: {why}"));
            assert_eq!(record.task, TaskId::new("03-wild", "t2"));
        }
    }

    #[test]
    fn a_file_that_is_not_a_records_path_is_refused() {
        // Nothing but `<spec-slug>/<task-id>.md` addresses a task, so
        // nothing else may be read as vouching for one — `reviews/` holds
        // `BACKLOG.md` too, and a gate that read it as a record would let
        // the debt list vouch for a task named `backlog`.
        for file in ["t2.md", "reviews/03-wild/t2.txt", "/t2.md"] {
            let Err(refusal) = parse(file, &well_formed("03-wild", "t2", "abc1234", "pass")) else {
                panic!("`{file}` was read as a review record");
            };
            assert!(
                refusal.to_string().contains(file),
                "the refusal does not name the file: {refusal}",
            );
        }
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
        // Two records on main open a findings line with `Verdict:` — the
        // stage's own summary of what it concluded. Only the first four
        // lines are grammar; a parser that scanned the whole file would
        // let the findings overturn the verdict. `Commit:` is left out of
        // this fixture on purpose: the shipped `review-record` job counts
        // `^Commit:` lines and refuses any record but one carrying
        // exactly one, so a record blessed here would fail there.
        let content = format!(
            "{}\nVerdict: **fail** would be the wrong read of this line.\n",
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
            // The file the review stage writes: the task id lowercased on
            // the way into the path, whatever case the header spells.
            let file = format!("reviews/{spec}/{}.md", task.to_lowercase());
            let read = parse(&file, &content);
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
                    missing.contains(&Uncovered {
                        task: target.clone(),
                        why: Why::ReviewFailed,
                    }),
                    "the gate failed, but not on {target} as a failed review: {missing:?}",
                ),
                Decision::AllAccounted { .. } => proptest::prop_assert!(
                    false,
                    "a fail record on {target} did not fail the gate",
                ),
            }
        }
    }
}
