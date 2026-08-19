//! Spec 07 — fan-out: one answer, one wave.
//!
//! T1's half: the fork after approval. `/keeler:spec` asks which road, and
//! the "graph" answer runs `just keeler-feature-branch <spec>` — the
//! branch, the checkout, the one commit. The recipe is driven as a
//! subprocess against a fixture repository, the way `tests/spawn.rs`
//! drives `keeler-spawn` and `tests/land.rs` drives `keeler-land`, so what
//! is under test is the shipped shell and not a Rust restatement of it.
//!
//! The rest — the question in `spec.md`, the hand-off, and what the
//! shipped rules say about the fork and the wave — is text an agent reads
//! as instruction, so it is asserted as text.

use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::OnceLock;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The fixture spec's slug — its file name without `.md`, which is the
/// only thing every name in graph mode is derived from.
const SLUG: &str = "42-fixture";

/// A root and a task needing it: enough of a graph that the fixture spec
/// is the shape `/keeler:tasks` leaves behind.
const TASKS: &str = "- [ ] **T1 — a root.** Scenarios: _one_.\n\
     - [ ] **T2 — the one that waits.** Needs: T1. Scenarios: _two_.\n";

fn spec_body(status: &str, tasks: &str) -> String {
    format!(
        "# Spec 42 — fixture\n\n**Status:** {status}\n\n## Tasks\n\n{tasks}\n---\n\n\
         ## Implementation Notes\n\nnone.\n"
    )
}

/// Where a spec lives in a project, which is how the recipes are called.
fn spec_path() -> String {
    format!("specs/{SLUG}.md")
}

/// The absolute path of the real `just`, resolved once against the
/// harness's own PATH.
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

/// A throwaway project holding the shipped `Justfile`, the graph script,
/// one spec and one unrelated file. Removed on drop.
struct Project(PathBuf);

