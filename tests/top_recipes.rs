//! Spec 10 — keeler-top. The shell around the board: the `keeler-top`
//! recipe that launches it, and the `paused` marker `keeler-status` reads
//! and `keeler-resume` clears.
//!
//! The board itself is a crate with its own suite; these scenarios are
//! recipes, so they live here beside the other recipe suites and drive
//! the shipped `Justfile` as a subprocess, the way `tests/spawn.rs` does.
//! `tmux` and `cargo` are PATH stubs — no session starts, no agent runs
//! and nothing is compiled, so the states the board reports are built by
//! the real recipes rather than described to them.

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

// ── T9

/// What the stub `cargo` plays where the board's frame would be. The
/// scenario is that stdout carries this and nothing else, so a notice or a
/// build line leaking into it shows up here as a line too many.
const STUB_FRAME: &str = "TASK  STATE    STAGE\nT1    running  qa\n";

/// The words the first-build notice is recognised by. The sentence around
/// them is the recipe's to word; that it is said, and said before cargo has
/// printed anything, is the scenario.
const FIRST_BUILD: &str = "building the board for the first time";

/// A `cargo` that compiles nothing. It records the argument list it was
/// handed, then plays the two streams the scenario is about: cargo's own
/// chatter on stderr, the board's frame on stdout.
///
/// The log is appended to rather than overwritten — the first-build
/// scenario launches twice, and has to see that the second launch reached
/// cargo at all rather than being skipped along with its notice.
const CARGO_STUB: &str = r#"#!/usr/bin/env bash
{ for a in "$@"; do printf '%s\037' "$a"; done; printf '\n'; } >> "$KEELER_STUB_CARGO_LOG"
echo "   Compiling ratatui v0.30.2" >&2
echo "    Finished release profile [optimized] target(s)" >&2
printf '%s' "$KEELER_STUB_CARGO_FRAME"
"#;

/// A plugin and a project in two directories that are not each other,
/// which is the whole point of this fixture: the recipe hands the binary
/// both paths, and a project holding the Justfile could not tell one from
/// the other.
///
/// The plugin is a copy of the shipped `Justfile` with a `keeler-top/`
/// beside it; the project gets a decoy crate of the same name, so a recipe
/// reaching for `keeler-top/Cargo.toml` relative to the working directory
/// would find one and still be pointed at the wrong tree.
struct Front {
    dir: PathBuf,
    plugin: PathBuf,
    project: PathBuf,
}

impl Front {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("keeler-front-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (plugin, project) = (dir.join("plugin"), dir.join("project"));
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        std::fs::create_dir_all(plugin.join("keeler-top")).unwrap();
        std::fs::create_dir_all(project.join("keeler-top")).unwrap();
        std::fs::create_dir_all(project.join("specs")).unwrap();
        std::fs::copy(repo_root().join("Justfile"), plugin.join("Justfile")).unwrap();
        for manifest in [
            plugin.join("keeler-top/Cargo.toml"),
            project.join("keeler-top/Cargo.toml"),
        ] {
            std::fs::write(manifest, "[package]\nname = \"keeler-top\"\n").unwrap();
        }
        std::fs::write(project.join("specs/01-foo.md"), "# Spec 01 — foo\n").unwrap();
        let stub = dir.join("bin/cargo");
        std::fs::write(&stub, CARGO_STUB).unwrap();
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
        // Resolved, because the recipe answers with `pwd -P` and the
        // wrapper with `cd -P`: on macOS the temporary directory is under
        // /var, a symlink to /private/var, and an unresolved path would
        // compare unequal to every path they report.
        let front = Self {
            plugin: std::fs::canonicalize(&plugin).unwrap(),
            project: std::fs::canonicalize(&project).unwrap(),
            dir: std::fs::canonicalize(&dir).unwrap(),
        };
        // A repository, because that is what the wrapper asks the project
        // for: `git rev-parse --show-toplevel` is where it gets the root it
        // passes as the working directory.
        let git = Command::new("git")
            .args(["init", "-qb", "main"])
            .current_dir(&front.project)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .output()
            .expect("failed to run git");
        assert!(
            git.status.success(),
            "{}",
            String::from_utf8_lossy(&git.stderr)
        );
        front
    }

