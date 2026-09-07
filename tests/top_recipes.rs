//! Spec 10 — keeler-top. The shell the board leans on: the `paused`
//! marker `keeler-status` reads and `keeler-resume` clears.
//!
//! The board itself is a crate with its own suite; these scenarios are
//! recipes, so they live here beside the other recipe suites and drive
//! the shipped `Justfile` as a subprocess, the way `tests/spawn.rs` does.
//! `tmux` is a PATH stub — no session starts and no agent runs, so the
//! states the board reports are built by the real recipes rather than
//! described to them.

use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;

// ── T8

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The fixture spec's slug — its file name without `.md`, which is what
/// every name in graph mode is derived from.
const SLUG: &str = "42-fixture";

/// Two roots and nothing else: T1 is the one that gets spawned and
/// paused, T2 the one still waiting to be — so a wave that offers the
/// paused task has T2 to be told apart from.
const TASKS: &str = "- [ ] **T1 — the one that gets paused.** Scenarios: _one_.\n\
     - [ ] **T2 — the one still to spawn.** Scenarios: _two_.\n";

fn spec_path() -> String {
    format!("specs/{SLUG}.md")
}

/// The absolute path of the real `just`, resolved once against the
/// harness's own PATH — a fixture's PATH carries the stub tmux, and
/// `just` itself must still be the real one.
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

/// Every session this suite builds has already ended — a pause is a
/// session that is gone — so `has-session` answers no and nothing the
/// stub is handed is ever run. `new-session` is the one call whose
/// success matters: the marker's removal is meant to follow it.
const TMUX_STUB: &str = r#"#!/usr/bin/env bash
case "${1:-}" in
has-session)
    exit 1
    ;;
new-session)
    if [ "${KEELER_STUB_TMUX_FAIL:-0}" = 1 ]; then
        echo "tmux stub: no server running on /tmp/tmux-501/default" >&2
        exit 1
    fi
    exit 0
    ;;
esac
exit 0
"#;

/// A throwaway project holding the shipped `Justfile`, the graph script
/// and one spec, checked out on the feature branch with the stub tmux on
/// its `bin/`. Removed on drop, together with the sibling worktrees a
/// spawn creates.
struct Project {
    dir: PathBuf,
    /// Whether the stub tmux refuses to start a session.
    tmux_fails: bool,
}

