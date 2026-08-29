//! Keeler's own repository tasks.
//!
//! The binary is the command line; this library is where the logic lives, so
//! unit tests, property tests and cargo-mutants can reach it without paying
//! a subprocess per case.

pub mod changelog;
pub mod checksum;
pub mod guard;
pub mod pipeline;

/// What `cargo xtask` prints when asked what it can do.
#[must_use]
pub fn usage() -> String {
    "cargo xtask <command>\n\nCommands:\n  \
     release-notes <version> <changelog>   print one version's notes\n  \
     checksum <file>                       print its sha256 checksum line\n  \
     release-guard <tag>                   refuse a tag that lies\n  \
     pipeline-check                        refuse a tick no review accounts for\n"
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
        "pipeline-check" => pipeline_check_command(std::path::Path::new("."), rest),
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
    let mut claims = root_claims(&root_manifest);
    for member in guard::workspace_members(&root_manifest) {
        // A glob is cargo's to expand. Skipping it leaves those members
        // unchecked, which is a smaller wrong than refusing a repository
        // whose manifest is perfectly valid.
        if guard::is_glob(&member) {
            continue;
        }
        claims.push(member_claim(root, &member)?);
    }

    // A gate that measured nothing must not report success. Every
    // repository this runs in declares a version somewhere; finding none
    // means the parse failed, not that everything agrees.
    let declared: Vec<(String, String)> = claims.into_iter().flatten().collect();
    if declared.is_empty() {
        return Err("no manifest declares a version — nothing could be compared".into());
    }
    Ok(declared)
}

/// What the root manifest claims: its own package version, and the version
/// it declares for members that inherit one. Without the second, a
/// workspace that moved to `version.workspace = true` left the guard with
/// nothing to compare and called that agreement.
fn root_claims(manifest: &str) -> Vec<Option<(String, String)>> {
    vec![
        guard::package_version(manifest)
            .map(|version| ("Cargo.toml".to_string(), version.to_string())),
        guard::workspace_version(manifest).map(|version| {
            (
                "Cargo.toml [workspace.package]".to_string(),
                version.to_string(),
            )
        }),
    ]
}

