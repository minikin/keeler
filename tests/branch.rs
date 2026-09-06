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
use std::process::{Command, Output};
use std::sync::OnceLock;

use common::{
    Repo, checks_out_full_history, indent_of, job_block, job_names, repo_root, run_script, said,
    shipped_workflow, step_block,
};

/// The task branch every fixture here runs on — the shape `keeler-spawn`
/// creates and the shape CI keys on.
const BRANCH: &str = "keeler/99-fixture/t4";

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
///
/// The recipe is the *last* argument, not the first: the Justfile ships in
/// the plugin and every self-call names it — `just --justfile <plugin>
/// --working-directory . dev` — so a stub keyed on `$1` would see a flag,
/// hand the whole gate to the real `just` and run it for real.
const JUST_STUB: &str = r#"#!/usr/bin/env bash
recipe=""
for a in "$@"; do recipe="$a"; done
case "$recipe" in
dev|crap-delta|mutants-diff)
    printf '%s\n' "$recipe" >> "$KEELER_STUB_JUST_LOG"
    if [ "$recipe" = "${KEELER_STUB_JUST_FAIL:-}" ]; then exit 3; fi
    exit 0
    ;;
esac
exec "$KEELER_REAL_JUST" "$@"
"#;

/// A recorded CRAP baseline, as a project commits one.
const BASELINE: &str = "{\"functions\":[]}\n";

/// A throwaway project holding the shipped `Justfile` and the stub `just`,
/// checked out on a task branch with a change on it — which is the only
/// place `keeler-branch` is ever run.
struct Project(Repo);

impl Project {
    fn new(name: &str, baseline: Option<&str>) -> Self {
        let repo = Repo::new("branch-gate", name);
        std::fs::create_dir_all(repo.path().join("bin")).unwrap();
        std::fs::copy(repo_root().join("Justfile"), repo.path().join("Justfile")).unwrap();
        if let Some(body) = baseline {
            repo.write("crap-baseline.json", body);
        }
        let stub = repo.path().join("bin/just");
        std::fs::write(&stub, JUST_STUB).unwrap();
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
        repo.write(".gitignore", "/bin/\n/just-calls\n");
        repo.git(&["add", "-A"]);
        repo.git(&["commit", "-qm", "main"]);
        repo.git(&["checkout", "-qb", BRANCH]);
        repo.commit("src/lib.rs", "pub fn t4() {}\n", "feat: t4");
        Self(repo)
    }

    /// Runs `just keeler-branch`, optionally with one of the three gates
    /// failing.
    fn keeler_branch(&self, fail: Option<&str>) -> Output {
        let dir = self.0.path();
        let path = std::env::var("PATH").unwrap();
        Command::new(real_just())
            .arg("keeler-branch")
            .current_dir(dir)
            .env("PATH", format!("{}:{path}", dir.join("bin").display()))
            .env("KEELER_STUB_JUST_LOG", dir.join("just-calls"))
            .env("KEELER_STUB_JUST_FAIL", fail.unwrap_or_default())
            .env("KEELER_REAL_JUST", real_just())
            .output()
            .expect("failed to run just keeler-branch")
    }

    /// The gates that ran, in the order they ran.
    fn gates(&self) -> Vec<String> {
        std::fs::read_to_string(self.0.path().join("just-calls"))
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }

