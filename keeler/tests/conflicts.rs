//! What happens when a project already has a file Keeler installs.
//!
//! The contract from spec 01, now the library's to keep: a project's own
//! content is never overwritten. Keeler's copy lands beside it as
//! `<name>.keeler` and the run says so. The rules file is the one
//! documented exception — it is Keeler's to own, so an upgrade replaces it
//! and keeps the old text as `.bak`.

use std::path::{Path, PathBuf};

/// A throwaway project that goes away even when an assertion fires.
struct Project(PathBuf);

impl Project {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("keeler-conflict-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(path.join("src")).unwrap();
        std::fs::write(
            path.join("Cargo.toml"),
            "[package]\nname = \"adopter\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        std::fs::write(path.join("src/lib.rs"), "pub fn adopter() {}\n").unwrap();
        Self(path)
    }

    fn write(&self, relative: &str, content: &str) {
        let path = self.0.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
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
fn a_projects_own_file_is_kept_and_keelers_lands_alongside() {
    // Given a project with its own Justfile
    let project = Project::new("own-justfile");
    project.write("Justfile", "their-recipe:\n\techo theirs\n");

    // When Keeler is installed
    let report = keeler::lay_down(&project).unwrap();

    // Then their file is untouched, Keeler's is beside it, and the run
    // named the conflict
    assert_eq!(project.read("Justfile"), "their-recipe:\n\techo theirs\n");
    assert_eq!(
        project.read("Justfile.keeler").as_bytes(),
        keeler::carried_bytes("Justfile").unwrap(),
    );
    assert_eq!(report.conflicts, vec!["Justfile".to_string()]);
}

#[test]
fn an_identical_file_is_not_a_conflict() {
    // Given a project that already has exactly what Keeler would install —
    // a second run, or an adopter who copied the file by hand
    let project = Project::new("identical");
    let carried = keeler::carried_bytes("clippy.toml").unwrap();
    project.write("clippy.toml", std::str::from_utf8(carried).unwrap());

    // When Keeler is installed
    let report = keeler::lay_down(&project).unwrap();

    // Then nothing is reported and no .keeler file is written: identical
    // bytes are agreement, not disagreement
    assert!(report.conflicts.is_empty(), "{:?}", report.conflicts);
    assert!(!project.join("clippy.toml.keeler").exists());
}

#[test]
fn the_rules_file_is_replaced_and_the_replaced_text_is_kept() {
    // Given a project whose rules file differs — an older Keeler, or one
    // someone edited
    let project = Project::new("rules");
    project.write(
        ".claude/keeler.md",
        "<!-- keeler-version: 0.0.1 -->\nold rules\n",
    );

    // When Keeler is installed
    keeler::lay_down(&project).unwrap();

    // Then the rules are ours to own: replaced wholesale, with the old
    // text kept rather than lost, and no .keeler left behind
    assert_eq!(
        project.read(".claude/keeler.md").as_bytes(),
        keeler::carried_bytes(".claude/keeler.md").unwrap(),
    );
    assert_eq!(
        project.read(".claude/keeler.md.bak"),
        "<!-- keeler-version: 0.0.1 -->\nold rules\n"
    );
    assert!(!project.join(".claude/keeler.md.keeler").exists());
}

#[test]
fn a_report_counts_what_it_did() {
    // Given a project with one file of its own
    let project = Project::new("counts");
    project.write("KEELER.md", "theirs\n");

    // When Keeler is installed
    let report = keeler::lay_down(&project).unwrap();

    // Then written counts only what was actually written, so a conflict
    // cannot pass for an install
    assert_eq!(report.written, keeler::shipped_files().len() - 1);
    assert_eq!(report.conflicts.len(), 1);
}

/// Every file a project may already have with content of its own — the
/// whole install set bar the rules file, which is Keeler's to own. The
/// invariant is stated over any pre-existing file, so the sample is the
/// set rather than a handful of it.
fn theirs() -> Vec<String> {
    keeler::shipped_files()
        .into_iter()
        .map(|(_, destination)| destination)
        .filter(|destination| destination != ".claude/keeler.md")
        .collect()
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig {
        cases: 64,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::WithSource("proptest-regressions"),
        )),
        ..proptest::prelude::ProptestConfig::default()
    })]

    /// Own-content preservation: whatever a project already had, and
    /// whatever it held, comes out byte-identical. The three append
    /// targets are not here — they are T3's business — so for this task
    /// every pre-existing file is untouchable.
    #[test]
    fn a_projects_own_content_is_never_overwritten(
        picks in proptest::collection::vec(proptest::bool::ANY, 18),
        contents in proptest::collection::vec("[ -~\n]{0,80}", 18),
    ) {
        let all = theirs();
        let which: Vec<&String> = all
            .iter()
            .zip(&picks)
            .filter_map(|(path, keep)| keep.then_some(path))
            .collect();
        let project = Project::new("preserved");
        let theirs: Vec<(&&String, &String)> = which.iter().zip(&contents).collect();
        for (path, content) in &theirs {
            project.write(path, content);
        }

        keeler::lay_down(&project).unwrap();

        for (path, content) in &theirs {
            proptest::prop_assert_eq!(
                &project.read(path), *content,
                "{} was changed", path,
            );
        }
    }

    /// Conflict totality: the names reported are exactly the `.keeler`
    /// files on disk. An unreported one is a surprise; a reported one that
    /// does not exist is a lie.
    #[test]
    fn every_conflict_is_reported_by_name(
        picks in proptest::collection::vec(proptest::bool::ANY, 18),
    ) {
        let project = Project::new("totality");
        let all = theirs();
        for (path, _) in all.iter().zip(&picks).filter(|(_, keep)| **keep) {
            project.write(path, "content of their own\n");
        }

        let report = keeler::lay_down(&project).unwrap();

        let mut reported = report.conflicts.clone();
        reported.sort();
        let mut on_disk: Vec<String> = keeler::shipped_files()
            .into_iter()
            .map(|(_, destination)| destination)
            .filter(|destination| project.join(format!("{destination}.keeler")).exists())
            .collect();
        on_disk.sort();

        proptest::prop_assert_eq!(reported, on_disk);
    }
}

