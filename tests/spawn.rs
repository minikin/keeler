//! Spec 06 — graph mode. The spawning side: `just keeler-spawn`, `just
//! keeler-status`, and the tmux requirement `install.sh` checks.
//!
//! Every test drives the shipped recipes as a subprocess, the way
//! `tests/graph.rs` drives `scripts/keeler-graph.sh` and `tests/installer.rs`
//! drives `install.sh`. `claude`, `tmux` and `just keeler-branch` are PATH
//! stubs that record what they were asked to run, so the prompt, the
//! permission flags, the session name and the worktree path are all
//! asserted without a real agent starting or a byte reaching the network.
//!
//! `just keeler-branch` is T4's deliverable and does not exist yet: the
//! stub stands in for it, which is also what lets these tests prove the
//! verdict is *its* exit code and not the agent's.

use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The fixture spec's slug — its file name without `.md`, which is the
/// only thing every name in graph mode is derived from.
const SLUG: &str = "42-fixture";

/// T1 done, T2 not; T3 ready on T1, T4 blocked on T2.
const TASKS: &str = "- [x] **T1 — the root, and it is done.** Scenarios: _one_.\n\
     - [ ] **T2 — another root, not done.** Scenarios: _two_.\n\
     - [ ] **T3 — the one that gets spawned.** Needs: T1. Scenarios: _three_.\n\
     - [ ] **T4 — the blocked one.** Needs: T2. Scenarios: _four_.\n";

/// Where a spec lives in a project, which is how the recipes are called.
fn spec_path(slug: &str) -> String {
    format!("specs/{slug}.md")
}

fn spec_body(status: &str, tasks: &str) -> String {
    format!(
        "# Spec 42 — fixture\n\n**Status:** {status}\n\n## Tasks\n\n{tasks}\n---\n\n\
         ## Implementation Notes\n\nnone.\n"
    )
}

/// The absolute path of the real `just`, resolved once against the
/// harness's own PATH. Tests invoke it by path so the stub `just` on the
/// project's PATH is only ever reached from inside a spawned session.
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

fn write_stub(path: &Path, body: &str) {
    std::fs::write(path, body).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// Records its argv — one field per `\x1f`, one call per `\x1e` — and
/// emulates enough of tmux for the recipes: `has-session` answers from
/// `KEELER_STUB_TMUX_SESSIONS` with tmux's own exact-then-prefix matching,
/// and `new-session` runs the command it was given only when the test asks
/// it to.
const TMUX_STUB: &str = r#"#!/usr/bin/env bash
{ for a in "$@"; do printf '%s\037' "$a"; done; printf '\036'; } >> "$KEELER_STUB_TMUX_LOG"
case "${1:-}" in
has-session)
    want=""
    while [ $# -gt 0 ]; do [ "$1" = -t ] && want="${2:-}"; shift; done
    exact=0
    case "$want" in =*) exact=1; want="${want#=}" ;; esac
    for s in ${KEELER_STUB_TMUX_SESSIONS:-}; do
        [ "$s" = "$want" ] && exit 0
        if [ "$exact" = 0 ]; then case "$s" in "$want"*) exit 0 ;; esac; fi
    done
    exit 1
    ;;
new-session)
    dir=""; cmd=""; prev=""
    for a in "$@"; do [ "$prev" = -c ] && dir="$a"; prev="$a"; cmd="$a"; done
    if [ "${KEELER_STUB_TMUX_RUN:-0}" = 1 ]; then ( cd "${dir:-.}" && eval "$cmd" ) || true; fi
    exit 0
    ;;
esac
exit 0
"#;

/// Exits zero however the turn went — which is exactly why the verdict
/// cannot be its exit code.
const CLAUDE_STUB: &str = r#"#!/usr/bin/env bash
{ for a in "$@"; do printf '%s\037' "$a"; done; printf '\036'; } >> "$KEELER_STUB_CLAUDE_LOG"
echo "claude stub: the turn ended in FAIL"
exit 0
"#;

