//! `keeler-top` — the live board.
//!
//! Thin on purpose: every decision lives in the library, where the tests
//! and the mutation gate can reach it without a terminal. What is left here
//! is the three things a binary is for — the arguments it was started with,
//! the environment it was started in, and the exit code and stderr it
//! leaves behind.
//!
//! The theme is read here and nowhere else. It is a value from this line
//! on, carried to the renderer, so that a test draws a board through a
//! theme it built rather than through the variables its own process
//! happens to have been exported.

fn main() -> std::process::ExitCode {
    let theme = keeler_top::theme::Theme::from_env();
    match keeler_top::cli::main(std::env::args().skip(1), theme) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(refusal) => {
            eprintln!("{refusal}");
            std::process::ExitCode::FAILURE
        }
    }
}
