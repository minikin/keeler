//! Spec 08 — the pipeline enforces itself: T4, the spec reader, and T5,
//! the command over it.
//!
//! Acceptance runs through the filesystem, the way `pipeline-check` reads
//! `specs/` — and once against this repository's real specs, because those
//! are the files the gate must never misread. T5's half runs the shipped
//! binary as CI runs it: a directory of our making, an exit code, and the
//! repository itself standing in front of its own gate.

use std::path::{Path, PathBuf};

use xtask::pipeline::decision::{Decision, TaskId, Uncovered, Why, decide};
use xtask::pipeline::{backlog, records, specs};

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
        Decision::Missing(vec![Uncovered {
            task: TaskId::new("09-demo", "t1"),
            why: Why::Unreviewed,
        }]),
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

// T5 — the command: `cargo xtask pipeline-check`, and the debt on the books.

/// The command as CI runs it: the shipped binary, in a directory of our
/// making. A library call could not see the exit code, and the exit code is
/// the whole of what CI reads.
fn pipeline_check(root: &Path) -> std::process::Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_xtask"))
        .arg("pipeline-check")
        .current_dir(root)
        .output()
        .expect("failed to run the xtask binary")
}

/// A throwaway repository root holding the given `(relative path, content)`.
fn repository(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let root = std::env::temp_dir().join(format!("xtask-gate-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    for (path, content) in files {
        let file = root.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, content).unwrap();
    }
    root
}

#[test]
fn the_gate_needs_no_git_history() {
    // Given a record whose `Commit:` names a SHA absent from this clone —
    // in a directory that is no clone at all, so there is no history to
    // consult even for a gate that wanted to
    let root = repository(
        "no-history",
        &[
            (
                "specs/09-demo.md",
                "**Status:** Implemented\n\n## Tasks\n\n- [x] **T1 — Shipped.**\n",
            ),
            (
                "reviews/09-demo/t1.md",
                "Spec: 09-demo\nTask: t1\nCommit: 0000000000000000000000000000000000000000\n\
                 Verdict: pass\n\n## Findings\n\nnone\n",
            ),
        ],
    );
    assert!(
        !root.join(".git").exists(),
        "the fixture is a git repository — the test would prove nothing",
    );

    // When the gate runs
    let output = pipeline_check(&root);
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();

    // Then the record still counts — ancestry belongs to the pull-request
    // check, and the gate consults no history at all
    assert!(
        output.status.success(),
        "an unresolvable `Commit:` failed the gate:\n{stdout}{}",
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(
        stdout.contains("1 ticked task"),
        "the gate does not say what it accounted for: {stdout}",
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_tick_this_repository_cannot_account_for_fails_the_command() {
    // Given a repository whose one ticked task has neither record nor debt
    let root = repository(
        "skipped",
        &[(
            "specs/09-demo.md",
            "**Status:** Approved\n\n## Tasks\n\n- [x] **T1 — Ticked anyway.**\n",
        )],
    );

    // When the gate runs
    let output = pipeline_check(&root);

    // Then it exits non-zero — CI reads the code, not the prose — and says
    // on stderr which task it is
    assert!(
        !output.status.success(),
        "a skipped review reported success — CI would merge on it",
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("09-demo/t1"),
        "the refusal does not name the task: {stderr}",
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).trim().is_empty(),
        "a failed gate printed a result to stdout",
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn this_repository_passes_its_own_gate() {
    // Given this repository, whose thirty-nine historical ticks are on the
    // committed backlog and whose every later tick holds a record
    let root = repo_root();

    // When the gate runs here
    let output = pipeline_check(&root);

    // Then it passes — the gate this spec exists to add is one this
    // repository already satisfies, or it would have been unmergeable
    assert!(
        output.status.success(),
        "Keeler does not pass its own pipeline gate:\n{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    // And the debt it leans on is honest: every line names a ticked task
    // that holds no record. A line for an unticked task is debt for
    // nothing; a line beside a record is debt already paid, and the gate
    // itself cannot see either — a record outranks the backlog, so dead
    // debt sits there unread.
    let debt = backlog::read_from(&root.join("reviews/BACKLOG.md")).unwrap();
    let ticked = specs::ticked(&specs::read_from(&root.join("specs")).unwrap());
    let recorded: Vec<TaskId> = records::read_from(&root.join("reviews"))
        .unwrap()
        .into_iter()
        .map(|record| record.task)
        .collect();
    for task in &debt {
        assert!(
            ticked.contains(task),
            "{task} is on the backlog but no spec ticks it",
        );
        assert!(
            !recorded.contains(task),
            "{task} is on the backlog and reviewed — the line is dead debt",
        );
    }

    // And it is the thirty-nine the spec names: specs 01–04's every tick,
    // plus spec 06's t1 and t2. Working one off is deliberate work, and
    // this number coming down is how it announces itself.
    assert_eq!(
        debt.len(),
        39,
        "the accepted-debt list is no longer the thirty-nine spec 08 seeded",
    );
    for spec in ["01-gated-deliverable", "06-graph-mode"] {
        assert!(
            debt.iter().any(|task| task.to_string().starts_with(spec)),
            "no debt line for {spec} — the seed is not what the spec describes",
        );
    }
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}