impl Project {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("keeler-top-{name}-{}", std::process::id()));
        remove_with_siblings(&dir);
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        std::fs::create_dir_all(dir.join("scripts")).unwrap();
        std::fs::create_dir_all(dir.join("specs")).unwrap();
        std::fs::copy(repo_root().join("Justfile"), dir.join("Justfile")).unwrap();
        std::fs::copy(
            repo_root().join("scripts/keeler-graph.sh"),
            dir.join("scripts/keeler-graph.sh"),
        )
        .unwrap();
        std::fs::write(
            dir.join("specs").join(format!("{SLUG}.md")),
            format!(
                "# Spec 42 — fixture\n\n**Status:** Approved\n\n## Tasks\n\n{TASKS}\n---\n\n\
                 ## Implementation Notes\n\nnone.\n"
            ),
        )
        .unwrap();
        std::fs::write(dir.join(".gitignore"), "/bin/\n/.keeler/\n").unwrap();
        let stub = dir.join("bin/tmux");
        std::fs::write(&stub, TMUX_STUB).unwrap();
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
        // Resolved, because `git rev-parse --show-toplevel` resolves too —
        // on macOS /var is a symlink to /private/var, and an unresolved
        // path would compare unequal to every path the recipes print.
        let dir = std::fs::canonicalize(&dir).unwrap();
        let project = Self {
            dir,
            tmux_fails: false,
        };
        project.git(&["init", "-qb", "main"]);
        project.git(&["add", "-A"]);
        project.git(&["commit", "-qm", "fixture"]);
        // A feature is developed on its own branch, and that is where its
        // tasks fan out from — so that is where the fixture stands.
        project.git(&["checkout", "-qb", &format!("feat/{SLUG}")]);
        project
    }

    fn git(&self, args: &[&str]) -> String {
        let output = Command::new("git")
            .args(["-c", "user.email=probe@keeler", "-c", "user.name=probe"])
            .args(args)
            .current_dir(&self.dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .output()
            .expect("failed to run git");
        assert!(
            output.status.success(),
            "git {args:?} failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    /// Runs a recipe with the stub tmux first on PATH.
    fn just(&self, args: &[&str]) -> Output {
        self.just_with_env(args, &[])
    }

    /// The one place a recipe is run, so a caller that needs one more
    /// variable cannot quietly lose the fixture's own.
    fn just_with_env(&self, args: &[&str], extra: &[(&str, &str)]) -> Output {
        let path = std::env::var("PATH").unwrap();
        let mut command = Command::new(real_just());
        command
            .args(args)
            .current_dir(&self.dir)
            .env("PATH", format!("{}:{path}", self.dir.join("bin").display()))
            .env(
                "KEELER_STUB_TMUX_FAIL",
                if self.tmux_fails { "1" } else { "0" },
            )
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null");
        for (key, value) in extra {
            command.env(key, value);
        }
        command.output().expect("failed to run just")
    }

    fn spawn(&self, task: &str) -> Output {
        self.just(&["keeler-spawn", &spec_path(), task])
    }

    fn status(&self) -> Output {
        self.just(&["keeler-status", &spec_path()])
    }

    /// The wave, with the one question answered in advance — `no`,
    /// because what is under test is which tasks it offers and not what
    /// spawning them does.
    fn fan_out_answering_no(&self) -> Output {
        self.just_with_env(
            &["keeler-fan-out", &spec_path()],
            &[("KEELER_FAN_OUT_YES", "no")],
        )
    }

    fn runs(&self) -> PathBuf {
        self.dir.join(".keeler/runs").join(SLUG)
    }

    /// The marker the board's `p` writes once its kill has succeeded.
    fn marker(&self, tid: &str) -> PathBuf {
        self.runs().join(format!("{tid}.paused"))
    }

    fn pause(&self, tid: &str) {
        std::fs::write(self.marker(tid), "").unwrap();
    }

    /// The worktree path the spec says a task gets: a sibling of the
    /// repository root, named for the repository, the slug and the id.
    fn worktree(&self, tid: &str) -> PathBuf {
        let root = self.dir.file_name().unwrap().to_string_lossy().into_owned();
        self.dir
            .parent()
            .unwrap()
            .join(format!("{root}-{SLUG}-{tid}"))
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        remove_with_siblings(&self.dir);
    }
}

/// A spawn puts worktrees *beside* the project, so cleaning up the
/// project alone would leave them behind.
fn remove_with_siblings(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir);
    let (Some(parent), Some(name)) = (dir.parent(), dir.file_name()) else {
        return;
    };
    let prefix = format!("{}-", name.to_string_lossy());
    let Ok(entries) = std::fs::read_dir(parent) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.file_name().to_string_lossy().starts_with(&prefix) {
            let _ = std::fs::remove_dir_all(entry.path());
            let _ = std::fs::remove_file(entry.path());
        }
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

/// The board's line for one task id.
fn task_line<'a>(listed: &'a str, id: &str) -> &'a str {
    listed
        .lines()
        .find(|line| line.split_whitespace().next() == Some(id))
        .unwrap_or_else(|| panic!("the board never mentions {id}:\n{listed}"))
}

