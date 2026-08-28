//! Spec 06 — the pipeline routes people through itself.
//!
//! Every stage of this workflow leaves evidence except review, and the
//! commands that ship to adopters used to route straight past it: the TDD
//! command's closing instruction named the next *task*, not the next
//! *stage*. Worked task by task — the normal way — review never came up.
//!
//! These tests are about the shipped commands, so they hold for every
//! project Keeler installs into, not only this one.

mod common;

use std::path::PathBuf;

use common::{job_block, repo_root, shipped_workflow};

fn command(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(".claude/commands/keeler")
        .join(format!("{name}.md"));
    std::fs::read_to_string(&path)
        .unwrap_or_else(|why| panic!("cannot read {}: {why}", path.display()))
}

/// The pipeline, in order, as `(stage, the stage that follows it)`.
const PIPELINE: [(&str, &str); 3] = [
    ("tdd", "/keeler:qa"),
    ("qa", "/keeler:review"),
    ("review", "/keeler:mutants"),
];

#[test]
fn every_stage_names_the_one_that_follows_it() {
    // Given each shipped pipeline command
    for (stage, successor) in PIPELINE {
        let text = command(stage);

        // Then its closing instruction names the next stage. A command
        // that ends without saying where to go next is a command people
        // leave from.
        let closing = text.lines().rev().take(6).collect::<Vec<_>>().join("\n");
        assert!(
            closing.contains(successor),
            "`{stage}` does not point at `{successor}` in its closing lines:\n{closing}",
        );
    }
}

#[test]
fn only_the_last_stage_sends_the_reader_to_the_next_task() {
    // Given the stages that come before the end of the pipeline
    for (stage, _) in PIPELINE {
        let text = command(stage);

        // Then none of them says to move on to another task. That is what
        // routed twenty tasks straight past the review stage.
        assert!(
            !text.contains("which task is next"),
            "`{stage}` sends the reader to the next task before this one is finished",
        );
    }
}

#[test]
fn the_checkbox_is_ticked_by_the_stage_that_can_honestly_tick_it() {
    // Given the first and last stages of the pipeline
    let tdd = command("tdd");
    let mutants = command("mutants");

    // Then the box is ticked at the end, not the beginning. Ticked by TDD
    // it means "one stage of four ran", so an unreviewed task and a
    // finished one look exactly alike in the spec.
    assert!(
        !tdd.contains("Tick the task's checkbox"),
        "the TDD stage still marks the task done before it is",
    );
    assert!(
        mutants.contains("Tick the task's checkbox"),
        "nothing ticks the checkbox, so a finished task never looks finished",
    );
}

#[test]
fn the_rules_say_which_stage_ticks_the_box() {
    // Given the workflow rules Keeler ships
    let rules = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".claude/keeler.md"),
    )
    .unwrap();

    // Then they agree with the commands. Rules that contradict the
    // commands are worse than no rules: both look authoritative.
    assert!(
        !rules.contains("/keeler:tdd ticks tasks"),
        "the rules still say the TDD stage ticks the checkbox",
    );
    assert!(
        rules.contains("/keeler:mutants ticks"),
        "the rules do not say which stage ticks the checkbox",
    );
}

#[test]
fn the_rules_warn_that_a_skipped_review_goes_unnoticed() {
    // Given the workflow rules Keeler ships into other people's projects
    let rules = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".claude/keeler.md"),
    )
    .unwrap();

    // Then they say the review stage is enforced in Keeler's own
    // repository and not in theirs. Describing a gate an adopter does not
    // have is worse than describing none: they would believe it is there.
    let review = rules
        .split("## ")
        .find(|section| section.starts_with("Quality gates"))
        .expect("the rules no longer describe the quality gates");
    assert!(
        review.contains("leaves no artifact"),
        "the rules do not warn that a skipped review goes unnoticed:\n{review}",
    );
}

// Spec 08 — the pipeline enforces itself. Unlike the tests above, this
// one is about our own repository's workflow, not the shipped commands —
// it holds nowhere but here.

#[test]
fn this_repository_runs_the_check_it_ships() {
    // Given the `review-record` job that `templates/keeler.yml` ships
    let shipped = job_block(&shipped_workflow(), "review-record");

    // When Keeler's own CI workflow is read
    let ci = std::fs::read_to_string(repo_root().join(".github/workflows/ci.yml")).unwrap();

    // Then it carries the same job — writing this spec revealed the check
    // guarded every adopter's keeler/* pull requests and never our own
    assert!(
        ci.lines().any(|line| line.trim_end() == "  review-record:"),
        "ci.yml has no review-record job — the check ships to adopters but never ran here",
    );
    let ours = job_block(&ci, "review-record");
    // Trailing whitespace aside: a blank line separating this job from one
    // added after it belongs to the file's layout, not to the job, and
    // `job_block` keeps it — equality over it would cry drift at an
    // invisible diff the moment either file gains a job below this one.
    assert_eq!(
        ours.trim_end(),
        shipped.trim_end(),
        "ci.yml's review-record job has drifted from templates/keeler.yml — \
         the copy is verbatim, `fetch-depth: 0` and all",
    );
    // And it guards keeler/* pull requests here — pinned on our copy, so
    // this holds even if the template's own guard moves
    assert!(
        ours.contains("github.event_name == 'pull_request'")
            && ours.contains("startsWith(github.head_ref, 'keeler/')"),
        "ci.yml's review-record job does not key on keeler/* pull requests:\n{ours}",
    );
}
