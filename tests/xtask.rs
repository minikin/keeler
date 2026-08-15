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
