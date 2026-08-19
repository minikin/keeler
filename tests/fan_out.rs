//! Spec 07 — fan-out: one answer, one wave. The reading side: `just
//! keeler-fan-out` names the wave and waits for the yes, and the guards it
//! shares with `keeler-spawn` fire once, in `_spawn-preflight`.
//!
//! Every test drives the shipped recipes as a subprocess against a
//! throwaway project, the way `tests/spawn.rs` does — same stubs for
//! `tmux`, `claude` and `just keeler-branch`, so a task can be put into
//! any state the board knows without an agent starting. The answer is
//! piped on stdin, which is how the spec says a test gives it.

use std::fmt::Write as _;
use std::io::Write as _;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The fixture spec's slug — its file name without `.md`.
const SLUG: &str = "42-fixture";

/// The wave the spec draws: T2, T3 and T5 ready, T4 blocked on T2 — and
/// T1 done, so that "done" has a line of its own to be listed as.
const TASKS: &str = "- [x] **T1 — the root, and it is done.** Scenarios: _one_.\n\
     - [ ] **T2 — a root.** Scenarios: _two_.\n\
     - [ ] **T3 — ready on the done root.** Needs: T1. Scenarios: _three_.\n\
     - [ ] **T4 — blocked on T2.** Needs: T2. Scenarios: _four_.\n\
     - [ ] **T5 — another root.** Scenarios: _five_.\n";

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
/// harness's own PATH, so the stub `just` on the project's PATH is only
/// ever reached from inside a recipe.
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
/// and `new-session` runs the command it was given only when asked to.
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

const CLAUDE_STUB: &str = r#"#!/usr/bin/env bash
{ for a in "$@"; do printf '%s\037' "$a"; done; printf '\036'; } >> "$KEELER_STUB_CLAUDE_LOG"
echo "claude stub: the turn ended"
exit 0
"#;

/// Stands in for `just keeler-branch`; everything else is the real `just`.
const JUST_STUB: &str = r#"#!/usr/bin/env bash
if [ "${1:-}" = keeler-branch ]; then
    echo "keeler-branch stub: the gate ran"
    exit "${KEELER_STUB_BRANCH_EXIT:-0}"
fi
exec "$KEELER_REAL_JUST" "$@"
"#;

/// What a run is given on stdin.
enum Answer<'a> {
    /// A line piped in, then EOF — the way a test answers.
    Piped(&'a str),
    /// `/dev/null`: nobody there, and EOF at once.
    Nobody,
}

/// A throwaway project holding the shipped `Justfile`, the graph script and
/// one spec, with the stubs on its `bin/`, checked out on the feature
/// branch. Removed on drop, together with the sibling worktrees a spawn
/// creates.
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
        let dir =
            std::env::temp_dir().join(format!("keeler-fan-out-{name}-{}", std::process::id()));
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
        // Resolved, because `git rev-parse --show-toplevel` resolves too.
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
        project.git(&["checkout", "-qb", &format!("feat/{slug}")]);
        project
    }

    fn spec_file(&self, slug: &str) -> PathBuf {
        self.dir.join(spec_path(slug))
    }

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

    /// A recipe invocation with the stubs first on PATH and nothing
    /// answered in advance — the harness's own environment must not leak
    /// a yes into a run that is meant to ask.
    fn just_command(&self, args: &[&str]) -> Command {
        let path = std::env::var("PATH").unwrap();
        let mut command = Command::new(real_just());
        command
            .args(args)
            .current_dir(&self.dir)
            .env("PATH", format!("{}:{path}", self.dir.join("bin").display()))
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
            .env_remove("KEELER_FAN_OUT_YES");
        command
    }

    fn just(&self, args: &[&str]) -> Output {
        self.just_command(args)
            .stdin(Stdio::null())
            .output()
            .expect("failed to run just")
    }

    fn spawn(&self, slug: &str, task: &str) -> Output {
        self.just(&["keeler-spawn", &spec_path(slug), task])
    }

    /// `just keeler-fan-out <spec>` with the given stdin and environment.
    fn fan_out_with(&self, slug: &str, answer: &Answer<'_>, env: &[(&str, &str)]) -> Output {
        let mut command = self.just_command(&["keeler-fan-out", &spec_path(slug)]);
        for (key, value) in env {
            command.env(key, value);
        }
        match answer {
            Answer::Nobody => command.stdin(Stdio::null()).output(),
            Answer::Piped(text) => {
                let mut child = command
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .expect("failed to run just");
                child
                    .stdin
                    .take()
                    .unwrap()
                    .write_all(text.as_bytes())
                    .unwrap();
                child.wait_with_output()
            }
        }
        .expect("failed to run just")
    }

    fn fan_out(&self, slug: &str, answer: &Answer<'_>) -> Output {
        self.fan_out_with(slug, answer, &[])
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

/// The line the run prints for one task id — the graph's line for a task
/// it offers or holds back, the board's line for one already spawned.
fn task_line<'a>(said: &'a str, id: &str) -> &'a str {
    said.lines()
        .find(|line| line.split_whitespace().next() == Some(id))
        .unwrap_or_else(|| panic!("the run never lists {id}:\n{said}"))
}

