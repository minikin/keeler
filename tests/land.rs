//! Spec 06 — graph mode. The fan-in side: `just keeler-land`, and the
//! shared main-resolution helper it and `mutants-diff` both call.
//!
//! Every test drives the shipped recipe as a subprocess against a fixture
//! repository, the way `tests/spawn.rs` drives `keeler-spawn` and
//! `tests/installer.rs` drives `install.sh`. `just dev` and `just
//! crap-baseline` are a PATH stub that records what it was asked to run,
//! so "gates first, baseline second" is asserted as an order and a red
//! main is a fixture setting rather than a broken project — and nothing a
//! test runs takes minutes or reaches the network.

use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The fixture spec's slug — its file name without `.md`.
const SLUG: &str = "42-fixture";

/// The baseline as it stands before a land, byte for byte.
const OLD_BASELINE: &str = "{\"baseline\":\"as it was on main\"}\n";

/// What the stub `just crap-baseline` writes, byte for byte.
const NEW_BASELINE: &str = "{\"baseline\":\"regenerated at fan-in\"}\n";

fn spec_body() -> String {
    format!(
        "# Spec 42 — fixture\n\n**Status:** Approved\n\n## Tasks\n\n\
         - [x] **T1 — the root, and it is done.** Scenarios: _one_.\n\
         - [ ] **T2 — the other one.** Needs: T1. Scenarios: _two_.\n\n---\n\n\
         ## Implementation Notes\n\nnone — {SLUG} is a fixture.\n"
    )
}

/// The absolute path of the real `just`, resolved once against the
/// harness's own PATH. Tests invoke it by path so the stub `just` on the
/// project's PATH is only ever reached from inside a recipe.
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

/// Stands in for the two slow recipes `keeler-land` drives, and records
/// every call so the order they ran in — or did not run in — is
/// assertable. Everything else is the real `just`, so the recipe's own
/// helpers still work.
const JUST_STUB: &str = r#"#!/usr/bin/env bash
printf '%s\n' "$*" >> "$KEELER_STUB_JUST_LOG"
case "${1:-}" in
dev)
    echo "dev stub: the full gate ran"
    exit "${KEELER_STUB_DEV_EXIT:-0}"
    ;;
crap-baseline)
    if [ "${KEELER_STUB_BASELINE_EXIT:-0}" != 0 ]; then
        echo "crap-baseline stub: the measurement itself broke" >&2
        exit "$KEELER_STUB_BASELINE_EXIT"
    fi
    if [ "${KEELER_STUB_BASELINE_SKIP:-0}" = 1 ]; then
        echo "no Rust sources to measure — skipping (no library or binary targets)"
        exit 0
    fi
    printf '%s' "$KEELER_STUB_NEW_BASELINE" > crap-baseline.json
    echo "crap-baseline stub: baseline regenerated"
    exit 0
    ;;
esac
exec "$KEELER_REAL_JUST" "$@"
"#;

