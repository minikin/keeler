//! The live board: one row per task of a spec, refreshed while a wave runs.
//!
//! The binary is the terminal; this library is where the decisions live, so
//! unit tests, property tests and cargo-mutants can reach them without a
//! terminal and without an agent.

pub mod board;
pub mod cli;
pub mod clock;
pub mod dispatch;
pub mod frame;
pub mod git;
pub mod graph;
pub mod run;
pub mod status;
pub mod stream;

/// A directory of one test's own, under whatever the machine calls
/// temporary.
///
/// Here rather than in a test module because three of them want it, and
/// what it is for is not obvious enough to say three times. The test's name
/// and the process id are not enough between them: a mutation run is
/// hundreds of mutants of these tests, four at a time, and nextest gives
/// every test a process of its own — tens of thousands of processes in five
/// minutes, which is enough for the pid space to come round. Every fixture
/// that uses this opens by removing whatever is at its path, so two live
/// runs of one test on one name is not two tests sharing a directory. It is
/// one of them deleting the other's files halfway through, which is a
/// failing gate nobody can reproduce. The instant the process started, and
/// a serial that only goes up, are what the pid is missing.
#[cfg(test)]
fn fixture_dir(name: &str) -> std::path::PathBuf {
    static SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let started = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "keeler-top-unit-{name}-{}-{started}-{}",
        std::process::id(),
        SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
    ))
}
