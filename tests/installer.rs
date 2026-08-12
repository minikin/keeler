//! Regression tests for what `install.sh` ships into a user's project.
//!
//! These read the installer and the files it copies — no project is created,
//! so they run in milliseconds under the normal test gate. The end-to-end
//! installer jobs in CI cover the rest.

use std::path::PathBuf;

/// Jobs a project that adopts Keeler can actually run: they need nothing but
/// the project's own sources and the tools the installer set up.
const USER_FACING_JOBS: [&str; 4] = ["lints", "test", "quality", "mutants"];

/// Paths that exist only in the Keeler repository. A shipped workflow that
/// mentions one of them is a workflow that fails on a user's first push.
const REPO_ONLY_PATHS: [&str; 3] = ["VERSION", "CHANGELOG.md", "./install.sh"];

/// Commands that only work in some projects. `cargo test --doc` errors with
/// "no library targets found" in a binary-only project — the `test` recipe
/// guards it, so the shipped workflow goes through `just`.
const NOT_EVERY_PROJECT: [&str; 1] = ["cargo test --doc"];

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The workflow `install.sh` copies to `.github/workflows/keeler.yml`, read
/// out of the installer itself so this test follows the installer instead of
/// drifting from it.
fn shipped_workflow_path() -> PathBuf {
    let installer = std::fs::read_to_string(repo_root().join("install.sh")).unwrap();
    let install = installer
        .lines()
        .map(str::trim)
        .find(|line| {
            line.starts_with("install_file ") && line.ends_with(".github/workflows/keeler.yml")
        })
        .expect("install.sh no longer installs a workflow to keeler.yml");
    let source = install
        .split_whitespace()
        .nth(1)
        .expect("install_file was given no source path");
    repo_root().join(source)
}

/// Names of the top-level entries under `jobs:` — enough YAML for a file we
/// also own.
fn job_names(workflow: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut in_jobs = false;
    for line in workflow.lines() {
        if !line.trim().is_empty() && !line.starts_with(char::is_whitespace) {
            in_jobs = line.trim_end() == "jobs:";
            continue;
        }
        if !in_jobs {
            continue;
        }
        let Some(entry) = line.strip_prefix("  ") else {
            continue;
        };
        if entry.starts_with(' ') || entry.starts_with('#') {
            continue;
        }
        if let Some(name) = entry.trim_end().strip_suffix(':') {
            names.push(name.to_string());
        }
    }
    names
}

#[test]
fn installed_workflow_gates_only_the_users_project() {
    // Given the workflow install.sh ships as .github/workflows/keeler.yml
    let path = shipped_workflow_path();
    let workflow = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("cannot read {}: {err}", path.display()));

    // When its jobs are read
    let jobs = job_names(&workflow);

    // Then it ships the gates ...
    for required in ["lints", "test"] {
        assert!(
            jobs.iter().any(|job| job == required),
            "{} ships no `{required}` job",
            path.display(),
        );
    }
    // ... and nothing beyond what a user's project can run
    for job in &jobs {
        assert!(
            USER_FACING_JOBS.contains(&job.as_str()),
            "{} ships `{job}`, a job that only makes sense in the Keeler repository",
            path.display(),
        );
    }
    // ... and it refers to no file that exists only here
    for path_only_here in REPO_ONLY_PATHS {
        assert!(
            !workflow.contains(path_only_here),
            "{} references `{path_only_here}`, which a user's project does not have",
            path.display(),
        );
    }
    // ... and it runs no command that only some projects can answer
    for fragile in NOT_EVERY_PROJECT {
        assert!(
            !workflow.contains(fragile),
            "{} runs `{fragile}` directly — use the guarded `just` recipe instead",
            path.display(),
        );
    }
}
