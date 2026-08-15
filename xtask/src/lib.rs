//! Keeler's own repository tasks.
//!
//! The binary is the command line; this library is where the logic lives, so
//! unit tests, property tests and cargo-mutants can reach it without paying
//! a subprocess per case.

pub mod changelog;
pub mod checksum;

/// What `cargo xtask` prints when asked what it can do.
#[must_use]
pub fn usage() -> String {
    "cargo xtask <command>\n\nCommands:\n  \
     release-notes <version> <changelog>   print one version's notes\n  \
     checksum <file>                       print its sha256 checksum line\n"
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
/// Dispatch only: each command is its own function, so adding one does not
/// make this harder to follow — and none of it hides in `main`, where
/// neither the tests nor the mutation gate could reach it.
///
/// # Errors
///
/// Returns the command's failure: an unknown command, missing arguments, an
/// unreadable file, or whatever the command itself refused to do.
pub fn run(args: &[String]) -> Result<String, Failure> {
    let Some((command, rest)) = args.split_first() else {
        return Ok(usage());
    };
    match command.as_str() {
        "--help" | "-h" => Ok(usage()),
        "release-notes" => release_notes_command(rest),
        "checksum" => checksum_command(rest),
        unknown => Err(format!("unknown command `{unknown}`\n\n{}", usage()).into()),
    }
}

/// `release-notes <version> <changelog>`
fn release_notes_command(args: &[String]) -> Result<String, Failure> {
    match args {
        [version, path] => Ok(changelog::release_notes(&read(path)?, version)?),
        _ => Err("usage: release-notes <version> <changelog>".into()),
    }
}

/// `checksum <file>`
fn checksum_command(args: &[String]) -> Result<String, Failure> {
    match args {
        [path] => {
            let bytes = std::fs::read(path).map_err(|why| format!("cannot read {path}: {why}"))?;
            Ok(checksum::checksum_line(&bytes, path))
        }
        _ => Err("usage: checksum <file>".into()),
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
    fn checksum_prints_a_verifiable_line() {
        let path = fixture("sum", "abc");
        let line = run(&["checksum".into(), path.display().to_string()]).unwrap();
        assert!(
            line.starts_with("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad  "),
            "unexpected checksum line: {line}",
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn checksum_without_a_file_says_what_it_needs() {
        let error = run(&["checksum".into()]).unwrap_err().to_string();
        assert!(error.contains("checksum <file>"), "no usage in: {error}");
    }

    #[test]
    fn checksum_names_a_file_it_cannot_read() {
        let args = ["checksum".into(), "/nonexistent/install.sh".into()];
        let error = run(&args).unwrap_err().to_string();
        assert!(
            error.contains("/nonexistent/install.sh"),
            "no path in: {error}"
        );
    }

    #[test]
    fn no_command_at_all_prints_the_usage() {
        assert!(run(&[]).unwrap().starts_with("cargo xtask"));
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
