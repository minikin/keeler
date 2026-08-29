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

use std::os::unix::fs::PermissionsExt as _;
use std::path::PathBuf;
use std::process::Command;

use common::{Repo, job_block, repo_root, said, shipped_workflow};

fn command(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(".claude/commands/keeler")
        .join(format!("{name}.md"));
    std::fs::read_to_string(&path)
        .unwrap_or_else(|why| panic!("cannot read {}: {why}", path.display()))
}

/// The shipped workflow rules — the file `install.sh` copies into every
/// adopting project, so a claim made here is made in their repository too.
fn rules() -> String {
    std::fs::read_to_string(repo_root().join(".claude/keeler.md")).unwrap()
}

/// One `## ` section of a Markdown document, from its heading to the next.
fn section(doc: &str, heading: &str) -> String {
    doc.split("\n## ")
        .find(|section| section.starts_with(heading))
        .unwrap_or_else(|| panic!("no `## {heading}` section"))
        .to_string()
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
    let rules = rules();

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

    // Then they say the review stage is enforced in Keeler's own
    // repository and not in theirs. Describing a gate an adopter does not
    // have is worse than describing none: they would believe it is there.
    let review = section(&rules(), "Quality gates");
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

// T7 — wired where the gates live.

/// Stands in for cargo, recording each invocation and answering nothing.
/// Nothing is answered on purpose: `test` and `crap` read `cargo metadata`
/// to decide whether this project has any Rust to measure, and silence is
/// "none" — so the four gates ahead of the pipeline check cost a fork
/// apiece and the recipe under test is the only one that does any work.
/// `KEELER_STUB_CARGO_FAIL` names the one invocation that refuses, which
/// is how a gate's verdict is made to reach `just dev`'s exit code.
const CARGO_STUB: &str = r#"#!/usr/bin/env bash
printf '%s\n' "$*" >> "$KEELER_STUB_CARGO_LOG"
if [ "$*" = "${KEELER_STUB_CARGO_FAIL:-}" ]; then exit 3; fi
exit 0
"#;

/// Stands in for shellcheck, which `lint` runs over shell this fixture has
/// none of.
const SHELLCHECK_STUB: &str = "#!/usr/bin/env bash\nexit 0\n";

/// `just dev` over the shipped Justfile in a throwaway project, returning
/// what it did — its exit status, and what it asked cargo for, in order.
/// The recipe is the real one: a Justfile read as text cannot say what
/// `just dev` runs, because the command could sit in a recipe nothing
/// calls, and it cannot say whether a refusal is passed on or swallowed.
///
/// `marker` is `templates/keeler.yml`, the file only Keeler's own
/// repository has and the same one `lint` keys its shellcheck branch on.
/// `refuses`, when given, is the cargo invocation the stub exits 3 on.
fn run_dev(name: &str, marker: bool, refuses: Option<&str>) -> (std::process::Output, Vec<String>) {
    let repo = Repo::new("dev-gate", name);
    std::fs::copy(repo_root().join("Justfile"), repo.path().join("Justfile")).unwrap();
    if marker {
        repo.write("templates/keeler.yml", "# the marker, not the workflow\n");
    }
    let bin = repo.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    for (tool, body) in [("cargo", CARGO_STUB), ("shellcheck", SHELLCHECK_STUB)] {
        let stub = bin.join(tool);
        std::fs::write(&stub, body).unwrap();
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let log = repo.path().join("cargo-calls");
    let output = Command::new("just")
        .arg("dev")
        .current_dir(repo.path())
        .env(
            "PATH",
            format!("{}:{}", bin.display(), std::env::var("PATH").unwrap()),
        )
        .env("KEELER_STUB_CARGO_LOG", &log)
        .env("KEELER_STUB_CARGO_FAIL", refuses.unwrap_or_default())
        .output()
        .expect("failed to run just dev — is just on PATH?");
    let calls = std::fs::read_to_string(&log)
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect();
    (output, calls)
}

/// `run_dev` where every gate passes, which is every case but one.
fn dev_calls(name: &str, marker: bool) -> Vec<String> {
    let (output, calls) = run_dev(name, marker, None);
    assert!(
        output.status.success(),
        "`just dev` failed over the shipped Justfile:\n{}",
        said(&output)
    );
    calls
}

/// The lines of one top-level key of a workflow — `on:` here, which no
/// `job_block` reaches.
fn top_level_block(workflow: &str, key: &str) -> String {
    let head = format!("{key}:");
    let mut lines = workflow.lines().skip_while(|line| line.trim_end() != head);
    let first = lines.next().unwrap_or_else(|| panic!("no `{key}:` key"));
    let mut block = vec![first];
    for line in lines {
        if !line.trim().is_empty() && !line.starts_with(' ') {
            break;
        }
        block.push(line);
    }
    block.join("\n")
}

#[test]
fn the_gate_is_one_recipe_away_everywhere_the_gates_live() {
    // Given the Justfile and the CI workflow of this repository

    // When `just dev` runs — the shipped recipe over a project carrying
    // the marker that says the gate lives here
    let calls = dev_calls("wired", true);

    // Then it runs `cargo xtask pipeline-check`, and runs it last: the
    // gate is milliseconds and the four ahead of it are minutes, so a
    // pipeline check that ran first would send a developer back to a
    // review stage before their tests had compiled
    assert_eq!(
        calls.last().map(String::as_str),
        Some("xtask pipeline-check"),
        "`just dev` does not end with the pipeline gate: {calls:?}",
    );
    assert_eq!(
        calls
            .iter()
            .filter(|call| call.starts_with("xtask pipeline-check"))
            .count(),
        1,
        "`just dev` runs the pipeline gate more than once: {calls:?}",
    );

    // And a gate that refuses is a `just dev` that refuses. Running the
    // command is not the requirement — reporting what it decided is:
    // `cargo xtask pipeline-check || true` would satisfy every assertion
    // above and leave the gate decorative.
    let (refused, calls) = run_dev("refused", true, Some("xtask pipeline-check"));
    assert!(
        !refused.status.success(),
        "`just dev` passed over a pipeline gate that refused: {calls:?}\n{}",
        said(&refused),
    );

    // And when CI runs on a push or a pull request, a job of its own runs
    // the same command
    let ci = std::fs::read_to_string(repo_root().join(".github/workflows/ci.yml")).unwrap();
    let triggers = top_level_block(&ci, "on");
    for event in ["push:", "pull_request:"] {
        assert!(
            triggers.contains(event),
            "ci.yml no longer runs on `{event}`:\n{triggers}",
        );
    }
    assert!(
        ci.lines()
            .any(|line| line.trim_end() == "  pipeline-check:"),
        "ci.yml has no pipeline-check job — the gate would run on one developer's machine and nowhere else",
    );
    let job = job_block(&ci, "pipeline-check");
    assert!(
        job.contains("cargo xtask pipeline-check"),
        "ci.yml's pipeline-check job does not run the command:\n{job}",
    );
    // And nothing narrows it away from either event. A job whose `if:`
    // keys on a branch or an event runs on the other one never, and the
    // gate's whole point is that no route to main skips it.
    assert!(
        !job.lines().any(|line| line.trim_start().starts_with("if:")),
        "ci.yml's pipeline-check job is conditional — it must run on a push and on a pull request alike:\n{job}",
    );
}

// T9 — the rules stop claiming nothing can notice.

/// The paragraph of the rules' Quality gates section that warns about the
/// review stage, found by the phrase `the_rules_warn_that_a_skipped_review_
/// goes_unnoticed` pins it by — so the two tests keep talking about the
/// same paragraph however it is rewritten around them.
fn review_stage_warning() -> String {
    section(&rules(), "Quality gates")
        .split("\n\n")
        .find(|paragraph| paragraph.contains("leaves no artifact"))
        .expect("the rules' Quality gates section no longer warns about the review stage")
        .to_string()
}

/// Prose with its line breaks taken out, so a phrase is looked for in the
/// sentence and not in the wrapping. `cargo xtask pipeline-check` falls
/// across a line break in the paragraph below; a raw `contains` would
/// demand a rewrap of any paragraph these phrases move within, which is
/// editing prose to suit a test.
fn unwrapped(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[test]
fn the_rules_stop_claiming_nothing_can_notice() {
    // Given the shipped workflow rules' review-stage warning
    // When the rules are read
    let paragraph = review_stage_warning();
    let warning = unwrapped(&paragraph);

    // Then they say the gate runs in the repository that ships Keeler. The
    // sentence they carried instead — "there is no mechanism that will
    // catch this for you" — was false here the moment the gate landed, and
    // rules that are wrong about their own gates are worse than silent
    // ones: both read as authoritative.
    assert!(
        !warning.contains("no mechanism that will catch this"),
        "the rules still say nothing can catch a skipped review:\n{paragraph}",
    );
    assert!(
        warning.contains("cargo xtask pipeline-check"),
        "the rules do not name the gate that catches it here:\n{paragraph}",
    );

    // And that an adopting project's review stage remains unenforced,
    // naming the one check they do get. This half is the load-bearing one:
    // describing a gate an adopter does not have would leave them trusting
    // a mechanism that is not in their repository.
    assert!(
        warning.contains("documented rather than enforced"),
        "the rules do not say an adopting project's review stage is unenforced:\n{paragraph}",
    );
    assert!(
        warning.contains("keeler/*"),
        "the rules do not name the check an adopting project does get:\n{paragraph}",
    );

    // And CONTRIBUTING, which told this repository's own contributors that
    // nothing enforced the review stage, no longer does either — it is the
    // doc a contributor here reads, and it named a gate that was parked.
    let section = section(
        &std::fs::read_to_string(repo_root().join("CONTRIBUTING.md")).unwrap(),
        "Review debt",
    );
    let debt = unwrapped(&section);
    assert!(
        !debt.contains("Nothing enforces this"),
        "CONTRIBUTING still says nothing enforces the review stage:\n{section}",
    );
    assert!(
        debt.contains("cargo xtask pipeline-check") && debt.contains("reviews/BACKLOG.md"),
        "CONTRIBUTING does not say what now enforces it, nor where the debt is recorded:\n{section}",
    );
}

#[test]
fn the_gate_stays_out_of_an_adopting_projects_dev() {
    // Given a project Keeler installed into — the same Justfile, and none
    // of the repository machinery the gate is: no xtask crate, and not
    // even the `cargo xtask` alias, which is never installed
    // When `just dev` runs there
    let calls = dev_calls("adopter", false);

    // Then the gate is inert. Shipping it would fail every adopter's gate
    // on a cargo subcommand their project has never heard of, and the spec
    // says plainly that their review stage stays documented, not enforced.
    assert!(
        !calls.iter().any(|call| call.starts_with("xtask")),
        "`just dev` runs the gate in a project that does not have it: {calls:?}",
    );

    // And the gates they do have ran, so this proves inertness and not a
    // recipe that failed before reaching anything
    assert!(
        calls.iter().any(|call| call.starts_with("clippy")),
        "`just dev` did not reach the lint gate at all: {calls:?}",
    );
}