    /// The path the recipe probes to decide whether the board has ever
    /// been built: a workspace member's artifacts land in the workspace's
    /// `target/`, not in its own.
    fn binary(&self) -> PathBuf {
        self.plugin.join("target/release/keeler-top")
    }

    /// A board that has been built once already.
    fn build(&self) {
        let binary = self.binary();
        std::fs::create_dir_all(binary.parent().unwrap()).unwrap();
        std::fs::write(&binary, "#!/usr/bin/env bash\ntrue\n").unwrap();
        std::fs::set_permissions(binary, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn log(&self) -> PathBuf {
        self.dir.join("cargo-calls")
    }

    /// The recipe as `keeler` runs it — the plugin's Justfile, the project
    /// as working directory — with the stub cargo first on PATH.
    fn top(&self, args: &[&str]) -> Output {
        let mut command = Command::new(real_just());
        command
            .arg("--justfile")
            .arg(self.plugin.join("Justfile"))
            .arg("--working-directory")
            .arg(&self.project)
            .arg("keeler-top")
            .args(args);
        self.run(command)
    }

    /// The recipe through the front door itself: `bin/keeler` as it ships,
    /// whose plugin is this repository and whose project is whichever one
    /// the caller is standing in.
    fn front_door(&self, args: &[&str]) -> Output {
        let mut command = Command::new(repo_root().join("bin/keeler"));
        command.arg("keeler-top").args(args);
        self.run(command)
    }

    /// The one place a launch is run, so the two doors above cannot differ
    /// in what the stub is given to see.
    fn run(&self, mut command: Command) -> Output {
        let path = std::env::var("PATH").unwrap();
        command
            .current_dir(&self.project)
            .env("PATH", format!("{}:{path}", self.dir.join("bin").display()))
            .env("KEELER_STUB_CARGO_LOG", self.log())
            .env("KEELER_STUB_CARGO_FRAME", STUB_FRAME)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .output()
            .expect("failed to launch the board")
    }

    /// Every argument list the stub cargo was handed, oldest first.
    fn calls(&self) -> Vec<Vec<String>> {
        let Ok(text) = std::fs::read_to_string(self.log()) else {
            return Vec::new();
        };
        text.lines()
            .filter(|line| !line.is_empty())
            .map(|line| {
                line.strip_suffix('\u{1f}')
                    .unwrap_or(line)
                    .split('\u{1f}')
                    .map(str::to_string)
                    .collect()
            })
            .collect()
    }

    /// The single launch this fixture made.
    fn call(&self) -> Vec<String> {
        let calls = self.calls();
        assert_eq!(calls.len(), 1, "cargo was not run exactly once: {calls:?}");
        calls.into_iter().next().expect("one call")
    }
}

impl Drop for Front {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// The value a recorded call gives a flag.
fn after<'a>(call: &'a [String], flag: &str) -> &'a str {
    let at = call
        .iter()
        .position(|arg| arg == flag)
        .unwrap_or_else(|| panic!("cargo was never handed {flag}: {call:?}"));
    call.get(at + 1)
        .unwrap_or_else(|| panic!("{flag} was handed nothing: {call:?}"))
}

/// What the binary was handed: everything after cargo's own `--`.
fn passed(call: &[String]) -> Vec<&str> {
    call.split(|arg| arg.as_str() == "--")
        .nth(1)
        .unwrap_or_else(|| panic!("cargo was never given a `--` to pass arguments after: {call:?}"))
        .iter()
        .map(String::as_str)
        .collect()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn the_recipe_builds_the_crate_from_the_plugin_tree_and_passes_it_three_paths() {
    // Given a plugin whose Justfile does not live in the project it is
    // watching, and a project carrying a decoy crate of the same name
    let front = Front::new("three-paths");

    // When the board is asked for, with a flag typed after the spec
    let output = front.top(&["specs/01-foo.md", "--once"]);
    assert!(output.status.success(), "{}", both(&output));

    // Then cargo was asked to run the crate in the plugin's own tree —
    // named against the Justfile's directory and not the working one, so
    // the board an adopter gets is the plugin's and not whatever crate
    // their project happens to have under that name
    let call = front.call();
    assert_eq!(
        call.first().map(String::as_str),
        Some("run"),
        "cargo was not asked to run anything: {call:?}"
    );
    assert!(
        call.iter().any(|arg| arg == "--release"),
        "the board is built unoptimised: {call:?}"
    );
    assert_eq!(
        after(&call, "--manifest-path"),
        front.plugin.join("keeler-top/Cargo.toml").to_str().unwrap(),
        "the recipe did not build the crate beside the Justfile it was run from: {call:?}"
    );

    // And the binary was handed the plugin root, the project root and the
    // spec — the three paths it has no other way of knowing — with the
    // flag that followed the spec after them, so what an adopter types
    // reaches the board rather than stopping at `just`
    assert_eq!(
        passed(&call),
        vec![
            "--plugin-root",
            front.plugin.to_str().unwrap(),
            "--root",
            front.project.to_str().unwrap(),
            "specs/01-foo.md",
            "--once",
        ],
    );
}

#[test]
fn once_through_the_front_door_prints_the_table_on_stdout() {
    // Given a project watched through `keeler` itself, the wrapper as it
    // ships — so the plugin is this repository and the manifest cargo is
    // pointed at is the real one
    let front = Front::new("front-door");

    // When `keeler keeler-top --once <spec>` runs there
    let output = front.front_door(&["--once", "specs/01-foo.md"]);
    assert!(output.status.success(), "{}", both(&output));

    // Then stdout holds the table and nothing else. The recipe's own
    // notice and cargo's chatter are stderr's, so `keeler keeler-top
    // --once <spec>` in a script reads a frame rather than a build log.
    assert_eq!(
        stdout(&output),
        STUB_FRAME,
        "stdout carries more than the frame:\n{}",
        both(&output)
    );

    // And cargo's build output, such as there is, went to stderr
    assert!(
        stderr(&output).contains("Compiling"),
        "cargo's own output is not on stderr:\n{}",
        both(&output)
    );

    // And the wrapper's two paths reached the binary: the plugin it was
    // run from, and the repository the caller was standing in
    let call = front.call();
    assert_eq!(
        after(&call, "--plugin-root"),
        std::fs::canonicalize(repo_root())
            .unwrap()
            .to_str()
            .unwrap(),
        "the front door named a plugin that is not the one it ships in: {call:?}"
    );
    assert_eq!(
        after(&call, "--root"),
        front.project.to_str().unwrap(),
        "the front door named a project that is not the one it was run in: {call:?}"
    );
    let passed = passed(&call);
    assert_eq!(
        &passed[passed.len() - 2..],
        ["--once", "specs/01-foo.md"],
        "the flag and the spec did not reach the binary in the order they were typed"
    );
}

#[test]
fn the_first_launch_says_it_is_building_before_cargo_starts() {
    // Given a plugin whose board has never been built
    let front = Front::new("first-build");
    assert!(
        !front.binary().exists(),
        "the fixture starts with a board already built"
    );

    // When the board is asked for
    let output = front.top(&["specs/01-foo.md", "--once"]);
    assert!(output.status.success(), "{}", both(&output));

    // Then stderr says so before cargo has printed a word. The first
    // launch compiles ratatui and its tree, and the pinned toolchain may
    // be fetched before that — minutes in which a silent terminal reads
    // as a hang, and the reader has no way to know it is not one.
    let said = stderr(&output);
    let notice = said
        .find(FIRST_BUILD)
        .unwrap_or_else(|| panic!("the first launch says nothing about building:\n{said}"));
    let toolchain = said.find("rust-toolchain.toml").unwrap_or_else(|| {
        panic!("the notice does not mention the toolchain it may fetch:\n{said}")
    });
    let compiling = said
        .find("Compiling")
        .unwrap_or_else(|| panic!("cargo never ran:\n{said}"));
    assert!(
        notice < compiling && toolchain < compiling,
        "the notice arrives after cargo's own output, where nobody waiting reads it:\n{said}"
    );

    // And once the binary is there it is not said again: the notice is
    // about the wait, and there is no wait to warn of
    front.build();
    let again = front.top(&["specs/01-foo.md", "--once"]);
    assert!(again.status.success(), "{}", both(&again));
    assert!(
        !stderr(&again).contains(FIRST_BUILD),
        "the notice is printed on every launch:\n{}",
        stderr(&again)
    );
    assert_eq!(
        front.calls().len(),
        2,
        "the second launch never reached cargo at all"
    );
}
