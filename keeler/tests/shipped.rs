//! What the binary carries, in both directions.
//!
//! Everything Keeler ships must be inside the binary — a file that lands
//! in an adopting project cannot come from anywhere else once install.sh
//! is gone. And nothing of this repository's own may be in there: an
//! adopter's project is not the place for our workspace, our tasks or our
//! release machinery.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the keeler crate sits in the workspace root")
        .to_path_buf()
}

/// Every file under `dir`, relative to `base`.
fn files_under(dir: &Path, base: &Path, into: &mut BTreeSet<String>) {
    for entry in std::fs::read_dir(dir).unwrap().map(Result::unwrap) {
        let path = entry.path();
        if path.is_dir() {
            files_under(&path, base, into);
        } else {
            into.insert(
                path.strip_prefix(base)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
            );
        }
    }
}

#[test]
fn everything_the_repository_ships_is_inside_the_binary() {
    // Given the trees whose every file is meant for adopters
    let mut expected = BTreeSet::new();
    for tree in [".claude/commands/keeler", ".claude/skills"] {
        files_under(&repo_root().join(tree), &repo_root(), &mut expected);
    }

    // When the binary's contents are listed
    let carried: BTreeSet<String> = keeler::shipped_files()
        .iter()
        .map(|(source, _)| (*source).to_string())
        .collect();

    // Then every one of them is carried
    let missing: Vec<&String> = expected.difference(&carried).collect();
    assert!(
        missing.is_empty(),
        "the repository ships these but the binary does not carry them: {missing:?}",
    );
}

#[test]
fn nothing_of_this_repositorys_own_is_inside_the_binary() {
    // Given the paths that exist to run Keeler, not to be installed by it
    const REPO_ONLY: [&str; 8] = [
        "VERSION",
        "CHANGELOG.md",
        "README.md",
        "install.sh",
        "scripts/",
        "xtask/",
        "keeler/",
        ".cargo/",
    ];

    // When the binary's contents are listed
    let carried = keeler::shipped_files();

    // Then none of them is in there
    let mut leaked = Vec::new();
    for (source, destination) in carried {
        for repo_only in REPO_ONLY {
            if source.starts_with(repo_only) || destination.starts_with(repo_only) {
                leaked.push(format!("{source} -> {destination}"));
            }
        }
    }
    assert!(
        leaked.is_empty(),
        "the binary carries this repository's own machinery: {leaked:?}",
    );
}

#[test]
fn every_carried_file_exists_and_carries_the_repositorys_bytes() {
    // A declaration naming a file that is not there, or carrying stale
    // bytes, is worse than no declaration: it looks like coverage.
    for (source, _) in keeler::shipped_files() {
        let path = repo_root().join(source);
        assert!(
            path.is_file(),
            "the binary declares `{source}`, which is not in the repository"
        );
        let on_disk = std::fs::read(&path).unwrap();
        let carried = keeler::carried_bytes(source)
            .unwrap_or_else(|| panic!("`{source}` is declared but nothing is embedded for it"));
        assert_eq!(
            carried,
            on_disk.as_slice(),
            "the bytes carried for `{source}` are not the repository's",
        );
    }
}

#[test]
fn the_binary_ships_exactly_what_install_sh_installs() {
    // A temporary gate, and the point of the whole migration: while both
    // exist, the Rust installer must ship neither more nor less than the
    // shell one. It retires with install.sh in T5.
    let script = std::fs::read_to_string(repo_root().join("install.sh")).unwrap();
    let mut from_shell = BTreeSet::new();
    let mut inside = false;
    for line in script.lines() {
        if line.starts_with("for f in ") {
            inside = true;
        }
        if inside {
            for token in line.split_whitespace() {
                let extension = Path::new(token).extension().and_then(|e| e.to_str());
                if token.contains('/') || matches!(extension, Some("md" | "toml")) {
                    if let Some(name) = token.strip_suffix('\\') {
                        from_shell.insert(name.to_string());
                    } else {
                        from_shell.insert(token.to_string());
                    }
                }
            }
            if line.trim_end().ends_with("; do") {
                break;
            }
        }
    }
    from_shell.remove("Justfile");
    assert!(
        !from_shell.is_empty(),
        "the install.sh file list could not be read — this gate is blind",
    );

    let carried: BTreeSet<String> = keeler::shipped_files()
        .iter()
        .map(|(source, _)| (*source).to_string())
        .collect();
    let only_in_shell: Vec<&String> = from_shell.difference(&carried).collect();
    assert!(
        only_in_shell.is_empty(),
        "install.sh installs these, the binary does not: {only_in_shell:?}",
    );
}

#[test]
fn init_lays_every_carried_file_into_a_fresh_project() {
    // Given an empty Rust project
    let project = std::env::temp_dir().join(format!("keeler-init-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&project);
    std::fs::create_dir_all(project.join("src")).unwrap();
    std::fs::write(
        project.join("Cargo.toml"),
        "[package]\nname = \"adopter\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    std::fs::write(project.join("src/lib.rs"), "pub fn adopter() {}\n").unwrap();

    // When the carried files are laid down
    let report = keeler::lay_down(&project).expect("laying the files down failed");

    // Then every one of them is on disk, with the bytes the binary carries
    for (source, destination) in keeler::shipped_files() {
        let landed = project.join(&destination);
        assert!(landed.is_file(), "`{destination}` never landed");
        assert_eq!(
            std::fs::read(&landed).unwrap(),
            keeler::carried_bytes(source).unwrap(),
            "`{destination}` landed with bytes that are not the carried ones",
        );
    }
    // And the report counts them, so a silent no-op cannot pass for a run
    assert_eq!(report.written, keeler::shipped_files().len());
    let _ = std::fs::remove_dir_all(project);
}
