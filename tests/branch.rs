//! Spec 06 — graph mode. The branch side: `just keeler-branch`, the CI job
//! that keeps a task branch off the whole-repo state, and the tick that
//! stays on the branch.
//!
//! The gate is three recipes in one order, so the tests drive the real
//! recipe with a stub `just` first on PATH: the stub records the recipe it
//! was handed and returns, which makes the order observable without a
//! cargo invocation or a byte reaching the network. The CI check is shell
//! inside the workflow, lifted out of the YAML and run against fixture
//! repositories, the way `tests/review_record.rs` drives its own job.

mod common;

use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;

use common::{Repo, job_block, repo_root, run_script, said, shipped_workflow};

// ---------------------------------------------------------------------------
// Scenario: Branch gates are diff-based only
// ---------------------------------------------------------------------------

/// The absolute path of the real `just`, resolved once against the
/// harness's own PATH. Tests invoke it by path so the stub `just` on the
/// project's PATH is only ever reached from inside the recipe body.
fn real_just() -> &'static str {
    static JUST: OnceLock<String> = OnceLock::new();
    JUST.get_or_init(|| {
        let out = Command::new("sh")
            .args(["-c", "command -v just"])
            .output()
            .expect("failed to look for just");
        assert!(out.status.success(), "just is not on PATH");
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    })
}

/// Stands in for the three gates `keeler-branch` composes, recording the
/// order it was asked for them in. Everything else is the real `just`.
const JUST_STUB: &str = r#"#!/usr/bin/env bash
case "${1:-}" in
dev|crap-delta|mutants-diff)
    printf '%s\n' "$1" >> "$KEELER_STUB_JUST_LOG"
    if [ "$1" = "${KEELER_STUB_JUST_FAIL:-}" ]; then exit 3; fi
    exit 0
    ;;
esac
exec "$KEELER_REAL_JUST" "$@"
"#;

/// A recorded CRAP baseline, as a project commits one.
const BASELINE: &str = "{\"functions\":[]}\n";

/// A throwaway project holding the shipped `Justfile` and the stub `just`.
struct Project(PathBuf);

