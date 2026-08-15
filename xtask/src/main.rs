//! `cargo xtask` — Keeler's repository tasks.
//!
//! Thin on purpose: every decision lives in the library, where the tests and
//! the mutation gate can reach it.

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match xtask::run(&args) {
        Ok(output) => {
            println!("{}", output.trim_end_matches('\n'));
            std::process::ExitCode::SUCCESS
        }
        Err(why) => {
            eprintln!("xtask: {why}");
            std::process::ExitCode::FAILURE
        }
    }
}
