//! The two files Keeler edits rather than installs: `Cargo.toml` and
//! `.gitignore`.
//!
//! Both are the project's own, so both are only ever appended to, and only
//! where something is missing. A workspace root is told to do it itself:
//! Keeler cannot add a dev-dependency to a manifest that has no package.

use std::path::{Path, PathBuf};

struct Project(PathBuf);

impl Project {
    fn with_manifest(name: &str, manifest: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("keeler-manifest-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(path.join("src")).unwrap();
        std::fs::write(path.join("Cargo.toml"), manifest).unwrap();
        std::fs::write(path.join("src/lib.rs"), "pub fn adopter() {}\n").unwrap();
        Self(path)
    }

    fn new(name: &str) -> Self {
        Self::with_manifest(
            name,
            "[package]\nname = \"adopter\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
    }

    fn read(&self, relative: &str) -> String {
        std::fs::read_to_string(self.0.join(relative)).unwrap()
    }
}

impl std::ops::Deref for Project {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn a_manifest_gains_what_it_is_missing() {
    // Given a plain package manifest
    let project = Project::new("plain");

    // When Keeler configures it
    let report = keeler::configure(&project).unwrap();

    // Then the sections Keeler's gates need are there, and it says what it
    // added rather than leaving the reader to diff
    let manifest = project.read("Cargo.toml");
    assert!(manifest.contains("[profile.mutants]"), "{manifest}");
    assert!(manifest.contains("[lints.clippy]"), "{manifest}");
    assert!(manifest.contains("proptest"), "{manifest}");
    assert!(
        report.added.contains(&"[profile.mutants]".to_string()),
        "{report:?}"
    );
}

#[test]
fn a_manifest_that_already_has_them_is_not_touched() {
    // Given a manifest already carrying everything
    let project = Project::new("already");
    keeler::configure(&project).unwrap();
    let after_first = project.read("Cargo.toml");

    // When Keeler configures it again
    let report = keeler::configure(&project).unwrap();

    // Then not a byte moves, and nothing is claimed
    assert_eq!(project.read("Cargo.toml"), after_first);
    assert!(report.added.is_empty(), "{report:?}");
}

#[test]
fn proptest_declared_as_a_table_is_detected() {
    // Given a project that declares proptest the other legal way
    let project = Project::with_manifest(
        "table",
        "[package]\nname = \"a\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n\
         [dev-dependencies.proptest]\nversion = \"1\"\n",
    );

    // When Keeler configures it
    keeler::configure(&project).unwrap();

    // Then it is left alone: one declaration form is not more real than
    // the other, and adding a second would break the build
    let manifest = project.read("Cargo.toml");
    assert_eq!(manifest.matches("proptest").count(), 1, "{manifest}");
}

#[test]
fn proptest_derive_alone_is_not_mistaken_for_proptest() {
    // Given a project with proptest-derive but not proptest
    let project = Project::with_manifest(
        "derive",
        "[package]\nname = \"a\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n\
         [dev-dependencies]\nproptest-derive = \"0.5\"\n",
    );

    // When Keeler configures it
    let report = keeler::configure(&project).unwrap();

    // Then proptest is still added: a derive macro is not the crate it
    // derives for, and a prefix match would have skipped it
    assert!(
        report.added.iter().any(|added| added.contains("proptest")),
        "{report:?}",
    );
}

#[test]
fn a_workspace_root_is_told_to_manage_its_own_manifest() {
    // Given a workspace root with no package of its own
    let project = Project::with_manifest(
        "workspace",
        "[workspace]\nmembers = [\"member\"]\nresolver = \"2\"\n",
    );

    // When Keeler configures it
    let report = keeler::configure(&project).unwrap();

    // Then nothing is added — there is no package to add it to — and the
    // run says so rather than leaving the project half-configured in
    // silence
    assert_eq!(
        project.read("Cargo.toml"),
        "[workspace]\nmembers = [\"member\"]\nresolver = \"2\"\n"
    );
    assert!(
        report
            .notes
            .iter()
            .any(|note| note.contains("workspace root")),
        "{report:?}",
    );
}

#[test]
fn gitignore_entries_are_added_once() {
    // Given a project with no .gitignore at all
    let project = Project::new("gitignore-fresh");

    // When Keeler configures it
    keeler::configure_gitignore(&project).unwrap();

    // Then the artifacts its gates produce are ignored — and
    // crap-baseline.json is not among them, because that one belongs in
    // git as the shared reference the delta gate measures against
    let ignored = project.read(".gitignore");
    for entry in ["/target", "lcov.info", "crap-report.json", "mutants.out*/"] {
        assert!(
            ignored.contains(entry),
            "`{entry}` missing from:\n{ignored}"
        );
    }
    assert!(
        !ignored.contains("crap-baseline"),
        "the baseline was ignored:\n{ignored}"
    );
}

#[test]
fn an_equivalent_pattern_is_not_duplicated() {
    // Given a project already ignoring the same paths, written its own way
    let project = Project::new("gitignore-equivalent");
    std::fs::write(project.join(".gitignore"), "target/\nlcov.info\n").unwrap();

    // When Keeler configures it
    keeler::configure_gitignore(&project).unwrap();

    // Then neither is added again: `/target`, `target/` and `target` cover
    // the same thing here, and a pile of equivalent lines is noise the
    // project has to read forever
    let ignored = project.read(".gitignore");
    assert_eq!(ignored.matches("target").count(), 1, "{ignored}");
    assert_eq!(ignored.matches("lcov.info").count(), 1, "{ignored}");
}

#[test]
fn a_gitignore_with_no_final_newline_still_converges() {
    // Given a .gitignore whose last line has no newline — a real one often
    // does not
    let project = Project::new("gitignore-no-newline");
    std::fs::write(project.join(".gitignore"), "/target\ntheirs/").unwrap();

    // When Keeler configures it twice
    keeler::configure_gitignore(&project).unwrap();
    let after_first = project.read(".gitignore");
    keeler::configure_gitignore(&project).unwrap();

    // Then their last line survived intact and the second run changed
    // nothing — without the newline guard the first appended entry would
    // glue itself onto `theirs/` and never match again
    assert!(
        after_first.contains("\ntheirs/\n"),
        "their line was mangled:\n{after_first}"
    );
    assert_eq!(project.read(".gitignore"), after_first);
}

/// Everything an install does to a project, in the order it does it.
fn install(project: &Path) -> Result<(), Box<dyn std::error::Error>> {
    keeler::lay_down(project)?;
    keeler::configure(project)?;
    keeler::configure_gitignore(project)?;
    Ok(())
}

/// Every file in a project, with its bytes.
fn tree(root: &Path) -> std::collections::BTreeMap<String, Vec<u8>> {
    fn walk(dir: &Path, base: &Path, into: &mut std::collections::BTreeMap<String, Vec<u8>>) {
        for entry in std::fs::read_dir(dir).unwrap().map(Result::unwrap) {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, base, into);
            } else {
                into.insert(
                    path.strip_prefix(base)
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                    std::fs::read(&path).unwrap(),
                );
            }
        }
    }
    let mut files = std::collections::BTreeMap::new();
    walk(root, root, &mut files);
    files
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig {
        cases: 48,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::WithSource("proptest-regressions"),
        )),
        ..proptest::prelude::ProptestConfig::default()
    })]

    /// Idempotence: whatever the project looked like, a second install
    /// leaves the tree byte-identical. This is the law that makes re-running
    /// safe to suggest — an upgrade, a retry, a CI step that does not know.
    #[test]
    fn installing_twice_leaves_the_tree_identical(
        gitignore in proptest::option::of("[a-z/.\n]{0,40}"),
        has_proptest in proptest::bool::ANY,
        trailing_newline in proptest::bool::ANY,
    ) {
        let mut manifest =
            String::from("[package]\nname = \"a\"\nversion = \"0.1.0\"\nedition = \"2021\"\n");
        if has_proptest {
            manifest.push_str("\n[dev-dependencies]\nproptest = \"1\"\n");
        }
        let project = Project::with_manifest("idempotent", &manifest);
        if let Some(mut content) = gitignore.clone() {
            if trailing_newline && !content.is_empty() {
                content.push('\n');
            }
            std::fs::write(project.join(".gitignore"), content).unwrap();
        }

        install(&project).unwrap();
        let after_first = tree(&project);
        install(&project).unwrap();

        proptest::prop_assert_eq!(tree(&project), after_first);
    }

    /// Entry merging: no artifact is ignored twice, however the project
    /// already spelled it.
    #[test]
    fn no_entry_is_ignored_twice(
        spellings in proptest::collection::vec(
            proptest::sample::select(vec!["/target", "target/", "target", "/target/"]),
            0..3,
        ),
    ) {
        let project = Project::new("merging");
        if !spellings.is_empty() {
            std::fs::write(
                project.join(".gitignore"),
                format!("{}\n", spellings.join("\n")),
            )
            .unwrap();
        }

        keeler::configure_gitignore(&project).unwrap();

        let ignored = project.read(".gitignore");
        let mentions = ignored
            .lines()
            .filter(|line| line.trim().trim_matches('/') == "target")
            .count();
        proptest::prop_assert_eq!(
            mentions, spellings.len().max(1),
            "target is ignored {} times:\n{}", mentions, ignored,
        );
    }
}

#[test]
fn an_unreadable_gitignore_is_refused_rather_than_replaced() {
    // Given a .gitignore that exists but cannot be read
    let project = Project::new("gitignore-unreadable");
    std::fs::write(project.join(".gitignore"), "their entries\n").unwrap();
    std::process::Command::new("chmod")
        .args(["0222", project.join(".gitignore").to_str().unwrap()])
        .status()
        .unwrap();
    let root = std::process::Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .is_some_and(|uid| uid.trim() == "0");
    if root {
        return;
    }

    // When Keeler configures it
    let outcome = keeler::configure_gitignore(&project);

    // Then it refuses instead of treating unreadable as absent, which
    // would replace their entries with Keeler's four
    std::process::Command::new("chmod")
        .args(["0644", project.join(".gitignore").to_str().unwrap()])
        .status()
        .unwrap();
    let error = outcome
        .expect_err("an unreadable .gitignore was overwritten")
        .to_string();
    assert!(error.contains(".gitignore"), "{error}");
    assert_eq!(project.read(".gitignore"), "their entries\n");
}
