//! The command-line tools Keeler's gates need.
//!
//! Deciding *what* to install is logic and lives here, tested and mutated.
//! Actually installing it is process orchestration with an exit code to
//! check — `cargo binstall` doing what only it can do — and that part is
//! kept as thin as it can be.

/// What the shipped `Justfile` invokes, as `(what to probe, what to
/// install)`. `cargo nextest --version` answers for `cargo-nextest`; `just`
/// is a plain command rather than a cargo subcommand.
pub const REQUIRED: [(&str, &str); 5] = [
    ("cargo nextest", "cargo-nextest"),
    ("cargo llvm-cov", "cargo-llvm-cov"),
    ("cargo mutants", "cargo-mutants"),
    ("cargo crap", "cargo-crap"),
    ("just", "just"),
];

/// The tools to install, given a way of asking whether one is present.
///
/// A pure decision: the caller says what it found, this says what to do
/// about it. Nothing here runs a process, so every branch is reachable
/// from a test.
#[must_use]
pub fn missing(present: &dyn Fn(&str) -> bool) -> Vec<&'static str> {
    REQUIRED
        .iter()
        .filter(|(probe, _)| !present(probe))
        .map(|(_, install)| *install)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{REQUIRED, missing};

    #[test]
    fn nothing_is_installed_when_everything_is_present() {
        assert!(missing(&|_| true).is_empty());
    }

    #[test]
    fn everything_is_installed_when_nothing_is_present() {
        assert_eq!(missing(&|_| false).len(), REQUIRED.len());
    }

    #[test]
    fn only_what_is_absent_is_installed() {
        // The probe and the package name differ — `cargo nextest` is how
        // you ask, `cargo-nextest` is what you install — and confusing
        // them would install a package that is already there.
        let found = missing(&|probe| probe != "cargo mutants");
        assert_eq!(found, vec!["cargo-mutants"]);
    }

    #[test]
    fn just_is_probed_as_a_command_not_a_cargo_subcommand() {
        let found = missing(&|probe| probe != "just");
        assert_eq!(found, vec!["just"]);
    }
}