/// Stands in for T4's `just keeler-branch`; everything else is the real
/// `just`, so a session that runs `just` for any other reason still works.
const JUST_STUB: &str = r#"#!/usr/bin/env bash
if [ "${1:-}" = keeler-branch ]; then
    echo "keeler-branch stub: the gate ran"
    exit "${KEELER_STUB_BRANCH_EXIT:-0}"
fi
exec "$KEELER_REAL_JUST" "$@"
"#;

/// A throwaway project holding the shipped `Justfile`, the graph script and
/// one spec, with the stubs on its `bin/`. Removed on drop, together with
/// the sibling worktrees a spawn creates.
struct Project {
    dir: PathBuf,
    /// Session names `tmux has-session` should answer yes to.
    sessions: String,
    /// Whether the stub tmux actually runs the command it is handed.
    run_sessions: bool,
    /// The exit code the stub `just keeler-branch` reports.
    branch_exit: i32,
}

impl Project {
    fn new(name: &str) -> Self {
        Self::with_spec(name, SLUG, &spec_body("Approved", TASKS))
    }

    fn with_spec(name: &str, slug: &str, body: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("keeler-spawn-{name}-{}", std::process::id()));
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
        std::fs::write(dir.join("specs").join(format!("{slug}.md")), body).unwrap();
        std::fs::write(
            dir.join(".gitignore"),
            "/bin/\n/.keeler/\n/tmux-calls\n/claude-calls\n",
        )
        .unwrap();
        for (name, body) in [
            ("tmux", TMUX_STUB),
            ("claude", CLAUDE_STUB),
            ("just", JUST_STUB),
        ] {
            write_stub(&dir.join("bin").join(name), body);
        }
        // Resolved, because `git rev-parse --show-toplevel` resolves too —
        // on macOS /var is a symlink to /private/var, and an unresolved
        // path would compare unequal to every path the recipes print.
        let dir = std::fs::canonicalize(&dir).unwrap();
        let project = Self {
            dir,
            sessions: String::new(),
            run_sessions: false,
            branch_exit: 0,
        };
        project.git(&["init", "-qb", "main"]);
        project.git(&["add", "-A"]);
        project.git(&["commit", "-qm", "fixture"]);
        project
    }

    fn path(&self) -> &Path {
        &self.dir
    }

    /// The worktree path the spec says a task gets: a sibling of the
    /// repository root, named for the repository, the slug and the
    /// lowercased id.
    fn worktree(&self, slug: &str, task: &str) -> PathBuf {
        let root = self.dir.file_name().unwrap().to_string_lossy().into_owned();
        self.dir
            .parent()
            .unwrap()
            .join(format!("{root}-{slug}-{}", task.to_lowercase()))
    }

    fn runs(&self, slug: &str) -> PathBuf {
        self.dir.join(".keeler/runs").join(slug)
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

    /// Runs a recipe with the stubs first on PATH.
    fn just(&self, args: &[&str]) -> Output {
        self.just_with_path(args, &{
            let path = std::env::var("PATH").unwrap();
            format!("{}:{path}", self.dir.join("bin").display())
        })
    }

    fn just_with_path(&self, args: &[&str], path: &str) -> Output {
        Command::new(real_just())
            .args(args)
            .current_dir(&self.dir)
            .env("PATH", path)
            .env("KEELER_STUB_TMUX_LOG", self.dir.join("tmux-calls"))
            .env("KEELER_STUB_CLAUDE_LOG", self.dir.join("claude-calls"))
            .env("KEELER_STUB_TMUX_SESSIONS", &self.sessions)
            .env(
                "KEELER_STUB_TMUX_RUN",
                if self.run_sessions { "1" } else { "0" },
            )
            .env("KEELER_STUB_BRANCH_EXIT", self.branch_exit.to_string())
            .env("KEELER_REAL_JUST", real_just())
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .output()
            .expect("failed to run just")
    }

    fn spawn(&self, slug: &str, task: &str) -> Output {
        self.just(&["keeler-spawn", &spec_path(slug), task])
    }

    fn status(&self, slug: &str) -> Output {
        self.just(&["keeler-status", &spec_path(slug)])
    }

    /// Every recorded call to a stub, argv by argv.
    fn calls(&self, stub: &str) -> Vec<Vec<String>> {
        let raw =
            std::fs::read_to_string(self.dir.join(format!("{stub}-calls"))).unwrap_or_default();
        raw.split('\u{1e}')
            .filter(|record| !record.is_empty())
            .map(|record| {
                record
                    .split('\u{1f}')
                    .filter(|field| !field.is_empty())
                    .map(str::to_string)
                    .collect()
            })
            .collect()
    }

    fn new_sessions(&self) -> Vec<Vec<String>> {
        self.calls("tmux")
            .into_iter()
            .filter(|call| call.first().is_some_and(|verb| verb == "new-session"))
            .collect()
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        remove_with_siblings(&self.dir);
    }
}

