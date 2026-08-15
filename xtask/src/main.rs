//! `cargo xtask` — Keeler's repository tasks.

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--help" | "-h") | None => {
            print!("{}", xtask::usage());
            std::process::ExitCode::SUCCESS
        }
        Some(unknown) => {
            eprintln!("xtask: unknown command `{unknown}`\n\n{}", xtask::usage());
            std::process::ExitCode::FAILURE
        }
    }
}
