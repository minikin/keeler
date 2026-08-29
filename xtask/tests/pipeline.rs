//! Spec 08 — the pipeline enforces itself: T4, the spec reader.
//!
//! Acceptance runs through the filesystem, the way `pipeline-check` will
//! read `specs/` once T5 wires the command — and once against this
//! repository's real specs, because those are the files the gate must
//! never misread.

use std::path::{Path, PathBuf};

use xtask::pipeline::decision::{Decision, TaskId, decide};
use xtask::pipeline::specs;

/// A throwaway specs directory holding the given `(file name, content)`.
fn fixture(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("xtask-specs-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (file, content) in files {
        std::fs::write(dir.join(file), content).unwrap();
    }
    dir
}

#[test]
fn an_implemented_spec_vouches_for_every_task() {
    // Given a spec marked Implemented with a task unticked, and its one
    // ticked task unreviewed
    let dir = fixture(
        "vouch",
        &[(
            "09-demo.md",
            "# Spec 09 — Demo\n\n**Status:** Implemented\n\n## Tasks\n\n\
             - [x] **T1 — Shipped.**\n- [ ] **T2 — Forgotten.**\n",
        )],
    );

    // When the gate's spec side runs over the directory
    let read = specs::read_from(&dir).unwrap();

    // Then the unticked task breaks the promise, named as spec and task
    let broken = specs::unkept_promises(&read);
    assert_eq!(broken, vec![TaskId::new("09-demo", "t2")]);
    assert_eq!(broken[0].to_string(), "09-demo/t2");

    // And the ticked task flows into the decision, which names it too —
    // Implemented cannot vouch for a task no record accounts for
    assert_eq!(
        decide(&specs::ticked(&read), &[], &[]),
        Decision::Missing(vec![TaskId::new("09-demo", "t1")]),
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn every_spec_this_repository_holds_is_read_but_the_template() {
    // Given this repository's own specs directory
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../specs");

    // When the reader takes all of it
    let read = specs::read_from(&dir).unwrap();
    let slugs: Vec<&str> = read.iter().map(|spec| spec.slug.as_str()).collect();

    // Then the template is not a spec, and every real one is read, in
    // file-name order
    assert!(
        !slugs.contains(&"TEMPLATE"),
        "the template was read: {slugs:?}"
    );
    for expected in [
        "01-gated-deliverable",
        "02-release-automation",
        "03-installer-against-the-wild",
        "04-xtask-migration",
        "05-installer-in-rust",
        "06-graph-mode",
        "07-fan-out",
        "08-the-pipeline-enforces-itself",
    ] {
        assert!(
            slugs.contains(&expected),
            "{expected} was not read: {slugs:?}"
        );
    }
    assert!(slugs.is_sorted(), "not in file-name order: {slugs:?}");

    // And the fenced example item in spec 06's implementation notes was
    // not read as a sixteenth, unticked task
    let graph_mode = read
        .iter()
        .find(|spec| spec.slug == "06-graph-mode")
        .unwrap();
    assert!(graph_mode.implemented);
    assert_eq!(graph_mode.tasks.len(), 15);
    assert!(graph_mode.tasks.iter().all(|task| task.ticked));

    // And a Tasks section with no items is a parked spec, not a refusal
    let parked = read
        .iter()
        .find(|spec| spec.slug == "05-installer-in-rust")
        .unwrap();
    assert!(!parked.implemented && parked.tasks.is_empty());

    // And today every Implemented spec keeps its promise, while the
    // ticked tasks — spec 08's t1 among them — flow on to the decision
    assert_eq!(specs::unkept_promises(&read), vec![]);
    assert!(specs::ticked(&read).contains(&TaskId::new("08-the-pipeline-enforces-itself", "t1")),);
}

#[test]
fn a_directory_that_cannot_be_read_is_named() {
    // Given no specs directory at all
    let missing = std::env::temp_dir().join("xtask-specs-nowhere");
    let _ = std::fs::remove_dir_all(&missing);

    // Then the failure names it — a gate that read nothing must not pass
    let refusal = specs::read_from(&missing).unwrap_err().to_string();
    assert!(
        refusal.contains("xtask-specs-nowhere"),
        "the directory is not named: {refusal}",
    );
}

#[test]
fn a_spec_that_breaks_the_grammar_is_refused_naming_file_and_line() {
    // Given one readable spec and one whose checkbox the grammar cannot read
    let dir = fixture(
        "broken",
        &[
            (
                "01-fine.md",
                "**Status:** Approved\n\n## Tasks\n\n- [x] **T1 — Fine.**\n",
            ),
            (
                "02-broken.md",
                "**Status:** Approved\n\n## Tasks\n\n- [x] fix the thing\n",
            ),
        ],
    );

    // Then the refusal names the file and the line, not just a message
    let refusal = specs::read_from(&dir).unwrap_err().to_string();
    assert!(
        refusal.contains("02-broken.md") && refusal.contains("line 5"),
        "file and line are not named: {refusal}",
    );

    let _ = std::fs::remove_dir_all(dir);
}