/// A spawn puts worktrees *beside* the project, so cleaning up the project
/// alone would leave them behind.
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
        }
    }
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn both(output: &Output) -> String {
    format!("{}{}", stdout(output), stderr(output))
}

#[test]
fn spawning_a_task_creates_an_isolated_agent() {
    // Given a spec with an unblocked task T3, and tmux on PATH
    let project = Project::new("isolated");
    let before = project.git(&["rev-parse", "HEAD"]);

    // When `just keeler-spawn <spec> T3` runs
    let output = project.spawn(SLUG, "T3");
    assert!(output.status.success(), "{}", both(&output));

    // Then a worktree exists outside the repository root, on the branch
    // named for the spec and the lowercased task id
    let worktree = project.worktree(SLUG, "T3");
    assert!(worktree.is_dir(), "no worktree at {}", worktree.display());
    assert!(
        !worktree.starts_with(project.path()),
        "the worktree is inside the repository: {}",
        worktree.display()
    );
    let branch = Command::new("git")
        .args(["symbolic-ref", "--short", "HEAD"])
        .current_dir(&worktree)
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&branch.stdout).trim(),
        format!("keeler/{SLUG}/t3"),
    );

    // And a detached tmux session named for the spec and the task is
    // running the agent in that worktree
    let sessions = project.new_sessions();
    assert_eq!(sessions.len(), 1, "expected one session: {sessions:?}");
    let call = &sessions[0];
    assert!(call.contains(&"-d".to_string()), "not detached: {call:?}");
    assert_eq!(
        call[call.iter().position(|a| a == "-s").unwrap() + 1],
        format!("keeler-{SLUG}-t3")
    );
    assert_eq!(
        Path::new(&call[call.iter().position(|a| a == "-c").unwrap() + 1]),
        worktree,
    );

    // And what the session was asked to run is a Claude Code session whose
    // prompt names the spec, the task and the whole per-task pipeline, with
    // `just keeler-branch` as its gate
    let ran = session_script(&project, call);
    for token in [
        "claude -p",
        "specs/42-fixture.md",
        "T3",
        "/keeler:tdd",
        "/keeler:qa",
        "/keeler:review",
        "/keeler:mutants",
        "just keeler-branch",
        "--permission-mode acceptEdits",
        "--allowedTools",
        "Bash(cargo:",
        "Bash(just:",
        "Bash(git:",
    ] {
        assert!(
            ran.contains(token),
            "the session never mentions `{token}`:\n{ran}"
        );
    }

    // And the recipe returns at once, printing how to attach
    let said = stdout(&output);
    assert!(
        said.contains(&format!("tmux attach -t keeler-{SLUG}-t3")),
        "the recipe does not say how to attach:\n{said}"
    );

    // And the main working tree is untouched
    assert_eq!(project.git(&["rev-parse", "HEAD"]), before);
    assert_eq!(project.git(&["symbolic-ref", "--short", "HEAD"]), "main");
    assert_eq!(project.git(&["status", "--porcelain"]), "");
}

