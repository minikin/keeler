//! Spec 02's contracts, now met by `cargo xtask` instead of shell.
//!
//! The scenario names and fixtures are unchanged on purpose: spec 02 is the
//! oracle for the migration in spec 04, so these tests must pass verbatim
//! against the new implementation. They drive the built binary, not the
//! library, because the exit code and the stream a message lands on are
//! part of the contract the release workflow depends on.
//!
//! The end-to-end act of publishing is only observable on GitHub; whatever
//! the workflow delegates to a command is pinned here against fixtures.

use std::fmt::Write as _;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The xtask binary this suite drives.
///
/// `CARGO_BIN_EXE_xtask` is only defined for the package that declares the
/// binary, and this suite lives in the harness — so the path is derived from
/// where the test binary itself was put, which follows the profile and any
/// `CARGO_TARGET_DIR` without being told.
fn xtask_bin() -> PathBuf {
    let mut path = std::env::current_exe().expect("the test binary has a path");
    path.pop();
    if path.ends_with("deps") {
        path.pop();
    }
    let binary = path.join("xtask");
    assert!(
        binary.is_file(),
        "{} is not built — run the suite with `--workspace` so it is",
        binary.display(),
    );
    binary
}

/// Runs an xtask command, as the release workflow does.
fn run_xtask(args: &[&str]) -> std::process::Output {
    std::process::Command::new(xtask_bin())
        .args(args)
        .output()
        .expect("failed to run the xtask binary")
}

/// A fixture file in its own temp directory, removed on drop — no test run
/// leaves droppings in $TMPDIR, pass or fail.
struct TempPath(PathBuf);

