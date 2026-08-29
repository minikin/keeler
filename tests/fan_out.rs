//! Spec 07 — fan-out. Two tasks wrote this file in parallel worktrees,
//! each with the fixture its own scenarios needed, and neither could see
//! the other's. They are kept apart rather than reconciled: a `Project`
//! that cuts feature branches and one that drives waves have almost
//! nothing in common but the name, and merging them would have invented
//! a third fixture neither task tested against.

mod wave {
    use std::fmt::Write as _;
    use std::io::Write as _;
    use std::os::unix::fs::PermissionsExt as _;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Output, Stdio};
    use std::sync::OnceLock;
    use std::thread;
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
    /// `new-session` runs the command it was given only when asked to, and
    /// `-P` prints a pane id for the caller that keeps one.
    /// A session inherits the environment of whoever started it, so what
    /// `KEELER_FAN_OUT_YES` reads here is what the spawned agent would get:
    /// recorded per `new-session`, one line apiece.
    const TMUX_STUB: &str = r#"#!/usr/bin/env bash
    { for a in "$@"; do printf '%s\037' "$a"; done; printf '\036'; } >> "$KEELER_STUB_TMUX_LOG"
    if [ "${1:-}" = new-session ]; then
        printf 'KEELER_FAN_OUT_YES=%s\n' "${KEELER_FAN_OUT_YES-<unset>}" >> "$KEELER_STUB_TMUX_ENV"
    fi
    case "${1:-}" in
    split-window)
        if [ "${KEELER_STUB_TMUX_SPLIT_FAIL:-0}" = 1 ]; then
            echo "can't find pane" >&2
            exit 1
        fi
        exit 0
        ;;
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
        dir=""; cmd=""; prev=""; before=""; printed=""
        for a in "$@"; do
            [ "$prev" = -c ] && dir="$a"
            [ "$a" = -P ] && printed=1
            before="$prev"; prev="$a"; cmd="$a"
        done
        # A session created without a command ends on a flag's value, not on
        # a command: `-y 50` is not something to run.
        case "$before" in -*) cmd="" ;; esac
        if [ "${KEELER_STUB_TMUX_RUN:-0}" = 1 ] && [ -n "$cmd" ]; then ( cd "${dir:-.}" && eval "$cmd" ) || true; fi
        [ -n "$printed" ] && echo "%0"
        exit 0
        ;;
    esac
    exit 0
    "#;

    /// What the real binary emits for a turn that finished: a result
    /// record, which is the only thing the runner gates on. The older
    /// two-line stub predated that check and made every stubbed run die
    /// before its gate — which is how a test came to assert `passed`
    /// against a board that said `died`.
    ///
    /// `KEELER_STUB_CLAUDE_HOLD` keeps the turn open for that many seconds,
    /// which is the only way a session started by a real tmux is still
    /// running when the test looks at it: a stub that returns at once takes
    /// its session down with it before the view is even built.
    const CLAUDE_STUB: &str = r#"#!/usr/bin/env bash
    { for a in "$@"; do printf '%s\037' "$a"; done; printf '\036'; } >> "$KEELER_STUB_CLAUDE_LOG"
    if [ -n "${KEELER_STUB_CLAUDE_HOLD:-}" ]; then sleep "$KEELER_STUB_CLAUDE_HOLD"; fi
    echo '{"type":"assistant","message":{"content":[{"type":"text","text":"claude stub: the turn ended"}]}}'
    echo '{"is_error":false,"type":"result","subtype":"success"}'
    exit 0
    "#;

    /// Stands in for the two gates a fixture cannot run — `just
    /// keeler-branch`, which a spawned run ends with, and `just dev`, which
    /// `keeler-land` opens with. Everything else is the real `just`.
    const JUST_STUB: &str = r#"#!/usr/bin/env bash
    case "${1:-}" in
    keeler-branch)
        echo "keeler-branch stub: the gate ran"
        exit "${KEELER_STUB_BRANCH_EXIT:-0}"
        ;;
    dev)
        echo "dev stub: the full gate ran"
        exit 0
        ;;
    esac
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
                "/bin/\n/.keeler/\n/tmux-calls\n/tmux-env\n/claude-calls\n",
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

        /// Takes the stub tmux off the project's PATH, so the recipes reach
        /// the real one. Everything else — `claude`, the branch gate — is
        /// still stubbed: what is under test is tmux and nothing besides.
        fn use_real_tmux(&self) {
            std::fs::remove_file(self.dir.join("bin/tmux")).unwrap();
        }

        /// Closes a task on its own branch: the review record and the
        /// tick, committed there. Without both the board says
        /// `incomplete`, which is right and is a different clause.
        fn close_on_branch(&self, slug: &str, task: &str) {
            let tid = task.to_lowercase();
            let wt = self.worktree(slug, task);
            let reviews = wt.join("reviews").join(slug);
            std::fs::create_dir_all(&reviews).unwrap();
            std::fs::write(
                reviews.join(format!("{tid}.md")),
                format!("Spec: {slug}\nTask: {tid}\nCommit: fixture\nVerdict: pass\n"),
            )
            .unwrap();
            let spec = wt.join("specs").join(format!("{slug}.md"));
            let text = std::fs::read_to_string(&spec).unwrap();
            std::fs::write(
                &spec,
                text.replace(&format!("- [ ] **{task}"), &format!("- [x] **{task}")),
            )
            .unwrap();
            for args in [
                vec!["add", "-A"],
                vec!["commit", "-qm", "the record and the tick"],
            ] {
                let out = Command::new("git")
                    .args(["-c", "user.email=p@k", "-c", "user.name=p"])
                    .args(&args)
                    .current_dir(&wt)
                    .env("GIT_CONFIG_GLOBAL", "/dev/null")
                    .output()
                    .expect("git");
                assert!(
                    out.status.success(),
                    "git {args:?}: {}",
                    String::from_utf8_lossy(&out.stderr)
                );
            }
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
        /// a yes into a run that is meant to ask, nor a `TMUX` into a run
        /// whose output says whether it thinks it is inside one. (It does:
        /// this suite is often run from a tmux pane.)
        fn just_command(&self, args: &[&str]) -> Command {
            let path = std::env::var("PATH").unwrap();
            let mut command = Command::new(real_just());
            command
                .args(args)
                .current_dir(&self.dir)
                .env("PATH", format!("{}:{path}", self.dir.join("bin").display()))
                .env("KEELER_STUB_TMUX_LOG", self.dir.join("tmux-calls"))
                .env("KEELER_STUB_TMUX_ENV", self.dir.join("tmux-env"))
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
                .env_remove("KEELER_FAN_OUT_YES")
                .env_remove("TMUX");
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

        /// Every recorded call to the stub tmux whose verb is `verb`, in the
        /// order it was made.
        fn tmux_calls(&self, verb: &str) -> Vec<Vec<String>> {
            self.calls("tmux")
                .into_iter()
                .filter(|call| call.first().is_some_and(|first| first == verb))
                .collect()
        }

        fn new_sessions(&self) -> Vec<Vec<String>> {
            self.tmux_calls("new-session")
        }

        /// The sessions the stub tmux was asked to start, in the order it was
        /// asked — the wave's runs, and the view fan-out builds over them.
        fn started_sessions(&self) -> Vec<String> {
            self.new_sessions()
                .iter()
                .filter_map(|call| flag(call, "-s").map(str::to_string))
                .collect()
        }

        /// The task sessions alone, in the order they were started — which is
        /// the order the wave was spawned in, since one `keeler-spawn` starts
        /// exactly one. The view is a session too, and it is not a run.
        fn task_sessions(&self) -> Vec<String> {
            let view = view_session(SLUG);
            self.started_sessions()
                .into_iter()
                .filter(|name| *name != view)
                .collect()
        }

        /// What `KEELER_FAN_OUT_YES` read in the environment each session was
        /// started with — which is the environment the agent inherits.
        fn session_environments(&self) -> Vec<String> {
            std::fs::read_to_string(self.dir.join("tmux-env"))
                .unwrap_or_default()
                .lines()
                .map(str::to_string)
                .collect()
        }

        fn branch_exists(&self, slug: &str, task: &str) -> bool {
            !self
                .git(&[
                    "branch",
                    "--list",
                    &format!("keeler/{slug}/{}", task.to_lowercase()),
                ])
                .is_empty()
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

    /// The session `keeler-spawn` gives one task, which is the name the board
    /// and a single attach use.
    fn session(slug: &str, task: &str) -> String {
        format!("keeler-{slug}-{}", task.to_lowercase())
    }

    /// The session holding the view fan-out builds over a wave — a name that
    /// is not a prefix of any task's, and that no task's is a prefix of.
    fn view_session(slug: &str) -> String {
        format!("keeler-{slug}-wave")
    }

    /// What one recorded call asked tmux to do.
    fn verb(call: &[String]) -> &str {
        call.first().map_or("", String::as_str)
    }

    /// The value tmux was given for a flag — `-s`, `-t`, `-x` — in one call.
    fn flag<'a>(call: &'a [String], name: &str) -> Option<&'a str> {
        let at = call.iter().position(|arg| arg == name)?;
        call.get(at + 1).map(String::as_str)
    }

    /// The tmux calls with one of these verbs, in order, each written as what
    /// it asked for: the verb, and for a layout the target and the layout it
    /// set. The sequence is how the window was built.
    fn steps_of(project: &Project, verbs: &[&str]) -> Vec<String> {
        project
            .calls("tmux")
            .into_iter()
            .filter(|call| verbs.contains(&verb(call)))
            .map(|call| match verb(&call) {
                "select-layout" => format!(
                    "select-layout {} {}",
                    flag(&call, "-t").unwrap_or_default(),
                    pane_command(&call)
                ),
                "kill-pane" => format!("kill-pane {}", flag(&call, "-t").unwrap_or_default()),
                other => other.to_string(),
            })
            .collect()
    }

    /// The command a pane was given, which is tmux's last argument.
    fn pane_command(call: &[String]) -> &str {
        call.last().map_or("", String::as_str)
    }

    /// The session a pane's command attaches to: the last word of it, since
    /// the runner takes the session as its one argument.
    fn pane_target(call: &[String]) -> &str {
        pane_command(call)
            .split_whitespace()
            .last()
            .unwrap_or_default()
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
    /// The state field of a board line — the second word, which is what
    /// the board decides. Matching the whole line matches the fixture's
    /// own paths, and a directory name is not a verdict.
    fn state_of(said: &str, id: &str) -> String {
        task_line(said, id)
            .split_whitespace()
            .nth(1)
            .unwrap_or_default()
            .to_string()
    }

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

    /// The ids on one of the run's two outcome lines — what it says spawned,
    /// and what it says was refused. The whole line after the prefix, so a
    /// summary that names no task at all reads as empty rather than as
    /// whatever else the run happened to print.
    fn reported(said: &str, outcome: &str) -> Vec<String> {
        said.lines()
            .find_map(|line| line.strip_prefix(&format!("keeler-fan-out: {outcome} ")))
            .map(|ids| {
                ids.split_whitespace()
                    .take_while(|word| word.starts_with('T'))
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default()
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
                state_of(&said, id) == "ready",
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
        // The third state: a task whose gate ran green and which has not
        // landed. Closed is three things, so the record and the tick go on
        // its branch too — without them the board says `incomplete`, which
        // is a fourth thing this clause does not claim.
        let mut green = Project::new("board-green");
        green.run_sessions = true;
        green.branch_exit = 0;
        assert!(green.spawn(SLUG, "T2").status.success());
        green.run_sessions = false;
        green.close_on_branch(SLUG, "T2");
        let said = stdout(&green.fan_out(SLUG, &Answer::Piped("no\n")));
        assert_eq!(wave_of(&said), ["T3", "T5"], "{said}");
        // The state field, not the whole line: the line carries the
        // fixture's own directory name, and a fixture called board-passed
        // satisfied `contains("passed")` while the board said `died`.
        assert_eq!(
            state_of(&said, "T2"),
            "passed",
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
        // Polled, not waited: a blocking wait shares the hang it is here to
        // detect, and a bound checked after the wait returns can only ever
        // fire on a process that exited — that is, on a slow success under
        // load, never on the hang. The deadline is generous because it must
        // hold under a parallel wave's gates and coverage instrumentation,
        // where an honest refusal has been observed to take over a minute.
        let started = Instant::now();
        while child.try_wait().unwrap().is_none() {
            if started.elapsed() > Duration::from_secs(120) {
                child.kill().ok();
                let said = both(&child.wait_with_output().unwrap());
                panic!("the run hung on an open pipe:\n{said}");
            }
            thread::sleep(Duration::from_millis(50));
        }
        let output = child.wait_with_output().unwrap();
        drop(held_open);
        let said = both(&output);
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
                said.to_lowercase().contains("nothing is ready"),
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
    fn yes_spawns_the_whole_wave_through_keeler_spawn() {
        // Given the wave — T2, T3 and T5 ready, T4 blocked on T2 — and the
        // answer yes
        let project = Project::new("spawns");

        // When fan-out continues
        let output = project.fan_out(SLUG, &Answer::Piped("yes\n"));
        let said = both(&output);
        assert!(output.status.success(), "the wave did not spawn:\n{said}");

        // Then it runs `just keeler-spawn <spec> T2`, T3 and T5, in that
        // order — one session apiece, started in the wave's order
        assert_eq!(wave_of(&stdout(&output)), ["T2", "T3", "T5"], "{said}");
        assert_eq!(
            project.task_sessions(),
            ["T2", "T3", "T5"].map(|id| session(SLUG, id)),
            "the wave was not spawned in order:\n{said}"
        );

        // And through the same recipe a hand would use: what each task got is
        // keeler-spawn's own doing — its branch, its worktree, its runner —
        // and its report is in the run's own words
        for id in ["T2", "T3", "T5"] {
            let tid = id.to_lowercase();
            assert!(
                project.worktree(SLUG, id).exists(),
                "{id} has no worktree:\n{said}"
            );
            assert!(
                project.branch_exists(SLUG, id),
                "{id} has no branch:\n{said}"
            );
            assert!(
                project.runs(SLUG).join(format!("{tid}.sh")).is_file(),
                "{id} has no runner:\n{said}"
            );
            assert!(
                said.contains(&format!("spawned {id} on keeler/{SLUG}/{tid}")),
                "the run does not report {id}'s outcome in keeler-spawn's words:\n{said}"
            );
        }

        // And it reports the wave's outcome in its own words too: all three
        // spawned, none refused
        assert_eq!(
            reported(&stdout(&output), "spawned"),
            ["T2", "T3", "T5"],
            "the run does not say what it spawned:\n{said}"
        );
        assert!(
            reported(&stderr(&output), "refused").is_empty(),
            "the run refused something after spawning everything:\n{said}"
        );

        // And the blocked task is not among them: the wave is what was offered
        assert!(
            !project.worktree(SLUG, "T4").exists() && !project.branch_exists(SLUG, "T4"),
            "the blocked T4 was spawned:\n{said}"
        );

        // And the wave is a loop over keeler-spawn, not a second
        // implementation of it: the guards and the cut live there alone
        let justfile = std::fs::read_to_string(repo_root().join("Justfile")).unwrap();
        let body = recipe_body(&justfile, "keeler-fan-out");
        assert!(
            body.contains("keeler-spawn"),
            "keeler-fan-out does not run keeler-spawn:\n{body}"
        );
        assert!(
            !body.contains("git worktree add"),
            "keeler-fan-out cuts a worktree of its own:\n{body}"
        );
    }

    #[test]
    fn a_refusal_mid_wave_is_named_and_the_rest_of_the_wave_still_spawns() {
        // Given the wave, and T3 become spawned meanwhile — its branch cut
        // after the board was read, which is what a second hand looks like
        let project = Project::new("refused-mid-wave");
        project.git(&["branch", &format!("keeler/{SLUG}/t3")]);

        // When fan-out continues on a yes
        let output = project.fan_out(SLUG, &Answer::Piped("yes\n"));
        let said = both(&output);

        // Then T3 is refused and named, in keeler-spawn's own words
        assert!(
            stderr(&output).contains(&format!("branch keeler/{SLUG}/t3 already exists")),
            "T3's refusal is not keeler-spawn's:\n{said}"
        );
        assert!(
            !project.worktree(SLUG, "T3").exists(),
            "the refused T3 was given a worktree:\n{said}"
        );

        // And the rest of the wave still spawns, in order
        assert_eq!(
            project.task_sessions(),
            ["T2", "T5"].map(|id| session(SLUG, id)),
            "the refusal took the rest of the wave with it:\n{said}"
        );
        for id in ["T2", "T5"] {
            assert!(
                project.worktree(SLUG, id).exists() && project.branch_exists(SLUG, id),
                "{id} did not spawn after T3's refusal:\n{said}"
            );
        }

        // And the run reports each task's outcome — the two it spawned and the
        // one it did not, each named on its own line — and exits non-zero,
        // because a spawn refused
        assert_eq!(
            reported(&stdout(&output), "spawned"),
            ["T2", "T5"],
            "the run does not say what it spawned:\n{said}"
        );
        assert_eq!(
            reported(&stderr(&output), "refused"),
            ["T3"],
            "the run does not say what was refused:\n{said}"
        );
        assert!(
            !output.status.success(),
            "a refused spawn was reported as a whole wave:\n{said}"
        );
    }

    #[test]
    fn the_yes_does_not_travel_to_the_agents_it_spawned() {
        // Given a wave and the yes given in advance, the way a caller who has
        // already decided gives it
        let project = Project::new("yes-does-not-travel");

        // When fan-out spawns the wave
        let output = project.fan_out_with(SLUG, &Answer::Nobody, &[("KEELER_FAN_OUT_YES", "1")]);
        let said = both(&output);
        assert!(output.status.success(), "{said}");
        assert_eq!(project.task_sessions().len(), 3, "{said}");

        // Then no session was started carrying the answer: an agent that
        // inherited it could run a wave of its own with the one question the
        // human's already answered. Four sessions, because the view over the
        // wave is one too, and the panes it holds inherit its environment.
        assert_eq!(
            project.session_environments(),
            vec!["KEELER_FAN_OUT_YES=<unset>"; 4],
            "the yes travelled into the sessions it spawned:\n{said}"
        );
    }

    #[test]
    fn the_next_wave_is_a_re_run() {
        // Given a wave that landed: T2, T3 and T5 spawned...
        let project = Project::new("re-run");
        let first = project.fan_out(SLUG, &Answer::Piped("yes\n"));
        assert!(first.status.success(), "{}", both(&first));
        assert_eq!(wave_of(&stdout(&first)), ["T2", "T3", "T5"]);

        // ...T2 finished — its review record and its tick, committed on its
        // own branch — its tick merged into feat/<spec-slug>, and
        // `just keeler-land` run there
        project.close_on_branch(SLUG, "T2");
        project.git(&["merge", "-q", "--no-edit", &format!("keeler/{SLUG}/t2")]);
        let landed = project.just(&["keeler-land"]);
        assert!(landed.status.success(), "{}", both(&landed));
        assert!(
            !project.worktree(SLUG, "T2").exists(),
            "the landing left T2's worktree behind:\n{}",
            both(&landed)
        );

        // When `just keeler-fan-out <spec>` runs again
        let output = project.fan_out(SLUG, &Answer::Piped("no\n"));
        let said = stdout(&output);

        // Then the tasks the landing unblocked are the new wave: T4 waited on
        // T2 and waits no longer
        assert_eq!(wave_of(&said), ["T4"], "{said}");

        // And the tasks already done are listed as done, not offered
        for id in ["T1", "T2"] {
            assert_eq!(
                state_of(&said, id),
                "done",
                "{id} is not listed as done:\n{said}"
            );
        }

        // And the tasks still out there are the board's, as they were
        for id in ["T3", "T5"] {
            assert_eq!(
                state_of(&said, id),
                "died",
                "{id} is not listed in the board's words:\n{said}"
            );
        }
    }

    #[test]
    fn the_wave_is_one_tmux_window_with_a_pane_per_run() {
        // Given a wave of three tasks spawned by fan-out
        let project = Project::new("one-window");
        let output = project.fan_out(SLUG, &Answer::Piped("yes\n"));
        let said = both(&output);
        assert!(output.status.success(), "the wave did not spawn:\n{said}");

        // Then a tmux session named keeler-<spec-slug>-wave holds one pane per
        // task that spawned, each attached to that task's session
        let view = view_session(SLUG);
        // A pane of that window, exactly: the `=` is read off the session part
        // of a target, so a target-pane says `=session:` — the trailing colon
        // is the session's current window. `=session` alone is looked up as a
        // pane name, and a real tmux does not find one.
        let pane_of = format!("={view}:");
        assert!(
            project.started_sessions().contains(&view),
            "no session {view} was created:\n{said}"
        );
        let splits = project.tmux_calls("split-window");
        let mut panes = Vec::new();
        for split in &splits {
            assert_eq!(
                flag(split, "-t"),
                Some(pane_of.as_str()),
                "a pane was split into some other window: {split:?}"
            );
            panes.push(pane_target(split).to_string());
        }
        assert_eq!(
            panes,
            ["T2", "T3", "T5"].map(|id| session(SLUG, id)),
            "the view is not one pane per run, each attached to that run:\n{said}"
        );

        // And each pane runs the attach through a runner, not inline: the pane
        // is run by the user's shell, and in zsh a word starting with `=` is a
        // command to look up rather than a tmux target
        let runner = project.runs(SLUG).join("wave.sh");
        assert!(runner.is_file(), "the wave has no runner at {runner:?}");
        let body = std::fs::read_to_string(&runner).unwrap();
        assert!(
            body.contains("TMUX=") && body.contains("=$1"),
            "the runner does not attach to an exact target with TMUX cleared:\n{body}"
        );
        for split in &splits {
            let command = pane_command(split);
            assert!(
                command.contains(&runner.display().to_string()),
                "a pane does not run the wave's runner: {command}"
            );
            assert!(
                !command.contains('='),
                "a pane carries the exact target on its command line: {command}"
            );
        }

        // And laid out so all are visible: every pane is tiled as it arrives,
        // because a detached window runs out of room otherwise — and once
        // more at the end, over the room the window's own pane gives back
        let tiled = format!("select-layout {pane_of} tiled");
        assert_eq!(
            steps_of(&project, &["split-window", "select-layout"]),
            [
                "split-window",
                tiled.as_str(),
                "split-window",
                tiled.as_str(),
                "split-window",
                tiled.as_str(),
                tiled.as_str()
            ],
            "the panes are not tiled as they are added:\n{said}"
        );

        // And every target the view uses is exact: tmux matches a bare name as
        // a prefix, and keeler-<spec-slug> is a prefix of every task's session.
        // A pane id — `%3` — is exact by construction and needs no marker.
        for call in project.calls("tmux") {
            if let Some(given) = flag(&call, "-t") {
                assert!(
                    given.starts_with('=') || given.starts_with('%'),
                    "a tmux target is not exact: {call:?}"
                );
            }
        }

        // And a wave that spawned nothing is nothing to show: no view is built
        let refused = Project::new("one-window-refused");
        for id in ["t2", "t3", "t5"] {
            refused.git(&["branch", &format!("keeler/{SLUG}/{id}")]);
        }
        let output = refused.fan_out(SLUG, &Answer::Piped("yes\n"));
        assert!(!output.status.success(), "{}", both(&output));
        assert!(
            refused.new_sessions().is_empty(),
            "a view was built over a wave that never spawned:\n{}",
            both(&output)
        );
    }

    /// Not a clause of its own — what makes the first one true when a run is
    /// short-lived. The window is built in a pane of its own and not in the
    /// first run's: a pane holding a run closes when that run ends, the last
    /// pane closing takes the session with it, and a task that fails in the
    /// second it takes to lay the wave out would leave every task after it
    /// with nowhere to be shown.
    #[test]
    fn the_window_is_built_in_a_pane_of_its_own_which_does_not_stay() {
        // Given a wave of three tasks spawned by fan-out
        let project = Project::new("anchor");
        let output = project.fan_out(SLUG, &Answer::Piped("yes\n"));
        let said = both(&output);
        assert!(output.status.success(), "the wave did not spawn:\n{said}");

        // Then the session was created detached, sized to split a wave into,
        // and holding no run of its own
        let view = view_session(SLUG);
        let made = project
            .new_sessions()
            .into_iter()
            .find(|call| flag(call, "-s") == Some(view.as_str()))
            .unwrap_or_else(|| panic!("no session {view} was created:\n{said}"));
        assert!(
            made.iter().any(|arg| arg == "-d"),
            "the view is not created detached: {made:?}"
        );
        for size in ["-x", "-y"] {
            assert!(
                flag(&made, size).is_some_and(|value| value.parse::<u32>().is_ok()),
                "the view is created without a {size} to split inside: {made:?}"
            );
        }
        assert!(
            !pane_command(&made).contains("wave.sh"),
            "the window is anchored to one of the runs: {made:?}"
        );

        // And the pane it was built in is asked for by id and killed once the
        // runs have theirs — after the last of them, never before
        assert_eq!(
            flag(&made, "-F"),
            Some("#{pane_id}"),
            "the view's own pane is not asked for by id: {made:?}"
        );
        assert_eq!(
            steps_of(&project, &["split-window", "kill-pane"]),
            [
                "split-window",
                "split-window",
                "split-window",
                "kill-pane %0"
            ],
            "the pane the window was built in outlived the wave, or took it with it:\n{said}"
        );
    }

    /// Not a clause of its own — the invariant the recipe states around the
    /// view: it is a window over the runs, and the exit code is about the
    /// runs. A tmux that will not build it may not turn a wave that went out
    /// into a wave that failed.
    #[test]
    fn a_view_tmux_will_not_build_is_said_and_not_counted() {
        // Given a wave, and a tmux that refuses to split a window
        let project = Project::new("view-refused");

        // When fan-out spawns it
        let output = project.fan_out_with(
            SLUG,
            &Answer::Piped("yes\n"),
            &[("KEELER_STUB_TMUX_SPLIT_FAIL", "1")],
        );
        let said = both(&output);

        // Then the wave went out all the same, and the run says so and exits
        // zero: the window is not what the exit code answers about
        assert!(
            output.status.success(),
            "a window that could not be built failed the wave:\n{said}"
        );
        assert_eq!(reported(&stdout(&output), "spawned"), ["T2", "T3", "T5"]);
        assert_eq!(
            project.task_sessions(),
            ["T2", "T3", "T5"].map(|id| session(SLUG, id)),
            "the runs did not spawn:\n{said}"
        );

        // And it says the view holds nothing, offers no attach to a window
        // that is not there, and leaves no empty one behind
        let view = view_session(SLUG);
        assert!(
            stderr(&output).contains("the view holds nothing"),
            "the run does not say the view was not built:\n{said}"
        );
        assert!(
            !stdout(&output).contains(&format!("={view}")),
            "the run offers an attach to a window it did not build:\n{said}"
        );
        assert!(
            project
                .tmux_calls("kill-session")
                .iter()
                .any(|call| flag(call, "-t") == Some(format!("={view}").as_str())),
            "the empty window was left standing:\n{said}"
        );

        // And a runner that cannot be written is the same: a disk that will
        // not take a file is not a wave that failed
        let unwritable = Project::new("view-unwritable");
        let runs = unwritable.runs(SLUG);
        std::fs::create_dir_all(&runs).unwrap();
        let runner = runs.join("wave.sh");
        std::fs::write(&runner, "as it was\n").unwrap();
        std::fs::set_permissions(&runner, std::fs::Permissions::from_mode(0o444)).unwrap();
        let output = unwritable.fan_out(SLUG, &Answer::Piped("yes\n"));
        std::fs::set_permissions(&runner, std::fs::Permissions::from_mode(0o644)).unwrap();
        let said = both(&output);
        assert!(
            output.status.success(),
            "a runner that could not be written failed the wave:\n{said}"
        );
        assert_eq!(reported(&stdout(&output), "spawned"), ["T2", "T3", "T5"]);
        assert_eq!(
            std::fs::read_to_string(&runner).unwrap(),
            "as it was\n",
            "the runner was written after all — the fixture proves nothing"
        );
        assert!(
            unwritable.tmux_calls("split-window").is_empty(),
            "panes were built for a runner that does not exist:\n{said}"
        );
    }

    /// The second clause of _The wave is one tmux window with a pane per run_:
    /// the run says how to get to the window it built.
    #[test]
    fn the_run_prints_the_command_that_shows_the_wave() {
        // Given a wave of three tasks spawned by fan-out
        let project = Project::new("shows-the-wave");
        let target = format!("={}", view_session(SLUG));

        // When it runs from outside tmux
        let output = project.fan_out(SLUG, &Answer::Piped("yes\n"));
        let said = both(&output);
        assert!(output.status.success(), "the wave did not spawn:\n{said}");

        // Then it prints the attach that shows the wave, exact target and all
        assert!(
            stdout(&output)
                .lines()
                .any(|line| line.contains("tmux attach") && line.contains(&target)),
            "the run does not print the attach that shows the wave:\n{said}"
        );

        // And `switch-client` when it is already inside tmux, where a nested
        // attach fails at once
        let inside = Project::new("shows-the-wave-inside-tmux");
        let nested =
            inside.fan_out_with(SLUG, &Answer::Piped("yes\n"), &[("TMUX", "/tmp/none,1,0")]);
        let said = both(&nested);
        assert!(nested.status.success(), "{said}");
        assert!(
            stdout(&nested)
                .lines()
                .any(|line| line.contains("switch-client") && line.contains(&target)),
            "inside tmux the run still prints an attach that would fail:\n{said}"
        );
    }

    /// The third clause of _The wave is one tmux window with a pane per run_:
    /// the view is over the runs and not instead of them.
    #[test]
    fn the_per_task_sessions_outlive_the_view_and_the_board_still_reads_them() {
        // Given a wave of three tasks spawned by fan-out
        let mut project = Project::new("sessions-remain");
        let output = project.fan_out(SLUG, &Answer::Piped("yes\n"));
        let said = both(&output);
        assert!(output.status.success(), "the wave did not spawn:\n{said}");

        // Then the per-task sessions keeler-<spec-slug>-<task> still exist, so
        // a single attach works exactly as before
        assert_eq!(
            project.task_sessions(),
            ["T2", "T3", "T5"].map(|id| session(SLUG, id)),
            "the view swallowed the sessions the runs are in:\n{said}"
        );

        // And keeler-status does too: with the three alive, the board says so
        project.sessions = ["T2", "T3", "T5"].map(|id| session(SLUG, id)).join(" ");
        let board = stdout(&project.just(&["keeler-status", &spec_path(SLUG)]));
        for id in ["T2", "T3", "T5"] {
            assert_eq!(
                state_of(&board, id),
                "running",
                "the board no longer finds {id}'s own session:\n{board}"
            );
        }

        // And the view is not mistaken for a run: with only it alive, no task
        // is running — its name is nobody's prefix, and the board asks exactly
        project.sessions = view_session(SLUG);
        let board = stdout(&project.just(&["keeler-status", &spec_path(SLUG)]));
        for id in ["T2", "T3", "T5"] {
            assert_ne!(
                state_of(&board, id),
                "running",
                "the view was taken for {id}'s session:\n{board}"
            );
        }
    }

    /// The stub tmux records what it was asked for and answers nothing, so
    /// only a real server can say whether what was asked for is a window with
    /// a client on every run. Ignored by default — it starts a tmux server and
    /// holds three sessions open — and run with
    /// `cargo nextest run --run-ignored all -E 'test(real_tmux)'`.
    #[test]
    #[ignore = "starts a real tmux server; run with --run-ignored all"]
    fn a_real_tmux_wave_is_a_window_with_a_client_on_every_run() {
        if Command::new("sh")
            .args(["-c", "command -v tmux"])
            .output()
            .is_ok_and(|out| !out.status.success())
        {
            return;
        }
        // Given a wave of three tasks, spawned with the real tmux — on a
        // server of the fixture's own, so the developer's sessions are neither
        // touched nor collided with
        let project = Project::new("real-tmux");
        project.use_real_tmux();
        // Not under the fixture: a unix socket's path is capped near 104
        // bytes, and the temporary directory a fixture lives in already
        // spends most of that on macOS.
        let socket = PathBuf::from("/tmp").join(format!("keeler-wave-{}", std::process::id()));
        std::fs::create_dir_all(&socket).unwrap();
        let socket = socket.display().to_string();
        let tmux = |args: &[&str]| -> Output {
            Command::new("tmux")
                .args(args)
                .env("TMUX_TMPDIR", &socket)
                .env_remove("TMUX")
                .output()
                .expect("failed to run tmux")
        };

        // When the spawns have started — each holding its turn open, so the
        // wave is still running when the view over it is looked at
        let output = project.fan_out_with(
            SLUG,
            &Answer::Piped("yes\n"),
            &[("TMUX_TMPDIR", &socket), ("KEELER_STUB_CLAUDE_HOLD", "20")],
        );
        let said = both(&output);
        assert!(output.status.success(), "the wave did not spawn:\n{said}");

        // Then the view holds one pane per run, each with a client on that
        // run's own session — which is what "attached to that task's session"
        // is, in tmux's own words
        let view = view_session(SLUG);
        let runs: Vec<String> = ["T2", "T3", "T5"].map(|id| session(SLUG, id)).to_vec();
        let deadline = Instant::now() + Duration::from_secs(15);
        let sessions = loop {
            let sessions = String::from_utf8_lossy(
                &tmux(&["list-sessions", "-F", "#{session_name} #{session_attached}"]).stdout,
            )
            .into_owned();
            let all_attached = runs
                .iter()
                .all(|run| sessions.lines().any(|line| line == format!("{run} 1")));
            if all_attached || Instant::now() > deadline {
                break sessions;
            }
            std::thread::sleep(Duration::from_millis(200));
        };
        let panes = String::from_utf8_lossy(
            &tmux(&["list-panes", "-t", &format!("={view}"), "-F", "#{pane_id}"]).stdout,
        )
        .lines()
        .count();
        // Everything is read before anything is asserted, so a failure takes
        // the server and its socket down with it rather than leaving three
        // sessions of somebody else's making behind.
        let _ = tmux(&["kill-server"]);
        let _ = std::fs::remove_dir_all(&socket);
        for run in &runs {
            assert!(
                sessions.lines().any(|line| line == format!("{run} 1")),
                "{run} has no client of its own from the view:\n{sessions}\n{said}"
            );
        }
        assert_eq!(
            panes, 3,
            "the view is not one window of three panes:\n{said}"
        );
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
}

mod fork {
    use std::path::{Path, PathBuf};
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
            just_from(&self.0, args)
        }

        fn dir(&self) -> &Path {
            &self.0
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

    /// `just`, run from a path that reaches the project — which is not always
    /// the path git will answer with. `PWD` is set the way a login shell sets
    /// it, because that is what makes bash's `pwd` logical.
    fn just_from(cwd: &Path, args: &[&str]) -> Output {
        Command::new(real_just())
            .args(args)
            .current_dir(cwd)
            .env("PWD", cwd)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .output()
            .expect("failed to run just")
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

    /// Not a scenario of its own — the path a human is standing in. `git
    /// rev-parse --show-toplevel` answers physically and a shell's `pwd`
    /// answers logically, so a repository reached through a symlink — /tmp on
    /// macOS is one — gives two names for one directory. A guard that
    /// compares them as strings refuses the spec it is looking at.
    #[test]
    fn a_repository_reached_through_a_symlink_is_still_this_repository() {
        // Given the project reached through a symlink, as a shell that
        // followed one would report it
        let project = Project::new("through-a-symlink");
        let link = std::env::temp_dir().join(format!("keeler-fan-out-link-{}", std::process::id()));
        let _ = std::fs::remove_file(&link);
        std::os::unix::fs::symlink(project.dir(), &link).unwrap();
        project.write_spec(&spec_body("Approved", TASKS));

        // When `just keeler-feature-branch <spec>` runs from there
        let output = just_from(&link, &["keeler-feature-branch", &spec_path()]);
        let _ = std::fs::remove_file(&link);

        // Then it is the same repository, and the same fork
        assert!(
            output.status.success(),
            "the fork refused a spec inside the repository it was run in:\n{}",
            both(&output),
        );
        assert_eq!(project.branch(), format!("feat/{SLUG}"));
        assert_eq!(project.touched("HEAD"), [spec_path()]);
    }

    /// Not a scenario of its own — the state a new spec is in when the human
    /// answers. `/keeler:spec` writes `specs/NN-slug.md` for the first time,
    /// and a human who ran `git add` before approving has a spec git knows
    /// and HEAD does not. Everything the recipe asks HEAD about has to be
    /// asked of HEAD, not of the index.
    #[test]
    fn a_spec_staged_but_never_committed_is_carried_across_like_any_other() {
        // Given a spec that exists only in the working tree and the index
        let project = Project::new("staged-not-committed");
        project.git(&["rm", "-q", "--cached", "--", &spec_path()]);
        project.git(&["commit", "-qm", "the repository before this spec existed"]);
        let approved = spec_body("Approved", TASKS);
        project.write_spec(&approved);
        project.git(&["add", "--", &spec_path()]);

        // When `just keeler-feature-branch <spec>` runs
        let output = project.feature_branch();

        // Then the branch is cut and the spec is committed on it, as it is for
        // a spec HEAD already knew
        assert!(output.status.success(), "{}", both(&output));
        assert_eq!(project.branch(), format!("feat/{SLUG}"));
        assert_eq!(project.touched("HEAD"), [spec_path()]);
        assert_eq!(
            project.git(&["show", &format!("HEAD:{}", spec_path())]),
            approved.trim()
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
}
