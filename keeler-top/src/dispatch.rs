//! Everything the board runs outside itself, behind one trait.
//!
//! The board is a reader: `keeler-status` decides what a task's state is,
//! `scripts/keeler-graph.sh` decides what its graph says, and the three
//! levers — pause, resume, attach — are `tmux` and one more recipe. All of
//! it is a subprocess, which is the one part of the board no unit test can
//! drive, so all of it goes through [`Dispatch`]: a test double records
//! what was asked for, and [`Shell`] is the one implementation that
//! actually runs anything.
//!
//! **Through the plugin's own Justfile, not the project's.** The board is
//! launched by `keeler keeler-top <spec>`, which runs the plugin's
//! Justfile with the project as working directory; the binary is handed
//! the plugin root so it can shell back to that same file. A bare `just
//! keeler-status` would look for a justfile in the project — where there
//! is none, or where there is a different one — which is the same reason
//! every recipe in that file names `{{justfile()}}` when it calls another.
//! `just` and `bash` come from PATH: how the board composes with the
//! shipped shell is behavior, and behavior is observed.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;

/// What the board asks the world for.
///
/// `Send + Sync` because one of these calls runs on a thread: `keeler-status`
/// is a `just` recipe, seconds where the board's tick is one, so it is asked
/// on a thread of its own and collected whenever it answers. Every
/// implementation is a description of how to run something rather than
/// something running, so the bound costs nothing to meet.
pub trait Dispatch: Send + Sync {
    /// `keeler-status`'s report for the spec the board is watching.
    ///
    /// # Errors
    ///
    /// The recipe's own refusal — not a git repository, a spec not
    /// committed on the ref it reads — relayed as it was written.
    fn status(&self) -> Result<String, String>;

    /// The graph script's report for a copy of the spec.
    ///
    /// # Errors
    ///
    /// The script's refusal: a cycle, or a `Needs:` naming no task.
    fn graph(&self, spec: &Path) -> Result<String, String>;

    /// Stops a task's session — the kill half of the board's `p`.
    ///
    /// # Errors
    ///
    /// tmux's own reason, which is what says whether anything was killed.
    /// The marker the board writes afterwards is a claim about this call,
    /// so nothing may be written on a refusal.
    fn kill(&self, session: &str) -> Result<(), String>;

    /// `keeler-resume <spec> <task>`, through the same Justfile.
    ///
    /// # Errors
    ///
    /// The recipe's refusal: a task still running, one that reached its
    /// gate, one that is done, one never spawned.
    fn resume(&self, task: &str) -> Result<String, String>;

    /// Whether the board is itself drawing inside tmux.
    ///
    /// Which decides both halves of `Enter`: what tmux is asked to do, and
    /// whether the board gives up its screen to let it.
    fn in_tmux(&self) -> bool;

    /// Puts the watcher in front of a task's session.
    ///
    /// # Errors
    ///
    /// tmux's own reason — a session that has ended between the report and
    /// the keypress, a tmux that is not installed at all.
    fn attach(&self, session: &str, inside: bool) -> Result<(), String>;
}

/// The implementation that runs things.
#[derive(Debug, Clone)]
pub struct Shell {
    plugin_root: PathBuf,
    root: PathBuf,
    spec: String,
}

impl Shell {
    /// The three paths `keeler keeler-top` hands the binary: where the
    /// plugin is, where the project is, and which spec is being watched.
    #[must_use]
    pub fn new(plugin_root: impl Into<PathBuf>, root: impl Into<PathBuf>, spec: &str) -> Self {
        Self {
            plugin_root: plugin_root.into(),
            root: root.into(),
            spec: spec.to_string(),
        }
    }

    /// One of the plugin's recipes, run with the project as working
    /// directory.
    ///
    /// Built apart from running it because the composition *is* the
    /// contract — which Justfile, and in which directory — and a test that
    /// only read the output could not tell a board that shelled back
    /// through the plugin from one that found a justfile somewhere else.
    fn recipe(&self, name: &str) -> Command {
        let mut command = Command::new("just");
        command
            .arg("--justfile")
            .arg(self.plugin_root.join("Justfile"))
            .arg("--working-directory")
            .arg(&self.root)
            // A refusal is the recipe's one sentence, written for whoever
            // has to act on it — not that sentence plus just's report that
            // a recipe the reader never named failed on some line of a
            // file they have never opened. `keeler-spawn` passes `-q` to
            // its own preflight for the same reason.
            //
            // It is safe for both recipes named here because both carry a
            // shebang: just runs such a recipe as one process and leaves
            // its two streams alone, while a *linewise* recipe under `-q`
            // gets both of them nulled — which would leave the board with
            // no report at all. The suite pins both halves.
            .arg("-q")
            .arg(name)
            .arg(&self.spec);
        command
    }

    /// The command [`Dispatch::status`] runs.
    #[must_use]
    pub fn status_command(&self) -> Command {
        self.recipe("keeler-status")
    }