/// What the session runs: the last argument tmux was handed, plus the
/// script it points at when it points at one — a session asked to run a
/// runner script was asked to run everything in it.
fn session_script(project: &Project, call: &[String]) -> String {
    let command = call.last().expect("tmux was given no command").clone();
    let path = command.split_whitespace().last().unwrap_or_default();
    let path = PathBuf::from(path.replace("\\ ", " "));
    let body = if path.is_absolute() {
        std::fs::read_to_string(&path).unwrap_or_default()
    } else {
        std::fs::read_to_string(project.path().join(&path)).unwrap_or_default()
    };
    format!("{command}\n{body}")
}

#[test]
fn a_spawned_agent_commits_on_its_branch_and_nowhere_else() {
    // Given a task spawned by `just keeler-spawn`
    let project = Project::new("commits");
    let output = project.spawn(SLUG, "T3");
    assert!(output.status.success(), "{}", both(&output));
    let ran = session_script(&project, &project.new_sessions()[0]);

    // Then the agent is told to commit on its branch, in that worktree, as
    // each stage finishes — so the review record has a commit to name
    let lower = ran.to_lowercase();
    assert!(
        lower.contains("commit"),
        "the prompt never mentions committing:\n{ran}"
    );
    assert!(
        ran.contains(&format!("keeler/{SLUG}/t3")),
        "the prompt never names the branch to commit on:\n{ran}"
    );

    // And nothing is pushed: the branch reaches the remote by hand only
    assert!(
        lower.contains("never push") || lower.contains("do not push"),
        "the prompt does not forbid pushing:\n{ran}"
    );

    // And the shipped rules say this is the one place an agent commits
    // without asking, and why the spawn was the asking
    let rules = std::fs::read_to_string(repo_root().join(".claude/keeler.md")).unwrap();
    let commits = section(&rules, "## Commits");
    for token in ["keeler-spawn", "keeler/", "push"] {
        assert!(
            commits.contains(token),
            "the Commits section does not carry the carve-out's `{token}`:\n{commits}"
        );
    }
    assert!(
        commits.to_lowercase().contains("without asking"),
        "the Commits section does not say the spawn is the asking:\n{commits}"
    );
}

/// One `## ` section of a markdown file, heading included.
fn section<'a>(text: &'a str, heading: &str) -> &'a str {
    let start = text.find(heading).expect("no such heading");
    let rest = &text[start + heading.len()..];
    let end = rest.find("\n## ").map_or(rest.len(), |at| at + 1);
    &text[start..start + heading.len() + end]
}

#[test]
fn a_finished_agent_leaves_a_verdict_the_gate_decided_and_a_log() {
    for (code, expected) in [(0, "passed"), (3, "failed")] {
        // Given a spawned session that has run and exited
        let mut project = Project::new(&format!("verdict-{code}"));
        project.run_sessions = true;
        project.branch_exit = code;
        let output = project.spawn(SLUG, "T3");
        assert!(output.status.success(), "{}", both(&output));

        // Then the verdict is the exit code of `just keeler-branch`, run
        // after the agent — not the agent's own, which is zero either way
        let verdict = std::fs::read_to_string(project.runs(SLUG).join("t3.exit"))
            .unwrap_or_else(|e| panic!("no verdict was written: {e}"));
        assert_eq!(
            verdict.trim(),
            code.to_string(),
            "the verdict is not the gate's"
        );
        assert!(
            !project.calls("claude").is_empty(),
            "the agent was never run, so the verdict means nothing"
        );

        // And the log holds everything the session printed
        let log = std::fs::read_to_string(project.runs(SLUG).join("t3.log")).unwrap();
        assert!(
            log.contains("the turn ended in FAIL"),
            "the agent's output is missing:\n{log}"
        );
        assert!(
            log.contains("the gate ran"),
            "the gate's output is missing:\n{log}"
        );

        // And status lists the task by that verdict
        let listed = stdout(&project.status(SLUG));
        let line = task_line(&listed, "T3");
        assert!(line.contains(expected), "T3 is not {expected}:\n{listed}");

        // And "running" is decided by asking tmux, not by the absence of a
        // file: the same finished run, with its session still alive, reads
        // as running
        project.sessions = format!("keeler-{SLUG}-t3");
        let listed = stdout(&project.status(SLUG));
        assert!(
            task_line(&listed, "T3").contains("running"),
            "status ignored tmux and read the verdict file:\n{listed}"
        );
    }
}

