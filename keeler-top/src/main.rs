//! `keeler-top` — the live board.
//!
//! Thin on purpose: every decision lives in the library, where the tests
//! and the mutation gate can reach it without a terminal.
//!
//! The command line is not here yet — the frame, the refusals and `--once`
//! arrive with the task that renders a frame, and the loop with the one
//! after. Until then the binary refuses rather than exiting zero having
//! shown nobody a board: a front door that reports success for work it did
//! not do is the one failure the whole spec exists to prevent elsewhere.

fn main() -> std::process::ExitCode {
    eprintln!("keeler-top: the board is not built yet — spec 10 is still being implemented.");
    std::process::ExitCode::FAILURE
}