    /// The command [`Dispatch::resume`] runs.
    ///
    /// The board adds nothing to it and gates nothing before it: the
    /// recipe refuses a task still running, one that reached its gate, one
    /// that is done and one never spawned, and a board with a second
    /// opinion about resumability would be a second answer to disagree
    /// with.
    #[must_use]
    pub fn resume_command(&self, task: &str) -> Command {
        let mut command = self.recipe("keeler-resume");
        command.arg(task);
        command
    }

    /// The command [`Dispatch::graph`] runs.
    #[must_use]
    pub fn graph_command(&self, spec: &Path) -> Command {
        let mut command = Command::new("bash");
        command
            .arg(self.plugin_root.join("scripts/keeler-graph.sh"))
            .arg(spec);
        command
    }
}

/// The command that stops a task's session.
///
/// The `=` is tmux's exact match, and every call in the Justfile carries it
/// for the same reason: without it `keeler-01-foo-t1` matches
/// `keeler-01-foo-t10` as a prefix, and the board would end a run nobody
/// asked it to.
#[must_use]
pub fn kill_command(session: &str) -> Command {
    let mut command = Command::new("tmux");
    command
        .arg("kill-session")
        .arg("-t")
        .arg(format!("={session}"));
    command
}

/// The command that puts a watcher in front of a task's session.
///
/// Two commands and not one, because inside tmux there is a client already:
/// it cannot attach a session to the terminal it is itself running in, and
/// what it does instead is move to it. Outside, tmux wants the terminal —
/// which is what makes the board give its screen back for one and not for
/// the other.
#[must_use]
pub fn attach_command(session: &str, inside: bool) -> Command {
    let mut command = Command::new("tmux");
    command
        .arg(if inside { "switch-client" } else { "attach" })
        .arg("-t")
        .arg(format!("={session}"));
    command
}

/// Whether the board is drawing inside tmux, from the variable tmux sets in
/// every client's environment.
///
/// A value rather than a look at the environment, so both answers are
/// reachable from a test: the one look is [`Shell::in_tmux`], which is a
/// line. Empty is outside — a variable exported without a value names no
/// server, and `attach` is the answer that works from a plain terminal.
#[must_use]
pub fn inside_tmux(tmux: Option<&OsStr>) -> bool {
    tmux.is_some_and(|value| !value.is_empty())
}

impl Dispatch for Shell {
    fn status(&self) -> Result<String, String> {
        run(self.status_command())
    }

    fn graph(&self, spec: &Path) -> Result<String, String> {
        run(self.graph_command(spec))
    }

    fn kill(&self, session: &str) -> Result<(), String> {
        run(kill_command(session)).map(drop)
    }

    fn resume(&self, task: &str) -> Result<String, String> {
        run(self.resume_command(task))
    }

    // Outside the mutation gate, and the only line in this file that is:
    // what the gate would replace it with is `true` or `false`, and a test
    // cannot arrange either. Setting a variable is a process-wide, unsafe
    // act in this edition, and the suite runs its tests in threads. The
    // decision the answer feeds is [`inside_tmux`], which is a function of
    // the variable and is tested against both of them.
    #[cfg_attr(test, mutants::skip)]
    fn in_tmux(&self) -> bool {
        inside_tmux(std::env::var_os("TMUX").as_deref())
    }

    fn attach(&self, session: &str, inside: bool) -> Result<(), String> {
        let command = attach_command(session, inside);
        // Inside tmux the board keeps the terminal, so `switch-client`'s
        // refusal has to be caught and put in the status line. Outside it,
        // the terminal is tmux's for as long as it runs and so is
        // everything it prints on it.
        if inside {
            return run(command).map(drop);
        }
        handed(command)
    }
}

/// Runs a command on the board's own terminal, rather than for its output.
///
/// `tmux attach` is a program the watcher is *in*: it wants the terminal
/// the board has just given back, and whatever it has to say it says on the
/// screen it is on. Only a command that could not be started, or one that
/// failed without a word of its own, is the board's to report.
///
/// # Errors
///
/// The command could not be started, or it ended unsuccessfully.
pub fn handed(mut command: Command) -> Result<(), String> {
    let program = command.get_program().to_string_lossy().into_owned();
    let status = command.status().map_err(|err| unstarted(&program, &err))?;
    if status.success() {
        return Ok(());
    }
    Err(format!("{program}: {status}"))
}

/// Runs a command for its stdout, or gives back the reason it failed.
///
/// The reason is the command's own stderr wherever there is one: every
/// recipe and script the board calls refuses in a sentence written for a
/// human, and a board that replaced those with words of its own would be
/// hiding the fix. Only a command that failed silently, or that could not
/// be started at all, is described from here.
fn run(mut command: Command) -> Result<String, String> {
    let program = command.get_program().to_string_lossy().into_owned();
    let output = command.output().map_err(|err| unstarted(&program, &err))?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
    }
    let said = String::from_utf8_lossy(&output.stderr).trim().to_string();
    Err(if said.is_empty() {
        format!("{program}: {}", output.status)
    } else {
        said
    })
}

