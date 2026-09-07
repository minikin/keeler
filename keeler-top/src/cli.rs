//! The command line, and the one pass over the board that `--once` makes.
//!
//! The binary is three lines around this: everything it decides — what the
//! flags mean, which refusal comes first, what a frame is printed to — is
//! here, where the tests can reach it.
//!
//! **The refusals are ordered.** The board refuses about what it read
//! before it refuses about where it would draw: a spec that is not
//! committed is wrong whether stdout is a terminal or a pipe, and telling
//! somebody to add `--once` only to refuse again for a reason they could
//! have been given the first time is two round trips through a recipe that
//! takes seconds to answer.

use std::io::{IsTerminal as _, Write as _};
use std::path::PathBuf;
use std::sync::Arc;

use crate::app::App;
use crate::board::Board;
use crate::clock::Timestamp;
use crate::dispatch::{Dispatch, Shell};
use crate::theme::Theme;

/// What `keeler keeler-top` hands the binary, and what a person adds after
/// it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    /// Where the plugin is: the Justfile the board shells back to, and the
    /// graph script beside it.
    pub plugin_root: PathBuf,
    /// The project being watched, which is the recipe's working directory.
    pub root: PathBuf,
    /// The spec, as it was named — relative to the project or absolute,
    /// exactly as `keeler-status` takes it.
    pub spec: String,
    /// One plain-text frame instead of a board.
    pub once: bool,
}

impl Args {
    /// Reads the command line.
    ///
    /// # Errors
    ///
    /// A flag it does not know, a flag whose value is missing, a second
    /// spec, or no spec at all. Each refusal names the word that caused it:
    /// the recipe passes flags through, so a typo in one reaches here
    /// rather than `just`.
    pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Self, String> {
        let mut plugin_root = None;
        let mut root = None;
        let mut spec = None;
        let mut once = false;
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--once" => once = true,
                "--plugin-root" => plugin_root = Some(value(&mut args, &arg)?),
                "--root" => root = Some(value(&mut args, &arg)?),
                _ if arg.starts_with('-') => {
                    return Err(format!("keeler-top: {arg} is not a flag it knows."));
                }
                _ if spec.is_some() => {
                    return Err(format!(
                        "keeler-top: one spec at a time — {arg} is a second."
                    ));
                }
                _ => spec = Some(arg),
            }
        }
        Ok(Self {
            // The recipe always passes both, so a default is only reached
            // by someone running the binary by hand — for whom the project
            // and the plugin they are standing in is the least surprising
            // answer.
            plugin_root: plugin_root.unwrap_or_else(|| PathBuf::from(".")),
            root: root.unwrap_or_else(|| PathBuf::from(".")),
            spec: spec.ok_or_else(|| {
                "keeler-top: name the spec to watch — keeler keeler-top specs/01-foo.md".to_string()
            })?,
            once,
        })
    }
}

/// The value after a flag that takes one.
fn value(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<PathBuf, String> {
    args.next()
        .map(PathBuf::from)
        .ok_or_else(|| format!("keeler-top: {flag} takes a path after it."))
}

/// Reads the board once and shows it.
///
/// # Errors
///
/// The command line, then whatever `keeler-status` or the graph script
/// refused — relayed in the words they refused in, because those are the
/// words written for whoever has to act on them — and last, a stdout that
/// is not a terminal.
pub fn main(args: impl IntoIterator<Item = String>, theme: Theme) -> Result<(), String> {
    let args = Args::parse(args)?;
    let shell: Arc<dyn Dispatch> = Arc::new(Shell::new(&args.plugin_root, &args.root, &args.spec));
    let report = shell.status()?;
    let status = crate::status::parse(&report)
        .ok_or_else(|| format!("keeler-top: keeler-status printed no board to read:\n{report}"))?;
    let graph = crate::graph::read(shell.as_ref(), &args.root, &status.git_ref, &status.rel)?;
    let now = Timestamp::now();
    let mut app = App::new(Arc::clone(&shell), args.root.clone(), status, graph, now, theme);
    match shown(&app.board, now, args.once, std::io::stdout().is_terminal())? {
        Show::Frame(frame) => written(std::io::stdout().write_all(frame.as_bytes())),
        Show::Live => crate::app::run(&mut app, shell),
    }
}

/// What a write to stdout means.
///
/// A pipe that closed under the write is not a failure. `keeler keeler-top
/// --once <spec> | head -3` closes it the moment it has the three lines it
/// asked for, and Rust ignores `SIGPIPE`, so the write returns an error
/// where a shell tool would simply have ended — and a `panic!` from
/// `print!` would report a reader who got exactly what they asked for as a
/// board that crashed. Every other write error is the board's to report.
fn written(wrote: std::io::Result<()>) -> Result<(), String> {
    match wrote {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
        Err(err) => Err(format!("keeler-top: stdout: {err}")),
    }
}

/// The two things the board can be: one frame of text, or a live screen.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Show {
    /// `--once`: the table, printed, and nothing after it.
    Frame(String),
    /// The board, drawn on the terminal until somebody leaves it.
    Live,
}