/// The status line for one task id.
fn task_line<'a>(listed: &'a str, id: &str) -> &'a str {
    listed
        .lines()
        .find(|line| line.split_whitespace().next() == Some(id))
        .unwrap_or_else(|| panic!("status never mentions {id}:\n{listed}"))
}

#[test]
fn a_dead_session_is_resumable_and_says_so() {
    // Given a spawned session that died before its pipeline finished — so
    // the gate never ran and no verdict was ever written
    let mut project = Project::new("dead");
    let output = project.spawn(SLUG, "T3");
    assert!(output.status.success(), "{}", both(&output));
    assert!(
        !project.runs(SLUG).join("t3.exit").exists(),
        "the fixture wrote a verdict; it is not a dead session"
    );

    // When `just keeler-status <spec>` runs
    let listed = stdout(&project.status(SLUG));

    // Then it distinguishes a task that died mid-pipeline from one that
    // failed its gate
    let line = task_line(&listed, "T3");
    assert!(
        line.contains("died"),
        "a dead session is not named as one:\n{listed}"
    );
    assert!(
        !line.contains("failed"),
        "a dead session reads as a failed gate:\n{listed}"
    );
    assert!(
        !line.contains("passed"),
        "a dead session reads as a pass:\n{listed}"
    );

    // And it names the log and the worktree, which together are what a
    // resume reads
    assert!(
        line.contains(&project.runs(SLUG).join("t3.log").display().to_string()),
        "the log is not named:\n{listed}"
    );
    assert!(
        line.contains(&project.worktree(SLUG, "T3").display().to_string()),
        "the worktree is not named:\n{listed}"
    );

    // And a task nobody spawned is neither of those things
    assert!(
        task_line(&listed, "T2").contains("not spawned"),
        "an unspawned task is reported as a run:\n{listed}"
    );

    // And tmux is asked about this session and no other: a longer name
    // that merely starts with it is a different session
    project.sessions = format!("keeler-{SLUG}-t3-something-else");
    let listed = stdout(&project.status(SLUG));
    assert!(
        task_line(&listed, "T3").contains("died"),
        "status matched another session by prefix:\n{listed}"
    );
}

#[test]
fn spawning_without_tmux_is_refused_and_says_how_to_get_it() {
    // Given a machine without tmux — a PATH that holds nothing but the
    // shell the recipe runs in
    let project = Project::new("no-tmux");
    let bare = project.path().join("nobin");
    std::fs::create_dir_all(&bare).unwrap();
    std::os::unix::fs::symlink("/bin/bash", bare.join("bash")).unwrap();

    // When `just keeler-spawn <spec> T3` runs
    let output = project.just_with_path(
        &["keeler-spawn", &spec_path(SLUG), "T3"],
        &bare.display().to_string(),
    );

    // Then it refuses before creating anything, naming tmux and how to
    // install it
    assert!(
        !output.status.success(),
        "a machine without tmux spawned:\n{}",
        both(&output)
    );
    let said = both(&output);
    assert!(
        said.contains("tmux"),
        "the refusal does not name tmux:\n{said}"
    );
    assert!(
        said.contains("brew install tmux") || said.contains("apt-get install tmux"),
        "the refusal does not say how to get tmux:\n{said}"
    );
    assert!(
        !project.worktree(SLUG, "T3").exists(),
        "a worktree was created"
    );
    assert!(
        !project.path().join(".keeler").exists(),
        "a run directory was created"
    );
}