/// What one member declares, if it declares a version of its own.
fn member_claim(root: &std::path::Path, member: &str) -> Result<Option<(String, String)>, Failure> {
    let rel = format!("{member}/Cargo.toml");
    let path = root.join(&rel);
    if !path.is_file() {
        return Err(format!("workspace member {member} has no Cargo.toml").into());
    }
    let manifest = read(&path.display().to_string())?;
    Ok(guard::package_version(&manifest).map(|version| (rel, version.to_string())))
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

/// `pipeline-check`, against the repository rooted at `root`.
///
/// The impure shell over the decision: it reads the three inputs and prints
/// what the pure rule concluded. The root is a parameter for the reason
/// `release-guard`'s is — an implicit working directory is what kept that
/// command untested — and everything it consults is a file. No git, no
/// network, no clock, so the gate runs identically in a worktree, a shallow
/// clone and CI.
fn pipeline_check_command(root: &std::path::Path, args: &[String]) -> Result<String, Failure> {
    if !args.is_empty() {
        return Err("usage: pipeline-check".into());
    }
    let specs = pipeline::specs::read_from(&root.join("specs"))?;
    // A gate that measured nothing must not report success: an empty
    // `specs/` accounts for every ticked task there is by accounting for
    // none, which is the failure `declared_versions` refuses above.
    if specs.is_empty() {
        return Err(format!(
            "no specs in {} — nothing could be checked",
            root.join("specs").display(),
        )
        .into());
    }
    let records = pipeline::records::read_from(&root.join("reviews"))?;
    let backlog = pipeline::backlog::read_from(&root.join("reviews/BACKLOG.md"))?;

    let broken: Vec<String> = pipeline::specs::unkept_promises(&specs)
        .iter()
        .map(|task| format!("{task} is unticked in a spec marked `Status: Implemented`"))
        .collect();
    let decision = pipeline::decision::decide(&pipeline::specs::ticked(&specs), &records, &backlog);
    match (decision, broken.is_empty()) {
        (pipeline::decision::Decision::AllAccounted { ticked }, true) => Ok(format!(
            "pipeline-check: {ticked} ticked task(s) accounted for — \
             {} review record(s), {} line(s) of accepted debt",
            records.len(),
            backlog.len(),
        )),
        (pipeline::decision::Decision::AllAccounted { .. }, false) => Err(refusal(&broken)),
        (pipeline::decision::Decision::Missing(missing), _) => {
            let mut all: Vec<String> = missing.iter().map(ToString::to_string).collect();
            all.extend(broken);
            Err(refusal(&all))
        }
    }
}

/// One refusal per line, so a reader fixing them can work down the list.
fn refusal(complaints: &[String]) -> Failure {
    format!(
        "refusing to vouch for this pipeline:\n  {}",
        complaints.join("\n  "),
    )
    .into()
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
    use super::{pipeline_check_command, release_guard_command, run, usage};

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
    fn a_repository_where_no_manifest_declares_a_version_is_refused() {
        // Not "everything agrees" — nothing was compared. A gate that
        // measured nothing must not report success, which is how the
        // inherited-version case slipped through before.
        let root = repo_fixture("no-version", "1.2.3", "1.2.3", "## [1.2.3]\n\n- shipped\n");
        std::fs::write(root.join("Cargo.toml"), "[workspace]\nmembers = []\n").unwrap();
        let _ = std::fs::remove_dir_all(root.join("member"));

        let error = release_guard_command(&root, &["v1.2.3".into()])
            .unwrap_err()
            .to_string();
        assert!(error.contains("nothing could be compared"), "{error}");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_workspace_that_declares_the_version_for_its_members_is_checked() {
        let root = repo_fixture("inherited", "1.2.3", "1.2.3", "## [1.2.3]\n\n- shipped\n");
        std::fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = []\n\n[workspace.package]\nversion = \"0.0.1\"\n",
        )
        .unwrap();
        let _ = std::fs::remove_dir_all(root.join("member"));

        let error = release_guard_command(&root, &["v1.2.3".into()])
            .unwrap_err()
            .to_string();
        assert!(error.contains("workspace.package"), "{error}");
        assert!(error.contains("0.0.1"), "{error}");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_glob_member_does_not_refuse_the_repository() {
        // `members = ["crates/*"]` is valid and ordinary. Cargo expands
        // it; this parser does not, so the pattern was looked up as a
        // directory and a truthful repository was refused.
        let root = repo_fixture("glob", "1.2.3", "1.2.3", "## [1.2.3]\n\n- shipped\n");
        std::fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/*\"]\n\n\
             [package]\nname = \"root\"\nversion = \"1.2.3\"\n",
        )
        .unwrap();
        let _ = std::fs::remove_dir_all(root.join("member"));

        let out = release_guard_command(&root, &["v1.2.3".into()])
            .expect("a valid workspace was refused");
        assert!(out.contains("consistent"), "{out}");
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

    /// A throwaway repository root holding the given `(relative path,
    /// content)` — `specs/`, `reviews/` and the backlog, in whatever state
    /// the case under test needs.
    fn gate_fixture(name: &str, files: &[(&str, &str)]) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("xtask-gate-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for (path, content) in files {
            let file = root.join(path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, content).unwrap();
        }
        root
    }

    /// One spec with one ticked task, and a well-formed pass record for it.
    fn spec_with_one_tick() -> (&'static str, &'static str) {
        (
            "specs/09-demo.md",
            "**Status:** Approved\n\n## Tasks\n\n- [x] **T1 — Shipped.**\n",
        )
    }

    fn pass_record() -> (&'static str, &'static str) {
        (
            "reviews/09-demo/t1.md",
            "Spec: 09-demo\nTask: t1\nCommit: abc1234\nVerdict: pass\n\n## Findings\n\nnone\n",
        )
    }

    #[test]
    fn the_gate_says_what_it_counted_and_what_it_counted_it_from() {
        // Two backlog lines against one record, so a message that swapped
        // the counts or invented one could not pass.
        let root = gate_fixture(
            "counts",
            &[
                spec_with_one_tick(),
                pass_record(),
                ("reviews/BACKLOG.md", "01-old/t1\n01-old/t2\n"),
            ],
        );
        assert_eq!(
            pipeline_check_command(&root, &[]).unwrap(),
            "pipeline-check: 1 ticked task(s) accounted for — \
             1 review record(s), 2 line(s) of accepted debt",
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_tick_with_no_evidence_is_refused_by_the_command() {
        let root = gate_fixture("unreviewed", &[spec_with_one_tick()]);
        let error = pipeline_check_command(&root, &[]).unwrap_err().to_string();
        assert!(
            error.contains("09-demo/t1") && error.contains("no review record"),
            "the refusal does not name the task and the lack: {error}",
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_backlog_line_carries_a_tick_the_records_do_not() {
        // The debt list is the gate's second source of coverage, and the
        // command must actually read it — the thirty-nine on this
        // repository's books are what stand between it and its own gate.
        let root = gate_fixture(
            "debt",
            &[spec_with_one_tick(), ("reviews/BACKLOG.md", "09-demo/t1\n")],
        );
        let accounted = pipeline_check_command(&root, &[]).unwrap();
        assert!(
            accounted.contains("1 line(s) of accepted debt"),
            "{accounted}"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn the_backlog_is_not_read_as_a_review_record() {
        // `reviews/BACKLOG.md` sits among the record directories and ends
        // in `.md` like they do. Read as a record it would be refused as
        // malformed, and the gate would fail over the file that exists to
        // make it pass.
        let root = gate_fixture(
            "backlog-not-a-record",
            &[
                spec_with_one_tick(),
                pass_record(),
                ("reviews/BACKLOG.md", "01-old/t1\n"),
            ],
        );
        assert!(pipeline_check_command(&root, &[]).is_ok());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_malformed_record_is_named_by_the_command() {
        let root = gate_fixture(
            "malformed",
            &[
                spec_with_one_tick(),
                (
                    "reviews/09-demo/t1.md",
                    "Spec: 09-demo\nTask: t1\nVerdict: pass\n",
                ),
            ],
        );
        let error = pipeline_check_command(&root, &[]).unwrap_err().to_string();
        assert!(
            error.contains("09-demo/t1.md") && error.contains("`Commit:`"),
            "the refusal names neither the file nor the header it lacks: {error}",
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_backlog_that_lies_is_named_with_its_file() {
        // The parked gate panicked here. The refusal names the file the
        // reader has to open and the line inside it.
        let root = gate_fixture(
            "duplicate",
            &[
                spec_with_one_tick(),
                pass_record(),
                ("reviews/BACKLOG.md", "01-old/t1\n01-old/t1\n"),
            ],
        );
        let error = pipeline_check_command(&root, &[]).unwrap_err().to_string();
        assert!(
            error.contains("BACKLOG.md") && error.contains("line 2 duplicates line 1"),
            "the refusal does not name the file and the duplicated line: {error}",
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn an_implemented_spec_with_an_unticked_task_is_refused() {
        // Given a spec marked Implemented with a task unticked
        let root = gate_fixture(
            "promise",
            &[
                (
                    "specs/09-demo.md",
                    "**Status:** Implemented\n\n## Tasks\n\n\
                     - [x] **T1 — Shipped.**\n- [ ] **T2 — Forgotten.**\n",
                ),
                pass_record(),
            ],
        );
        // Then the gate fails, naming the spec and the task
        let error = pipeline_check_command(&root, &[]).unwrap_err().to_string();
        assert!(
            error.contains("09-demo/t2") && error.contains("Implemented"),
            "the refusal does not name the broken promise: {error}",
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_repository_with_no_specs_is_refused_not_passed() {
        // A gate that measured nothing must not report success — the
        // failure `declared_versions` already refuses one line above.
        let root = gate_fixture("no-specs", &[("reviews/BACKLOG.md", "")]);
        std::fs::create_dir_all(root.join("specs")).unwrap();
        let error = pipeline_check_command(&root, &[]).unwrap_err().to_string();
        assert!(
            error.contains("nothing could be checked"),
            "an empty specs directory passed the gate: {error}",
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_repository_with_neither_reviews_nor_backlog_is_read_as_no_evidence() {
        // Absence is not an error here: it is a repository where nothing
        // has been reviewed and nothing is owed, which fails loudly the
        // moment anything is ticked and passes while nothing is.
        let root = gate_fixture(
            "bare",
            &[(
                "specs/09-demo.md",
                "**Status:** Approved\n\n## Tasks\n\n- [ ] **T1 — Not yet.**\n",
            )],
        );
        assert_eq!(
            pipeline_check_command(&root, &[]).unwrap(),
            "pipeline-check: 0 ticked task(s) accounted for — \
             0 review record(s), 0 line(s) of accepted debt",
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_reviews_path_that_is_not_a_directory_is_named() {
        // Not the absence the gate forgives: `reviews` is there and is not
        // what it must be, and a gate that read that as "no records" would
        // pass a repository whose every review it could not see.
        let root = gate_fixture(
            "reviews-a-file",
            &[spec_with_one_tick(), ("reviews", "not a directory\n")],
        );
        let error = pipeline_check_command(&root, &[]).unwrap_err().to_string();
        assert!(
            error.contains("reviews"),
            "the refusal does not name what it could not read: {error}",
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_backlog_path_that_is_not_a_file_is_named() {
        let root = gate_fixture("backlog-a-dir", &[spec_with_one_tick(), pass_record()]);
        std::fs::create_dir_all(root.join("reviews/BACKLOG.md")).unwrap();
        let error = pipeline_check_command(&root, &[]).unwrap_err().to_string();
        assert!(
            error.contains("BACKLOG.md"),
            "the refusal does not name what it could not read: {error}",
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn what_is_not_a_record_in_a_records_directory_is_passed_over() {
        // A directory the operating system litters — `.DS_Store` — must
        // not be read as a review of a task called `.DS_Store`, nor refuse
        // the whole gate for not being one.
        let root = gate_fixture(
            "litter",
            &[
                spec_with_one_tick(),
                pass_record(),
                ("reviews/09-demo/.DS_Store", "\u{0}\u{0}"),
                ("reviews/09-demo/notes.txt", "scratch\n"),
            ],
        );
        assert!(pipeline_check_command(&root, &[]).is_ok());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_markdown_file_where_records_live_is_held_to_the_grammar() {
        // The other half of the rule above, and the deliberate one: prose
        // filed among the records is refused by name, not passed over. A
        // record misnamed by one letter is the case that matters — passed
        // over, its task reads as unreviewed while the file sits right
        // there, and nothing names the file that would have explained it.
        let root = gate_fixture(
            "prose",
            &[
                spec_with_one_tick(),
                pass_record(),
                ("reviews/09-demo/README.md", "# Notes about this spec\n"),
            ],
        );
        let error = pipeline_check_command(&root, &[]).unwrap_err().to_string();
        assert!(
            error.contains("09-demo/README.md") && error.contains("`Spec:`"),
            "the refusal does not name the file and what it is not: {error}",
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn pipeline_check_takes_no_arguments() {
        let error = pipeline_check_command(std::path::Path::new("."), &["specs".into()])
            .unwrap_err()
            .to_string();
        assert!(error.contains("pipeline-check"), "no usage in: {error}");
    }

    #[test]
    fn the_usage_lists_the_gate() {
        assert!(
            usage().contains("pipeline-check"),
            "a command nobody is told about: {}",
            usage(),
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