#[test]
fn an_unchanged_rules_file_is_left_alone() {
    // Given a project whose rules file is already exactly ours — the
    // ordinary case of running the installer twice
    let project = Project::new("rules-same");
    let carried = keeler::carried_bytes(".claude/keeler.md").unwrap();
    project.write(".claude/keeler.md", std::str::from_utf8(carried).unwrap());

    // When Keeler is installed
    let report = keeler::lay_down(&project).unwrap();

    // Then no .bak is written: there is no replaced text to keep, and a
    // .bak that duplicates the file it shadows is noise in every upgrade
    assert!(
        !project.join(".claude/keeler.md.bak").exists(),
        "a .bak was written for a file that did not change",
    );
    // And it is not counted as written, because it was not written: the
    // count exists so a run that did nothing cannot read like an install.
    assert_eq!(report.written, keeler::shipped_files().len() - 1);
}

#[test]
fn replacing_the_rules_file_counts_as_one_write() {
    // Given a project whose rules differ
    let project = Project::new("rules-count");
    project.write(".claude/keeler.md", "old rules\n");

    // When Keeler is installed
    let report = keeler::lay_down(&project).unwrap();

    // Then the count is what actually happened — every carried file
    // written, the rules among them, and no conflict
    assert_eq!(report.written, keeler::shipped_files().len());
    assert!(report.conflicts.is_empty(), "{:?}", report.conflicts);
}

/// True when the process can read anything regardless of permissions, which
/// makes a permission-based test meaningless.
fn running_as_root() -> bool {
    std::process::Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .is_some_and(|uid| uid.trim() == "0")
}

#[test]
fn an_unreadable_file_is_not_mistaken_for_an_absent_one() {
    if running_as_root() {
        return;
    }
    // Given a project whose Justfile exists but cannot be read — a
    // root-owned or ACL-restricted config is the ordinary way this happens
    let project = Project::new("unreadable");
    project.write("Justfile", "THEIR PRECIOUS RECIPE\n");
    std::process::Command::new("chmod")
        .args(["0222", project.join("Justfile").to_str().unwrap()])
        .status()
        .unwrap();

    // When Keeler is installed
    let report = keeler::lay_down(&project).unwrap();

    // Then their file is left alone and named as a conflict. Absent and
    // unreadable are different things: install.sh tested for existence,
    // and reading instead would destroy content it could not even see.
    std::process::Command::new("chmod")
        .args(["0644", project.join("Justfile").to_str().unwrap()])
        .status()
        .unwrap();
    assert_eq!(project.read("Justfile"), "THEIR PRECIOUS RECIPE\n");
    assert!(
        report.conflicts.contains(&"Justfile".to_string()),
        "{report:?}"
    );
    assert!(project.join("Justfile.keeler").exists());
}

#[test]
fn a_directory_where_a_file_belongs_is_a_conflict_not_a_crash() {
    // Given a project that keeps a directory where Keeler installs a file
    let project = Project::new("directory");
    std::fs::create_dir_all(project.join("specs/TEMPLATE.md")).unwrap();

    // When Keeler is installed
    let report = keeler::lay_down(&project).unwrap();

    // Then it is reported like any other conflict, and every other file
    // still lands — one odd path must not leave a half-installed project
    assert!(
        report.conflicts.contains(&"specs/TEMPLATE.md".to_string()),
        "{report:?}",
    );
    assert!(
        project.join("specs/TEMPLATE.md").is_dir(),
        "their directory was removed"
    );
    assert!(
        project.join("KEELER.md").is_file(),
        "the install stopped early"
    );
    assert!(project.join(".claude/commands/keeler/spec.md").is_file());
}

#[test]
fn an_unreadable_rules_file_is_refused_rather_than_replaced() {
    if running_as_root() {
        return;
    }
    // Given a rules file that exists but cannot be read
    let project = Project::new("unreadable-rules");
    project.write(".claude/keeler.md", "their edited rules\n");
    std::process::Command::new("chmod")
        .args(["0222", project.join(".claude/keeler.md").to_str().unwrap()])
        .status()
        .unwrap();

    // When Keeler is installed
    let outcome = keeler::lay_down(&project);

    // Then it refuses, naming the file: the promise that makes owning this
    // file acceptable is that the replaced text is kept, and text that
    // cannot be read cannot be kept
    std::process::Command::new("chmod")
        .args(["0644", project.join(".claude/keeler.md").to_str().unwrap()])
        .status()
        .unwrap();
    let error = outcome
        .expect_err("the rules file was replaced unread")
        .to_string();
    assert!(
        error.contains(".claude/keeler.md"),
        "the refusal does not name it: {error}"
    );
    assert_eq!(project.read(".claude/keeler.md"), "their edited rules\n");
    assert!(
        !project.join("KEELER.md").exists(),
        "files were written before the refusal"
    );
}

#[test]
fn a_second_run_reports_nothing_written() {
    // Given a project Keeler was just installed into
    let project = Project::new("second-run");
    keeler::lay_down(&project).unwrap();

    // When it runs again
    let report = keeler::lay_down(&project).unwrap();

    // Then it says so: nothing written, nothing in conflict. A count that
    // included files already identical would report a full install every
    // time and mean nothing.
    assert_eq!(report.written, 0, "a second run claimed to write files");
    assert!(report.conflicts.is_empty(), "{:?}", report.conflicts);
}
