//! Regression tests for what `install.sh` ships into a user's project.
//!
//! The workflow tests read the installer and the files it copies. The
//! behavioral tests run `install.sh --no-tools` against a generated project
//! with a stub `cargo` on PATH, so they stay fast and never touch the
//! network. The end-to-end installer jobs in CI cover the rest.

use std::path::{Path, PathBuf};

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

/// A throwaway project for one test run, removed on drop. The `cargo` on its
/// PATH is a stub that logs every invocation to `cargo-calls.log` — any real
/// `cargo` call the installer makes shows up there instead of on the network.
struct TempProject {
    dir: PathBuf,
}

impl TempProject {
    fn new(name: &str, manifest: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("keeler-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        std::fs::write(dir.join("Cargo.toml"), manifest).unwrap();

        let shim = dir.join("bin/cargo");
        std::fs::write(
            &shim,
            "#!/usr/bin/env bash\necho \"$@\" >> \"$(dirname \"$0\")/../cargo-calls.log\"\n",
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        Self { dir }
    }

    fn path(&self) -> &Path {
        &self.dir
    }

    /// Runs `install.sh <project> --no-tools` with the stub cargo first on
    /// PATH; panics if the installer fails.
    fn install(&self) -> String {
        let path_var = std::env::var("PATH").unwrap();
        let output = std::process::Command::new("bash")
            .arg(repo_root().join("install.sh"))
            .arg(&self.dir)
            .arg("--no-tools")
            .env(
                "PATH",
                format!("{}:{path_var}", self.dir.join("bin").display()),
            )
            .output()
            .expect("failed to run install.sh");
        assert!(
            output.status.success(),
            "install.sh failed:\n{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    fn cargo_calls(&self) -> String {
        std::fs::read_to_string(self.dir.join("cargo-calls.log")).unwrap_or_default()
    }
}

impl Drop for TempProject {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn proptest_declared_as_a_table_is_detected() {
    // Given a project that declares proptest in table form
    let project = TempProject::new(
        "proptest-table",
        "[package]\nname = \"probe\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n\
         [dev-dependencies.proptest]\nversion = \"1\"\n",
    );

    // When the installer runs
    let stdout = project.install();

    // Then it sees the existing dependency and never reaches for cargo add
    assert!(
        stdout.contains("proptest already a dev-dependency"),
        "table-form proptest went undetected:\n{stdout}",
    );
    assert!(
        !project.cargo_calls().contains("add"),
        "installer ran `cargo add` for a dependency the project already has: {}",
        project.cargo_calls(),
    );
}

#[test]
fn proptest_derive_alone_is_not_mistaken_for_proptest() {
    // Given a project whose only matching-prefix dependency is proptest-derive
    let project = TempProject::new(
        "proptest-derive",
        "[package]\nname = \"probe\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n\
         [dev-dependencies]\nproptest-derive = \"0.5\"\n",
    );

    // When the installer runs
    let stdout = project.install();

    // Then proptest itself is still added
    assert!(
        !stdout.contains("proptest already a dev-dependency"),
        "proptest-derive was mistaken for proptest:\n{stdout}",
    );
    assert!(
        project.cargo_calls().contains("add --dev --quiet proptest"),
        "installer never added proptest: {:?}",
        project.cargo_calls(),
    );
}

#[test]
fn equivalent_gitignore_patterns_are_not_duplicated() {
    // Given a project that already ignores target/ the standard Rust way
    let project = TempProject::new(
        "gitignore-dup",
        "[package]\nname = \"probe\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n\
         [dev-dependencies]\nproptest = \"1\"\n",
    );
    std::fs::write(project.path().join(".gitignore"), "target/\n").unwrap();

    // When the installer runs
    project.install();

    // Then no equivalent target pattern is appended ...
    let gitignore = std::fs::read_to_string(project.path().join(".gitignore")).unwrap();
    let target_lines: Vec<&str> = gitignore
        .lines()
        .filter(|line| line.trim_matches('/') == "target")
        .collect();
    assert_eq!(
        target_lines,
        ["target/"],
        "installer duplicated an existing target pattern:\n{gitignore}",
    );
    // ... while genuinely missing entries still land
    assert!(
        gitignore.lines().any(|line| line == "lcov.info"),
        "missing entries were not appended:\n{gitignore}",
    );
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