/// A command that never started.
///
/// A missing program is named as one. `tmux` absent from an adopter's PATH
/// is the whole of what `p` and `Enter` need, and `just` absent is the whole
/// of what the board needs — and "No such file or directory (os error 2)" is
/// a sentence the reader has to translate before they can act on it.
fn unstarted(program: &str, err: &std::io::Error) -> String {
    if err.kind() == std::io::ErrorKind::NotFound {
        return format!("keeler-top: {program} is not installed, or not on this PATH.");
    }
    format!("{program}: {err}")
}

#[cfg(test)]
mod tests {
    use super::{Command, run};

    #[test]
    fn a_commands_stdout_is_its_answer() {
        let mut echo = Command::new("printf");
        echo.args(["T1 ready\n"]);

        assert_eq!(run(echo), Ok("T1 ready\n".to_string()));
    }

    #[test]
    fn a_refusal_is_relayed_in_the_words_that_made_it() {
        let mut refuse = Command::new("sh");
        refuse.args([
            "-c",
            "echo 'keeler-status: not a git repository' >&2; exit 1",
        ]);

        assert_eq!(
            run(refuse),
            Err("keeler-status: not a git repository".to_string()),
        );
    }

    #[test]
    fn a_command_that_failed_without_a_word_is_described_by_the_board() {
        let refused = run(Command::new("false")).expect_err("false fails");

        assert!(
            refused.starts_with("false: "),
            "the failure names neither the command nor its status: {refused}",
        );
    }

    #[test]
    fn a_command_that_is_not_there_is_a_refusal_and_not_a_panic() {
        // `just` absent from an adopter's PATH is exactly this, and the
        // board must say so rather than end — in words that name the fix
        // rather than in errno's.
        for refused in [
            run(Command::new("keeler-top-no-such-program")).expect_err("no such program"),
            super::handed(Command::new("keeler-top-no-such-program")).expect_err("no such program"),
        ] {
            assert_eq!(
                refused,
                "keeler-top: keeler-top-no-such-program is not installed, or not on this PATH.",
            );
        }
    }

    #[test]
    fn a_program_that_ran_on_the_boards_terminal_answers_with_how_it_ended() {
        // Nothing is captured here — a program the watcher is *in* has the
        // terminal — so success is silence and a failure is its status.
        assert_eq!(super::handed(Command::new("true")), Ok(()));

        let refused = super::handed(Command::new("false")).expect_err("false fails");
        assert!(
            refused.starts_with("false: "),
            "the failure names neither the command nor its status: {refused}",
        );
    }

    #[test]
    fn the_session_is_named_exactly_wherever_tmux_is_asked_about_one() {
        // Without the `=`, tmux matches a prefix: every call about
        // `keeler-01-foo-t1` would answer about `keeler-01-foo-t10`, and
        // the board's two levers would reach a run nobody named.
        let kill = super::kill_command("keeler-01-foo-t1");
        assert_eq!(kill.get_program(), "tmux");
        assert_eq!(
            kill.get_args().collect::<Vec<_>>(),
            ["kill-session", "-t", "=keeler-01-foo-t1"],
        );

        for (inside, verb) in [(false, "attach"), (true, "switch-client")] {
            let attach = super::attach_command("keeler-01-foo-t1", inside);
            assert_eq!(attach.get_program(), "tmux");
            assert_eq!(
                attach.get_args().collect::<Vec<_>>(),
                [verb, "-t", "=keeler-01-foo-t1"],
            );
        }
    }

    /// Not a scenario of its own: it is the one thing the levers can be
    /// asked on a machine running a test suite. A session nobody started
    /// cannot be killed, switched to or attached to, and every one of the
    /// three has to say so — a board that reported success would write the
    /// paused marker for a kill that killed nothing, or come back from an
    /// attach that never happened without a word about why.
    ///
    /// It holds whether or not tmux is installed, which is what makes it
    /// safe to run anywhere: the answer is the program's refusal on a
    /// machine that has one and "not installed" on a machine that has not.
    #[test]
    fn a_lever_pulled_on_a_session_that_is_not_there_is_a_refusal() {
        use super::{Dispatch as _, Shell};

        let shell = Shell::new("/keeler-top-no-such-plugin", "/", "specs/01-foo.md");
        let session = format!("keeler-top-no-such-session-{}", std::process::id());

        for (lever, refused) in [
            ("kill", shell.kill(&session)),
            ("switch-client", shell.attach(&session, true)),
            // With stdout a pipe rather than a terminal, and no session of
            // that name on any server: tmux refuses and returns, which is
            // the only way this may be run from a test at all.
            ("attach", shell.attach(&session, false)),
            ("resume", shell.resume("T1").map(drop)),
        ] {
            let said = refused.expect_err(&format!("{lever} answered about a session nobody has"));
            assert!(
                !said.is_empty(),
                "{lever} refused without saying anything about it",
            );
        }
    }

    #[test]
    fn the_board_is_inside_tmux_when_the_variable_names_a_server() {
        use std::ffi::OsStr;

        assert!(super::inside_tmux(Some(OsStr::new(
            "/private/tmp/tmux-501/default,9084,0"
        ))));
        assert!(!super::inside_tmux(None));
        // Exported without a value, which names no server: `switch-client`
        // there would refuse, and `attach` is what works from a plain
        // terminal.
        assert!(!super::inside_tmux(Some(OsStr::new(""))));
    }
}