impl std::ops::Deref for TempPath {
    type Target = std::path::Path;
    fn deref(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempPath {
    fn drop(&mut self) {
        if let Some(dir) = self.0.parent() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

/// Writes `content` to a fixture file in a fresh per-name directory.
fn temp_file(name: &str, content: &str) -> TempPath {
    let dir = std::env::temp_dir().join(format!("keeler-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("fixture");
    std::fs::write(&path, content).unwrap();
    TempPath(path)
}

const FIXTURE_CHANGELOG: &str = "\
# Changelog

Preamble text that belongs to no version.

## [Unreleased]

### Added

- an unreleased entry

## [0.2.0] — 2026-08-14

### Changed

- the second release entry
[breaking]: the --foo flag was removed

### Fixed

- another second release entry

## [0.1.0] — 2026-08-11

### Added

- the first release entry

[Unreleased]: https://example.com/compare/v0.2.0...HEAD
[0.2.0]: https://example.com/releases/tag/v0.2.0
[0.1.0]: https://example.com/releases/tag/v0.1.0
";

#[test]
fn the_release_notes_are_exactly_the_versions_changelog_section() {
    // Given a CHANGELOG with sections for several versions and an
    // Unreleased heading
    let changelog = temp_file("notes-fixture", FIXTURE_CHANGELOG);

    // When the notes for version 0.2.0 are extracted
    let output = run_xtask(&["release-notes", "0.2.0", changelog.to_str().unwrap()]);
    let notes = String::from_utf8_lossy(&output.stdout);

    // Then the output is the body of the 0.2.0 section ...
    assert!(
        output.status.success(),
        "extraction failed: {notes}{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        notes.contains("the second release entry")
            && notes.contains("another second release entry")
            && notes.contains("[breaking]: the --foo flag was removed"),
        "the section's own entries are missing:\n{notes}",
    );
    // ... and it contains no heading, entry, or link line from any other
    // section
    for foreign in [
        "an unreleased entry",
        "the first release entry",
        "## [0.2.0]",
        "## [Unreleased]",
        "Preamble text",
        "https://example.com",
    ] {
        assert!(
            !notes.contains(foreign),
            "extraction leaked `{foreign}`:\n{notes}",
        );
    }
}

#[test]
fn extracting_notes_for_an_absent_version_fails_loudly() {
    // Given a CHANGELOG with no section for version 9.9.9
    let changelog = temp_file("absent-fixture", FIXTURE_CHANGELOG);

    // When the notes for 9.9.9 are extracted
    let output = run_xtask(&["release-notes", "9.9.9", changelog.to_str().unwrap()]);

    // Then the extraction exits non-zero and names the version
    assert!(
        !output.status.success(),
        "extraction succeeded for a version that has no section",
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("9.9.9"),
        "the error does not name the version it could not find:\n{}",
        String::from_utf8_lossy(&output.stderr),
    );
}

fn release_workflow() -> String {
    std::fs::read_to_string(repo_root().join(".github/workflows/release.yml"))
        .expect("no release workflow at .github/workflows/release.yml")
}

#[test]
fn a_pushed_tag_that_matches_version_produces_the_release() {
    // The end-to-end act is observable only on GitHub (spec constraint);
    // the harness pins the wiring that produces it: tag trigger, the
    // guard, notes from the CHANGELOG, and both assets.
    let workflow = release_workflow();
    for wired in [
        "tags:",
        "cargo xtask release-guard",
        "cargo xtask release-notes",
        "cargo xtask checksum",
        "--notes-file",
        "install.sh.sha256",
    ] {
        assert!(workflow.contains(wired), "release.yml is missing `{wired}`");
    }
}

#[test]
fn the_gates_run_before_anything_is_published() {
    // Given a tag whose lint or test gate would fail, the workflow must
    // die before creating anything: guard and gates strictly precede the
    // release step.
    let workflow = release_workflow();
    let guard = workflow
        .find("cargo xtask release-guard")
        .expect("no guard step");
    let gates = workflow.find("just ci").expect("no gates step");
    let create = workflow.find("gh release create").expect("no create step");
    assert!(
        guard < create && gates < create,
        "the release step does not come after the guard and the gates",
    );
}

#[test]
fn a_release_is_never_overwritten() {
    // `create` fails on an existing release; nothing may edit or clobber
    let workflow = release_workflow();
    assert!(workflow.contains("gh release create"));
    for forbidden in ["gh release edit", "gh release delete", "--clobber"] {
        assert!(
            !workflow.contains(forbidden),
            "release.yml can overwrite a published release via `{forbidden}`",
        );
    }
}

#[test]
fn the_verification_story_is_documented_where_adopters_look() {
    // Given the README install section and SECURITY.md
    let readme = std::fs::read_to_string(repo_root().join("README.md")).unwrap();
    let security = std::fs::read_to_string(repo_root().join("SECURITY.md")).unwrap();
    let contributing = std::fs::read_to_string(repo_root().join("CONTRIBUTING.md")).unwrap();

    // Then both name the pin-and-verify path
    for (name, doc) in [("README.md", &readme), ("SECURITY.md", &security)] {
        assert!(
            doc.contains("sha256sum -c"),
            "{name} does not tell adopters how to verify install.sh",
        );
    }
    // And the maintainer side — the release checklist — is written down
    assert!(
        contributing.contains("## Cutting a release"),
        "CONTRIBUTING.md carries no release checklist",
    );
}

/// A directory shaped like the repo's release-relevant corner: VERSION,
/// the rules-file marker, and a CHANGELOG with the version's section.
fn release_fixture(name: &str, version: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("keeler-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join(".claude")).unwrap();
    std::fs::write(dir.join("VERSION"), format!("{version}\n")).unwrap();
    std::fs::write(
        dir.join(".claude/keeler.md"),
        format!("<!-- keeler-version: {version} -->\n# rules\n"),
    )
    .unwrap();
    std::fs::write(
        dir.join("CHANGELOG.md"),
        format!("# Changelog\n\n## [{version}] — 2026-08-14\n\n- an entry\n"),
    )
    .unwrap();
    dir
}

/// Runs the release guard from inside a fixture directory.
fn run_guard(dir: &std::path::Path, tag: &str) -> std::process::Output {
    std::process::Command::new(xtask_bin())
        .args(["release-guard", tag])
        .current_dir(dir)
        .output()
        .expect("failed to run the xtask binary")
}

#[test]
fn a_lookalike_changelog_heading_does_not_satisfy_the_guard() {
    // Given VERSION 0.1.0 but a CHANGELOG whose only heading merely
    // pattern-matches it when dots are treated as wildcards
    let dir = release_fixture("guard-lookalike", "0.1.0");
    std::fs::write(
        dir.join("CHANGELOG.md"),
        "# Changelog\n\n## [0x1y0] — 2026-08-14\n\n- an entry\n",
    )
    .unwrap();

    // When the guard runs for the honest tag
    let output = run_guard(&dir, "v0.1.0");

    // Then it refuses: there is no real 0.1.0 section
    assert!(
        !output.status.success(),
        "a lookalike heading satisfied the CHANGELOG check",
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_tag_that_disagrees_with_version_is_refused() {
    // Given VERSION reads 0.1.0
    let dir = release_fixture("guard-refuse", "0.1.0");

    // When a tag naming any other version is pushed
    let output = run_guard(&dir, "v0.2.0");

    // Then the release workflow fails, naming both versions
    assert!(
        !output.status.success(),
        "the guard accepted a tag that disagrees with VERSION",
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("v0.2.0") && stderr.contains("0.1.0"),
        "the refusal does not name both versions:\n{stderr}",
    );

    // And an honest tag passes the same guard
    let honest = run_guard(&dir, "v0.1.0");
    assert!(
        honest.status.success(),
        "the guard refused a truthful tag:\n{}",
        String::from_utf8_lossy(&honest.stderr),
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Verifies a checksum file the way an adopter would: `sha256sum -c`, or
/// `shasum -a 256 -c` where sha256sum does not exist (macOS).
fn verify_checksum(dir: &std::path::Path) -> std::process::Output {
    let (tool, prefix_args): (&str, &[&str]) = if std::process::Command::new("sha256sum")
        .arg("--version")
        .output()
        .is_ok()
    {
        ("sha256sum", &[])
    } else {
        ("shasum", &["-a", "256"])
    };
    std::process::Command::new(tool)
        .args(prefix_args)
        .args(["-c", "install.sh.sha256"])
        .current_dir(dir)
        .output()
        .expect("no sha256 verifier available")
}

#[test]
fn the_published_checksum_verifies_the_script() {
    // Given the checksum file produced for install.sh
    let dir = std::env::temp_dir().join(format!("keeler-checksum-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let script = dir.join("install.sh");
    std::fs::write(&script, "#!/usr/bin/env bash\necho keeler\n").unwrap();
    let output = run_xtask(&["checksum", script.to_str().unwrap()]);
    assert!(
        output.status.success(),
        "checksum failed: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    std::fs::write(dir.join("install.sh.sha256"), &output.stdout).unwrap();

    // When sha256sum -c runs in a directory holding that install.sh,
    // verification succeeds ...
    let verified = verify_checksum(&dir);
    assert!(
        verified.status.success(),
        "verification failed for identical bytes:\n{}{}",
        String::from_utf8_lossy(&verified.stdout),
        String::from_utf8_lossy(&verified.stderr),
    );

    // ... and fails for an install.sh whose bytes differ
    std::fs::write(&script, "#!/usr/bin/env bash\necho tampered\n").unwrap();
    let tampered = verify_checksum(&dir);
    assert!(
        !tampered.status.success(),
        "verification passed for differing bytes",
    );
    let _ = std::fs::remove_dir_all(&dir);
}

proptest::proptest! {
    // Each case spawns the script once per section — keep counts small.
    #![proptest_config(proptest::prelude::ProptestConfig {
        cases: 12,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::WithSource("proptest-regressions"),
        )),
        ..proptest::prelude::ProptestConfig::default()
    })]

    /// Extraction totality: every generated entry line lands in exactly its
    /// own version's extraction — never another's, never dropped.
    #[test]
    fn every_changelog_entry_lands_in_exactly_one_extraction(
        entry_counts in proptest::collection::vec(1usize..4, 1..5),
    ) {
        // Given a CHANGELOG whose section Sᵢ holds entries unique to it
        let mut changelog = String::from("# Changelog\n\n## [Unreleased]\n\n- pending\n\n");
        let versions: Vec<String> =
            (0..entry_counts.len()).map(|i| format!("0.{i}.9")).collect();
        for (i, count) in entry_counts.iter().enumerate() {
            let _ = write!(changelog, "## [{}] — 2026-01-0{}\n\n", versions[i], i + 1);
            for j in 0..*count {
                let _ = writeln!(changelog, "- s{i}-entry{j}");
            }
            changelog.push('\n');
        }
        for version in &versions {
            let _ = writeln!(changelog, "[{version}]: https://example.com/{version}");
        }
        let path = temp_file("totality-fixture", &changelog);

        // When each version is extracted, every entry appears in its own
        // extraction and in no other
        for (i, version) in versions.iter().enumerate() {
            let output = run_xtask(&["release-notes", version, path.to_str().unwrap()]);
            proptest::prop_assert!(output.status.success(), "extraction failed for {version}");
            let notes = String::from_utf8_lossy(&output.stdout).into_owned();
            for (k, count) in entry_counts.iter().enumerate() {
                for j in 0..*count {
                    let entry = format!("s{k}-entry{j}");
                    proptest::prop_assert_eq!(
                        notes.contains(&entry),
                        k == i,
                        "entry {} vs extraction of {}:\n{}", entry, version, notes,
                    );
                }
            }
            proptest::prop_assert!(
                !notes.contains("pending") && !notes.contains("https://example.com"),
                "Unreleased or link lines leaked into {}:\n{}", version, notes,
            );
        }
    }
}