/// The wave the run offered: the ids on its `wave:` line, in order — or
/// none, when it printed no such line.
fn wave_of(said: &str) -> Vec<String> {
    said.lines()
        .find_map(|line| line.strip_prefix("wave:"))
        .map(|ids| ids.split_whitespace().map(str::to_string).collect())
        .unwrap_or_default()
}

/// The question the run asks before it spawns — its prompt line.
fn asked(said: &str) -> bool {
    said.contains("[yes/no]")
}

fn nothing_spawned(project: &Project, said: &str) {
    assert!(
        project.new_sessions().is_empty(),
        "a session was started:\n{said}"
    );
    for task in ["T2", "T3", "T4", "T5"] {
        assert!(
            !project.worktree(SLUG, task).exists(),
            "a worktree was created for {task}:\n{said}"
        );
        assert_eq!(
            project.git(&[
                "branch",
                "--list",
                &format!("keeler/{SLUG}/{}", task.to_lowercase())
            ]),
            "",
            "a branch was created for {task}:\n{said}"
        );
    }
}

#[test]
fn fan_out_names_the_wave_and_waits_for_yes() {
    // Given feat/<spec-slug> is checked out and the graph has T2, T3 and
    // T5 ready and T4 blocked on T2
    let project = Project::new("wave");

    // When `just keeler-fan-out <spec>` runs, answered no
    let output = project.fan_out(SLUG, &Answer::Piped("no\n"));
    let said = stdout(&output);

    // Then it prints the ready tasks — T2, T3, T5 — and the blocked one
    // with what it waits on, and asks for a yes before anything is spawned
    assert_eq!(wave_of(&said), ["T2", "T3", "T5"], "{said}");
    for id in ["T2", "T3", "T5"] {
        assert!(
            task_line(&said, id).contains("ready"),
            "{id} is not listed as ready:\n{said}"
        );
    }
    let blocked = task_line(&said, "T4");
    assert!(
        blocked.contains("blocked") && blocked.contains("T2"),
        "T4 is not listed as blocked on T2:\n{said}"
    );
    assert!(
        task_line(&said, "T1").contains("done"),
        "T1 is not listed as done:\n{said}"
    );
    assert!(asked(&said), "the run never asked:\n{said}");

    // And the answer no spawns nothing, creates nothing, and exits non-zero
    assert!(!output.status.success(), "no was taken for yes:\n{said}");
    nothing_spawned(&project, &both(&output));
}

#[test]
fn the_answer_is_yes_or_y_case_aside_and_nothing_else() {
    for (i, answer) in ["yes\n", "y\n", "YES\n", "Y\n"].into_iter().enumerate() {
        // Given the wave, and an answer that reads as yes
        let project = Project::new(&format!("yes-{i}"));

        // When fan-out is answered so
        let output = project.fan_out(SLUG, &Answer::Piped(answer));

        // Then the yes is taken — the run went past its question
        let said = both(&output);
        assert!(asked(&said), "the run never asked:\n{said}");
        assert!(
            output.status.success(),
            "{answer:?} was not taken for yes:\n{said}"
        );
    }
    for (i, answer) in ["no\n", "\n", "yes please\n", "1\n"]
        .into_iter()
        .enumerate()
    {
        // Given the same wave, and an answer that is anything else
        let project = Project::new(&format!("not-yes-{i}"));

        // When fan-out is answered so
        let output = project.fan_out(SLUG, &Answer::Piped(answer));

        // Then nothing spawns, nothing is created, and the exit is non-zero
        let said = both(&output);
        assert!(asked(&said), "the run never asked:\n{said}");
        assert!(
            !output.status.success(),
            "{answer:?} was taken for yes:\n{said}"
        );
        nothing_spawned(&project, &said);
    }
}

