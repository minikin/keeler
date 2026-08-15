//! Keeler's own repository tasks.
//!
//! The binary is the command line; this library is where the logic lives, so
//! unit tests, property tests and cargo-mutants can reach it without paying
//! a subprocess per case.

/// What `cargo xtask` prints when asked what it can do.
#[must_use]
pub fn usage() -> String {
    "cargo xtask <command>\n\nCommands:\n  (none yet)\n".to_string()
}

#[cfg(test)]
mod tests {
    use super::usage;

    #[test]
    fn usage_names_the_command_it_belongs_to() {
        assert!(usage().starts_with("cargo xtask"));
    }
}