/// What the board has to show, once it has read one.
///
/// A value rather than a print, and the terminal an argument rather than a
/// question asked here: a test runs with its stdout captured, so the one
/// condition this turns on is the one condition a test cannot arrange. As
/// an argument both answers are reachable, and the untestable part is the
/// single call that supplies it.
fn shown(board: &Board, now: Timestamp, once: bool, terminal: bool) -> Result<Show, String> {
    if once {
        return Ok(Show::Frame(crate::frame::once(board, now)));
    }
    if terminal {
        return Ok(Show::Live);
    }
    Err(
        "keeler-top: the board needs a terminal to draw on — --once prints one frame instead."
            .to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::Args;
    use std::path::PathBuf;

    fn parse(args: &[&str]) -> Result<Args, String> {
        Args::parse(args.iter().map(|arg| (*arg).to_string()))
    }

    #[test]
    fn the_recipes_own_command_line_reads_as_it_is_composed() {
        // `cargo run … -- --plugin-root <plugin> --root <cwd> ARGS`, with
        // the flags an adopter typed after the spec arriving in ARGS.
        assert_eq!(
            parse(&[
                "--plugin-root",
                "/p",
                "--root",
                "/r",
                "--once",
                "specs/01-foo.md",
            ]),
            Ok(Args {
                plugin_root: PathBuf::from("/p"),
                root: PathBuf::from("/r"),
                spec: "specs/01-foo.md".to_string(),
                once: true,
            }),
        );
    }

    #[test]
    fn a_flag_after_the_spec_reaches_the_board_too() {
        // What `keeler keeler-top specs/01-foo.md --once` composes: the
        // recipe passes its arguments through in the order they were given.
        assert_eq!(
            parse(&["specs/01-foo.md", "--once"]).map(|args| args.once),
            Ok(true),
        );
    }

    #[test]
    fn a_binary_run_by_hand_stands_where_it_was_started() {
        let args = parse(&["specs/01-foo.md"]).expect("a spec is all it needs");

        assert_eq!(args.plugin_root, PathBuf::from("."));
        assert_eq!(args.root, PathBuf::from("."));
        assert!(!args.once);
    }

    #[test]
    fn every_refusal_names_the_word_that_caused_it() {
        for (args, word) in [
            (vec!["--verbose", "specs/01-foo.md"], "--verbose"),
            (vec!["--root"], "--root"),
            (vec!["--plugin-root"], "--plugin-root"),
            (
                vec!["specs/01-foo.md", "specs/02-bar.md"],
                "specs/02-bar.md",
            ),
        ] {
            let refused = parse(&args).expect_err("the fixture is not a command line");
            assert!(
                refused.contains(word) && refused.starts_with("keeler-top: "),
                "the refusal does not name {word}: {refused}",
            );
        }
    }

    #[test]
    fn a_frame_goes_to_a_pipe_only_when_it_was_asked_for_as_one() {
        use crate::board::{Board, Runs};
        use crate::clock::Timestamp;

        let status = crate::status::parse("graph: s.md on HEAD\nT1     done\n").expect("a report");
        let now = Timestamp::default();
        let board = Board::assemble(&status, &[], &mut Runs::default(), now);

        // `--once` is the frame, wherever stdout goes: it is what a script
        // asked for, and a terminal does not make it something else.
        for terminal in [true, false] {
            assert_eq!(
                super::shown(&board, now, true, terminal),
                Ok(super::Show::Frame(crate::frame::once(&board, now))),
            );
        }

        // Without it, a terminal is a board and a pipe is a refusal naming
        // the flag that turns one into the other.
        assert_eq!(
            super::shown(&board, now, false, true),
            Ok(super::Show::Live)
        );
        let piped = super::shown(&board, now, false, false).expect_err("a pipe is not a board");
        assert!(
            piped.contains("terminal") && piped.contains("--once"),
            "the refusal names neither the terminal nor the way out: {piped}",
        );
    }

    #[test]
    fn a_pipe_that_closed_under_the_frame_is_not_a_failure() {
        use std::io::{Error, ErrorKind};

        assert_eq!(super::written(Ok(())), Ok(()));
        assert_eq!(
            super::written(Err(Error::from(ErrorKind::BrokenPipe))),
            Ok(()),
            "`--once | head -3` was reported as a board that crashed",
        );

        let refused = super::written(Err(Error::from(ErrorKind::StorageFull)))
            .expect_err("a full disk is the board's to report");
        assert!(
            refused.starts_with("keeler-top: stdout: "),
            "the refusal does not say where the frame did not go: {refused}",
        );
    }

    #[test]
    fn a_command_line_with_no_spec_says_how_to_name_one() {
        let refused = parse(&["--once"]).expect_err("there is no spec to watch");

        assert!(
            refused.contains("keeler keeler-top specs/"),
            "the refusal does not show the command that works: {refused}",
        );
    }
}