#[test]
fn spawning_from_an_uncommitted_or_unapproved_spec_is_refused() {
    // Given three specs no worktree could build from
    let approved = spec_body("Approved", TASKS);

    // (a) a working-tree copy that differs from HEAD
    let modified = Project::new("uncommitted");
    std::fs::write(
        modified.path().join(format!("specs/{SLUG}.md")),
        format!("{approved}\nan uncommitted edit.\n"),
    )
    .unwrap();

    // (b) a committed spec whose Status: is not Approved
    let draft = Project::with_spec("unapproved", SLUG, &spec_body("Draft", TASKS));

    // (c) a spec that was never committed at all
    let fresh = Project::new("uncommitted-new");
    std::fs::write(fresh.path().join("specs/43-fresh.md"), &approved).unwrap();

    for (project, slug, expected) in [
        (&modified, SLUG, "HEAD"),
        (&draft, SLUG, "Approved"),
        (&fresh, "43-fresh", "HEAD"),
    ] {
        // When `just keeler-spawn <spec> T3` runs
        let output = project.spawn(slug, "T3");

        // Then it refuses before creating anything, saying which
        let said = both(&output);
        assert!(!output.status.success(), "the spawn was allowed:\n{said}");
        assert!(
            said.contains(expected),
            "the refusal does not say `{expected}`:\n{said}"
        );
        assert!(
            project.new_sessions().is_empty(),
            "a session was started:\n{said}"
        );
        assert!(
            !project.worktree(slug, "T3").exists(),
            "a worktree was created"
        );
        assert_eq!(
            project.git(&["branch", "--list", &format!("keeler/{slug}/t3")]),
            ""
        );
    }
}

#[test]
fn spawning_a_task_that_is_already_spawned_is_refused() {
    // Given a worktree for T3 that already exists
    let project = Project::new("already");
    assert!(project.spawn(SLUG, "T3").status.success());
    let worktree = project.worktree(SLUG, "T3");
    let head = project.git(&["rev-parse", "HEAD"]);

    // When `just keeler-spawn <spec> T3` runs again
    let output = project.spawn(SLUG, "T3");

    // Then it refuses naming the path, and creates nothing
    let said = both(&output);
    assert!(
        !output.status.success(),
        "a second spawn was allowed:\n{said}"
    );
    assert!(
        said.contains(&worktree.display().to_string()),
        "the refusal does not name the worktree:\n{said}"
    );
    assert_eq!(
        project.new_sessions().len(),
        1,
        "a second session was started"
    );
    assert_eq!(project.git(&["rev-parse", "HEAD"]), head);
}

#[test]
fn spawning_a_blocked_task_is_refused() {
    // Given a spec where T4 needs the unfinished T2
    let project = Project::new("blocked");

    // When `just keeler-spawn <spec> T4` runs
    let output = project.spawn(SLUG, "T4");

    // Then it refuses, naming the unmet dependency
    let said = both(&output);
    assert!(!output.status.success(), "a blocked task spawned:\n{said}");
    assert!(
        said.contains("T4"),
        "the refusal does not name the task:\n{said}"
    );
    assert!(
        said.contains("T2"),
        "the refusal does not name the unmet need:\n{said}"
    );

    // And no worktree or branch is created
    assert!(
        !project.worktree(SLUG, "T4").exists(),
        "a worktree was created"
    );
    assert_eq!(
        project.git(&["branch", "--list", &format!("keeler/{SLUG}/t4")]),
        ""
    );
    assert!(project.new_sessions().is_empty(), "a session was started");

    // And a task the spec does not define cannot be spawned either — there
    // is no line to read its needs from
    let output = project.spawn(SLUG, "T9");
    let said = both(&output);
    assert!(
        !output.status.success(),
        "an undefined task spawned:\n{said}"
    );
    assert!(
        said.contains("T9"),
        "the refusal does not name the task:\n{said}"
    );
}