impl Project {
    fn new(name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("keeler-fan-out-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("scripts")).unwrap();
        std::fs::create_dir_all(dir.join("specs")).unwrap();
        std::fs::copy(repo_root().join("Justfile"), dir.join("Justfile")).unwrap();
        std::fs::copy(
            repo_root().join("scripts/keeler-graph.sh"),
            dir.join("scripts/keeler-graph.sh"),
        )
        .unwrap();
        std::fs::write(dir.join(spec_path()), spec_body("Draft", TASKS)).unwrap();
        std::fs::write(dir.join("notes.txt"), "as committed\n").unwrap();
        std::fs::write(dir.join("staged.txt"), "as committed\n").unwrap();
        // Resolved, because `git rev-parse --show-toplevel` resolves too —
        // on macOS /var is a symlink to /private/var, and an unresolved
        // path would compare unequal to every path the recipes print.
        let project = Self(std::fs::canonicalize(&dir).unwrap());
        project.git(&["init", "-qb", "main"]);
        // The recipe commits with git's own identity, not the harness's, so
        // the fixture has to carry one the way a real repository does.
        project.git(&["config", "user.email", "probe@keeler"]);
        project.git(&["config", "user.name", "probe"]);
        project.git(&["add", "-A"]);
        project.git(&["commit", "-qm", "fixture"]);
        project
    }

    fn git_output(&self, args: &[&str]) -> Output {
        Command::new("git")
            .args(args)
            .current_dir(&self.0)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .output()
            .expect("failed to run git")
    }

    fn git(&self, args: &[&str]) -> String {
        let output = self.git_output(args);
        assert!(
            output.status.success(),
            "git {args:?} failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    fn just(&self, args: &[&str]) -> Output {
        Command::new(real_just())
            .args(args)
            .current_dir(&self.0)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .output()
            .expect("failed to run just")
    }

    /// The fork, run the way `/keeler:spec` runs it on the "graph" answer.
    fn feature_branch(&self) -> Output {
        self.just(&["keeler-feature-branch", &spec_path()])
    }

    fn write(&self, file: &str, body: &str) {
        std::fs::write(self.0.join(file), body).unwrap();
    }

    fn write_spec(&self, body: &str) {
        self.write(&spec_path(), body);
    }

    fn read_spec(&self) -> String {
        std::fs::read_to_string(self.0.join(spec_path())).unwrap()
    }

    fn branch(&self) -> String {
        self.git(&["symbolic-ref", "--short", "HEAD"])
    }

    /// `git status --porcelain`, untrimmed: the first two columns are the
    /// answer, and trimming the output eats the leading space of the first
    /// line — the one that says "modified, not staged".
    fn status(&self) -> String {
        let output = self.git_output(&["status", "--porcelain"]);
        assert!(output.status.success(), "git status failed");
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    /// The paths one commit touched — what "and nothing else" is measured
    /// against.
    fn touched(&self, commit: &str) -> Vec<String> {
        self.git(&["show", "--name-only", "--pretty=format:", commit])
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(str::to_string)
            .collect()
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn both(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

/// A shipped command file, read as the agent reads it.
fn command(name: &str) -> String {
    let path = repo_root()
        .join(".claude/commands/keeler")
        .join(format!("{name}.md"));
    std::fs::read_to_string(&path)
        .unwrap_or_else(|why| panic!("cannot read {}: {why}", path.display()))
}

/// The numbered walk-through both shipped documents carry — "a feature,
/// start to finish" — which is where the steps a human takes are named.
/// Lowercased, so an assertion about a step is not an assertion about its
/// capitalisation.
fn the_day(text: &str) -> String {
    let (_, after) = text
        .split_once("A feature, start to finish")
        .expect("the document no longer walks through a feature start to finish");
    after
        .split("\n## ")
        .next()
        .expect("split always yields one part")
        .to_lowercase()
}

/// The document as one line, so a sentence can be looked for without its
/// line wrapping mattering.
fn unwrapped(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

// ---- T1: the fork after approval -----------------------------------------

#[test]
fn approval_asks_which_road() {
    // Given the spec stage's command file
    let spec = command("spec");

    // Then after setting Status: Approved it instructs asking which road,
    // and waiting for the answer. Read from the approval onwards, because
    // that is where the scenario puts the question: a fork asked earlier
    // is asked about a spec nobody has agreed to yet.
    let approved = spec
        .find("Approved")
        .expect("spec.md no longer sets Status: Approved");
    let asked = unwrapped(&spec[approved..]);
    for road in ["linearly", "graph"] {
        assert!(
            asked.contains(road),
            "spec.md never offers the `{road}` road after the spec is approved:\n{asked}",
        );
    }
    assert!(
        asked.contains("wait for the answer") || asked.contains("waiting for the answer"),
        "spec.md asks which road without waiting for the answer:\n{asked}",
    );

    // And the question says that "graph" creates feat/<spec-slug> and
    // commits the approved spec there — the answer is the consent for it
    let unwrapped = unwrapped(&spec);
    for said in ["feat/<spec-slug>", "commits the approved spec", "consent"] {
        assert!(
            unwrapped.contains(said),
            "the question never says `{said}`, so the human answers without knowing what it buys",
        );
    }

    // And "linearly" hands off to /keeler:tasks with nothing else done
    let linear = spec
        .lines()
        .find(|line| line.contains("linearly") && line.contains("/keeler:tasks"))
        .expect("spec.md does not hand the linear answer to /keeler:tasks");
    assert!(
        linear.contains("nothing else"),
        "the linear answer does not say that nothing else is done: {linear:?}",
    );
}

#[test]
fn the_graph_answer_cuts_the_branch_and_commits_the_spec() {
    // Given the spec Approved in the working tree of main, and unrelated
    // uncommitted changes beside it — one modified, one staged
    let project = Project::new("cuts-the-branch");
    let approved = spec_body("Approved", TASKS);
    project.write_spec(&approved);
    project.write(
        "notes.txt",
        "an unrelated change, left in the working tree\n",
    );
    project.write("staged.txt", "an unrelated change, staged\n");
    project.git(&["add", "--", "staged.txt"]);

    // When `just keeler-feature-branch <spec>` runs
    let output = project.feature_branch();
    assert!(output.status.success(), "{}", both(&output));

    // Then feat/42-fixture exists and is checked out
    assert_eq!(project.branch(), format!("feat/{SLUG}"));

    // And its HEAD commit touches the spec and nothing else
    assert_eq!(project.touched("HEAD"), [spec_path()]);
    assert_eq!(
        project.git(&["show", &format!("HEAD:{}", spec_path())]),
        approved.trim()
    );

    // And the unrelated changes are still uncommitted, as they were
    let status = project.status();
    assert!(
        status.lines().any(|line| line == " M notes.txt"),
        "the unrelated change did not survive the fork:\n{status}",
    );
    assert!(
        status.lines().any(|line| line == "M  staged.txt"),
        "the unrelated staged change did not survive the fork:\n{status}",
    );
    assert!(
        !status.contains(SLUG),
        "the spec is still uncommitted after the commit that was meant to land it:\n{status}",
    );

    // And the command file instructs running that recipe on "graph", then
    // /keeler:tasks, then the next steps in order: commit the graph, then
    // `just keeler-fan-out <spec>`
    // The order is the instruction: each step is looked for in what
    // follows the one before it, so a hand-off that names the wave before
    // the graph is committed fails here rather than reads as present.
    let spec_command = unwrapped(&command("spec"));
    let mut at = 0;
    for said in [
        "just keeler-feature-branch",
        "/keeler:tasks",
        "commit the graph",
        "just keeler-fan-out",
    ] {
        let found = spec_command[at..].find(said).unwrap_or_else(|| {
            panic!("spec.md's graph answer does not say `{said}` where it belongs, in order")
        });
        at += found + said.len();
    }
}

#[test]
fn a_feature_branch_that_already_exists_is_used_not_remade() {
    // Given feat/42-fixture already exists holding an earlier copy of the
    // spec, and the working tree holds the newly approved one
    let project = Project::new("already-exists");
    let earlier = spec_body("Approved", TASKS);
    project.git(&["checkout", "-qb", &format!("feat/{SLUG}")]);
    project.write_spec(&earlier);
    project.git(&["add", "-A"]);
    project.git(&["commit", "-qm", "the copy approved the first time"]);
    let before = project.git(&["rev-parse", "HEAD"]);
    project.git(&["checkout", "-q", "main"]);
    let newly = spec_body(
        "Approved",
        &format!("{TASKS}- [ ] **T3 — what the amendment added.** Scenarios: _three_.\n"),
    );
    project.write_spec(&newly);

    // When `just keeler-feature-branch <spec>` runs
    let output = project.feature_branch();
    assert!(output.status.success(), "{}", both(&output));

    // Then it checks the branch out, carries the working-tree spec across,
    // and commits it there because it differs — saying which of the three
    // it did
    assert_eq!(project.branch(), format!("feat/{SLUG}"));
    assert_eq!(
        project.git(&["rev-parse", "HEAD~1"]),
        before,
        "the branch was remade rather than used: its earlier commit is gone",
    );
    assert_eq!(
        project.git(&["show", &format!("HEAD:{}", spec_path())]),
        newly.trim()
    );
    assert_eq!(project.touched("HEAD"), [spec_path()]);
    let said = both(&output);
    assert!(
        said.contains("already existed") && said.contains("committed"),
        "the run does not say that it used the branch it found and committed to it:\n{said}",
    );

    // And a working tree whose spec equals the branch's is checked out and
    // nothing is committed
    let landed = project.git(&["rev-parse", "HEAD"]);
    let again = project.feature_branch();
    assert!(again.status.success(), "{}", both(&again));
    assert_eq!(project.branch(), format!("feat/{SLUG}"));
    assert_eq!(
        project.git(&["rev-parse", "HEAD"]),
        landed,
        "a spec that had not changed was committed a second time",
    );
    assert!(
        both(&again).contains("nothing was committed"),
        "the run does not say that it committed nothing:\n{}",
        both(&again),
    );
}

/// Not a scenario of its own — the promise underneath two of them. To
/// switch branches at all, the recipe has to put the spec back to what
/// this branch committed, so the approved text exists nowhere in git
/// while the switch happens. A switch that fails there would take the
/// human's approval with it, and the answer to "which road" is not a
/// thing anyone can be asked to give twice.
#[test]
fn the_approved_spec_survives_a_checkout_that_cannot_happen() {
    // Given a feature branch holding a file the working tree also has,
    // untracked — which is a checkout git refuses
    let project = Project::new("checkout-refused");
    project.git(&["checkout", "-qb", &format!("feat/{SLUG}")]);
    project.write("blocker.txt", "as the branch has it\n");
    project.git(&["add", "-A"]);
    project.git(&["commit", "-qm", "a file only the feature branch has"]);
    project.git(&["checkout", "-q", "main"]);
    project.write("blocker.txt", "as the working tree has it\n");
    let approved = spec_body("Approved", TASKS);
    project.write_spec(&approved);

    // When `just keeler-feature-branch <spec>` runs
    let output = project.feature_branch();

    // Then it refuses, and the spec in the working tree is still the copy
    // the user approved — not the one this branch happens to have committed
    assert!(
        !output.status.success(),
        "the fork reported success over a checkout that did not happen:\n{}",
        both(&output),
    );
    assert!(
        both(&output).contains("could not check out"),
        "the run refused for some other reason than the checkout it could not do:\n{}",
        both(&output),
    );
    assert_eq!(project.branch(), "main");
    assert_eq!(
        project.read_spec(),
        approved,
        "the approved spec was lost to a checkout that never happened",
    );
}

#[test]
fn a_feature_branch_is_cut_from_main_and_nowhere_else() {
    // Given a checkout on another spec's feat/* branch, or on a keeler/*
    // task branch
    for (name, elsewhere) in [
        ("another-feature", "feat/99-other".to_string()),
        ("a-task-branch", format!("keeler/{SLUG}/t1")),
    ] {
        let project = Project::new(name);
        project.git(&["checkout", "-qb", &elsewhere]);
        let approved = spec_body("Approved", TASKS);
        project.write_spec(&approved);

        // When `just keeler-feature-branch <spec>` runs
        let output = project.feature_branch();

        // Then it refuses before creating anything, saying that a feature
        // branch is cut from main and naming the branch it is on
        assert!(
            !output.status.success(),
            "the fork ran on {elsewhere}:\n{}",
            both(&output),
        );
        let said = both(&output);
        assert!(
            said.contains(&elsewhere),
            "the refusal does not name the branch it is on:\n{said}",
        );
        assert!(
            said.contains("main"),
            "the refusal does not say a feature branch is cut from main:\n{said}",
        );
        assert!(
            !project
                .git_output(&["rev-parse", "--verify", &format!("refs/heads/feat/{SLUG}")])
                .status
                .success(),
            "the refusal cut feat/{SLUG} anyway",
        );
        assert_eq!(project.branch(), elsewhere);
        assert_eq!(
            project.read_spec(),
            approved,
            "the refusal changed the spec in the working tree",
        );
    }
}

#[test]
fn the_rules_describe_the_fork_and_the_wave() {
    // Given the shipped rules and KEELER.md
    for name in [".claude/keeler.md", "KEELER.md"] {
        let text = std::fs::read_to_string(repo_root().join(name)).unwrap();

        // Then the graph-mode day names both commands in the steps a human
        // takes
        let day = the_day(&text);
        for recipe in ["just keeler-feature-branch", "just keeler-fan-out"] {
            assert!(
                day.contains(recipe),
                "{name}'s walk-through never names `{recipe}`, so the human types spec 06's steps",
            );
        }

        // And they say the graph answer commits the spec, and that
        // answering "graph" is the consent for that one commit
        let sentences = unwrapped(&text).to_lowercase();
        let consent = sentences
            .split('.')
            .find(|sentence| sentence.contains("consent") && sentence.contains("graph"));
        let consent = consent.unwrap_or_else(|| {
            panic!("{name} never says that the \"graph\" answer is the consent for a commit")
        });
        assert!(
            consent.contains("commit"),
            "{name} says the answer is consent without saying what for: {consent:?}",
        );

        // And they say the graph is committed before fan-out reads it
        let commit = day
            .find("commit the graph")
            .unwrap_or_else(|| panic!("{name}'s walk-through never says to commit the graph"));
        let wave = day.find("just keeler-fan-out").unwrap();
        assert!(
            commit < wave,
            "{name} names the wave before the graph it reads is committed",
        );
    }
}

#[test]
fn the_linear_road_is_untouched() {
    // Given the command files of the linear road
    for stage in ["tasks", "tdd", "qa", "review", "mutants"] {
        let text = command(stage);

        // Then none carries a new instruction for the "linearly" answer,
        // and none names a branch to create or tmux to require
        for detour in [
            "linearly",
            "tmux",
            "checkout -b",
            "keeler-feature-branch",
            "keeler-fan-out",
        ] {
            assert!(
                !text.contains(detour),
                "/keeler:{stage} now carries `{detour}` — the fork reached the linear road",
            );
        }
    }

    // And /keeler:feature, whose step 1 is /keeler:spec, says that on the
    // "graph" answer it stops after /keeler:tasks with the same hand-off
    let feature = command("feature");
    assert!(
        feature.contains("/keeler:spec"),
        "/keeler:feature no longer starts at the spec stage",
    );
    let unwrapped = unwrapped(&feature);
    let fork = unwrapped
        .find("graph")
        .expect("/keeler:feature never mentions the graph answer");
    let after = &unwrapped[fork..];
    for said in ["/keeler:tasks", "commit the graph", "just keeler-fan-out"] {
        assert!(
            after.contains(said),
            "/keeler:feature's graph answer never says `{said}`:\n{after}",
        );
    }
}