#[test]
fn a_ready_task_the_board_knows_is_listed_in_its_words_and_not_offered() {
    // Given T3 spawned and died — its worktree stands, its session is
    // gone, no verdict was written — and T5 spawned and still running
    let mut project = Project::new("board");
    assert!(project.spawn(SLUG, "T3").status.success());
    assert!(project.spawn(SLUG, "T5").status.success());
    project.sessions = format!("keeler-{SLUG}-t5");
    let spawned_before = project.new_sessions().len();

    // When fan-out runs
    let output = project.fan_out(SLUG, &Answer::Piped("no\n"));
    let said = stdout(&output);

    // Then the wave is what is ready and not yet spawned: T2 alone
    assert_eq!(wave_of(&said), ["T2"], "{said}");

    // And T3 and T5 are listed in the board's own words — the state, the
    // log and the worktree — and not offered
    let died = task_line(&said, "T3");
    assert!(died.contains("died"), "T3 is not listed as died:\n{said}");
    assert!(
        died.contains(&project.runs(SLUG).join("t3.log").display().to_string())
            && died.contains(&project.worktree(SLUG, "T3").display().to_string()),
        "T3's line does not carry the board's log and worktree:\n{said}"
    );
    assert!(
        task_line(&said, "T5").contains("running"),
        "T5 is not listed as running:\n{said}"
    );
    assert!(asked(&said), "the run never asked:\n{said}");
    assert_eq!(project.new_sessions().len(), spawned_before);

    // And a task that passed but has not landed is the same: known to the
    // board, listed by its verdict, not offered
    let mut passed = Project::new("board-passed");
    passed.run_sessions = true;
    passed.branch_exit = 0;
    assert!(passed.spawn(SLUG, "T2").status.success());
    passed.run_sessions = false;
    let said = stdout(&passed.fan_out(SLUG, &Answer::Piped("no\n")));
    assert_eq!(wave_of(&said), ["T3", "T5"], "{said}");
    assert!(
        task_line(&said, "T2").contains("passed"),
        "T2 is not listed as passed:\n{said}"
    );
}

#[test]
fn nobody_to_ask_is_refused_naming_the_variable() {
    // Given a wave, and a run with nobody to ask — stdin is not a
    // terminal, carries no answer, and KEELER_FAN_OUT_YES is unset
    let project = Project::new("nobody");

    // When fan-out runs
    let output = project.fan_out(SLUG, &Answer::Nobody);

    // Then it refuses naming the variable rather than exiting in silence
    let said = both(&output);
    assert!(!output.status.success(), "nobody said yes:\n{said}");
    assert!(
        stderr(&output).contains("KEELER_FAN_OUT_YES"),
        "the refusal does not name KEELER_FAN_OUT_YES:\n{said}"
    );
    nothing_spawned(&project, &said);

    // And a pipe that never closes is nobody too: it refuses the same
    // way, rather than hanging on a line that will never come
    let mut command = project.just_command(&["keeler-fan-out", &spec_path(SLUG)]);
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // Held open for the whole run, so the recipe never sees EOF.
    let held_open = child.stdin.take().unwrap();
    let started = Instant::now();
    let output = child.wait_with_output().unwrap();
    drop(held_open);
    let said = both(&output);
    assert!(
        started.elapsed() < Duration::from_secs(20),
        "the run hung on an open pipe:\n{said}"
    );
    assert!(!output.status.success(), "an open pipe said yes:\n{said}");
    assert!(
        stderr(&output).contains("KEELER_FAN_OUT_YES"),
        "the refusal does not name KEELER_FAN_OUT_YES:\n{said}"
    );

    // And KEELER_FAN_OUT_YES=1 is the answer given in advance, for the
    // caller who has already decided: the same run goes past its question
    let output = project.fan_out_with(SLUG, &Answer::Nobody, &[("KEELER_FAN_OUT_YES", "1")]);
    let said = both(&output);
    assert!(
        output.status.success(),
        "KEELER_FAN_OUT_YES=1 was not taken for yes:\n{said}"
    );
    assert!(
        said.contains("KEELER_FAN_OUT_YES"),
        "the run does not say the yes came from the variable:\n{said}"
    );
}

