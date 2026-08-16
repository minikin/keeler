//! Keeler's own repository tasks.
//!
//! The binary is the command line; this library is where the logic lives, so
//! unit tests, property tests and cargo-mutants can reach it without paying
//! a subprocess per case.

pub mod changelog;
pub mod checksum;
pub mod guard;

/// What `cargo xtask` prints when asked what it can do.
#[must_use]
pub fn usage() -> String {
    "cargo xtask <command>\n\nCommands:\n  \
     release-notes <version> <changelog>   print one version's notes\n  \
     checksum <file>                       print its sha256 checksum line\n  \
     release-guard <tag>                   refuse a tag that lies\n"
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
        "release-guard" => release_guard_command(std::path::Path::new("."), rest),
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

/// Every package manifest in the repository and the version it declares:
/// the root, plus each workspace member. A manifest without a version of
/// its own — a virtual root, or a member inheriting from the workspace —
/// contributes nothing to compare.
fn declared_versions(root: &std::path::Path) -> Result<Vec<(String, String)>, Failure> {
    let root_manifest = read(&root.join("Cargo.toml").display().to_string())?;
    let mut found = Vec::new();
    if let Some(version) = guard::package_version(&root_manifest) {
        found.push(("Cargo.toml".to_string(), version.to_string()));
    }
    // The claim a workspace makes on behalf of every member that inherits
    // it. Without this, a workspace that moved to `version.workspace =
    // true` left the guard with nothing to compare and calling that
    // agreement — the very drift T7 exists to catch.
    if let Some(version) = guard::workspace_version(&root_manifest) {
        found.push((
            "Cargo.toml [workspace.package]".to_string(),
            version.to_string(),
        ));
    }
    for member in guard::workspace_members(&root_manifest) {
        let rel = format!("{member}/Cargo.toml");
        let path = root.join(&rel);
        if !path.is_file() {
            return Err(format!("workspace member {member} has no Cargo.toml").into());
        }
        let manifest = read(&path.display().to_string())?;
        if let Some(version) = guard::package_version(&manifest) {
            found.push((rel, version.to_string()));
        }
    }
    // A gate that measured nothing must not report success. Every
    // repository this runs in declares a version somewhere; finding none
    // means the parse failed, not that everything agrees.
    if found.is_empty() {
        return Err("no manifest declares a version — nothing could be compared".into());
    }
    Ok(found)
}

/// `release-guard <tag>`, against the repository rooted at `root`.
///
/// The root is a parameter rather than the working directory so the command
/// is reachable by a test that owns a fixture — an implicit cwd is what kept
/// this function untested, and the coverage and mutation gates both said so.
fn release_guard_command(root: &std::path::Path, args: &[String]) -> Result<String, Failure> {
    let [tag] = args else {
        return Err("usage: release-guard <tag>".into());
    };
    let at = |name: &str| root.join(name).display().to_string();
    let version = read(&at("VERSION"))?.trim().to_string();
    let rules = read(&at(".claude/keeler.md"))?;
    let marker = guard::marker(&rules).unwrap_or_default().to_string();
    let changelog = read(&at("CHANGELOG.md"))?;

    let manifests = declared_versions(root)?;
    let found = guard::disagreements(tag, &version, &marker, &changelog, &manifests);
    if found.is_empty() {
        return Ok(format!(
            "v{version} is consistent — tag, VERSION, marker, CHANGELOG \
             and {} manifest(s) agree",
            manifests.len(),
        ));
    }
    Err(format!("refusing to release:\n  {}", found.join("\n  ")).into())
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
    use super::{release_guard_command, run, usage};

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

    fn repo_fixture(
        name: &str,
        version: &str,
        marker: &str,
        changelog: &str,
    ) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("xtask-guard-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".claude")).unwrap();
        std::fs::write(root.join("VERSION"), format!("{version}\n")).unwrap();
        std::fs::write(
            root.join(".claude/keeler.md"),
            format!("<!-- keeler-version: {marker} -->\n"),
        )
        .unwrap();
        std::fs::write(root.join("CHANGELOG.md"), changelog).unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            format!(
                "[workspace]\nmembers = [\"member\"]\n\n\
                 [package]\nname = \"root\"\nversion = \"{version}\"\n"
            ),
        )
        .unwrap();
        std::fs::create_dir_all(root.join("member")).unwrap();
        std::fs::write(
            root.join("member/Cargo.toml"),
            format!("[package]\nname = \"member\"\nversion = \"{version}\"\n"),
        )
        .unwrap();
        root
    }

    #[test]
    fn release_guard_accepts_a_consistent_release() {
        let root = repo_fixture("ok", "1.2.3", "1.2.3", "## [1.2.3]\n\n- shipped\n");
        let out = release_guard_command(&root, &["v1.2.3".into()]).unwrap();
        assert!(out.contains("1.2.3") && out.contains("consistent"), "{out}");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn release_guard_refuses_and_lists_every_disagreement() {
        let root = repo_fixture("bad", "2.0.0", "0.0.1", "# Changelog\n");
        let error = release_guard_command(&root, &["v9.9.9".into()])
            .unwrap_err()
            .to_string();
        for expected in ["v9.9.9", "0.0.1", "CHANGELOG"] {
            assert!(
                error.contains(expected),
                "`{expected}` missing from: {error}"
            );
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn release_guard_checks_every_member_not_only_the_root() {
        // A member left behind at the old version is the failure this
        // check exists for: it agreed with VERSION only by coincidence
        // until someone bumped one manifest and not the other.
        let root = repo_fixture(
            "member-drift",
            "1.2.3",
            "1.2.3",
            "## [1.2.3]\n\n- shipped\n",
        );
        std::fs::write(
            root.join("member/Cargo.toml"),
            "[package]\nname = \"member\"\nversion = \"0.0.1\"\n",
        )
        .unwrap();

        let error = release_guard_command(&root, &["v1.2.3".into()])
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("member/Cargo.toml") && error.contains("0.0.1"),
            "the refusal does not name the member and its version: {error}",
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn release_guard_names_a_member_whose_manifest_is_missing() {
        let root = repo_fixture("member-gone", "1.2.3", "1.2.3", "## [1.2.3]\n\n- shipped\n");
        std::fs::remove_file(root.join("member/Cargo.toml")).unwrap();

        let error = release_guard_command(&root, &["v1.2.3".into()])
            .unwrap_err()
            .to_string();
        assert!(error.contains("member"), "no member named in: {error}");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn release_guard_without_a_tag_says_what_it_needs() {
        let error = release_guard_command(std::path::Path::new("."), &[])
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("release-guard <tag>"),
            "no usage in: {error}"
        );
    }

    #[test]
    fn release_guard_names_a_repository_file_it_cannot_read() {
        let missing = std::env::temp_dir().join("xtask-guard-nowhere");
        let _ = std::fs::remove_dir_all(&missing);
        let error = release_guard_command(&missing, &["v1.0.0".into()])
            .unwrap_err()
            .to_string();
        assert!(error.contains("VERSION"), "no file named in: {error}");
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