#[test]
fn keeler_status_reports_paused_when_the_marker_is_there() {
    // Given a spawned task whose session is gone with no verdict — which
    // is `died` and nothing else today — and the marker the board writes
    // after its kill succeeds
    let project = Project::new("paused");
    let spawned = project.spawn("T1");
    assert!(spawned.status.success(), "{}", both(&spawned));
    assert!(
        !project.runs().join("t1.exit").exists(),
        "the fixture wrote a verdict; this is not a session that ended early"
    );
    assert!(
        task_line(&stdout(&project.status()), "T1").contains("died"),
        "the fixture does not start from a death:\n{}",
        stdout(&project.status())
    );
    project.pause("t1");

    // When the board is read
    let listed = stdout(&project.status());
    let line = task_line(&listed, "T1");

    // Then the marker settles what a kill by hand and a crash cannot be
    // told apart by: the row says paused rather than died.
    assert!(line.contains("paused"), "the marker is not read:\n{listed}");
    assert!(
        !line.contains("died"),
        "a paused task still reads as a death:\n{listed}"
    );

    // And it carries the log and the worktree died carries — a pause is
    // resumed from exactly what a death is resumed from
    assert!(
        line.contains(&project.runs().join("t1.log").display().to_string()),
        "the log is not named:\n{listed}"
    );
    assert!(
        line.contains(&project.worktree("t1").display().to_string()),
        "the worktree is not named:\n{listed}"
    );
    assert!(
        listed.contains(&format!("keeler-resume {} T1", spec_path())),
        "the resume hint died carries is missing:\n{listed}"
    );

    // And a second hint names the rm that clears it. A marker nobody
    // resumes — the human removed the worktree by hand — would otherwise
    // say paused for ever, with no path out the tool ever mentions.
    assert!(
        listed.contains(&format!("rm {}", project.marker("t1").display())),
        "the board does not say how to clear a marker nobody will resume:\n{listed}"
    );

    // And without the marker the same run is a death again: the word is
    // the marker's, not the recipe's opinion of an ended session
    std::fs::remove_file(project.marker("t1")).unwrap();
    let listed = stdout(&project.status());
    assert!(
        task_line(&listed, "T1").contains("died"),
        "a kill that bypassed the board is not still a death:\n{listed}"
    );
}

#[test]
fn a_paused_task_is_not_re_spawned_by_the_wave() {
    // Given a paused task, and another that is ready and never spawned
    let project = Project::new("wave");
    assert!(project.spawn("T1").status.success());
    project.pause("t1");

    // When the wave is named
    let output = project.fan_out_answering_no();
    let said = both(&output);

    // Then the paused task is not in it. The wave is the ready tasks the
    // board calls "not spawned", and a pause is not one of those: it is a
    // run standing in a worktree with commits on its branch, and spawning
    // it again would cut a second branch over the first.
    let wave = said
        .lines()
        .find(|line| line.starts_with("wave:"))
        .unwrap_or_else(|| panic!("the wave was never named:\n{said}"));
    assert!(
        wave.contains("T2") && !wave.contains("T1"),
        "the wave offers a paused task:\n{said}"
    );

    // And it is listed in the board's own word, so the reason it was
    // skipped is on the screen beside it
    assert!(
        task_line(&said, "T1").contains("paused"),
        "the wave does not say why T1 was skipped:\n{said}"
    );
}

#[test]
fn keeler_resume_removes_the_marker_once_the_session_is_started() {
    // Given a paused task, resumable in the worktree and branch it has
    let mut project = Project::new("resume");
    assert!(project.spawn("T1").status.success());
    project.pause("t1");

    // When it is resumed
    let output = project.just(&["keeler-resume", &spec_path(), "T1"]);
    assert!(
        output.status.success(),
        "a paused task would not resume:\n{}",
        both(&output)
    );

    // Then the marker is gone: the human paused this run and has now
    // restarted it, and a marker left behind would have the board report
    // paused about a session that is running.
    assert!(
        !project.marker("t1").exists(),
        "the resume left the marker behind:\n{}",
        both(&output)
    );

    // And a resume that never started a session leaves it. The marker is
    // removed after `tmux new-session` returns, not before — clearing it
    // first would turn a failed resume into a task the board calls died,
    // which is the one thing the marker exists to deny.
    project.pause("t1");
    project.tmux_fails = true;
    let output = project.just(&["keeler-resume", &spec_path(), "T1"]);
    assert!(
        !output.status.success(),
        "a resume whose tmux failed reported success:\n{}",
        both(&output)
    );
    assert!(
        project.marker("t1").exists(),
        "a resume that started nothing cleared the marker:\n{}",
        both(&output)
    );
}