#[test]
fn an_empty_wave_says_so() {
    // Given three graphs with nothing to offer
    // (a) every task done
    let done = TASKS.replace("- [ ]", "- [x]");
    let all_done = Project::with_spec("empty-done", SLUG, &spec_body("Approved", &done));

    // (b) every remaining task blocked, or ready and already spawned: T2
    // and T5 spawned, T3 spawned, T4 blocked on T2
    let all_spawned = Project::new("empty-spawned");
    for task in ["T2", "T3", "T5"] {
        assert!(all_spawned.spawn(SLUG, task).status.success());
    }
    let spawned_before = all_spawned.new_sessions().len();

    for (project, why) in [(&all_done, "done"), (&all_spawned, "died")] {
        // When fan-out runs — with a yes on stdin, which must not matter
        let output = project.fan_out(SLUG, &Answer::Piped("yes\n"));

        // Then it says nothing is ready and why, asks nothing, spawns
        // nothing, and exits zero
        let said = stdout(&output);
        assert!(
            output.status.success(),
            "an empty wave is a failure:\n{}",
            both(&output)
        );
        assert!(
            said.to_lowercase().contains("nothing"),
            "the run does not say nothing is ready:\n{said}"
        );
        assert!(
            wave_of(&said).is_empty(),
            "an empty wave was offered:\n{said}"
        );
        assert!(
            !asked(&said),
            "the run asked with nothing to offer:\n{said}"
        );
        for id in ["T2", "T3", "T5"] {
            assert!(
                task_line(&said, id).contains(why),
                "{id} is not listed as {why}:\n{said}"
            );
        }
    }
    let said = stdout(&all_spawned.fan_out(SLUG, &Answer::Piped("yes\n")));
    let blocked = task_line(&said, "T4");
    assert!(
        blocked.contains("blocked") && blocked.contains("T2"),
        "T4 is not listed as blocked on T2:\n{said}"
    );
    assert_eq!(all_spawned.new_sessions().len(), spawned_before);
    let said = stdout(&all_done.fan_out(SLUG, &Answer::Piped("yes\n")));
    assert!(
        task_line(&said, "T4").contains("done"),
        "T4 is not listed as done:\n{said}"
    );
    assert!(all_done.new_sessions().is_empty());
}

#[test]
fn fan_out_refuses_where_spawn_would() {
    // Given three checkouts keeler-spawn refuses: not on feat/<spec-slug>,
    // a spec that differs from HEAD, and one that is not Approved
    let elsewhere = Project::new("refuse-branch");
    elsewhere.git(&["checkout", "-q", "main"]);
    let differs = Project::new("refuse-differs");
    let spec = differs.spec_file(SLUG);
    let text = std::fs::read_to_string(&spec).unwrap();
    std::fs::write(&spec, format!("{text}\nan uncommitted edit.\n")).unwrap();
    let draft = Project::with_spec("refuse-draft", SLUG, &spec_body("Draft", TASKS));

    for project in [&elsewhere, &differs, &draft] {
        // When both run there
        let refused_spawn = project.spawn(SLUG, "T2");
        let refused_fan_out = project.fan_out(SLUG, &Answer::Piped("yes\n"));

        // Then fan-out refuses before printing a wave, with the same
        // message keeler-spawn gives, and asks nothing
        assert!(!refused_spawn.status.success(), "{}", both(&refused_spawn));
        assert!(
            !refused_fan_out.status.success(),
            "fan-out did not refuse:\n{}",
            both(&refused_fan_out)
        );
        let reason = stderr(&refused_spawn)
            .lines()
            .find(|line| line.starts_with("keeler-spawn:"))
            .unwrap_or_else(|| panic!("spawn gave no reason:\n{}", both(&refused_spawn)))
            .to_string();
        assert!(
            stderr(&refused_fan_out).contains(&reason),
            "fan-out's refusal is not spawn's:\n{}\nspawn said:\n{reason}",
            both(&refused_fan_out)
        );
        let said = stdout(&refused_fan_out);
        assert!(
            wave_of(&said).is_empty() && !said.contains("T2"),
            "a wave was printed before the refusal:\n{said}"
        );
        assert!(!asked(&said), "the run asked after refusing:\n{said}");
        nothing_spawned(project, &both(&refused_fan_out));
    }
}

