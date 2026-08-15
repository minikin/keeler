//! Spec 04 — the release tooling moves to `cargo xtask`.
//!
//! The xtask crate is repository machinery. Everything here guards the line
//! between what Keeler *uses* to build and release itself and what it
//! *ships* to the projects it installs into.

use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Repository machinery that must never reach an adopting project.
const REPO_ONLY_MACHINERY: [&str; 2] = ["xtask", ".cargo"];

#[test]
fn adopters_receive_nothing_new() {
    // Given a repository that has an xtask crate and a cargo alias — the
    // test is worthless if these do not exist here
    for path in REPO_ONLY_MACHINERY {
        assert!(
            repo_root().join(path).exists(),
            "`{path}` is missing from this repository — nothing is being guarded",
        );
    }
    assert!(
        repo_root().join(".cargo/config.toml").is_file(),
        "the cargo alias is missing — `cargo xtask` would not run here",
    );

    // And `cargo xtask` actually resolves in this repository
    let help =
        std::process::Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
            .args(["xtask", "--help"])
            .current_dir(repo_root())
            .output()
            .expect("failed to run cargo xtask");
    assert!(
        help.status.success(),
        "`cargo xtask --help` failed:\n{}{}",
        String::from_utf8_lossy(&help.stdout),
        String::from_utf8_lossy(&help.stderr),
    );

    // When Keeler is installed into a fresh project
    let project = std::env::temp_dir().join(format!("keeler-xtask-ship-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&project);
    std::fs::create_dir_all(project.join("src")).unwrap();
    std::fs::write(
        project.join("Cargo.toml"),
        "[package]\nname = \"adopter\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    std::fs::write(project.join("src/lib.rs"), "pub fn adopter() {}\n").unwrap();

    let install = std::process::Command::new("bash")
        .arg(repo_root().join("install.sh"))
        .arg(&project)
        .arg("--no-tools")
        .output()
        .expect("failed to run install.sh");
    assert!(
        install.status.success(),
        "install.sh failed:\n{}{}",
        String::from_utf8_lossy(&install.stdout),
        String::from_utf8_lossy(&install.stderr),
    );

    // Then none of the machinery lands in it
    let mut shipped = Vec::new();
    for path in REPO_ONLY_MACHINERY {
        if project.join(path).exists() {
            shipped.push(path);
        }
    }
    // And the workflow set is unchanged — one workflow, the adopter's gate
    let workflows: Vec<String> = std::fs::read_dir(project.join(".github/workflows"))
        .map(|dir| {
            dir.map(Result::unwrap)
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    let _ = std::fs::remove_dir_all(&project);

    assert!(
        shipped.is_empty(),
        "Keeler's own machinery landed in an adopting project: {shipped:?}",
    );
    assert_eq!(
        workflows,
        vec!["keeler.yml".to_string()],
        "the shipped workflow set changed",
    );
}

fn release_workflow() -> String {
    std::fs::read_to_string(repo_root().join(".github/workflows/release.yml")).unwrap()
}

#[test]
fn the_release_workflow_speaks_xtask() {
    // Given the release workflow
    let workflow = release_workflow();

    // Then guard, notes and checksum run through `cargo xtask`
    for command in ["release-guard", "release-notes", "checksum"] {
        assert!(
            workflow.contains(&format!("cargo xtask {command}")),
            "the workflow does not run `cargo xtask {command}`",
        );
    }

    // And no step invokes a script under scripts/
    let script_steps: Vec<&str> = workflow
        .lines()
        .filter(|line| line.contains("scripts/"))
        .collect();
    assert!(
        script_steps.is_empty(),
        "the release still runs shell scripts: {script_steps:?}",
    );

    // And the guard and the gates still strictly precede publication
    let position = |needle: &str| {
        workflow
            .find(needle)
            .unwrap_or_else(|| panic!("the workflow lost `{needle}`"))
    };
    let publish = position("gh release create");
    assert!(
        position("cargo xtask release-guard") < publish && position("just ci") < publish,
        "the guard or the gates no longer precede publication",
    );
}

#[test]
fn no_workflow_calls_a_script_that_is_not_there() {
    // A workflow naming a deleted script fails only when it runs, which for
    // the release workflow means at the moment of a release. Cheap to check
    // here, expensive to discover there.
    let workflows = repo_root().join(".github/workflows");
    let mut dangling = Vec::new();
    for entry in std::fs::read_dir(&workflows).unwrap().map(Result::unwrap) {
        let workflow = std::fs::read_to_string(entry.path()).unwrap();
        for (number, line) in workflow.lines().enumerate() {
            let Some(rest) = line.split("scripts/").nth(1) else {
                continue;
            };
            let script: String = rest
                .chars()
                .take_while(|c| !c.is_whitespace() && *c != '"' && *c != '\'')
                .collect();
            if !repo_root().join("scripts").join(&script).exists() {
                dangling.push(format!(
                    "{}: line {}: scripts/{script}",
                    entry.file_name().to_string_lossy(),
                    number + 1,
                ));
            }
        }
    }
    assert!(
        dangling.is_empty(),
        "workflows call scripts that do not exist: {dangling:?}",
    );
}

#[test]
fn the_shell_gate_covers_exactly_the_shell_that_remains() {
    // Given the repository after the migration
    let mut scripts: Vec<String> = Vec::new();
    for dir in [".", "scripts"] {
        let Ok(entries) = std::fs::read_dir(repo_root().join(dir)) else {
            continue;
        };
        for entry in entries.map(Result::unwrap) {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if path.extension().is_some_and(|ext| ext == "sh") {
                scripts.push(if dir == "." {
                    name
                } else {
                    format!("{dir}/{name}")
                });
            }
        }
    }
    scripts.sort();

    // Then install.sh and the spec 03 contract checker are the only ones
    assert_eq!(
        scripts,
        vec![
            "install.sh".to_string(),
            "scripts/integration-check.sh".to_string(),
        ],
        "the shell that remains is not what the spec says it should be",
    );

    // And no release logic remains in shell
    for gone in ["release-notes.sh", "release-guard.sh", "checksum.sh"] {
        assert!(
            !repo_root().join("scripts").join(gone).exists(),
            "{gone} is still here — the release logic did not move",
        );
    }

    // And the lint gate covers exactly those two — by covering everything
    // under scripts/, which is the same set while the inventory above
    // holds. Naming them instead would leave tomorrow's script ungated,
    // which spec 02 forbids.
    let justfile = std::fs::read_to_string(repo_root().join("Justfile")).unwrap();
    assert!(
        justfile.contains("shellcheck install.sh scripts/*.sh"),
        "the lint gate no longer covers the shell that remains",
    );
}
