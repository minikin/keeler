//! Everything the board runs outside itself, behind one trait.
//!
//! The board is a reader: `keeler-status` decides what a task's state is,
//! `scripts/keeler-graph.sh` decides what its graph says, and later the
//! levers — pause, resume, attach — will be recipes too. All of it is a
//! subprocess, which is the one part of the board no unit test can drive,
//! so all of it goes through [`Dispatch`]: a test double records what was
//! asked for, and [`Shell`] is the one implementation that actually runs
//! anything.
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

    /// The command [`Dispatch::status`] runs.
    ///
    /// Built apart from running it because the composition *is* the
    /// contract — which Justfile, and in which directory — and a test that
    /// only read the output could not tell a board that shelled back
    /// through the plugin from one that found a justfile somewhere else.
    #[must_use]
    pub fn status_command(&self) -> Command {
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
            // It is safe here because `keeler-status` carries a shebang:
            // just runs such a recipe as one process and leaves its two
            // streams alone, while a *linewise* recipe under `-q` gets
            // both of them nulled — which would leave the board with no
            // report at all. The suite pins both halves.
            .arg("-q")
            .arg("keeler-status")
            .arg(&self.spec);
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

impl Dispatch for Shell {
    fn status(&self) -> Result<String, String> {
        run(self.status_command())
    }

    fn graph(&self, spec: &Path) -> Result<String, String> {
        run(self.graph_command(spec))
    }
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
    let output = command
        .output()
        .map_err(|err| format!("{program}: {err}"))?;
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
        // board must say so rather than end.
        let refused = run(Command::new("keeler-top-no-such-program")).expect_err("no such program");

        assert!(
            refused.starts_with("keeler-top-no-such-program: "),
            "the failure does not name the command it could not run: {refused}",
        );
    }
}