fn write_stub(path: &Path, body: &str) {
    std::fs::write(path, body).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// A throwaway project holding the shipped `Justfile`, a committed
/// baseline and one spec, with the stub `just` on its `bin/`.
struct Project {
    dir: PathBuf,
    /// The exit code the stub `just dev` reports — a red main is a
    /// setting, not a broken fixture.
    dev_exit: i32,
    /// Whether the stub `just crap-baseline` produces nothing, the way the
    /// shipped recipe does in a project with no Rust targets to measure.
    baseline_skip: bool,
    /// The exit code the stub `just crap-baseline` reports — the
    /// measurement machinery breaking is not the same as a red main.
    baseline_exit: i32,
}

impl Project {
    fn new(name: &str) -> Self {
        Self::on_branch(name, "main")
    }

    /// A project whose only branch is `branch` — the fixture for "main is
    /// whatever this repository calls main".
    fn on_branch(name: &str, branch: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("keeler-land-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        std::fs::create_dir_all(dir.join("specs")).unwrap();
        std::fs::copy(repo_root().join("Justfile"), dir.join("Justfile")).unwrap();
        std::fs::write(dir.join("crap-baseline.json"), OLD_BASELINE).unwrap();
        std::fs::write(dir.join("specs").join(format!("{SLUG}.md")), spec_body()).unwrap();
        std::fs::write(dir.join(".gitignore"), "/bin/\n/just-calls\n").unwrap();
        write_stub(&dir.join("bin/just"), JUST_STUB);
        // Resolved, because `git rev-parse --show-toplevel` resolves too —
        // on macOS /var is a symlink to /private/var.
        let dir = std::fs::canonicalize(&dir).unwrap();
        let project = Self {
            dir,
            dev_exit: 0,
            baseline_skip: false,
            baseline_exit: 0,
        };
        project.git(&["init", "-qb", branch]);
        project.git(&["add", "-A"]);
        project.git(&["commit", "-qm", "fixture"]);
        project
    }

    fn path(&self) -> &Path {
        &self.dir
    }

    fn git_output(&self, args: &[&str]) -> Output {
        Command::new("git")
            .args(["-c", "user.email=probe@keeler", "-c", "user.name=probe"])
            .args(args)
            .current_dir(&self.dir)
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

    /// Runs a recipe with the stub `just` first on PATH.
    fn just(&self, args: &[&str]) -> Output {
        let path = std::env::var("PATH").unwrap();
        Command::new(real_just())
            .args(args)
            .current_dir(&self.dir)
            .env("PATH", format!("{}:{path}", self.dir.join("bin").display()))
            .env("KEELER_STUB_JUST_LOG", self.dir.join("just-calls"))
            .env("KEELER_STUB_DEV_EXIT", self.dev_exit.to_string())
            .env("KEELER_STUB_NEW_BASELINE", NEW_BASELINE)
            .env("KEELER_STUB_BASELINE_EXIT", self.baseline_exit.to_string())
            .env(
                "KEELER_STUB_BASELINE_SKIP",
                if self.baseline_skip { "1" } else { "0" },
            )
            .env("KEELER_REAL_JUST", real_just())
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .output()
            .expect("failed to run just")
    }

    fn land(&self) -> Output {
        self.just(&["keeler-land"])
    }

    /// Every recipe the stub `just` was asked to run, in order.
    fn calls(&self) -> Vec<String> {
        std::fs::read_to_string(self.dir.join("just-calls"))
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }

    /// The gates it ran, in order — every recipe it drove that is not a
    /// private helper, because `_main-ref` answering a question is not a
    /// gate and a test that counted it as one would be asserting the
    /// wrong thing.
    fn gates(&self) -> Vec<String> {
        self.calls()
            .into_iter()
            .filter(|call| !call.starts_with('_'))
            .collect()
    }

    fn baseline(&self) -> String {
        std::fs::read_to_string(self.dir.join("crap-baseline.json")).unwrap()
    }

    fn staged(&self) -> Vec<String> {
        let listed = self.git(&["diff", "--cached", "--name-only"]);
        listed
            .lines()
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .collect()
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn both(output: &Output) -> String {
    format!(
        "{}{}",
        stdout(output),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn baseline_updates_happen_at_fan_in_on_main() {
    // Given a task branch that passed its gates and was merged — a main
    // whose tree is what fan-in left, with the old baseline committed
    let project = Project::new("fan-in");
    let head = project.git(&["rev-parse", "HEAD"]);

    // When `just keeler-land` runs on main
    let output = project.land();
    assert!(output.status.success(), "{}", both(&output));

    // Then `just dev` runs on main, and the baseline is regenerated after
    // it — gates first, baseline second
    assert_eq!(
        project.gates(),
        vec!["dev".to_string(), "crap-baseline".to_string()],
        "gates first, baseline second is not what ran: {:?}",
        project.calls()
    );
    assert_eq!(project.baseline(), NEW_BASELINE);

    // And it is staged, not committed, and the run says to review the diff
    // and commit
    assert_eq!(project.staged(), vec!["crap-baseline.json".to_string()]);
    assert_eq!(
        project.git(&["rev-parse", "HEAD"]),
        head,
        "keeler-land committed; the human commits"
    );
    let said = stdout(&output).to_lowercase();
    assert!(
        said.contains("review the diff") && said.contains("commit"),
        "the run does not ask for a human's commit:\n{}",
        stdout(&output)
    );

    // And the working tree holds exactly that staged change and nothing else
    assert_eq!(
        project.git(&["status", "--porcelain"]),
        "M  crap-baseline.json",
        "the working tree holds more than the staged baseline"
    );
}

#[test]
fn a_branch_that_was_green_alone_can_still_redden_main() {
    // Given two task branches that each passed their gates, whose merged
    // tree does not: `just dev` on main fails
    let mut project = Project::new("red-main");
    project.dev_exit = 1;

    // When the second is merged and `just keeler-land` runs on main
    let output = project.land();

    // Then `just dev` ran on main before anything else
    assert_eq!(
        project.gates().first().map(String::as_str),
        Some("dev"),
        "`just dev` was not the first gate to run: {:?}",
        project.calls()
    );

    // And the baseline is left exactly as it was, nothing is staged, and
    // the run exits non-zero saying main is red after fan-in
    assert_eq!(project.baseline(), OLD_BASELINE, "the baseline moved");
    assert_eq!(
        project.gates(),
        vec!["dev".to_string()],
        "a red main ran more than the gate it failed: {:?}",
        project.calls()
    );
    assert!(project.staged().is_empty(), "a red main staged something");
    assert_eq!(project.git(&["status", "--porcelain"]), "");
    assert!(
        !output.status.success(),
        "a red main landed anyway:\n{}",
        both(&output)
    );
    let said = both(&output).to_lowercase();
    assert!(
        said.contains("main is red") && said.contains("fan-in"),
        "the run does not say main is red after fan-in:\n{}",
        both(&output)
    );
}

#[test]
fn keeler_land_refuses_to_run_anywhere_but_main() {
    // Given the current branch is a task branch
    let project = Project::new("off-main");
    let branch = format!("keeler/{SLUG}/t3");
    project.git(&["checkout", "-q", "-b", &branch]);
    let spec = project.path().join("specs").join(format!("{SLUG}.md"));
    let spec_before = std::fs::read_to_string(&spec).unwrap();

    // When `just keeler-land` runs
    let output = project.land();

    // Then it refuses before running any gate, naming the branch it is on
    assert!(
        !output.status.success(),
        "keeler-land ran off main:\n{}",
        both(&output)
    );
    assert!(
        both(&output).contains(&branch),
        "the refusal does not name the branch it is on:\n{}",
        both(&output)
    );
    assert!(
        project.gates().is_empty(),
        "a gate ran before the refusal: {:?}",
        project.calls()
    );

    // And crap-baseline.json and the spec are untouched
    assert_eq!(project.baseline(), OLD_BASELINE);
    assert_eq!(std::fs::read_to_string(&spec).unwrap(), spec_before);
    assert!(project.staged().is_empty());
    assert_eq!(project.git(&["status", "--porcelain"]), "");
}

#[test]
fn a_project_with_nothing_to_measure_stages_nothing() {
    // Given a green main in a project whose crap-baseline has nothing to
    // measure — Keeler installed before the first Rust source exists
    let mut project = Project::new("nothing-to-measure");
    project.baseline_skip = true;
    project.git(&["rm", "-q", "crap-baseline.json"]);
    project.git(&["commit", "-qm", "no baseline yet"]);

    // When `just keeler-land` runs on main
    let output = project.land();

    // Then it says nothing was staged, rather than failing on a file that
    // was never written
    assert!(output.status.success(), "{}", both(&output));
    assert!(
        stdout(&output).contains("nothing is staged"),
        "the run does not say the baseline is missing:\n{}",
        both(&output)
    );
    assert!(project.staged().is_empty());
}

#[test]
fn a_detached_head_is_refused_by_the_name_it_has() {
    // Given a working tree on no branch at all — the shape a mid-rebase or
    // a `git checkout <sha>` leaves, where "am I on main?" has no answer
    let project = Project::new("detached");
    project.git(&["checkout", "-q", "--detach"]);

    // When `just keeler-land` runs
    let output = project.land();

    // Then it refuses before running any gate, saying what it is on
    assert!(
        !output.status.success(),
        "keeler-land landed off a branch:\n{}",
        both(&output)
    );
    assert!(
        both(&output).contains("detached HEAD"),
        "the refusal does not say the tree is on no branch:\n{}",
        both(&output)
    );
    assert!(project.gates().is_empty(), "{:?}", project.calls());
    assert_eq!(project.baseline(), OLD_BASELINE);
}

#[test]
fn a_baseline_that_could_not_be_regenerated_is_not_staged() {
    // Given a green main whose measurement machinery itself breaks
    let mut project = Project::new("baseline-broke");
    project.baseline_exit = 2;

    // When `just keeler-land` runs on main
    let output = project.land();

    // Then it fails, and stages nothing: only a baseline that was actually
    // regenerated is one a human can be asked to commit
    assert!(
        !output.status.success(),
        "a broken measurement landed anyway:\n{}",
        both(&output)
    );
    assert!(project.staged().is_empty(), "a failed run staged something");
    assert_eq!(project.baseline(), OLD_BASELINE);
    assert_eq!(project.git(&["status", "--porcelain"]), "");
}

#[test]
fn main_is_resolved_in_one_place() {
    // Given the shipped Justfile
    let justfile = std::fs::read_to_string(repo_root().join("Justfile")).unwrap();

    // Then the sequence that decides where main is appears exactly once —
    // two recipes that each carried their own could disagree
    let occurrences = justfile
        .matches("origin/main origin/master main master")
        .count();
    assert_eq!(
        occurrences, 1,
        "main is resolved in {occurrences} places; the spec allows one",
    );

    // And both recipes that need main call that one helper
    for recipe in ["keeler-land", "mutants-diff"] {
        let body = recipe_body(&justfile, recipe);
        assert!(
            body.contains("_main-ref"),
            "`{recipe}` does not call the shared helper:\n{body}",
        );
    }
}

/// One recipe's lines — from its header to the next unindented line.
fn recipe_body(justfile: &str, recipe: &str) -> String {
    let header = format!("{recipe}:");
    let mut lines = justfile.lines().skip_while(|line| *line != header);
    let first = lines
        .next()
        .unwrap_or_else(|| panic!("the Justfile has no `{recipe}` recipe"));
    let rest: Vec<&str> = lines
        .take_while(|line| {
            line.trim().is_empty() || line.starts_with(' ') || line.starts_with('\t')
        })
        .collect();
    format!("{first}\n{}", rest.join("\n"))
}

#[test]
fn main_is_whatever_this_repository_calls_it() {
    // Given a repository whose main branch is called master
    let project = Project::on_branch("master-repo", "master");

    // Then the shared helper says so
    let resolved = project.just(&["_main-ref"]);
    assert!(resolved.status.success(), "{}", both(&resolved));
    assert_eq!(stdout(&resolved).trim(), "master");

    // And keeler-land runs there rather than refusing
    let output = project.land();
    assert!(output.status.success(), "{}", both(&output));
    assert_eq!(project.staged(), vec!["crap-baseline.json".to_string()]);

    // And a remote-tracking main outranks the local branches, as
    // mutants-diff has always had it — while still landing on main
    let with_origin = Project::new("origin-main");
    with_origin.git(&["update-ref", "refs/remotes/origin/main", "HEAD"]);
    let resolved = with_origin.just(&["_main-ref"]);
    assert_eq!(stdout(&resolved).trim(), "origin/main");
    let output = with_origin.land();
    assert!(output.status.success(), "{}", both(&output));
}

proptest::proptest! {
    // Each case runs git and a recipe as subprocesses — keep the count low.
    #![proptest_config(proptest::prelude::ProptestConfig {
        cases: 8,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::WithSource("proptest-regressions"),
        )),
        ..proptest::prelude::ProptestConfig::default()
    })]

    /// Whatever the branch is called, if it is not main the refusal names
    /// it and no gate runs: the guard is about the branch, not about the
    /// one name a test happened to pick.
    #[test]
    fn any_branch_but_main_is_refused_by_name(
        branch in "keeler/[a-z0-9-]{1,8}/t[1-9]|[a-z][a-z0-9_-]{0,10}",
    ) {
        proptest::prop_assume!(branch != "main" && branch != "master");

        let project = Project::new("any-branch");
        project.git(&["checkout", "-q", "-b", &branch]);

        let output = project.land();

        proptest::prop_assert!(!output.status.success(), "landed on {}", branch);
        proptest::prop_assert!(
            both(&output).contains(&branch),
            "the refusal on {} does not name it:\n{}",
            branch,
            both(&output)
        );
        proptest::prop_assert!(project.gates().is_empty(), "a gate ran on {}", branch);
        proptest::prop_assert_eq!(project.baseline(), OLD_BASELINE);
    }
}
