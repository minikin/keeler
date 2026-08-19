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

/// T1 done, T2 not; T3 ready on T1, T4 blocked on T2, and T5 blocked on
/// one need of two — the case that tells "waiting on what is unmet" apart
/// from "waiting on everything it declares".
const TASKS: &str = "- [x] **T1 — the root, and it is done.** Scenarios: _one_.\n\
     - [ ] **T2 — another root, not done.** Scenarios: _two_.\n\
     - [ ] **T3 — the one that gets spawned.** Needs: T1. Scenarios: _three_.\n\
     - [ ] **T4 — the blocked one.** Needs: T2. Scenarios: _four_.\n\
     - [ ] **T5 — blocked on one need of two.** Needs: T1, T2. Scenarios: _five_.\n";

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
    if [ "${KEELER_STUB_TMUX_FAIL:-0}" = 1 ]; then
        echo "tmux stub: no server running on /tmp/tmux-501/default" >&2
        exit 1
    fi
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
if [ "${KEELER_STUB_CLAUDE_EXIT:-0}" != 0 ]; then
    # An API death that reports itself: the turn never finished.
    exit "$KEELER_STUB_CLAUDE_EXIT"
fi
if [ "${KEELER_STUB_CLAUDE_SILENT_DEATH:-0}" = 1 ]; then
    # The death this project actually met: a session limit, no result
    # record in the stream, and exit zero all the same.
    echo '{"type":"assistant","message":{"content":[{"type":"text","text":"You have hit your session limit"}]}}'
    exit 0