    fn read(&self, file: &str) -> Option<Vec<u8>> {
        std::fs::read(self.0.path().join(file)).ok()
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
    //
    // Byte equality with `dev: fmt lint test crap` said that once and says
    // it no longer: spec 08's T7 gave `dev` a body of its own, keyed on a
    // marker file only this repository has, and `tests/pipeline.rs` runs
    // the recipe in a project without that marker to prove it is inert
    // there. What this test still owns is the branch gate's own two steps
    // staying out of `dev` — they are what would make the linear road
    // diff-based behind an adopter's back.
    let justfile = std::fs::read_to_string(repo_root().join("Justfile")).unwrap();
    let dev = recipe_block(&justfile, "dev");
    // The dependency line is still pinned exactly, and not by a prefix: a
    // fifth gate appended to it — `cov`, `mutants-all` — is precisely the
    // change byte equality was here to catch.
    assert_eq!(
        dev.lines().next(),
        Some("dev: fmt lint test crap"),
        "`just dev` no longer runs the four gates it did, and only those:\n{dev}"
    );
    for gate in ["crap-delta", "mutants-diff"] {
        assert!(
            !dev.contains(gate),
            "the branch gate's `{gate}` has moved into `just dev`:\n{dev}"
        );
    }
}

#[test]
fn every_recipe_is_listed_by_a_summary_not_by_the_tail_of_its_rationale() {
    // `just --list` is the road into the whole Justfile, and it shows only
    // the *last* comment line above a recipe. A recipe documented with a
    // paragraph is therefore listed by whatever that paragraph happened to
    // end on — "run. A test that is never run is not a test." — which
    // reads as noise to someone meeting the project for the first time.
    let output = Command::new(real_just())
        .arg("--list")
        .current_dir(repo_root())
        .output()
        .expect("failed to run just --list");
    let listing = String::from_utf8_lossy(&output.stdout).into_owned();

    let mut fragments = Vec::new();
    for line in listing.lines() {
        let Some((name, summary)) = line.split_once('#') else {
            continue;
        };
        let name = name.trim();
        let summary = summary.trim();
        if name.is_empty() || summary.is_empty() {
            continue;
        }
        // A summary opens the way a sentence or a code span does. A
        // fragment opens mid-clause: lowercase prose, or a word that ended
        // someone else's sentence.
        let opens = summary.chars().next().expect("a non-empty summary");
        if !(opens.is_ascii_uppercase() || opens == '`') {
            fragments.push(format!("{name}  # {summary}"));
        }
    }
    assert!(
        fragments.is_empty(),
        "these recipes are listed by the tail of their rationale, not by a summary:\n{}",
        fragments.join("\n")
    );
}

#[test]
fn the_branch_gate_is_listed_by_what_it_does() {
    // `just --list` is the road into every recipe, and it shows the last
    // comment line above one. A recipe whose rationale ends in an aside is
    // listed by that aside.
    let output = Command::new(real_just())
        .arg("--list")
        .current_dir(repo_root())
        .output()
        .expect("failed to run just --list");
    let listed = String::from_utf8_lossy(&output.stdout)
        .lines()
        .find(|line| line.trim_start().starts_with("keeler-branch "))
        .unwrap_or_else(|| {
            panic!(
                "just --list does not show keeler-branch:\n{}",
                said(&output)
            )
        })
        .to_string();
    let summary = listed
        .split_once('#')
        .expect("keeler-branch is listed without a summary")
        .1
        .trim();
    for gate in ["dev", "crap-delta", "mutants-diff"] {
        assert!(
            summary.contains(gate),
            "`just --list` does not say keeler-branch runs `{gate}`: {summary}"
        );
    }
    // And it reads as a summary rather than the tail of a paragraph — the
    // trap of a long rationale is that its last line is what gets listed.
    let opens = summary.chars().next().expect("the summary is empty");
    assert!(
        opens.is_ascii_uppercase() || opens == '`',
        "`just --list` shows keeler-branch as the tail of a sentence: {summary}"
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

/// A justfile of the project's own, holding recipes of the shape the
/// coverage bar used to live in. Nothing Keeler ships reads it any more:
/// the recipes are the plugin's, and the bars are the workflow's two
/// environment variables.
fn their_own_justfile(bar: &str) -> String {
    format!(
        "# a fixture justfile, the project's own\ncov:\n    cargo llvm-cov --fail-under-lines {bar}\n\n\
         crap:\n    cargo crap --threshold 15\n"
    )
}

/// A fixture with `main` at one commit carrying a baseline, and a task
/// branch checked out on top of it — with no justfile of any spelling,
/// which is the shape of every project once the recipes left it.
fn task_branch(name: &str) -> Repo {
    let repo = Repo::new("branch-baseline", name);
    repo.commit("crap-baseline.json", BASELINE, "baseline");
    repo.git(&["checkout", "-qb", BRANCH]);
    repo
}

fn check(repo: &Repo, script: &str) -> Output {
    repo.run(script, &[("BASE_REF", "main")])
}

#[test]
fn a_jobs_block_stops_before_the_next_jobs_documentation() {
    // The comment block above a job documents *that* job, and it sits at
    // the same indent as the job keys. A reader that only stops at keys
    // hands back the next job's rationale as if it were this job's body —
    // benign until the day an assertion says a job does *not* mention
    // something, and the thing it does not mention is next door.
    let workflow = shipped_workflow();
    for job in [BASELINE_JOB, "mutants", "quality"] {
        let block = job_block(&workflow, job);
        let strays: Vec<&str> = block
            .lines()
            .skip(1)
            .filter(|line| line.starts_with("  ") && !line.starts_with("   "))
            .collect();
        assert!(
            strays.is_empty(),
            "the `{job}` block reaches into what follows it: {strays:?}"
        );
    }
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
        checks_out_full_history(&job),
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
        said(&out).contains("crap-baseline.json") && said(&out).contains("keeler-land"),
        "the refusal names neither the file nor where baselines move:\n{}",
        said(&out)
    );

    // And a branch that changed only its own sources passes
    let repo = task_branch("additive");
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

/// Runs `just keeler-graph` in a fixture project, which is the road the
/// scenario names — the recipe, not the parser it wraps.
fn keeler_graph(repo: &Repo, spec: &str) -> Output {
    Command::new(real_just())
        .args(["keeler-graph", spec])
        .current_dir(repo.path())
        .output()
        .expect("failed to run just keeler-graph")
}

/// The state the recipe reports for one task.
fn state_of(output: &Output, id: &str) -> String {
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .find(|line| line.split_whitespace().next() == Some(id))
        .unwrap_or_else(|| panic!("the recipe reported no {id}:\n{}", said(output)))
        .split_whitespace()
        .nth(1)
        .unwrap()
        .to_string()
}

const SPEC: &str = "specs/99-fixture.md";
const TICK_BRANCH: &str = "keeler/99-fixture/t2";

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

/// The Status: line of a spec, whichever revision it is read from.
fn status_line(body: &str) -> &str {
    body.lines()
        .find(|line| line.contains("Status:"))
        .expect("the fixture spec has no Status: line")
}

#[test]
fn a_branch_ticks_its_task_and_nothing_else() {
    // Given a task branch keeler/<spec-slug>/t2 whose pipeline reached
    // /keeler:mutants with zero survivors — which is the stage that ticks,
    // and the stage the branch condition had to reach
    let mutants = std::fs::read_to_string(repo_root().join("commands/mutants.md")).unwrap();
    assert!(
        mutants.contains("Tick the task's checkbox"),
        "mutants.md no longer ticks the task's checkbox"
    );
    let branch_rule = mutants
        .lines()
        .find(|line| line.contains("keeler/*"))
        .expect("mutants.md says nothing about a keeler/* branch");
    assert!(
        branch_rule.contains("Status:") && branch_rule.contains("keeler-land"),
        "mutants.md does not leave Status: to keeler-land on a branch: {branch_rule}"
    );

    // ... in a project holding the spec on main
    let repo = Repo::new("branch-tick", "spec");
    std::fs::create_dir_all(repo.path().join("scripts")).unwrap();
    std::fs::copy(repo_root().join("Justfile"), repo.path().join("Justfile")).unwrap();
    std::fs::copy(
        repo_root().join("scripts/keeler-graph.sh"),
        repo.path().join("scripts/keeler-graph.sh"),
    )
    .unwrap();
    repo.git(&["add", "-A"]);
    repo.git(&["commit", "-qm", "keeler"]);
    repo.commit(SPEC, &fixture_spec(false), "spec");
    // ... on the branch its feature is developed on, which is where the
    // graph is read from and where its tasks fan out
    let feature = format!(
        "feat/{}",
        SPEC.trim_start_matches("specs/").trim_end_matches(".md")
    );
    repo.git(&["checkout", "-qb", &feature]);

    // When the stage finishes
    repo.git(&["checkout", "-qb", TICK_BRANCH]);
    repo.commit(SPEC, &fixture_spec(true), "docs: T2 done");

    // Then T2's checkbox is ticked in the spec on that branch, and the
    // branch touched the spec and nothing else in it
    assert_eq!(
        repo.git(&["diff", "--name-only", &feature, "HEAD"]),
        SPEC,
        "the tick reached beyond the spec"
    );
    let changed: Vec<String> = repo
        .git(&["diff", "-U0", &feature, "HEAD", "--", SPEC])
        .lines()
        .filter(|line| {
            (line.starts_with('+') || line.starts_with('-'))
                && !line.starts_with("+++")
                && !line.starts_with("---")
        })
        .map(str::to_string)
        .collect();
    assert_eq!(
        changed,
        [
            "-- [ ] **T2 — the one on the branch.** Needs: T1. Scenarios: _two_.",
            "+- [x] **T2 — the one on the branch.** Needs: T1. Scenarios: _two_.",
        ],
        "the branch changed more of the spec than T2's checkbox"
    );

    // And the spec's Status: line is unchanged
    let on_feature = repo.git(&["show", &format!("{feature}:{SPEC}")]);
    let on_branch = repo.git(&["show", &format!("HEAD:{SPEC}")]);
    assert_eq!(
        status_line(&on_feature),
        status_line(&on_branch),
        "the branch rewrote the spec's Status: line"
    );

    // And `just keeler-graph` still reports T2 as not done — from the task
    // branch as much as from the feature branch, because it reads the
    // feature branch either way and a tick on an unlanded task branch is
    // not a landing
    for standing_on in [TICK_BRANCH, feature.as_str()] {
        repo.git(&["checkout", "-q", standing_on]);
        let graph = keeler_graph(&repo, SPEC);
        assert_eq!(
            state_of(&graph, "T2"),
            "ready",
            "on {standing_on}, a tick on an unlanded task branch was read as done:\n{}",
            said(&graph)
        );
        assert_eq!(
            state_of(&graph, "T3"),
            "blocked",
            "on {standing_on}, a tick on an unlanded task branch unblocked the task after it:\n{}",
            said(&graph)
        );
    }
}

// ---------------------------------------------------------------------------
// Scenario: The graph answers from the feature's branch
// ---------------------------------------------------------------------------

/// A fixture project holding the shipped `Justfile` and graph parser.
fn graph_project(name: &str) -> Repo {
    let repo = Repo::new("graph-ref", name);
    std::fs::create_dir_all(repo.path().join("scripts")).unwrap();
    std::fs::copy(repo_root().join("Justfile"), repo.path().join("Justfile")).unwrap();
    std::fs::copy(
        repo_root().join("scripts/keeler-graph.sh"),
        repo.path().join("scripts/keeler-graph.sh"),
    )
    .unwrap();
    repo.git(&["add", "-A"]);
    repo.git(&["commit", "-qm", "keeler"]);
    repo
}

#[test]
fn the_graph_answers_from_the_features_branch() {
    // Given a spec whose tasks are ticked differently in the working tree
    // and on feat/<spec-slug>
    let repo = graph_project("working-tree");
    repo.commit(SPEC, &fixture_spec(false), "spec");
    let feature = "feat/99-fixture";
    repo.git(&["checkout", "-qb", feature]);
    repo.write(SPEC, &fixture_spec(true));

    // When `just keeler-graph` runs against the spec
    let out = keeler_graph(&repo, SPEC);
    assert!(out.status.success(), "{}", said(&out));

    // Then it reports readiness from the spec as committed on
    // feat/<spec-slug>: an uncommitted tick is one nothing else in graph
    // mode counts
    assert_eq!(
        state_of(&out, "T2"),
        "ready",
        "an uncommitted tick was read as done:\n{}",
        said(&out)
    );
    assert_eq!(
        state_of(&out, "T3"),
        "blocked",
        "an uncommitted tick unblocked the task after it:\n{}",
        said(&out)
    );

    // And it names the ref it read
    assert!(
        String::from_utf8_lossy(&out.stdout).contains(&format!("graph: {SPEC} on {feature}")),
        "the recipe does not say which ref it answered from:\n{}",
        said(&out)
    );

    // And it falls back to HEAD when the feature branch does not exist,
    // which is where a landed feature leaves it
    let landed = graph_project("no-feature-branch");
    landed.commit(SPEC, &fixture_spec(true), "spec, landed");
    let out = keeler_graph(&landed, SPEC);
    assert!(out.status.success(), "{}", said(&out));
    assert_eq!(
        state_of(&out, "T2"),
        "done",
        "the fallback to HEAD did not read the landed tick:\n{}",
        said(&out)
    );
    assert!(
        String::from_utf8_lossy(&out.stdout).contains(&format!("graph: {SPEC} on HEAD")),
        "the fallback does not say it read HEAD:\n{}",
        said(&out)
    );
}

#[test]
fn a_committed_spec_reads_through_a_symlinked_path() {
    // Given a repository reached through a symlink — which is every repo
    // under $TMPDIR on macOS, and any symlinked home or mount
    let repo = graph_project("symlinked");
    repo.commit(SPEC, &fixture_spec(false), "spec");
    let link = std::env::temp_dir().join(format!("keeler-symlink-{}", std::process::id()));
    let _ = std::fs::remove_file(&link);
    std::os::unix::fs::symlink(repo.path(), &link).unwrap();

    // When `just keeler-graph` runs against the spec through that path
    // PWD as an interactive shell leaves it: the logical path it was
    // reached by. Without it bash resets PWD to the physical path and the
    // mismatch this test is about never arises.
    let out = Command::new(real_just())
        .args(["keeler-graph", SPEC])
        .current_dir(&link)
        .env("PWD", &link)
        .output()
        .expect("failed to run just keeler-graph");
    let _ = std::fs::remove_file(&link);

    // Then it reads the graph. `git rev-parse --show-toplevel` answers
    // with the physical path while bash's `pwd` answers with the logical
    // one, so a spec path built from the latter strips nothing and the
    // recipe reports a committed spec as uncommitted.
    assert!(
        out.status.success(),
        "a committed spec read as uncommitted through a symlinked path:\n{}",
        said(&out)
    );
    assert_eq!(state_of(&out, "T2"), "ready", "{}", said(&out));
}

#[test]
fn every_spec_path_is_resolved_physically() {
    // Given the shipped Justfile. `git rev-parse --show-toplevel` answers
    // with the physical path, so any recipe that builds a spec's absolute
    // path logically strips nothing from it and reports a committed spec
    // as uncommitted. One recipe carried `pwd -P` and the rest did not;
    // this is the guard that keeps them together.
    let justfile = std::fs::read_to_string(repo_root().join("Justfile")).unwrap();

    // Then no line asks for a working directory logically
    let logical: Vec<&str> = justfile
        .lines()
        .filter(|line| line.contains("pwd)") || line.contains("pwd "))
        .filter(|line| !line.contains("pwd -P"))
        .collect();
    assert!(
        logical.is_empty(),
        "these resolve a path logically, and git will not agree with them: {logical:?}"
    );
}

#[test]
fn the_graph_reads_a_spec_the_working_tree_does_not_hold() {
    // Given a spec committed on its feature branch, with main checked out
    // — where the file is not in the working tree at all
    let repo = graph_project("absent-from-tree");
    repo.git(&["checkout", "-qb", "feat/99-fixture"]);
    repo.commit(SPEC, &fixture_spec(false), "spec");
    repo.git(&["checkout", "-q", "main"]);
    assert!(
        !repo.path().join(SPEC).exists(),
        "the fixture still holds the spec in its working tree"
    );

    // When `just keeler-graph` runs against it
    let out = keeler_graph(&repo, SPEC);

    // Then it answers from the branch. The working tree does not gate a
    // reading that comes from a ref.
    assert!(
        out.status.success(),
        "the recipe refused a spec its own ref carries:\n{}",
        said(&out)
    );
    assert_eq!(state_of(&out, "T2"), "ready", "{}", said(&out));
}

/// The graph-mode recipes, each with arguments that get it as far as the
/// repository lookup and no further, and the name its refusal must carry.
/// Two speak as `keeler-spawn`: `_spawn-preflight`, which is private and
/// speaks as the command the human typed, and `keeler-fan-out`, which
/// fires those same guards before it prints a wave — so every preflight
/// refusal reads alike wherever it was reached from.
const GRAPH_RECIPES: [(&[&str], &str); 8] = [
    (
        &["keeler-feature-branch", "specs/99-fixture.md"],
        "keeler-feature-branch",
    ),
    (&["keeler-graph", "specs/99-fixture.md"], "keeler-graph"),
    (
        &["keeler-spawn", "specs/99-fixture.md", "T1"],
        "keeler-spawn",
    ),
    (&["_spawn-preflight", "specs/99-fixture.md"], "keeler-spawn"),
    (&["keeler-status", "specs/99-fixture.md"], "keeler-status"),
    (
        &["keeler-resume", "specs/99-fixture.md", "T1"],
        "keeler-resume",
    ),
    (&["keeler-fan-out", "specs/99-fixture.md"], "keeler-spawn"),
    (&["keeler-land"], "keeler-land"),
];

#[test]
fn a_graph_mode_recipe_outside_a_git_repository_refuses_in_its_own_voice() {
    // Given a directory that is not inside a git repository
    let dir = std::env::temp_dir().join(format!("keeler-no-repo-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("specs")).unwrap();
    std::fs::create_dir_all(dir.join("scripts")).unwrap();
    std::fs::copy(repo_root().join("Justfile"), dir.join("Justfile")).unwrap();
    std::fs::copy(
        repo_root().join("scripts/keeler-graph.sh"),
        dir.join("scripts/keeler-graph.sh"),
    )
    .unwrap();
    std::fs::write(dir.join(SPEC), fixture_spec(false)).unwrap();
    // Four of these check for tmux before they look for the repository,
    // so without one on PATH the test would fail about that guard instead
    // — on any machine where tmux is not installed, CI's included.
    std::fs::create_dir_all(dir.join("bin")).unwrap();
    let tmux = dir.join("bin/tmux");
    std::fs::write(&tmux, "#!/usr/bin/env bash\nexit 0\n").unwrap();
    std::fs::set_permissions(&tmux, std::fs::Permissions::from_mode(0o755)).unwrap();
    let path = format!(
        "{}:{}",
        dir.join("bin").display(),
        std::env::var("PATH").unwrap()
    );

    for (recipe, name) in GRAPH_RECIPES {
        // When a graph-mode recipe runs there
        let out = Command::new(real_just())
            .args(recipe)
            .current_dir(&dir)
            .env("PATH", &path)
            .output()
            .expect("failed to run just");
        let said = said(&out);

        // Then it fails naming itself and saying graph mode needs a
        // repository, and not in git's voice — "fatal: not a git
        // repository" names no recipe and offers the reader nothing.
        // just prints its own `error: recipe <name> failed` tail, which
        // names the recipe whatever the body did — so the refusal is read
        // without it, or every one of these passes on just's words.
        let refusal: String = said
            .lines()
            .filter(|line| !line.starts_with("error: recipe"))
            .collect::<Vec<&str>>()
            .join("\n");
        assert!(
            !out.status.success(),
            "`{name}` succeeded outside a repository"
        );
        assert!(
            refusal.contains(name),
            "`{name}` refuses without naming itself:\n{said}"
        );
        assert!(
            refusal.contains("git repository"),
            "`{name}` does not say a repository is what is missing:\n{said}"
        );
        // And git's own reason is relayed rather than swallowed: the
        // failure may be one git can explain — a checkout owned by
        // another user — and that message is what carries the fix.
        assert!(
            refusal.contains("fatal:"),
            "`{name}` swallows git's reason, so a refusal git could explain arrives blank:\n{said}"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn every_recipe_that_resolves_the_repository_is_covered() {
    // Given the shipped Justfile. Whether a lookup needs a guard of its
    // own depends on what its callers already did, which no reading of a
    // single line can tell — so what is pinned here is coverage: a recipe
    // that asks where the repository is must be answered for by the check
    // above, which runs each of them and reads what it says.
    let justfile = std::fs::read_to_string(repo_root().join("Justfile")).unwrap();
    let covered: Vec<&str> = GRAPH_RECIPES.iter().map(|(argv, _)| argv[0]).collect();

    // Then every one of them is in that list, or is private and reached
    // only through a recipe that is.
    let mut recipe = "";
    let mut uncovered: Vec<&str> = Vec::new();
    for line in justfile.lines() {
        if !line.starts_with(char::is_whitespace) && line.trim_end().ends_with(':') {
            recipe = line.split([' ', ':']).next().unwrap_or("");
        }
        let resolves = line.contains("git rev-parse --show-toplevel")
            || line.contains("git rev-parse --git-dir");
        if resolves && !recipe.starts_with('_') && !covered.contains(&recipe) {
            uncovered.push(recipe);
        }
    }
    uncovered.sort_unstable();
    uncovered.dedup();
    assert!(
        uncovered.is_empty(),
        "these resolve the repository but the no-repository check never runs them: {uncovered:?}"
    );
}

#[test]
fn an_uncommitted_spec_is_refused_naming_the_ref_it_was_sought_on() {
    // Given a spec that exists in the working tree and on no branch —
    // the state /keeler:tasks leaves behind, before the graph is committed
    for recipe in ["keeler-graph", "keeler-status"] {
        let repo = graph_project("uncommitted");
        repo.write(SPEC, &fixture_spec(false));

        // When the recipe runs against it
        let out = Command::new(real_just())
            .args([recipe, SPEC])
            .current_dir(repo.path())
            .output()
            .expect("failed to run just");

        // Then it refuses, naming the ref it looked on — which is what
        // tells the reader the file in front of them is not the answer
        assert!(
            !out.status.success(),
            "`{recipe}` read a graph no ref carries:\n{}",
            said(&out)
        );
        assert!(
            said(&out).contains("is not committed on HEAD"),
            "`{recipe}` does not name the ref it sought the spec on:\n{}",
            said(&out)
        );
    }
}

// ---------------------------------------------------------------------------
// Spec 09 — T12: the workflow runs the gates from a fetched Keeler
// ---------------------------------------------------------------------------

/// The prefix every recipe invocation in the workflow must carry: the
/// `Justfile` the fetch step unpacked, against the project as the working
/// directory. CI cannot see the plugin cache, and the project has no
/// justfile of its own any more — a bare `just <recipe>` there reads
/// nothing at all.
const FETCHED_JUSTFILE: &str = "just --justfile \"$RUNNER_TEMP/keeler/Justfile\" \
                                --working-directory .";

/// The step every job with a gate runs before it, by the name the workflow
/// gives it.
const FETCH_STEP: &str = "Fetch Keeler at the pinned tag";

/// Every command in `text` whose first token is `just`, as `(line number,
/// command)`. The line is stripped of a step's `- ` and `run: ` prefixes and
/// split on the shell's separators, so `uses: taiki-e/install-action@just`
/// and `tool: cargo-nextest,just` are not invocations while a `just` after
/// `&&` is one.
fn just_invocations(text: &str) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let mut rest = line.trim();
        if rest.starts_with('#') {
            continue;
        }
        rest = rest.strip_prefix("- ").unwrap_or(rest).trim_start();
        rest = rest.strip_prefix("run: ").unwrap_or(rest).trim_start();
        for command in rest.split(['&', '|', ';']) {
            let command = command.trim();
            if command.split_whitespace().next() == Some("just") {
                found.push((index + 1, command.to_string()));
            }
        }
    }
    found
}

/// The `actions/cache` step of a job: its `id:` and its own lines.
fn cache_step(job: &str) -> Option<(String, String)> {
    let lines: Vec<&str> = job.lines().collect();
    let start = lines
        .iter()
        .position(|line| line.trim().starts_with("- uses: actions/cache@"))?;
    let indent = indent_of(lines[start]);
    let mut block = vec![lines[start]];
    for line in &lines[start + 1..] {
        if !line.trim().is_empty() && indent_of(line) <= indent {
            break;
        }
        block.push(line);
    }
    let block = block.join("\n");
    let id = block
        .lines()
        .find_map(|line| line.trim().strip_prefix("id:"))?
        .trim()
        .to_string();
    Some((id, block))
}

/// The workflow's top-level `env:` block — the keys every job inherits.
fn top_level_env(workflow: &str) -> String {
    let mut lines = workflow
        .lines()
        .skip_while(|line| line.trim_end() != "env:");
    lines.next().expect("the workflow has no top-level `env:`");
    lines
        .take_while(|line| line.trim().is_empty() || line.starts_with(' '))
        .collect::<Vec<&str>>()
        .join("\n")
}

#[test]
fn the_workflow_runs_every_gate_from_the_fetched_justfile() {
    // Given the shipped workflow
    let workflow = shipped_workflow();

    // When every command it hands to `just` is read
    let invocations = just_invocations(&workflow);
    assert!(
        !invocations.is_empty(),
        "the shipped workflow runs no gate through a recipe at all"
    );

    // Then each one names the fetched Justfile and the project as the
    // working directory. A gate that ran a recipe from anywhere else would
    // be measuring with a ruler nobody pinned.
    for (line, command) in &invocations {
        assert!(
            command.starts_with(FETCHED_JUSTFILE),
            "line {line} runs a recipe from somewhere else: {command}"
        );
    }
}

#[test]
fn the_mutants_job_goes_through_the_recipe() {
    // Given the shipped workflow's mutation job
    let job = job_block(&shipped_workflow(), "mutants");

    // Then the gate is the recipe, handed the base CI diffs against — the
    // flags `.cargo-mutants.toml` used to carry live in the recipe now, and
    // a bare `cargo mutants` here would run without a single one of them
    let step = step_block(&job, "Mutation tests (changed lines only)");
    assert!(
        step.contains("mutants-diff \"origin/${BASE_REF}\""),
        "the mutation gate does not go through the recipe with CI's base:\n{step}"
    );
    // Comments are read past: the rationale beside the step is free to say
    // what a bare `cargo mutants` would have cost, and an assertion that
    // tripped over its own explanation would push the rationale out.
    let direct: Vec<&str> = job
        .lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .filter(|line| line.contains("cargo mutants"))
        .collect();
    assert!(
        direct.is_empty(),
        "the mutation job still calls the tool directly: {direct:?}"
    );

    // And it is still a pull-request gate: a full run on an existing
    // codebase takes hours and reports debt the push did not write
    assert!(
        step.contains("if: github.event_name == 'pull_request'"),
        "the mutation gate no longer keys on pull requests:\n{step}"
    );
}

#[test]
fn the_workflow_fetches_the_pinned_tag_once_per_ref() {
    // Given the shipped workflow
    let workflow = shipped_workflow();

    // When each job that runs a gate is read
    let mut gate_jobs = Vec::new();
    for name in job_names(&workflow) {
        let job = job_block(&workflow, &name);
        if just_invocations(&job).is_empty() {
            continue;
        }
        gate_jobs.push(name.clone());

        // Then it caches the unpacked checkout under a key that moves with
        // the pin — six jobs and every push would otherwise each hit
        // codeload for the same bytes
        let (id, cache) = cache_step(&job)
            .unwrap_or_else(|| panic!("`{name}` runs a gate with no cache step:\n{job}"));
        assert!(
            cache.contains("path: ${{ runner.temp }}/keeler"),
            "`{name}` caches something other than the fetched Keeler:\n{cache}"
        );
        let key = cache
            .lines()
            .find(|line| line.trim().starts_with("key:"))
            .unwrap_or_else(|| panic!("`{name}`'s cache step has no key:\n{cache}"));
        assert!(
            key.contains("${{ env.KEELER_REF }}"),
            "`{name}`'s cache key does not move with the pin: {key}"
        );

        // And the fetch runs only on a miss, from the pinned tag, into the
        // directory the recipes are then read from
        let fetch = step_block(&job, FETCH_STEP);
        assert!(
            fetch.contains(&format!("if: steps.{id}.outputs.cache-hit != 'true'")),
            "`{name}` fetches even when the cache answered:\n{fetch}"
        );
        assert!(
            fetch.contains("https://codeload.github.com/minikin/keeler/tar.gz/${KEELER_REF}"),
            "`{name}` does not fetch the pinned tag:\n{fetch}"
        );
        assert!(
            fetch.contains("$RUNNER_TEMP/keeler"),
            "`{name}` unpacks somewhere the recipes are not read from:\n{fetch}"
        );
    }
    assert_eq!(
        gate_jobs,
        ["lints", "test", "quality", "mutants"],
        "the jobs that run a gate are not the ones this scenario covers"
    );
}

#[test]
fn the_thresholds_are_one_uncomment_away_in_ci() {
    // Given the shipped workflow
    let workflow = shipped_workflow();

    // Then its top-level env carries the pin, and the two bars offered
    // rather than set — an uncommented default would be a value the
    // adopter has to recognise as one before they may move it
    let env = top_level_env(&workflow);
    assert!(
        env.lines()
            .any(|line| line.trim().starts_with("KEELER_REF:")),
        "the workflow's env no longer pins the Keeler it fetches:\n{env}"
    );
    for bar in ["KEELER_COV_MIN", "KEELER_CRAP_MAX"] {
        let line = env
            .lines()
            .find(|line| line.contains(bar))
            .unwrap_or_else(|| panic!("the workflow's env does not offer {bar}:\n{env}"));
        assert!(
            line.trim().starts_with('#'),
            "{bar} is set rather than offered: {line}"
        );
    }

    // And the header says so where a reader meets the file, rather than
    // sending them to a README section about recipe lines they no longer
    // have
    let header: Vec<&str> = workflow
        .lines()
        .take_while(|line| line.starts_with('#') || line.trim().is_empty())
        .collect();
    let header = header.join("\n");
    for bar in ["KEELER_COV_MIN", "KEELER_CRAP_MAX"] {
        assert!(
            header.contains(bar),
            "the header does not name {bar} as the bar to move:\n{header}"
        );
    }
    assert!(
        !header.contains("README"),
        "the header still sends the reader to a README section on ratcheting recipes:\n{header}"
    );
}

#[test]
fn a_ref_that_does_not_resolve_is_named() {
    // Given the fetch step's shell, and a curl that exits 22 as it does on
    // an HTTP 404 — the shape of an init run from an unreleased checkout,
    // whose pin names a tag that was never cut
    let job = job_block(&shipped_workflow(), "lints");
    let script = run_script(&step_block(&job, FETCH_STEP));
    let repo = Repo::new("keeler-fetch", "unresolved-ref");
    std::fs::create_dir_all(repo.path().join("bin")).unwrap();
    let stub = repo.path().join("bin/curl");
    std::fs::write(&stub, "#!/usr/bin/env bash\nexit 22\n").unwrap();
    std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
    let path = format!(
        "{}:{}",
        repo.path().join("bin").display(),
        std::env::var("PATH").unwrap()
    );

    // When the step runs against a tag that does not resolve
    let out = repo.run(
        &script,
        &[
            ("KEELER_REF", "v9.9.9"),
            ("RUNNER_TEMP", repo.path().to_str().unwrap()),
            ("PATH", &path),
        ],
    );

    // Then it fails naming the ref and the line that carries it. Without
    // pipefail the step's status would be tar's, and CI would go on to run
    // every gate against an empty directory.
    assert!(
        !out.status.success(),
        "the step reported success having fetched nothing:\n{}",
        said(&out)
    );
    assert!(
        said(&out).contains("v9.9.9") && said(&out).contains("KEELER_REF"),
        "the failure names neither the ref nor the line to fix:\n{}",
        said(&out)
    );
}

#[test]
fn the_branch_baseline_job_no_longer_reads_the_projects_justfile() {
    // Given the shipped workflow's branch-baseline job
    let job = job_block(&shipped_workflow(), BASELINE_JOB);

    // Then it still guards the one piece of whole-repo state left in the
    // adopter's repository
    assert!(
        job.contains("crap-baseline.json"),
        "the branch-baseline job no longer names the baseline it guards:\n{job}"
    );

    // And it reads no justfile. The check failed outright where none
    // existed, which after this change is every adopter — so it would have
    // refused every keeler/* pull request there is.
    let lowered = job.to_lowercase();
    for marker in ["justfile", "cov:", "cov recipe", "cov_recipe"] {
        assert!(
            !lowered.contains(marker),
            "the branch-baseline job still reads `{marker}`:\n{job}"
        );
    }

    // And the chapters that state the rule say what the job now does. A
    // rule promising a check CI does not run is the worst kind of stale
    // documentation: it is the one an agent reads before acting.
    for chapter in ["graph-mode.md", "docs/KEELER.md"] {
        let text = std::fs::read_to_string(repo_root().join(chapter)).unwrap();
        assert!(
            text.contains("crap-baseline.json"),
            "{chapter} no longer states the shared-reference rule"
        );
        let stale: Vec<&str> = text
            .lines()
            .filter(|line| line.contains("coverage bar"))
            .collect();
        assert!(
            stale.is_empty(),
            "{chapter} still names a bar CI guards nowhere: {stale:?}"
        );
    }
}

#[test]
fn a_task_branch_that_moves_crap_baseline_json_is_still_refused() {
    // Given a keeler/* branch whose diff against main rewrites the baseline
    let script = run_script(&job_block(&shipped_workflow(), BASELINE_JOB));
    let repo = task_branch("still-refused");
    repo.commit(
        "crap-baseline.json",
        "{\"functions\":[{\"crap\":9}]}\n",
        "move it",
    );

    // When the job's shell runs against it
    let out = check(&repo, &script);

    // Then it is refused, naming the file and where baselines move — the
    // half of the check that survived losing the other half
    assert!(
        !out.status.success(),
        "a moved baseline passed the trimmed check:\n{}",
        said(&out)
    );
    assert!(
        said(&out).contains("crap-baseline.json") && said(&out).contains("keeler keeler-land"),
        "the refusal names neither the file nor where baselines move:\n{}",
        said(&out)
    );
}

#[test]
fn a_task_branch_that_leaves_the_baseline_alone_passes_without_a_justfile() {
    // Given a project with no justfile of any spelling — the shape of
    // every adopter now — on a keeler/* branch that changed only its own
    // sources
    let script = run_script(&job_block(&shipped_workflow(), BASELINE_JOB));
    let repo = task_branch("passes-without-a-justfile");
    repo.commit("src/lib.rs", "pub fn t4() {}\n", "feat: t4");

    // When the job's shell runs against it
    let out = check(&repo, &script);

    // Then it passes. The old check read the project's justfile to tell
    // whether the coverage bar had moved, and refused when it found none.
    assert!(
        out.status.success(),
        "a branch that moved nothing was refused for having no justfile:\n{}",
        said(&out)
    );

    // And a project that keeps a justfile of its own may edit it freely:
    // it is theirs, Keeler reads nothing in it, and the bars it used to
    // hold are the workflow's two environment variables now
    let repo = Repo::new("branch-baseline", "their-own-justfile");
    repo.commit("crap-baseline.json", BASELINE, "baseline");
    repo.commit("justfile", &their_own_justfile("90"), "their own recipes");
    repo.git(&["checkout", "-qb", BRANCH]);
    repo.commit("justfile", &their_own_justfile("10"), "their own business");
    let out = check(&repo, &script);
    assert!(
        out.status.success(),
        "a branch was refused over a recipe file that is the project's own:\n{}",
        said(&out)
    );
}

#[test]
fn the_review_record_job_stays() {
    // Given a keeler/* branch whose record names a commit the branch did
    // not make — the case tests/review_record.rs pins, re-run here because
    // the job beside it was rewritten and this one must not follow
    let script = run_script(&job_block(&shipped_workflow(), "review-record"));
    let repo = Repo::new("branch-review-record", "stays");
    let on_main = repo.commit("README.md", "hello\n", "init");
    repo.git(&["checkout", "-qb", BRANCH]);
    repo.commit("src/lib.rs", "pub fn t4() {}\n", "feat: t4");
    repo.write(
        "reviews/99-fixture/t4.md",
        &format!("Spec: 99-fixture\nTask: t4\nCommit: {on_main}\nVerdict: pass\n\nnone\n"),
    );

    // When the job's shell runs against it
    let out = repo.run(&script, &[("HEAD_REF", BRANCH), ("BASE_REF", "main")]);

    // Then it is refused: a record naming a commit already on the base
    // covers none of the branch's work
    assert!(
        !out.status.success(),
        "the review-record job passed a record naming no work of the branch:\n{}",
        said(&out)
    );
    assert!(
        said(&out).contains("names no work of this branch"),
        "the refusal does not say what is wrong with the record:\n{}",
        said(&out)
    );
}

#[test]
fn workflow_messages_name_the_wrapper_too() {
    // Given the shipped workflow
    let workflow = shipped_workflow();

    // Then the one message that tells a human what to run next names the
    // wrapper, which is the spelling that works from their project
    assert!(
        workflow.contains("run keeler keeler-land there"),
        "the branch-baseline refusal does not send the reader to the wrapper"
    );

    // And no message anywhere in it tells them to type `just keeler-…`,
    // which in a project without a justfile finds no such recipe
    let strays: Vec<&str> = workflow
        .lines()
        .filter(|line| line.contains("just keeler-"))
        .collect();
    assert!(
        strays.is_empty(),
        "these lines send the reader to a recipe their project does not have: {strays:?}"
    );
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

    /// Whatever else a task branch changes — its own sources, its own
    /// justfile if it keeps one, however many commits — CI passes it
    /// exactly when its diff against the base leaves `crap-baseline.json`
    /// alone. That is now the whole of the whole-repo state a branch can
    /// reach: the bars left the project with the recipes, and live in the
    /// workflow's environment where no branch edit can lower them unseen.
    #[test]
    fn ci_passes_a_branch_exactly_when_it_left_the_whole_repo_state_alone(
        moved_baseline in proptest::bool::ANY,
        moved_their_justfile in proptest::bool::ANY,
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
        if moved_their_justfile {
            repo.commit("justfile", &their_own_justfile("70"), "their own recipes");
        }
        if main_moved_after {
            repo.git(&["checkout", "-q", "main"]);
            repo.commit("crap-baseline.json", "{\"functions\":[{\"crap\":3}]}\n", "land");
            repo.git(&["checkout", "-q", BRANCH]);
        }
        let out = check(&repo, &script);
        proptest::prop_assert_eq!(
            out.status.success(),
            !moved_baseline,
            "baseline {} justfile {} main {} — the check said:\n{}",
            moved_baseline, moved_their_justfile, main_moved_after, said(&out)
        );
    }
}