impl Project {
    fn new(name: &str, baseline: Option<&str>) -> Self {
        let dir = std::env::temp_dir().join(format!("keeler-branch-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        std::fs::copy(repo_root().join("Justfile"), dir.join("Justfile")).unwrap();
        if let Some(body) = baseline {
            std::fs::write(dir.join("crap-baseline.json"), body).unwrap();
        }
        let stub = dir.join("bin/just");
        std::fs::write(&stub, JUST_STUB).unwrap();
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
        Self(dir)
    }

    /// Runs `just keeler-branch`, optionally with one of the three gates
    /// failing.
    fn keeler_branch(&self, fail: Option<&str>) -> Output {
        let path = std::env::var("PATH").unwrap();
        Command::new(real_just())
            .arg("keeler-branch")
            .current_dir(&self.0)
            .env("PATH", format!("{}:{path}", self.0.join("bin").display()))
            .env("KEELER_STUB_JUST_LOG", self.0.join("just-calls"))
            .env("KEELER_STUB_JUST_FAIL", fail.unwrap_or_default())
            .env("KEELER_REAL_JUST", real_just())
            .output()
            .expect("failed to run just keeler-branch")
    }

    /// The gates that ran, in the order they ran.
    fn gates(&self) -> Vec<String> {
        std::fs::read_to_string(self.0.join("just-calls"))
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }

    fn read(&self, file: &str) -> Option<Vec<u8>> {
        std::fs::read(self.0.join(file)).ok()
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The lines of one `Justfile` recipe — from its `name:` line to the next
/// line that opens something new at the left margin.
fn recipe_block(justfile: &str, name: &str) -> String {
    let head = format!("{name}:");
    let mut lines = justfile.lines().skip_while(|line| !line.starts_with(&head));
    let Some(first) = lines.next() else {
        panic!("the Justfile has no `{name}` recipe");
    };
    let mut block = vec![first];
    for line in lines {
        if !line.trim().is_empty() && !line.starts_with(char::is_whitespace) {
            break;
        }
        block.push(line);
    }
    block.join("\n").trim_end().to_string()
}

#[test]
fn branch_gates_are_diff_based_only() {
    // Given a task branch with changes, and the bytes of crap-baseline.json
    // and the Justfile's cov recipe as they were on main
    let project = Project::new("diff-based", Some(BASELINE));
    let baseline_before = project.read("crap-baseline.json").unwrap();
    let justfile_before = String::from_utf8(project.read("Justfile").unwrap()).unwrap();
    let cov_before = recipe_block(&justfile_before, "cov");

    // When `just keeler-branch` runs on that branch
    let output = project.keeler_branch(None);
    assert!(output.status.success(), "{}", said(&output));

    // Then the full local gate runs, then crap-delta, then mutants-diff
    assert_eq!(
        project.gates(),
        ["dev", "crap-delta", "mutants-diff"],
        "the branch gate did not run dev, then crap-delta, then mutants-diff:\n{}",
        said(&output)
    );

    // And afterwards crap-baseline.json and the cov recipe are
    // byte-identical to what they were — a branch measures against the
    // baseline and never moves it
    assert_eq!(
        project.read("crap-baseline.json").unwrap(),
        baseline_before,
        "the branch gate moved crap-baseline.json"
    );
    let justfile_after = String::from_utf8(project.read("Justfile").unwrap()).unwrap();
    assert_eq!(
        recipe_block(&justfile_after, "cov"),
        cov_before,
        "the branch gate rewrote the cov recipe"
    );
    assert_eq!(
        justfile_after, justfile_before,
        "the branch gate rewrote the Justfile"
    );
}

#[test]
fn the_branch_gate_leaves_dev_alone() {
    // The adopter on the linear road must see no change: the branch gate
    // *calls* dev, it does not restate it or add a step to it.
    let justfile = std::fs::read_to_string(repo_root().join("Justfile")).unwrap();
    assert_eq!(
        recipe_block(&justfile, "dev"),
        "dev: fmt lint test crap",
        "`just dev` is no longer the recipe it was"
    );
}

#[test]
fn the_branch_gate_skips_the_delta_when_no_baseline_is_committed() {
    // Given a project with no committed baseline — `crap-delta` fails
    // outright there, which would make the branch gate unrunnable in a
    // project that has not turned the ratchet on
    let project = Project::new("no-baseline", None);

    // When the branch gate runs
    let output = project.keeler_branch(None);

    // Then it runs the other two and says why the delta was skipped, the
    // same guard /keeler:qa and the shipped workflow already use
    assert!(output.status.success(), "{}", said(&output));
    assert_eq!(
        project.gates(),
        ["dev", "mutants-diff"],
        "the branch gate ran crap-delta with no baseline to measure against"
    );
    assert!(
        said(&output).contains("crap-baseline.json"),
        "the branch gate skipped the delta gate silently:\n{}",
        said(&output)
    );
}

#[test]
fn the_branch_gate_stops_at_the_first_gate_that_fails() {
    // A gate that carried on past a red `dev` would report the verdict of
    // whatever ran last, and the spawn writes that verdict to disk.
    for (failing, expected) in [
        ("dev", &["dev"][..]),
        ("crap-delta", &["dev", "crap-delta"]),
        ("mutants-diff", &["dev", "crap-delta", "mutants-diff"]),
    ] {
        let project = Project::new(&format!("fail-{failing}"), Some(BASELINE));
        let output = project.keeler_branch(Some(failing));
        assert!(
            !output.status.success(),
            "a failing `{failing}` left the branch gate green:\n{}",
            said(&output)
        );
        assert_eq!(
            project.gates(),
            expected,
            "the branch gate ran on past a failing `{failing}`"
        );
    }
}

// ---------------------------------------------------------------------------
// Scenario: A branch that moved the baseline is refused by CI
// ---------------------------------------------------------------------------

/// The job that keeps a task branch off the whole-repo state, by the name
/// the workflow gives it.
const BASELINE_JOB: &str = "branch-baseline";

const BRANCH: &str = "keeler/99-fixture/t4";

/// A `Justfile` whose `cov` recipe is the shipped one, plus a marker the
/// tests can move without touching it.
fn justfile(cov_bar: &str, extra: &str) -> String {
    format!(
        "# a fixture Justfile\ncov:\n    cargo llvm-cov --fail-under-lines {cov_bar}\n\n\
         crap:\n    cargo crap --threshold 15\n{extra}"
    )
}

/// A fixture with `main` at one commit carrying a baseline and a Justfile,
/// and a task branch checked out on top of it.
fn task_branch(name: &str) -> Repo {
    let repo = Repo::new("branch-baseline", name);
    repo.commit("crap-baseline.json", BASELINE, "baseline");
    repo.commit("Justfile", &justfile("90", ""), "justfile");
    repo.git(&["checkout", "-qb", BRANCH]);
    repo
}

fn check(repo: &Repo, script: &str) -> Output {
    repo.run(script, &[("BASE_REF", "main")])
}

#[test]
fn a_branch_that_moved_the_baseline_is_refused_by_ci() {
    // Given a keeler/* branch, and the shipped CI workflow
    let workflow = shipped_workflow();
    let job = job_block(&workflow, BASELINE_JOB);

    // When the shipped CI workflow runs on its pull request — keyed on
    // keeler/* pull requests, with the history the diff needs
    assert!(
        job.contains("startsWith(github.head_ref, 'keeler/')")
            && job.contains("github.event_name == 'pull_request'"),
        "the branch-baseline job does not key on keeler/* pull requests:\n{job}"
    );
    assert!(
        job.contains("fetch-depth: 0"),
        "the branch-baseline job checks out shallow and cannot diff against the base:\n{job}"
    );
    let script = run_script(&job);

    // Then a branch whose diff against main touches crap-baseline.json
    // fails, naming the file
    let repo = task_branch("moved-baseline");
    repo.commit(
        "crap-baseline.json",
        "{\"functions\":[{\"crap\":1}]}\n",
        "move it",
    );
    let out = check(&repo, &script);
    assert!(!out.status.success(), "a moved baseline passed");
    assert!(
        said(&out).contains("crap-baseline.json") && said(&out).contains("main"),
        "the refusal names neither the file nor where baselines move:\n{}",
        said(&out)
    );

    // And so does one that touches the Justfile's cov recipe
    let repo = task_branch("moved-cov");
    repo.commit("Justfile", &justfile("80", ""), "lower the bar");
    let out = check(&repo, &script);
    assert!(!out.status.success(), "a moved cov recipe passed");
    assert!(
        said(&out).contains("cov") && said(&out).contains("main"),
        "the refusal names neither the recipe nor where baselines move:\n{}",
        said(&out)
    );

    // And a branch that added its own recipe to the Justfile — the
    // additive edit the same-region rule allows — passes
    let repo = task_branch("additive");
    repo.commit(
        "Justfile",
        &justfile("90", "\nkeeler-branch:\n    just dev\n"),
        "add a recipe",
    );
    repo.commit("src/lib.rs", "pub fn t4() {}\n", "feat: t4");
    let out = check(&repo, &script);
    assert!(
        out.status.success(),
        "an additive branch was refused:\n{}",
        said(&out)
    );

    // And a baseline that main moved after the branch left is main's
    // business, not the branch's: the diff is against the merge base
    let repo = task_branch("main-moved");
    repo.commit("src/lib.rs", "pub fn t4() {}\n", "feat: t4");
    repo.git(&["checkout", "-q", "main"]);
    repo.commit(
        "crap-baseline.json",
        "{\"functions\":[{\"crap\":2}]}\n",
        "land",
    );
    repo.git(&["checkout", "-q", BRANCH]);
    let out = check(&repo, &script);
    assert!(
        out.status.success(),
        "a branch was blamed for main's own baseline commit:\n{}",
        said(&out)
    );
}

// ---------------------------------------------------------------------------
// Scenario: A branch ticks its task and nothing else
// ---------------------------------------------------------------------------

/// Runs the shipped parser on a spec file, the way `just keeler-graph` does.
fn graph(spec: &Path) -> Output {
    Command::new("bash")
        .arg(repo_root().join("scripts/keeler-graph.sh"))
        .arg(spec)
        .output()
        .expect("failed to run keeler-graph.sh")
}

/// The state the parser reports for one task.
fn state_of(output: &Output, id: &str) -> String {
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .find(|line| line.split_whitespace().next() == Some(id))
        .unwrap_or_else(|| panic!("the parser reported no {id}"))
        .split_whitespace()
        .nth(1)
        .unwrap()
        .to_string()
}

fn fixture_spec(t2_done: bool) -> String {
    let t2 = if t2_done { 'x' } else { ' ' };
    format!(
        "# Spec 99 — fixture\n\n**Status:** Approved\n\n## Tasks\n\n\
         - [x] **T1 — the root.** Scenarios: _one_.\n\
         - [{t2}] **T2 — the one on the branch.** Needs: T1. Scenarios: _two_.\n\
         - [ ] **T3 — the one after it.** Needs: T2. Scenarios: _three_.\n\
         \n---\n\n## Implementation Notes\n\nnone.\n"
    )
}

#[test]
fn a_branch_ticks_its_task_and_nothing_else() {
    // Given a task branch keeler/06-graph-mode/t2 whose pipeline reached
    // /keeler:mutants with zero survivors
    let mutants =
        std::fs::read_to_string(repo_root().join(".claude/commands/keeler/mutants.md")).unwrap();

    // When the stage finishes
    // Then T2's checkbox is ticked in the spec on that branch ...
    assert!(
        mutants.contains("Tick the task's checkbox"),
        "mutants.md no longer ticks the task's checkbox"
    );
    // ... and the spec's Status: line is unchanged: on a keeler/* branch
    // the stage leaves Status: to keeler-land, on main, at fan-in
    let branch_rule = mutants
        .lines()
        .find(|line| line.contains("keeler/*"))
        .expect("mutants.md says nothing about a keeler/* branch");
    assert!(
        branch_rule.contains("Status:") && branch_rule.contains("keeler-land"),
        "mutants.md does not leave Status: to keeler-land on a branch: {branch_rule}"
    );

    // And the mechanical half: a branch's tick is a tick and nothing more
    let dir = std::env::temp_dir().join(format!("keeler-branch-tick-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let on_main = dir.join("main.md");
    let on_branch = dir.join("branch.md");
    std::fs::write(&on_main, fixture_spec(false)).unwrap();
    std::fs::write(&on_branch, fixture_spec(true)).unwrap();

    let main_body = std::fs::read_to_string(&on_main).unwrap();
    let branch_body = std::fs::read_to_string(&on_branch).unwrap();
    let differing: Vec<(&str, &str)> = main_body
        .lines()
        .zip(branch_body.lines())
        .filter(|(a, b)| a != b)
        .collect();
    assert_eq!(
        differing.len(),
        1,
        "the branch changed more than one line: {differing:?}"
    );
    assert!(
        differing[0].1.starts_with("- [x] **T2"),
        "the one changed line is not T2's checkbox: {:?}",
        differing[0]
    );
    let status = |body: &str| {
        body.lines()
            .find(|line| line.contains("Status:"))
            .unwrap()
            .to_string()
    };
    assert_eq!(
        status(&main_body),
        status(&branch_body),
        "the branch rewrote the spec's Status: line"
    );

    // And `just keeler-graph` on main still reports T2 as not done,
    // because readiness is read from main, not from an unlanded branch
    assert_eq!(state_of(&graph(&on_branch), "T2"), "done");
    assert_eq!(
        state_of(&graph(&on_main), "T2"),
        "ready",
        "a tick on an unlanded branch was read as done on main"
    );
    assert_eq!(
        state_of(&graph(&on_main), "T3"),
        "blocked",
        "a tick on an unlanded branch unblocked the task after it"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig {
        cases: 16,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::WithSource("proptest-regressions"),
        )),
        ..proptest::prelude::ProptestConfig::default()
    })]

    /// Whatever else a task branch changes — its own recipes, its own
    /// sources, however many commits — CI passes it exactly when its diff
    /// against the base leaves both pieces of whole-repo state alone.
    /// Neither half alone is the gate: a branch that moved only the cov
    /// bar has moved the ratchet just as surely as one that rewrote the
    /// baseline file.
    #[test]
    fn ci_passes_a_branch_exactly_when_it_left_the_whole_repo_state_alone(
        moved_baseline in proptest::bool::ANY,
        moved_cov in proptest::bool::ANY,
        added_recipe in proptest::bool::ANY,
        main_moved_after in proptest::bool::ANY,
        commits in 1usize..3,
    ) {
        let script = run_script(&job_block(&shipped_workflow(), BASELINE_JOB));
        let repo = task_branch("property");
        for i in 0..commits {
            repo.commit(&format!("src/t{i}.rs"), "pub fn t() {}\n", "work");
        }
        if moved_baseline {
            repo.commit("crap-baseline.json", "{\"functions\":[{\"crap\":9}]}\n", "move");
        }
        let extra = if added_recipe { "\nkeeler-branch:\n    just dev\n" } else { "" };
        if moved_cov || added_recipe {
            let bar = if moved_cov { "70" } else { "90" };
            repo.commit("Justfile", &justfile(bar, extra), "justfile");
        }
        if main_moved_after {
            repo.git(&["checkout", "-q", "main"]);
            repo.commit("crap-baseline.json", "{\"functions\":[{\"crap\":3}]}\n", "land");
            repo.git(&["checkout", "-q", BRANCH]);
        }
        let out = check(&repo, &script);
        proptest::prop_assert_eq!(
            out.status.success(),
            !(moved_baseline || moved_cov),
            "baseline {} cov {} recipe {} main {} — the check said:\n{}",
            moved_baseline, moved_cov, added_recipe, main_moved_after, said(&out)
        );
    }
}
