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
    spec_with(&[true, false], "Approved")
}

/// A fixture spec of two tasks — T1 a root, T2 needing it — with the boxes
/// ticked as given and the `Status:` asked for. The graph is the same
/// shape whatever the ticks are, so what a test varies is one thing.
fn spec_with(ticks: &[bool], status: &str) -> String {
    let tasks: Vec<String> = ticks
        .iter()
        .enumerate()
        .map(|(index, ticked)| {
            let id = index + 1;
            let box_ = if *ticked { "x" } else { " " };
            let needs = if index == 0 { "" } else { " Needs: T1." };
            format!("- [{box_}] **T{id} — fixture task {id}.**{needs} Scenarios: _one_.")
        })
        .collect();
    let tasks = tasks.join("\n");
    format!(
        "# Spec 42 — fixture\n\n**Status:** {status}\n\n## Tasks\n\n{tasks}\n\n---\n\n\
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
        Self::build(name, "main", &spec_body())
    }

    /// A project whose only branch is `branch` — the fixture for "main is
    /// whatever this repository calls main".
    fn on_branch(name: &str, branch: &str) -> Self {
        Self::build(name, branch, &spec_body())
    }

    /// A project whose one spec is the given text — the fixture for what a
    /// land does, or does not do, to a spec.
    fn with_spec(name: &str, spec: &str) -> Self {
        Self::build(name, "main", spec)
    }

    /// A project on its feature branch, cut from a main that exists — the
    /// shape a feature is developed in, and the one keeler-land must tell
    /// apart from main by name.
    fn on_feature_branch(name: &str, spec: &str) -> Self {
        let project = Self::build(name, "main", spec);
        project.git(&["checkout", "-qb", &format!("feat/{SLUG}")]);
        project
    }

    fn build(name: &str, branch: &str, spec: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("keeler-land-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        std::fs::create_dir_all(dir.join("specs")).unwrap();
        std::fs::create_dir_all(dir.join("scripts")).unwrap();
        std::fs::copy(repo_root().join("Justfile"), dir.join("Justfile")).unwrap();
        // The graph script ships beside the Justfile and the recipe reads
        // readiness through it — a fixture without it is not the project
        // Keeler installs.
        std::fs::copy(
            repo_root().join("scripts/keeler-graph.sh"),
            dir.join("scripts/keeler-graph.sh"),
        )
        .unwrap();
        std::fs::write(dir.join("crap-baseline.json"), OLD_BASELINE).unwrap();
        std::fs::write(dir.join("specs").join(format!("{SLUG}.md")), spec).unwrap();
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

    fn spec_path(&self) -> PathBuf {
        self.dir.join("specs").join(format!("{SLUG}.md"))
    }

    fn spec(&self) -> String {
        std::fs::read_to_string(self.spec_path()).unwrap()
    }

    /// Where a task's worktree lands: a sibling of the repository root,
    /// named `<repo>-<spec-slug>-<task-id>`, as `keeler-spawn` puts it.
    fn worktree_path(&self, tid: &str) -> PathBuf {
        let name = self.dir.file_name().unwrap().to_string_lossy().into_owned();
        self.dir
            .parent()
            .unwrap()
            .join(format!("{name}-{SLUG}-{tid}"))
    }

    /// A worktree on `keeler/<slug>/<tid>`, the way a spawn leaves one.
    fn add_worktree(&self, tid: &str) -> PathBuf {
        let path = self.worktree_path(tid);
        let _ = std::fs::remove_dir_all(&path);
        self.git(&[
            "worktree",
            "add",
            "-q",
            "-b",
            &format!("keeler/{SLUG}/{tid}"),
            path.to_str().unwrap(),
            "HEAD",
        ]);
        path
    }

    /// Runs git inside one of this project's worktrees.
    fn git_in(worktree: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .args(["-c", "user.email=probe@keeler", "-c", "user.name=probe"])
            .args(args)
            .current_dir(worktree)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .output()
            .expect("failed to run git");
        assert!(
            output.status.success(),
            "git {args:?} in {} failed:\n{}",
            worktree.display(),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    /// The run record `keeler-spawn` leaves for a task: the verdict, the
    /// log and the runner script.
    fn runs_dir(&self) -> PathBuf {
        self.dir.join(".keeler").join("runs").join(SLUG)
    }

    fn write_run_record(&self, tid: &str) {
        let runs = self.runs_dir();
        std::fs::create_dir_all(&runs).unwrap();
        std::fs::write(runs.join(format!("{tid}.exit")), "0\n").unwrap();
        std::fs::write(runs.join(format!("{tid}.log")), "the run said things\n").unwrap();
        std::fs::write(runs.join(format!("{tid}.sh")), "#!/usr/bin/env bash\n").unwrap();
    }

    /// Every branch this repository has, by name.
    fn branches(&self) -> Vec<String> {
        self.git(&["branch", "--format=%(refname:short)"])
            .lines()
            .map(str::to_string)
            .collect()
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
        // The worktrees are siblings of the root, so removing the root
        // alone would leave them behind in the temp directory.
        let name = self.dir.file_name().unwrap().to_string_lossy().into_owned();
        if let Ok(entries) = std::fs::read_dir(self.dir.parent().unwrap()) {
            for entry in entries.flatten() {
                if entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(&format!("{name}-"))
                {
                    let _ = std::fs::remove_dir_all(entry.path());
                }
            }
        }
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
fn landing_the_last_task_marks_the_spec_implemented() {
    // Given a spec on main whose every task is ticked
    let project = Project::with_spec("last-task", &spec_with(&[true, true], "Approved"));
    let head = project.git(&["rev-parse", "HEAD"]);
    std::fs::set_permissions(project.spec_path(), std::fs::Permissions::from_mode(0o640)).unwrap();

    // When `just keeler-land` runs and the gates are green
    let output = project.land();
    assert!(output.status.success(), "{}", both(&output));

    // Then the spec's Status: is set to Implemented and staged, alongside
    // the baseline, for the same human commit
    assert!(
        project.spec().contains("**Status:** Implemented"),
        "the finished spec was not marked Implemented:\n{}",
        project.spec()
    );
    assert_eq!(
        project.staged(),
        vec!["crap-baseline.json".to_string(), format!("specs/{SLUG}.md"),],
        "the spec and the baseline are not staged together for one commit"
    );
    assert_eq!(
        project.git(&["rev-parse", "HEAD"]),
        head,
        "keeler-land committed; the human commits"
    );
    assert!(
        both(&output).contains(&format!("specs/{SLUG}.md")),
        "the run does not name the spec it finished:\n{}",
        both(&output)
    );

    // And the spec is still the file it was — same permissions, no
    // half-written copy left beside it: the mark is a move onto the spec,
    // not a truncate-then-write a stopped run could leave empty
    let mode = std::fs::metadata(project.spec_path())
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o640, "marking the spec changed its permissions");
    let left_behind: Vec<PathBuf> = std::fs::read_dir(project.path().join("specs"))
        .unwrap()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path != &project.spec_path())
        .collect();
    assert!(
        left_behind.is_empty(),
        "the mark left a temporary file behind: {left_behind:?}"
    );

    // And landing again changes nothing: the spec is already Implemented,
    // so there is no second write and no second `Approved` to replace
    let once = project.spec();
    let output = project.land();
    assert!(output.status.success(), "{}", both(&output));
    assert_eq!(
        project.spec(),
        once,
        "a second land rewrote a spec it had already finished"
    );

    // And a spec with any task unticked is left as it was
    let unfinished = Project::with_spec(
        "last-task-unfinished",
        &spec_with(&[true, false], "Approved"),
    );
    let before = unfinished.spec();
    let output = unfinished.land();
    assert!(output.status.success(), "{}", both(&output));
    assert_eq!(
        unfinished.spec(),
        before,
        "a spec with an unticked task was rewritten"
    );
    assert_eq!(unfinished.staged(), vec!["crap-baseline.json".to_string()]);
}

#[test]
fn landing_on_the_feature_branch_takes_the_ticks_and_the_worktrees() {
    // Given a feature branch where two tasks have landed — their ticks are
    // committed there and their worktrees are clean
    let project =
        Project::on_feature_branch("feature-level", &spec_with(&[true, true], "Approved"));
    let clean = project.add_worktree("t1");
    project.write_run_record("t1");
    let other = project.add_worktree("t2");
    project.write_run_record("t2");

    // When `just keeler-land` runs there
    let output = project.land();
    assert!(
        output.status.success(),
        "keeler-land refused the feature branch:\n{}",
        both(&output)
    );

    // Then the landed worktrees and branches go — that is the feature
    // level's work
    assert!(
        !clean.exists(),
        "a landed worktree survived on the feature branch:\n{}",
        both(&output)
    );
    assert!(
        !other.exists(),
        "a landed worktree survived on the feature branch:\n{}",
        both(&output)
    );
    assert!(
        !project
            .branches()
            .iter()
            .any(|b| b.starts_with(&format!("keeler/{SLUG}/"))),
        "task branches survived: {:?}",
        project.branches()
    );

    // And nothing that belongs to main happens here: the baseline is not
    // regenerated, nothing is staged, and Status: is exactly as it was.
    // The baseline is the whole team's reference and moves in one place;
    // Implemented is what main says about a feature that has arrived.
    assert!(
        !project.calls().iter().any(|c| c == "crap-baseline"),
        "the feature level regenerated the baseline:\n{}",
        both(&output)
    );
    assert_eq!(
        project.baseline(),
        OLD_BASELINE,
        "the baseline moved on the feature branch"
    );
    assert_eq!(
        project.git(&["diff", "--cached", "--name-only"]),
        "",
        "the feature level staged something:\n{}",
        both(&output)
    );
    assert!(
        project.spec().contains("**Status:** Approved"),
        "the feature level wrote Status::\n{}",
        project.spec()
    );
}

#[test]
fn landing_on_main_takes_status_and_the_baseline_and_no_worktrees() {
    // Given main, after the feature merged into it with every box ticked —
    // and a task worktree that someone left behind
    let project = Project::with_spec("main-level", &spec_with(&[true, true], "Approved"));
    let leftover = project.add_worktree("t1");

    // When `just keeler-land` runs on main
    let output = project.land();
    assert!(output.status.success(), "{}", both(&output));

    // Then Status: becomes Implemented and the baseline is staged — main's
    // work — and the worktree is left where it is, named: task worktrees
    // fan out from the feature branch, and are its to clean up
    assert!(
        project.spec().contains("**Status:** Implemented"),
        "main did not mark the finished spec Implemented:\n{}",
        project.spec()
    );
    assert_eq!(
        project.baseline(),
        NEW_BASELINE,
        "main did not regenerate the baseline"
    );
    assert!(
        leftover.exists(),
        "main removed a task worktree — that is the feature branch's work:\n{}",
        both(&output)
    );
}

#[test]
fn landing_cleans_up_only_what_is_clean() {
    // Given a landed task whose worktree has no uncommitted changes, and
    // another landed task whose worktree has some
    let project = Project::on_feature_branch("cleanup", &spec_with(&[true, true], "Approved"));
    let clean = project.add_worktree("t1");
    let dirty = project.add_worktree("t2");
    project.write_run_record("t1");
    std::fs::write(
        dirty.join("crap-baseline.json"),
        "{\"still\":\"working\"}\n",
    )
    .unwrap();

    // When `just keeler-land` finishes
    let output = project.land();
    assert!(output.status.success(), "{}", both(&output));

    // Then the worktree and the branch are removed
    assert!(
        !clean.exists(),
        "a clean landed worktree survived:\n{}",
        both(&output)
    );
    assert!(
        !project.branches().contains(&format!("keeler/{SLUG}/t1")),
        "the branch of a removed worktree survived: {:?}",
        project.branches()
    );

    // And a worktree with uncommitted changes is left in place and named,
    // for the human to look at first
    assert!(dirty.exists(), "a dirty worktree was removed");
    assert!(
        project.branches().contains(&format!("keeler/{SLUG}/t2")),
        "the branch of a dirty worktree was deleted: {:?}",
        project.branches()
    );
    assert!(
        both(&output).contains(dirty.to_str().unwrap()),
        "the run does not name the dirty worktree it left:\n{}",
        both(&output)
    );
    // And the verdict and runner of the run that finished go with the
    // worktree, while its log stays: `keeler-status` reads the verdict
    // before the graph, so a stale one has the board naming a worktree
    // this very run deleted
    let runs = project.runs_dir();
    assert!(!runs.join("t1.exit").exists(), "a stale verdict was kept");
    assert!(!runs.join("t1.sh").exists(), "a runner for a gone worktree");
    assert!(
        runs.join("t1.log").exists(),
        "the run's log was thrown away"
    );
    let board = project.just(&["keeler-status", &format!("specs/{SLUG}.md")]);
    assert!(board.status.success(), "{}", both(&board));
    assert!(
        !stdout(&board).contains(clean.to_str().unwrap()),
        "the board still names the worktree the land removed:\n{}",
        stdout(&board)
    );

    // And it says why it left it. Without the check, `git worktree remove`
    // refuses a dirty worktree by itself — the same three facts hold and
    // the human is told only that git said no, which is not "look at this
    // before anything removes it".
    assert!(
        both(&output).contains("uncommitted changes"),
        "the run does not say why the worktree was left:\n{}",
        both(&output)
    );
}

#[test]
fn a_worktree_that_will_not_go_is_named_rather_than_fatal() {
    // Given a landed task whose worktree is clean but cannot be removed —
    // git holds it locked, as it does for a removable disk
    let project = Project::on_feature_branch("locked", &spec_with(&[true, true], "Approved"));
    let locked = project.add_worktree("t1");
    let clean = project.add_worktree("t2");
    project.git(&["worktree", "lock", locked.to_str().unwrap()]);

    // When `just keeler-land` runs on main
    let output = project.land();

    // Then the land still succeeds and says so: the baseline and the spec
    // are already staged by this point, and a tidy-up that could not tidy
    // must not end the run with a raw git error over work already done
    assert!(
        output.status.success(),
        "a worktree that would not go failed the land:\n{}",
        both(&output)
    );
    assert!(
        both(&output).contains(locked.to_str().unwrap()),
        "the run does not name the worktree it could not remove:\n{}",
        both(&output)
    );

    // And the next task's worktree is still cleaned up — one that would
    // not go does not stop the rest
    assert!(!clean.exists(), "the cleanup stopped at the first refusal");
}

#[test]
fn landing_reads_the_ticks_that_are_committed() {
    // Given a spec whose last box is ticked only in the working tree —
    // /keeler:mutants ticks and, by the commit law, does not commit — and
    // a clean worktree for that task
    let project = Project::with_spec("uncommitted-tick", &spec_with(&[true, false], "Approved"));
    let in_flight = project.add_worktree("t2");
    std::fs::write(project.spec_path(), spec_with(&[true, true], "Approved")).unwrap();
    let before = project.spec();

    // When `just keeler-land` runs on main
    let output = project.land();

    // Then nothing acts on that tick: the spec is not marked, the
    // worktree is not removed, and the run says why — readiness is what
    // the repository records, not what a file happens to say right now,
    // which is the rule keeler-spawn already puts on the spec it reads
    assert!(output.status.success(), "{}", both(&output));
    assert_eq!(project.spec(), before, "an uncommitted tick was acted on");
    assert!(in_flight.exists(), "an uncommitted tick removed a worktree");
    assert!(
        both(&output).contains(&format!("specs/{SLUG}.md")),
        "the run does not name the spec it passed over:\n{}",
        both(&output)
    );

    // And only the baseline is staged: a spec with uncommitted edits would
    // otherwise sweep them into the commit the human is asked to review as
    // the baseline and the Status: line
    assert_eq!(project.staged(), vec!["crap-baseline.json".to_string()]);
}

#[test]
fn a_branch_whose_commits_are_not_on_main_keeps_its_worktree() {
    // Given two landed tasks with clean worktrees, one of whose branches
    // carries a commit that never reached main
    let project = Project::on_feature_branch("unmerged", &spec_with(&[true, true], "Approved"));
    let unmerged = project.add_worktree("t1");
    let merged = project.add_worktree("t2");
    std::fs::write(unmerged.join("notes.txt"), "work nobody merged\n").unwrap();
    Project::git_in(&unmerged, &["add", "notes.txt"]);
    Project::git_in(&unmerged, &["commit", "-qm", "work nobody merged"]);

    // When `just keeler-land` runs on main
    let output = project.land();
    assert!(output.status.success(), "{}", both(&output));

    // Then that worktree and its branch are left in place and named:
    // deleting the branch would take the commit with it, and a land is
    // not a place to lose work no one has looked at
    assert!(
        unmerged.exists(),
        "an unmerged branch's worktree was removed"
    );
    assert!(
        project.branches().contains(&format!("keeler/{SLUG}/t1")),
        "a branch with unmerged commits was deleted: {:?}",
        project.branches()
    );
    assert!(
        both(&output).contains(unmerged.to_str().unwrap()),
        "the run does not name the worktree it kept:\n{}",
        both(&output)
    );

    // And the one whose commits are on main is still cleaned up
    assert!(!merged.exists(), "the merged worktree survived");
}

#[test]
fn a_worktree_that_moved_to_another_branch_is_left_alone() {
    // Given a landed task whose worktree a human switched to a branch of
    // their own — the task's branch now stands somewhere else entirely
    let project = Project::on_feature_branch("moved", &spec_with(&[true, true], "Approved"));
    let moved = project.add_worktree("t1");
    Project::git_in(&moved, &["switch", "-q", "-c", "fixup"]);

    // When `just keeler-land` runs on main
    let output = project.land();
    assert!(output.status.success(), "{}", both(&output));

    // Then neither is touched: this is not the task's worktree any more,
    // and the branch is one this recipe never looked at
    assert!(moved.exists(), "someone else's worktree was removed");
    assert!(
        project.branches().contains(&format!("keeler/{SLUG}/t1")),
        "a branch no worktree held was deleted: {:?}",
        project.branches()
    );
    assert!(
        both(&output).contains("fixup"),
        "the run does not say what the worktree is on now:\n{}",
        both(&output)
    );
}

#[test]
fn an_unlanded_task_keeps_its_worktree() {
    // Given a task that is not ticked — work in flight, not landed —
    // whose worktree is clean because its agent commits as it goes
    let project = Project::with_spec("in-flight", &spec_with(&[true, false], "Approved"));
    let in_flight = project.add_worktree("t2");

    // When `just keeler-land` runs on main
    let output = project.land();
    assert!(output.status.success(), "{}", both(&output));

    // Then the worktree and the branch are still there: cleanup follows
    // landing, and a task the graph does not call done has not landed
    assert!(
        in_flight.exists(),
        "an unfinished task's worktree was removed:\n{}",
        both(&output)
    );
    assert!(project.branches().contains(&format!("keeler/{SLUG}/t2")));
}

#[test]
fn only_an_approved_graph_can_be_finished() {
    // Given a spec that is still a Draft, though every box is ticked
    let draft = Project::with_spec("draft", &spec_with(&[true, true], "Draft"));
    let before = draft.spec();

    // When `just keeler-land` runs on main
    let output = draft.land();

    // Then it is left as it was: Implemented follows Approved, and a spec
    // nobody approved is not a contract that can have been fulfilled
    assert!(output.status.success(), "{}", both(&output));
    assert_eq!(draft.spec(), before, "a Draft spec was marked Implemented");
    assert_eq!(draft.staged(), vec!["crap-baseline.json".to_string()]);
    // And it does not say it marked one either: a run whose only mark is
    // that the substitution found no `Approved` to replace still announces
    // a spec it finished, and would rewrite one whose Status: line merely
    // mentions the word.
    assert!(
        !both(&output).contains("is now Implemented"),
        "the run claims to have finished a Draft spec:\n{}",
        both(&output)
    );

    // And a spec with no tasks at all is left as it was too — nothing was
    // finished, because nothing was ever asked for
    let empty = Project::with_spec(
        "no-tasks",
        "# Spec 42 — fixture\n\n**Status:** Approved\n\n## Tasks\n\nNone yet.\n",
    );
    let before = empty.spec();
    let output = empty.land();
    assert!(output.status.success(), "{}", both(&output));
    assert_eq!(empty.spec(), before, "a spec with no tasks was Implemented");
    assert_eq!(empty.staged(), vec!["crap-baseline.json".to_string()]);
    assert!(
        !both(&output).contains("is now Implemented"),
        "the run claims to have finished a spec with no tasks:\n{}",
        both(&output)
    );

    // And a Status: line that merely mentions the word — the template's
    // own `Draft | Approved | Implemented` menu — is not an approval: the
    // line must *be* Approved, or the substitution rewrites the menu
    let template = Project::with_spec(
        "template-menu",
        &spec_with(&[true, true], "Draft | Approved | Implemented"),
    );
    let before = template.spec();
    let output = template.land();
    assert!(output.status.success(), "{}", both(&output));
    assert_eq!(
        template.spec(),
        before,
        "a Status: line that only mentions Approved was rewritten"
    );
    assert_eq!(template.staged(), vec!["crap-baseline.json".to_string()]);
}

#[test]
fn a_spec_that_does_not_parse_is_left_alone_and_named() {
    // Given a spec whose graph the parser refuses — a cycle — with every
    // box ticked, and a clean worktree for one of its tasks
    let project = Project::with_spec(
        "unparseable",
        "# Spec 42 — fixture\n\n**Status:** Approved\n\n## Tasks\n\n\
         - [x] **T1 — one.** Needs: T2. Scenarios: _one_.\n\
         - [x] **T2 — two.** Needs: T1. Scenarios: _two_.\n",
    );
    let worktree = project.add_worktree("t1");
    let before = project.spec();

    // When `just keeler-land` runs on main
    let output = project.land();

    // Then the baseline still lands, and the spec is left exactly as it
    // is — a graph nobody can read says nothing about what is finished —
    // and the run names it rather than passing over it in silence
    assert!(output.status.success(), "{}", both(&output));
    assert_eq!(project.staged(), vec!["crap-baseline.json".to_string()]);
    assert_eq!(project.spec(), before, "an unreadable spec was rewritten");
    assert!(
        worktree.exists(),
        "an unreadable spec's worktree was removed"
    );
    assert!(
        both(&output).contains(&format!("specs/{SLUG}.md")),
        "the run does not name the spec it could not read:\n{}",
        both(&output)
    );
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

    /// A spec is Implemented exactly when every one of its boxes is
    /// ticked — not when most are, not when the last one happens to be,
    /// and whatever the number of tasks. The rule is about the whole
    /// graph, not about the pattern one example test picked.
    #[test]
    fn a_spec_is_implemented_exactly_when_every_box_is_ticked(
        // Weighted towards ticked: an unticked box is the common case by
        // accident, and the interesting half of "exactly when" is the
        // spec that is actually finished.
        ticks in proptest::collection::vec(proptest::bool::weighted(0.75), 1..5),
    ) {
        let project = Project::with_spec("every-box", &spec_with(&ticks, "Approved"));

        let output = project.land();
        proptest::prop_assert!(output.status.success(), "{}", both(&output));

        let finished = ticks.iter().all(|ticked| *ticked);
        proptest::prop_assert_eq!(
            project.spec().contains("**Status:** Implemented"),
            finished,
            "ticks {:?} produced:\n{}",
            ticks,
            project.spec()
        );
        proptest::prop_assert_eq!(
            project.staged().contains(&format!("specs/{SLUG}.md")),
            finished,
            "ticks {:?} staged {:?}",
            ticks,
            project.staged()
        );
    }
}
