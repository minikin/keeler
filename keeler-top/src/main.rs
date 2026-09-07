//! `keeler-top` — the live board.
//!
//! Thin on purpose: every decision lives in the library, where the tests
//! and the mutation gate can reach it without a terminal. What is left here
//! is the two things a binary is for — the arguments it was started with,
//! and the exit code and stderr it leaves behind.

fn main() -> std::process::ExitCode {
    match keeler_top::cli::main(std::env::args().skip(1)) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(refusal) => {
            eprintln!("{refusal}");
            std::process::ExitCode::FAILURE
        }
    }
}