fi
# What the real binary emits for a turn that ran a failing command and
# then finished: the tool result carries is_error, the result record does
# not. Red-then-green is every Keeler task, so this is the ordinary shape.
echo '{"type":"assistant","message":{"content":[{"type":"text","text":"claude stub: running the failing test first"}]}}'
echo '{"type":"user","message":{"role":"user","content":[{"type":"tool_result","content":"Exit code 1","is_error":true}]}}'
echo '{"type":"assistant","message":{"content":[{"type":"text","text":"claude stub: the turn ended in FAIL"}]}}'
echo '{"is_error":false,"type":"result","subtype":"success"}'
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
    /// Non-zero makes the stub agent die without finishing its turn.
    claude_exit: i32,
    /// The death that exits zero: no result record, nothing to complain of.
    claude_silent_death: bool,
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
            claude_exit: 0,
            claude_silent_death: false,
        };
        project.git(&["init", "-qb", "main"]);
        project.git(&["add", "-A"]);
        project.git(&["commit", "-qm", "fixture"]);
        // A feature is developed on its own branch, and that is where its
        // tasks fan out from — so that is where a fixture stands unless a
        // test moves it.
        project.git(&["checkout", "-qb", &format!("feat/{slug}")]);
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
            .env("KEELER_STUB_CLAUDE_EXIT", self.claude_exit.to_string())
            .env(
                "KEELER_STUB_CLAUDE_SILENT_DEATH",
                if self.claude_silent_death { "1" } else { "0" },
            )
            .env(
                "KEELER_STUB_TMUX_FAIL",
                if self.dir.join(".stub-tmux-fail").exists() {
                    "1"
                } else {
                    "0"
                },
            )
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

    /// Writes the review record a closed task carries, naming HEAD.
    fn write_review_record(&self, slug: &str, tid: &str) {
        let dir = self.dir.join("reviews").join(slug);
        std::fs::create_dir_all(&dir).unwrap();
        let head = self.git(&["rev-parse", "HEAD"]);
        std::fs::write(
            dir.join(format!("{tid}.md")),
            format!("Spec: {slug}\nTask: {tid}\nCommit: {head}\nVerdict: pass\n\nnone\n"),
        )
        .unwrap();
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

    /// Makes the stub tmux refuse to start a session, as a tmux with no
    /// server, or one rejecting the name, would.
    fn break_tmux_new_session(&self) {
        std::fs::write(self.dir.join(".stub-tmux-fail"), "1").unwrap();
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
            let _ = std::fs::remove_file(entry.path());
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
        // "nothing is pushed" is a permission, not a request: Bash(git:*)
        // grants push, so the promise holds only if push is taken back.
        "--disallowedTools",
        "Bash(git push:",
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

    // And the working tree the spawn was run from is untouched — still on
    // the feature branch, at the same commit, with nothing written into it
    assert_eq!(project.git(&["rev-parse", "HEAD"]), before);
    assert_eq!(
        project.git(&["symbolic-ref", "--short", "HEAD"]),
        format!("feat/{SLUG}")
    );
    assert_eq!(project.git(&["status", "--porcelain"]), "");
}

/// What the session runs: the last argument tmux was handed, plus the
/// script it points at when it points at one — a session asked to run a
/// runner script was asked to run everything in it.
fn session_script(project: &Project, call: &[String]) -> String {
    let command = call.last().expect("tmux was given no command").clone();
    let path = PathBuf::from(
        command
            .strip_prefix("bash ")
            .unwrap_or(&command)
            .replace("\\ ", " "),
    );
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
    assert!(
        lower.contains("pull request"),
        "the prompt does not leave the pull request to the human:\n{ran}"
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
    // A green gate with no record and no tick is `incomplete`, not
    // `passed` — closed is three things, and this test is about the
    // verdict the gate decided, which is one of them.
    for (code, expected) in [(0, "incomplete"), (3, "failed")] {
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
        // Teed, not redirected: a redirect would leave `tmux attach` — the
        // live view the spec promises — showing an empty pane.
        let ran = session_script(&project, &project.new_sessions()[0]);
        assert!(
            ran.contains("| tee "),
            "the session redirects its output instead of teeing it:\n{ran}"
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
fn the_board_reads_the_graph_that_spawn_and_land_read() {
    // Given an uncommitted tick — someone editing the spec, or a stage
    // that has not been committed yet
    let project = Project::new("uncommitted-tick");
    let spec = project.dir.join("specs").join(format!("{SLUG}.md"));
    let text = std::fs::read_to_string(&spec).unwrap();
    std::fs::write(&spec, text.replace("- [ ] **T3", "- [x] **T3")).unwrap();

    // When the board is read
    let listed = stdout(&project.status(SLUG));

    // Then T3 is not done. keeler-spawn refuses a spec that differs from
    // HEAD and computes readiness from main; keeler-land acts only on
    // committed ticks. A board answering from the working tree would be
    // the one place in graph mode where an uncommitted edit counts —
    // reporting a task finished that neither of the other two believes,
    // and hiding the log and worktree a resume needs.
    let line = task_line(&listed, "T3");
    assert!(
        !line.contains("done"),
        "the board counted an uncommitted tick:\n{listed}"
    );
}

#[test]
fn a_task_that_landed_after_a_failed_run_reads_as_done() {
    // Given a task whose gate once failed — a verdict on disk saying so —
    // and which was then fixed and landed, so the graph counts it done
    let mut project = Project::new("failed-then-landed");
    project.run_sessions = true;
    project.branch_exit = 1;
    let output = project.spawn(SLUG, "T3");
    assert!(output.status.success(), "{}", both(&output));
    assert!(
        project.runs(SLUG).join("t3.exit").exists(),
        "the fixture wrote no verdict; there is nothing to outrank"
    );
    // The task then lands: its box is ticked and committed on main, which
    // is the state keeler-land leaves and the state the board reads
    let spec = project.dir.join("specs").join(format!("{SLUG}.md"));
    let text = std::fs::read_to_string(&spec).unwrap();
    std::fs::write(&spec, text.replace("- [ ] **T3", "- [x] **T3")).unwrap();
    project.git(&["add", "-A"]);
    project.git(&["commit", "-qm", "land T3"]);

    // When the board is read
    let listed = stdout(&project.status(SLUG));

    // Then it reads as done. A stale verdict is the record of a run that
    // has since been superseded; the graph is the authority on what
    // landed, and a board that reports `failed` forever for finished work
    // is the second answer this ordering exists to prevent.
    let line = task_line(&listed, "T3");
    assert!(
        line.contains("done"),
        "a landed task still reads by its stale verdict:\n{listed}"
    );
    assert!(
        !line.contains("failed"),
        "a landed task reads as a failed gate:\n{listed}"
    );
}

#[test]
fn a_death_that_exits_zero_is_still_a_death() {
    // Given the death this project actually met: a session limit reached
    // mid-work, no result record in the stream — and exit zero, because
    // the process ended tidily even though the turn did not
    let mut project = Project::new("silent-death");
    project.run_sessions = true;
    project.claude_silent_death = true;
    let output = project.spawn(SLUG, "T3");
    assert!(output.status.success(), "{}", both(&output));

    // Then the gate still did not run. An exit code says how the process
    // ended, not whether the turn did; the stream's final result record
    // is the only thing that says the latter, and its absence is the
    // death — however calmly the process exited.
    assert!(
        !project.runs(SLUG).join("t3.exit").exists(),
        "a turn that never finished was gated and given a verdict"
    );
    let listed = stdout(&project.status(SLUG));
    let line = task_line(&listed, "T3");
    assert!(
        line.contains("died"),
        "a death that exits zero reads as something else:\n{line}"
    );
    assert!(
        !line.contains("passed"),
        "a death that exits zero reads as a pass:\n{line}"
    );
}

#[test]
fn an_agent_that_never_finished_its_turn_is_died_and_not_failed() {
    // Given an agent that ends without finishing its turn — an API death
    // mid-read, which is what five of this project's first six spawns did
    let mut project = Project::new("api-death");
    project.run_sessions = true;
    project.claude_exit = 1;
    let output = project.spawn(SLUG, "T3");
    assert!(output.status.success(), "{}", both(&output));

    // Then the gate never ran, so there is no verdict to read — and the
    // board says died. Gating unconditionally turns the agent's death
    // into the gate's verdict on an untouched worktree, and the board
    // then reports `failed` about work nobody did.
    assert!(
        !project.runs(SLUG).join("t3.exit").exists(),
        "a gate ran after an unfinished turn and left a verdict"
    );
    let listed = stdout(&project.status(SLUG));
    let line = task_line(&listed, "T3");
    assert!(
        line.contains("died"),
        "an API death is not named died:\n{listed}"
    );
    assert!(
        !line.contains("failed"),
        "an API death reads as a failed gate:\n{listed}"
    );
}

#[test]
fn the_runner_is_written_as_it_was_meant_not_as_the_shell_ate_it() {
    // Given a spawn, and the runner it wrote
    let project = Project::new("heredoc");
    let output = project.spawn(SLUG, "T3");
    assert!(output.status.success(), "{}", both(&output));
    let runner = std::fs::read_to_string(project.runs(SLUG).join("t3.sh")).unwrap();

    // Then nothing in it was executed on the way. The heredoc is
    // unquoted, so a backtick in a comment is command substitution: the
    // spawn printed "command not found", and the comment reached the
    // runner with its subject silently removed — documentation gutted in
    // the artifact, and a latent way for any future backticked word to
    // run at spawn time.
    assert!(
        !both(&output).contains("command not found"),
        "the runner's own text ran commands while being written:\n{}",
        both(&output)
    );
    assert!(
        !runner.contains("# , because") && !runner.contains("final  record"),
        "a comment reached the runner with its subject eaten:\n{runner}"
    );
}

#[test]
fn the_log_carries_progress_and_not_only_the_final_answer() {
    // Given the runner a spawn writes
    let project = Project::new("verbose-log");
    let output = project.spawn(SLUG, "T3");
    assert!(output.status.success(), "{}", both(&output));
    let runner = std::fs::read_to_string(project.runs(SLUG).join("t3.sh")).unwrap();

    // Then it asks claude for its progress as it goes. Without this the
    // log stays empty until the final answer, and four minutes of honest
    // work looks exactly like a hang — which is what it looked like.
    assert!(
        runner.contains("--verbose") && runner.contains("stream-json"),
        "the runner asks for no progress, so the log cannot show any — and \
         the binary refuses stream-json without --verbose, so dropping \
         either kills every run:\n{runner}"
    );
}

#[test]
fn a_failing_tool_call_is_not_a_failed_turn() {
    // Given the ordinary shape of a Keeler task: the agent runs the
    // failing test first, so its stream carries a tool result with
    // is_error, and then finishes its turn cleanly
    let mut project = Project::new("red-then-green");
    project.run_sessions = true;
    let output = project.spawn(SLUG, "T3");
    assert!(output.status.success(), "{}", both(&output));

    // Then the gate ran and the verdict is the gate's. `is_error` is a
    // field of every failed tool result as well as of the result record,
    // and red-then-green is what test-first development *is* — so an
    // unscoped check calls every honest task dead.
    assert!(
        project.runs(SLUG).join("t3.exit").exists(),
        "a turn that finished after a failing tool call was called dead:\n{}",
        both(&output)
    );
    let listed = stdout(&project.status(SLUG));
    let line = task_line(&listed, "T3");
    assert!(
        !line.contains("died"),
        "red-then-green read as a death:\n{line}"
    );
}

#[test]
fn the_board_succeeds_whatever_it_reports() {
    // Given a board with a died task among tasks in other states
    let mut project = Project::new("board-exit");
    project.run_sessions = true;
    project.claude_silent_death = true;
    project.spawn(SLUG, "T3");

    // When it is read
    let output = project.status(SLUG);

    // Then it exits zero. A board is a report, and a report that says
    // "one of these died" has not itself failed — a non-zero exit here
    // stops any script that reads the board, and `just` prints a failure
    // over a listing that was correct.
    assert!(
        output.status.success(),
        "the board reported a died task and exited non-zero:\n{}",
        both(&output)
    );
    assert!(
        stdout(&output).contains("keeler-resume"),
        "the board does not offer the resume:\n{}",
        stdout(&output)
    );
}

#[test]
fn resuming_a_done_task_is_refused() {
    // Given a task that died, was then finished and ticked — its runner
    // still on disk, because keeler-land leaves it behind whenever
    // cleanup is skipped
    let mut project = Project::new("resume-done");
    project.run_sessions = true;
    project.claude_silent_death = true;
    project.spawn(SLUG, "T3");
    let spec = project.dir.join("specs").join(format!("{SLUG}.md"));
    let text = std::fs::read_to_string(&spec).unwrap();
    std::fs::write(&spec, text.replace("- [ ] **T3", "- [x] **T3")).unwrap();
    project.git(&["add", "-A"]);
    project.git(&["commit", "-qm", "land T3"]);

    // When it is resumed
    let output = project.just(&["keeler-resume", &format!("specs/{SLUG}.md"), "T3"]);

    // Then it is refused. keeler-spawn asks the graph and refuses a done
    // task; resume must ask the same graph, or a landed task gets rebuilt
    // on top of itself.
    let said = both(&output);
    assert!(!output.status.success(), "a done task was resumed:\n{said}");
    assert!(
        said.contains("done"),
        "the refusal does not say why:\n{said}"
    );
}

#[test]
fn resuming_a_task_whose_worktree_is_gone_is_refused() {
    // Given a died task whose worktree was removed by hand
    let mut project = Project::new("resume-no-worktree");
    project.run_sessions = true;
    project.claude_silent_death = true;
    project.spawn(SLUG, "T3");
    let worktree = project.worktree(SLUG, "T3");
    project.git(&["worktree", "remove", "--force", worktree.to_str().unwrap()]);

    // When it is resumed
    let output = project.just(&["keeler-resume", &format!("specs/{SLUG}.md"), "T3"]);

    // Then it refuses naming the path, rather than starting a session
    // whose first act is to fail its `cd` where nobody will read it — a
    // resume that reports success having run nothing wedges the task,
    // since spawn refuses the branch and resume no-ops forever.
    let said = both(&output);
    assert!(
        !output.status.success(),
        "a resume ran without a worktree:\n{said}"
    );
    assert!(
        said.contains(worktree.to_str().unwrap()),
        "the refusal does not name the missing worktree:\n{said}"
    );
}

#[test]
fn a_resume_runs_the_runner_in_the_worktree_whatever_the_path() {
    // Given a project whose path holds a space — a Desktop, a Dropbox
    let mut project = Project::new("resume path with space");
    project.run_sessions = true;
    project.claude_silent_death = true;
    project.spawn(SLUG, "T3");
    let before = project.calls("tmux").len();

    // When it is resumed
    let output = project.just(&["keeler-resume", &format!("specs/{SLUG}.md"), "T3"]);
    assert!(output.status.success(), "{}", both(&output));

    // Then tmux was handed a command it can actually run, and the session
    // starts in the worktree. Unquoted, the runner path splits at the
    // space and the session exits at once — resume printing success
    // having run nothing.
    let started: Vec<Vec<String>> = project
        .calls("tmux")
        .into_iter()
        .skip(before)
        .filter(|c| c.first().is_some_and(|v| v == "new-session"))
        .collect();
    assert_eq!(
        started.len(),
        1,
        "the resume started no session: {started:?}"
    );
    let call = &started[0];
    // Escaped, not bare: `printf %q` is what makes the shell tmux runs it
    // in see one path rather than three words.
    assert!(
        call.iter()
            .any(|a| a.contains(".sh") && a.contains("path\\ with\\ space")),
        "the runner path was not escaped for the shell that runs it: {call:?}"
    );
    assert!(
        call.windows(2)
            .any(|w| w[0] == "-c" && w[1] == project.worktree(SLUG, "T3").to_str().unwrap()),
        "the session does not start in the task's worktree: {call:?}"
    );
}

#[test]
fn a_task_is_closed_by_three_things_not_one() {
    // Given a task whose gate ran green — and nothing else yet
    let mut project = Project::new("closed-by-three");
    project.run_sessions = true;
    let output = project.spawn(SLUG, "T3");
    assert!(output.status.success(), "{}", both(&output));
    assert!(
        project.runs(SLUG).join("t3.exit").exists(),
        "the fixture has no verdict"
    );

    // Then the board does not call it passed. A green gate is one stage
    // of four: the second live spawn passed its gate on eight hundred
    // lines of real tests having done only the first, and `passed` there
    // would have invited landing work nobody had read.
    let listed = stdout(&project.status(SLUG));
    let line = task_line(&listed, "T3");
    assert!(
        line.contains("incomplete"),
        "a green gate alone was called closed:\n{listed}"
    );
    assert!(
        line.contains("review") || line.contains("record"),
        "the board does not name what is missing:\n{line}"
    );

    // And with the record but no tick it is still not closed
    project.write_review_record(SLUG, "t3");
    let listed = stdout(&project.status(SLUG));
    let line = task_line(&listed, "T3");
    assert!(
        line.contains("incomplete"),
        "a gate and a record without a tick were called closed:\n{listed}"
    );
    assert!(
        line.contains("tick") || line.contains("box"),
        "the board does not name the missing tick:\n{line}"
    );

    // And with all three it is
    let spec = project.dir.join("specs").join(format!("{SLUG}.md"));
    let text = std::fs::read_to_string(&spec).unwrap();
    std::fs::write(&spec, text.replace("- [ ] **T3", "- [x] **T3")).unwrap();
    project.git(&["add", "-A"]);
    project.git(&["commit", "-qm", "land T3"]);
    let listed = stdout(&project.status(SLUG));
    assert!(
        task_line(&listed, "T3").contains("done"),
        "a task with all three was not closed:\n{listed}"
    );
}

#[test]
fn a_dead_task_is_resumed_by_name() {
    // Given a task the board reports as died
    let mut project = Project::new("resume");
    project.run_sessions = true;
    project.claude_exit = 1;
    project.spawn(SLUG, "T3");
    let worktree = project.worktree(SLUG, "T3");
    assert!(task_line(&stdout(&project.status(SLUG)), "T3").contains("died"));

    // And the board offers the command beside it
    assert!(
        stdout(&project.status(SLUG)).contains("keeler-resume"),
        "the board does not offer a resume for a died task:\n{}",
        stdout(&project.status(SLUG))
    );

    // When it is resumed
    project.claude_exit = 0;
    let output = project.just(&["keeler-resume", &format!("specs/{SLUG}.md"), "T3"]);

    // Then the run happens in the worktree and branch it already had —
    // whose commits say how far the pipeline got — and nothing new is made
    assert!(
        output.status.success(),
        "a died task would not resume:\n{}",
        both(&output)
    );
    assert_eq!(project.worktree(SLUG, "T3"), worktree, "the worktree moved");
    assert_eq!(
        project
            .git(&["worktree", "list", "--porcelain"])
            .matches("worktree ")
            .count(),
        2,
        "resuming created a second worktree"
    );

    // And a task that is not died is refused by name
    for (task, why) in [("T1", "done"), ("T2", "not spawned")] {
        let output = project.just(&["keeler-resume", &format!("specs/{SLUG}.md"), task]);
        let said = both(&output);
        assert!(
            !output.status.success(),
            "{task} ({why}) was resumed:\n{said}"
        );
        assert!(
            said.contains(task),
            "the refusal does not name {task}:\n{said}"
        );
    }
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
    // And a task the spec has ticked is done, not merely unspawned: the
    // board reads the graph's answer rather than inventing one
    assert!(
        task_line(&listed, "T1").contains("done"),
        "a task ticked in the spec reads as never spawned:\n{listed}"
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
fn spawning_from_anywhere_but_the_features_branch_is_refused() {
    // Given a checkout on main, and on an unrelated branch — neither is
    // the branch this spec's feature is developed on
    let project = Project::new("wrong-branch");
    for branch in ["main", "some-other-work"] {
        if branch == "main" {
            project.git(&["checkout", "-q", "main"]);
        } else {
            project.git(&["checkout", "-qb", branch]);
        }

        // When a ready task is spawned from there
        let output = project.spawn(SLUG, "T3");

        // Then it refuses, naming the branch it expected. Which branch
        // holds the graph is a name a machine checks: readiness is read
        // from the feature's branch, and spawning from anywhere else
        // would cut a worktree from a history the graph never saw.
        let said = both(&output);
        assert!(
            !output.status.success(),
            "a spawn ran from {branch}:\n{said}"
        );
        assert!(
            said.contains(&format!("feat/{SLUG}")),
            "the refusal from {branch} does not name the branch it expected:\n{said}"
        );
        assert!(
            !project.worktree(SLUG, "T3").exists(),
            "a worktree was created from {branch}"
        );
        assert!(
            !project.dir.join(".keeler").join("runs").exists(),
            "a run directory was created from {branch}"
        );
        assert_eq!(
            project.git(&["branch", "--list", &format!("keeler/{SLUG}/t3")]),
            "",
            "a branch was created from {branch}"
        );
    }
}

#[test]
fn a_dependency_ticked_on_the_feature_branch_unblocks_its_dependent() {
    // Given a feature branch where T1 is not yet done, so T3 is blocked
    let project = Project::with_spec(
        "feature-branch-readiness",
        SLUG,
        &spec_body("Approved", &TASKS.replace("- [x] **T1", "- [ ] **T1")),
    );
    let output = project.spawn(SLUG, "T3");
    assert!(
        !output.status.success(),
        "T3 spawned with its need unticked:\n{}",
        both(&output)
    );

    // When T1 lands on the feature branch — its tick arriving there is
    // the landing
    let spec = project.dir.join("specs").join(format!("{SLUG}.md"));
    std::fs::write(&spec, spec_body("Approved", TASKS)).unwrap();
    project.git(&["add", "-A"]);
    project.git(&["commit", "-qm", "land T1 into the feature branch"]);

    // Then T3 spawns. Reading readiness from main would have called this
    // blocked while its dependency sat ready on disk — work refused for a
    // reason that was true of another branch.
    let output = project.spawn(SLUG, "T3");
    assert!(
        output.status.success(),
        "a dependency landed on the feature branch did not unblock its dependent:\n{}",
        both(&output)
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
        // "Before creating anything" includes the run directory: a refusal
        // that has already made one has created something.
        assert!(
            !project.path().join(".keeler").exists(),
            "a run directory was created:\n{said}"
        );
    }

    // And (d) a spec in no repository at all is the extreme of the same
    // thing: HEAD cannot hold what the repository does not contain
    let outside = fresh.path().with_extension("outside.md");
    std::fs::write(&outside, &approved).unwrap();
    let output = fresh.just(&["keeler-spawn", &outside.display().to_string(), "T3"]);
    let said = both(&output);
    let _ = std::fs::remove_file(&outside);
    assert!(
        !output.status.success(),
        "a spec outside the repository spawned:\n{said}"
    );
    assert!(
        said.contains("HEAD"),
        "the refusal does not say why:\n{said}"
    );
    assert!(
        fresh.new_sessions().is_empty(),
        "a session was started:\n{said}"
    );
}

#[test]
fn spawning_a_task_that_is_already_spawned_is_refused() {
    // Given a worktree for T3 that already exists, and a run that reached
    // its gate — the state a second spawn must not destroy
    let mut project = Project::new("already");
    project.run_sessions = true;
    assert!(project.spawn(SLUG, "T3").status.success());
    project.run_sessions = false;
    let worktree = project.worktree(SLUG, "T3");
    let verdict = project.runs(SLUG).join("t3.exit");
    assert!(verdict.is_file(), "the fixture has no verdict to protect");
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
    assert!(verdict.is_file(), "the refusal destroyed the run's verdict");

    // And a branch whose worktree is gone is the same task, half cleaned
    // up: the commits are still there, so this is still already spawned
    project.git(&[
        "worktree",
        "remove",
        "--force",
        &worktree.display().to_string(),
    ]);
    let output = project.spawn(SLUG, "T3");
    let said = both(&output);
    assert!(
        !output.status.success(),
        "a task whose branch still exists spawned again:\n{said}"
    );
    assert!(
        said.contains(&format!("keeler/{SLUG}/t3")),
        "the refusal does not name the branch:\n{said}"
    );
    // And it is Keeler's refusal, not git's crash: without a guard of its
    // own the recipe would still exit non-zero, deep inside `git worktree
    // add`, having said `fatal:` instead of what to do about it.
    assert!(
        said.contains("keeler-spawn:") && !said.contains("fatal:"),
        "the branch was refused by git rather than by the recipe:\n{said}"
    );
    assert!(verdict.is_file(), "the refusal destroyed the run's verdict");
    assert_eq!(project.new_sessions().len(), 1, "a session was started");

    // And once the branch is gone too, the task can be spawned afresh —
    // with no verdict, because the old one is not this run's
    project.git(&["branch", "-D", &format!("keeler/{SLUG}/t3")]);
    let output = project.spawn(SLUG, "T3");
    assert!(output.status.success(), "{}", both(&output));
    assert!(
        !verdict.exists(),
        "a verdict from the previous run survived into this one"
    );
}

#[test]
fn spawning_a_task_that_is_already_done_is_refused() {
    // Given a spec whose T1 is ticked
    let project = Project::new("already-done");

    // When it is spawned anyway
    let output = project.spawn(SLUG, "T1");

    // Then it refuses. A done task has landed; spawning it would cut a
    // branch to redo work the graph already counts, and the guard that
    // only refuses `blocked` lets exactly that through.
    let said = both(&output);
    assert!(!output.status.success(), "a done task spawned:\n{said}");
    assert!(said.contains("T1") && said.contains("done"), "{said}");
    assert!(
        !project.worktree(SLUG, "T1").exists(),
        "a worktree was created for a landed task"
    );
    assert!(project.new_sessions().is_empty(), "a session was started");
}

#[test]
fn a_session_that_fails_to_start_leaves_nothing_behind() {
    // Given tmux that fails when asked for a new session — a server that
    // will not start, a name the server rejects
    let project = Project::new("tmux-fails");
    project.break_tmux_new_session();

    // When a ready task is spawned
    let output = project.spawn(SLUG, "T3");

    // Then it fails, and nothing survives: a branch and worktree left
    // behind would wedge the task, since every retry then refuses with
    // "already spawned" for a run that never started
    let said = both(&output);
    assert!(!output.status.success(), "a failed launch reported success");
    assert!(
        !project.worktree(SLUG, "T3").exists(),
        "the worktree outlived the failed launch:\n{said}"
    );
    assert_eq!(
        project.git(&["branch", "--list", &format!("keeler/{SLUG}/t3")]),
        "",
        "the branch outlived the failed launch:\n{said}"
    );
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

    // And what it names is what is unmet, not everything the task
    // declares: T5 needs T1 and T2, and only T2 is outstanding
    let output = project.spawn(SLUG, "T5");
    let said = both(&output);
    assert!(!output.status.success(), "a blocked task spawned:\n{said}");
    assert!(
        said.contains("T2"),
        "the refusal does not name the unmet need:\n{said}"
    );
    assert!(
        !said.contains("T1"),
        "the refusal names T1, which is done:\n{said}"
    );

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

#[test]
fn the_installer_ignores_the_run_directory() {
    // Given a fresh Rust project, and an installer run that cannot reach
    // the network: cargo is logged rather than executed, curl fails loudly
    let project = Project::new("installer-ignore");
    let dest = project.path().join("adopter");
    std::fs::create_dir_all(&dest).unwrap();
    std::fs::write(
        dest.join("Cargo.toml"),
        "[package]\nname = \"probe\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n\
         [dev-dependencies]\nproptest = \"1\"\n",
    )
    .unwrap();
    let real_cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    write_stub(
        &project.path().join("bin/cargo"),
        &format!(
            "#!/usr/bin/env bash\nif [ \"$1\" = metadata ]; then exec \"{real_cargo}\" \"$@\"; fi\n"
        ),
    );
    write_stub(
        &project.path().join("bin/curl"),
        "#!/usr/bin/env bash\necho \"harness: network refused: curl $*\" >&2\nexit 7\n",
    );

    // When Keeler is installed into it
    let path = std::env::var("PATH").unwrap();
    let output = Command::new("bash")
        .arg(repo_root().join("install.sh"))
        .arg(&dest)
        .arg("--no-tools")
        .env(
            "PATH",
            format!("{}:{path}", project.path().join("bin").display()),
        )
        .output()
        .expect("failed to run install.sh");
    assert!(
        output.status.success(),
        "install.sh failed:\n{}",
        both(&output)
    );

    // Then the run directory a spawn writes is ignored, so no adopter ever
    // commits a log, a verdict or a runner script
    let ignored = std::fs::read_to_string(dest.join(".gitignore")).unwrap();
    assert!(
        ignored.lines().any(|line| line.trim() == ".keeler/"),
        "the installer does not ignore .keeler/:\n{ignored}"
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

/// Not one of T3's eight scenarios: the Implementation Notes' "readiness
/// is read from the spec **on main**", which T3 left open because the
/// shared main-resolution helper is T5's deliverable and a second
/// resolution here is the disagreement the spec forbids.
#[test]
fn readiness_is_read_from_the_committed_spec_not_the_task_branch() {
    // Given a spec whose T1 is not done on the feature branch — so T3,
    // which needs it, is blocked — and a task branch that ticks T1
    let project = Project::with_spec(
        "readiness",
        SLUG,
        &spec_body("Approved", &TASKS.replace("- [x] **T1", "- [ ] **T1")),
    );
    project.git(&["checkout", "-q", "-b", &format!("keeler/{SLUG}/t1")]);
    let spec = project.path().join("specs").join(format!("{SLUG}.md"));
    std::fs::write(&spec, spec_body("Approved", TASKS)).unwrap();
    project.git(&["add", "-A"]);
    project.git(&["commit", "-qm", "tick T1 on the task branch"]);
    // The task branch keeps its tick to itself: back on the feature
    // branch, T1 is still unticked
    project.git(&["checkout", "-q", &format!("feat/{SLUG}")]);

    // When `just keeler-spawn <spec> T3` runs there
    let output = project.spawn(SLUG, "T3");

    // Then it refuses, naming the unmet need: a tick on a task branch
    // unblocks nothing until it lands on the feature branch, which is
    // what keeps parallel branches from racing each other's dependencies
    assert!(
        !output.status.success(),
        "a task branch's own tick unblocked T3:\n{}",
        both(&output)
    );
    let said = both(&output);
    assert!(
        said.contains("blocked") && said.contains("T1"),
        "the refusal does not name the unmet need:\n{said}"
    );

    // And nothing was created
    assert!(!project.worktree(SLUG, "T3").exists());
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