#[test]
fn the_guards_are_written_once() {
    // Given the shipped Justfile
    let justfile = std::fs::read_to_string(repo_root().join("Justfile")).unwrap();

    // Then the guards keeler-spawn and keeler-fan-out share live in one
    // private recipe both call — refusing on the same grounds without
    // writing the checks twice
    assert!(
        justfile.contains("\n_spawn-preflight SPEC:"),
        "there is no _spawn-preflight recipe"
    );
    for recipe in ["keeler-spawn", "keeler-fan-out"] {
        let body = recipe_body(&justfile, recipe);
        assert!(
            body.contains("_spawn-preflight"),
            "{recipe} does not run _spawn-preflight:\n{body}"
        );
    }
    for guard in [
        "keeler-spawn: tmux is required",
        "keeler-spawn: $rel differs from HEAD",
        "keeler-spawn: $rel is not Approved",
        "but this spec's tasks fan out from $feature",
    ] {
        assert_eq!(
            justfile.matches(guard).count(),
            1,
            "the guard `{guard}` is written more than once"
        );
        assert!(
            recipe_body(&justfile, "_spawn-preflight").contains(guard),
            "the guard `{guard}` is not in _spawn-preflight"
        );
    }
}

/// The body of one recipe: from its header line to the next unindented line.
fn recipe_body(justfile: &str, recipe: &str) -> String {
    let header = format!("{recipe} ");
    let mut lines = justfile
        .lines()
        .skip_while(|line| !(line.starts_with(&header) || *line == format!("{recipe}:")));
    let first = lines.next().unwrap_or_else(|| panic!("no recipe {recipe}"));
    let mut body = vec![first];
    for line in lines {
        if !line.is_empty() && !line.starts_with(' ') {
            break;
        }
        body.push(line);
    }
    body.join("\n")
}

#[test]
fn no_command_file_sets_the_yes_in_advance() {
    // Given every command file under .claude/commands/
    let mut files = Vec::new();
    collect_files(&repo_root().join(".claude/commands"), &mut files);
    assert!(!files.is_empty(), "no command files found");

    // Then none of them sets KEELER_FAN_OUT_YES: the zero-yes path is the
    // human's, and an agent that could set it for itself would have found
    // the way around the one question that is theirs
    for file in files {
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(
            !text.contains("KEELER_FAN_OUT_YES="),
            "{} sets KEELER_FAN_OUT_YES",
            file.display()
        );
    }
}

fn collect_files(dir: &Path, into: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, into);
        } else {
            into.push(path);
        }
    }
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig {
        // Each case is a project and two recipe runs; eight of them is a
        // fair sample of graphs at a cost the whole suite can carry.
        cases: 8,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::WithSource("proptest-regressions"),
        )),
        ..proptest::prelude::ProptestConfig::default()
    })]

    /// Over generated graphs — any ticks, any acyclic needs — the wave
    /// fan-out prints is exactly the set `keeler-graph` reports ready, in
    /// the graph's order: readiness is never computed here, only read.
    #[test]
    fn the_wave_is_exactly_what_keeler_graph_reports_ready(
        tasks in proptest::collection::vec(
            (proptest::bool::ANY, proptest::collection::vec(proptest::bool::ANY, 0..8)),
            1..8,
        ),
    ) {
        let mut body = String::new();
        for (i, (ticked, needs)) in tasks.iter().enumerate() {
            let id = i + 1;
            let needed: Vec<String> = needs
                .iter()
                .enumerate()
                .filter(|(j, on)| **on && *j < i)
                .map(|(j, _)| format!("T{}", j + 1))
                .collect();
            let tick = if *ticked { "x" } else { " " };
            let _ = write!(body, "- [{tick}] **T{id} — a task.**");
            if !needed.is_empty() {
                let _ = write!(body, " Needs: {}.", needed.join(", "));
            }
            body.push_str(" Scenarios: _one_.\n");
        }
        let project = Project::with_spec(
            &format!("property-{}", tasks.len()), SLUG, &spec_body("Approved", &body),
        );

        let graph = project.just(&["keeler-graph", &spec_path(SLUG)]);
        proptest::prop_assert!(graph.status.success(), "{}", both(&graph));
        let ready: Vec<String> = stdout(&graph)
            .lines()
            .filter(|line| line.split_whitespace().nth(1) == Some("ready"))
            .map(|line| line.split_whitespace().next().unwrap().to_string())
            .collect();

        let output = project.fan_out(SLUG, &Answer::Piped("no\n"));
        let said = stdout(&output);
        proptest::prop_assert_eq!(wave_of(&said), ready, "{}", both(&output));
    }
}
