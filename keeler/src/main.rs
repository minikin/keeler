//! `keeler` — put the workflow in a Rust project.
//!
//! Thin on purpose: every decision lives in the library, where the tests
//! and the mutation gate can reach it.

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match keeler::run(&args) {
        Ok(output) => {
            print!("{output}");
            std::process::ExitCode::SUCCESS
        }
        Err(why) => {
            eprintln!("keeler: {why}");
            std::process::ExitCode::FAILURE
        }
    }
}
