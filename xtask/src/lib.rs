//! Keeler's own repository tasks.
//!
//! The binary is the command line; this library is where the logic lives, so
//! unit tests, property tests and cargo-mutants can reach it without paying
//! a subprocess per case.

pub mod changelog;

/// What `cargo xtask` prints when asked what it can do.
#[must_use]
pub fn usage() -> String {
    "cargo xtask <command>\n\nCommands:\n  \
     release-notes <version> <changelog>   print one version's notes\n"
        .to_string()
}

/// Anything a command can fail with, phrased for someone reading CI output.
pub type Failure = Box<dyn std::error::Error>;

/// Reads a file, naming it if that fails — a path in the message beats
/// "No such file or directory" with no clue which file.
fn read(path: &str) -> Result<String, Failure> {
    std::fs::read_to_string(path).map_err(|why| format!("cannot read {path}: {why}").into())
}

/// Runs one command and returns what it should print.
///
/// The dispatch lives here rather than in `main` so it is reachable by unit
/// tests and by cargo-mutants.
///
/// # Errors
///
/// Returns the command's failure: an unknown command, missing arguments, an
/// unreadable file, or whatever the command itself refused to do.
pub fn run(args: &[String]) -> Result<String, Failure> {
    match args.first().map(String::as_str) {
        Some("--help" | "-h") | None => Ok(usage()),
        Some("release-notes") => match &args[1..] {
            [version, changelog] => Ok(changelog::release_notes(&read(changelog)?, version)?),
            _ => Err("usage: release-notes <version> <changelog>".into()),
        },
        Some(unknown) => Err(format!("unknown command `{unknown}`\n\n{}", usage()).into()),
    }
}

#[cfg(test)]
mod tests {
    use super::{run, usage};

    fn fixture(name: &str, content: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("xtask-{name}-{}", std::process::id()));
        std::fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn usage_names_the_command_it_belongs_to() {
        assert!(usage().starts_with("cargo xtask"));
    }

    #[test]
    fn release_notes_prints_the_sections_body() {
        let path = fixture("notes", "## [1.0.0]\n\n- shipped\n");
        let args = [
            "release-notes".into(),
            "1.0.0".into(),
            path.display().to_string(),
        ];
        assert_eq!(run(&args).unwrap(), "- shipped");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn an_absent_version_fails_loudly_naming_it() {
        let path = fixture("absent", "## [1.0.0]\n\n- shipped\n");
        let args = [
            "release-notes".into(),
            "2.0.0".into(),
            path.display().to_string(),
        ];
        let error = run(&args).unwrap_err().to_string();
        assert!(
            error.contains("2.0.0"),
            "the error does not name the version: {error}"
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn an_unreadable_changelog_names_the_path() {
        let args = [
            "release-notes".into(),
            "1.0.0".into(),
            "/nonexistent/CHANGELOG.md".into(),
        ];
        let error = run(&args).unwrap_err().to_string();
        assert!(
            error.contains("/nonexistent/CHANGELOG.md"),
            "the error does not name the file it could not read: {error}",
        );
    }

    #[test]
    fn a_command_called_without_its_arguments_says_what_it_needs() {
        let error = run(&["release-notes".into()]).unwrap_err().to_string();
        assert!(
            error.contains("release-notes <version>"),
            "the error does not show the usage: {error}",
        );
    }

    #[test]
    fn an_unknown_command_is_refused() {
        let error = run(&["fly".into()]).unwrap_err().to_string();
        assert!(
            error.contains("fly"),
            "the error does not name the command: {error}"
        );
    }
}