#[test]
fn the_installer_checks_tmux_the_way_it_checks_just() {
    // Given the tmux requirement check lifted out of install.sh, the way
    // the review-record tests lift the check out of the workflow
    let check = tmux_check();
    let project = Project::new("installer-tmux");

    // When it runs on a machine with tmux
    let with = run_check(&check, &project.path().join("bin").display().to_string());

    // Then it reports it as present
    assert!(
        with.contains("tmux"),
        "the check never mentions tmux:\n{with}"
    );
    assert!(
        with.contains('✓'),
        "a present tmux is not reported as ok:\n{with}"
    );

    // When it runs on a machine without one — PATH empty, so `command -v`
    // finds nothing
    let without = run_check(&check, "");

    // Then it says tmux is missing and how to install it
    assert!(
        without.contains("tmux"),
        "the check never names tmux:\n{without}"
    );
    assert!(
        without.contains("brew install tmux") && without.contains("apt-get install tmux"),
        "the check does not say how to get tmux:\n{without}"
    );
    assert!(
        without.contains("keeler-spawn"),
        "the check does not say what needs tmux:\n{without}"
    );
}

/// The `if command -v tmux` block of `install.sh`, with the `ok`/`note`
/// helpers it calls, so the shipped text is what the test runs.
fn tmux_check() -> String {
    let installer = std::fs::read_to_string(repo_root().join("install.sh")).unwrap();
    let mut lines = installer
        .lines()
        .skip_while(|line| !line.trim_start().starts_with("if command -v tmux"));
    let first = lines.next().expect("install.sh no longer checks for tmux");
    let mut block = vec![first.trim_start()];
    for line in lines {
        block.push(line.trim_start());
        if line.trim() == "fi" {
            break;
        }
    }
    let helpers = "ok()  { printf '  ✓ %s\\n' \"$*\"; }\nnote(){ printf '  · %s\\n' \"$*\"; }\n";
    format!("{helpers}{}\n", block.join("\n"))
}

fn run_check(script: &str, path: &str) -> String {
    // By absolute path: the whole point of one of these runs is a PATH
    // that finds nothing, and that includes the shell.
    let output = Command::new("/bin/bash")
        .args(["-c", script])
        .env("PATH", path)
        .output()
        .expect("failed to run the tmux check");
    both(&output)
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig {
        cases: 12,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::WithSource("proptest-regressions"),
        )),
        ..proptest::prelude::ProptestConfig::default()
    })]

    /// One lowercased id in four places: whatever the spec is called and
    /// whatever the task is numbered, the branch, the session, the
    /// worktree and the run files all spell the same `<slug>` and the same
    /// lowercase `<id>`. Nothing is slugged from a title, and nothing
    /// lowercases twice or not at all.
    #[test]
    fn one_lowercased_id_in_four_places(
        number in 1usize..40,
        slug in "[0-9]{2}-[a-z][a-z-]{0,8}[a-z]",
    ) {
        let id = format!("T{number}");
        let lower = id.to_lowercase();
        let tasks = format!("- [ ] **{id} — the only task.** Scenarios: _one_.\n");
        let project = Project::with_spec(
            &format!("naming-{slug}-{lower}"), &slug, &spec_body("Approved", &tasks),
        );

        let output = project.spawn(&slug, &id);
        proptest::prop_assert!(output.status.success(), "{}", both(&output));

        let worktree = project.worktree(&slug, &id);
        proptest::prop_assert!(worktree.is_dir(), "no worktree at {}", worktree.display());
        let branch = Command::new("git")
            .args(["symbolic-ref", "--short", "HEAD"])
            .current_dir(&worktree)
            .output()
            .unwrap();
        let on = String::from_utf8_lossy(&branch.stdout);
        proptest::prop_assert_eq!(on.trim(), format!("keeler/{slug}/{lower}"));
        let sessions = project.new_sessions();
        proptest::prop_assert_eq!(sessions.len(), 1);
        let named = sessions[0].iter().position(|a| a == "-s").unwrap() + 1;
        proptest::prop_assert_eq!(&sessions[0][named], &format!("keeler-{slug}-{lower}"));
        proptest::prop_assert!(
            project.runs(&slug).join(format!("{lower}.log")).parent().unwrap().is_dir(),
            "no run directory for {}", slug,
        );
    }
}
