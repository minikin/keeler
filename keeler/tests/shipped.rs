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

/// True for a swept path the filesystem left behind rather than one the
/// repository ships: a hidden or backup component anywhere below the tree
/// root. The roots themselves live under `.claude/`, so the check starts
/// after them — and a declared single may legitimately be a dotfile.
fn is_swept_junk(source: &str) -> bool {
    [".claude/commands/keeler/", ".claude/skills/"]
        .iter()
        .find_map(|root| source.strip_prefix(root))
        .is_some_and(|below| {
            Path::new(below)
                .components()
                .filter_map(|part| part.as_os_str().to_str())
                .any(|part| part.starts_with('.') || part.ends_with('~'))
        })
}

/// A throwaway directory that goes away even when an assertion fires.
///
/// A test that cleans up on its last line cleans up only when it passes,
/// so every failure leaves a tree behind in a shared directory — and the
/// next run of the same test inherits it.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("keeler-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl std::ops::Deref for TempDir {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
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
    // Whatever the filesystem left lying around is not something the
    // repository ships, so it is not something the binary must carry.
    expected.retain(|path| !is_swept_junk(path));

    // When the binary's contents are listed
    let carried: BTreeSet<String> = keeler::shipped_files()
        .iter()
        .map(|(source, _)| source.clone())
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
        let path = repo_root().join(&source);
        assert!(
            path.is_file(),
            "the binary declares `{source}`, which is not in the repository"
        );
        let on_disk = std::fs::read(&path).unwrap();
        let carried = keeler::carried_bytes(&source)
            .unwrap_or_else(|| panic!("`{source}` is declared but nothing is embedded for it"));
        assert_eq!(
            carried,
            on_disk.as_slice(),
            "the bytes carried for `{source}` are not the repository's",
        );
    }
}

/// What `install.sh` installs, read from the script itself.
///
/// Two places, both of which the first version of this parser missed: the
/// `for f in …` list, and the `install_file` calls that stand outside it
/// because their source and destination names differ.
fn files_install_sh_installs(script: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();

    let mut inside_loop = false;
    for line in script.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("for f in ") {
            inside_loop = true;
            add_tokens(rest, &mut found);
            continue;
        }
        if inside_loop {
            if let Some(rest) = line.strip_suffix("; do") {
                add_tokens(rest, &mut found);
                inside_loop = false;
            } else {
                add_tokens(line, &mut found);
            }
            continue;
        }
        // The rules file is installed outside install_file on purpose: an
        // upgrade replaces it wholesale and keeps the old text as .bak, so
        // it cannot go through the conflict convention. It is still
        // shipped, and reading the script must say so.
        if let Some(rest) = line.strip_prefix("rules=\"$DEST/") {
            if let Some(path) = rest.strip_suffix('"') {
                found.insert(path.to_string());
            }
            continue;
        }
        // `install_file <source> [destination]` — the source is what the
        // repository holds, which is what the binary must carry.
        if let Some(source) = line
            .strip_prefix("install_file ")
            .and_then(|rest| rest.split_whitespace().next())
            .map(|source| source.trim_matches('"'))
            .filter(|source| !source.starts_with('<') && !source.starts_with('$'))
        {
            found.insert(source.to_string());
        }
    }
    found
}

/// Every path token on a line of the loop's file list, minus the shell's
/// line continuations and terminators.
fn add_tokens(line: &str, into: &mut BTreeSet<String>) {
    for token in line.split_whitespace() {
        let token = token.trim_end_matches('\\').trim_end_matches(';');
        if token.is_empty() || token == "do" {
            continue;
        }
        into.insert(token.to_string());
    }
}

#[test]
fn the_install_sh_list_is_read_whole() {
    // The gate below is only worth its name if it sees every file the
    // script installs. The first version silently missed three — Justfile
    // (no extension), rustfmt.toml (the shell `;` clung to it) and
    // templates/keeler.yml (installed after the loop) — and stayed green
    // while claiming to compare the whole set.
    let script = std::fs::read_to_string(repo_root().join("install.sh")).unwrap();
    let found = files_install_sh_installs(&script);

    for expected in [
        ".claude/keeler.md",
        "Justfile",
        "rustfmt.toml",
        "templates/keeler.yml",
        "clippy.toml",
        ".cargo-mutants.toml",
        "KEELER.md",
        "specs/TEMPLATE.md",
        ".claude/commands/keeler/spec.md",
        ".claude/skills/property-testing/SKILL.md",
    ] {
        assert!(
            found.contains(expected),
            "the parser missed `{expected}`: {found:?}"
        );
    }
    assert_eq!(found.len(), 18, "the script installs 18 files: {found:?}");
}

#[test]
fn the_binary_ships_exactly_what_install_sh_installs() {
    // A temporary gate, and the point of the whole migration: while both
    // exist, the Rust installer must ship neither more nor less than the
    // shell one. It retires with install.sh in T5.
    let script = std::fs::read_to_string(repo_root().join("install.sh")).unwrap();
    let from_shell = files_install_sh_installs(&script);
    let carried: BTreeSet<String> = keeler::shipped_files()
        .iter()
        .map(|(source, _)| source.clone())
        .collect();

    let only_in_shell: Vec<&String> = from_shell.difference(&carried).collect();
    let only_in_binary: Vec<&String> = carried.difference(&from_shell).collect();
    assert!(
        only_in_shell.is_empty() && only_in_binary.is_empty(),
        "install.sh installs these and the binary does not: {only_in_shell:?}\n\
         the binary carries these and install.sh does not: {only_in_binary:?}",
    );
}

#[test]
fn init_lays_every_carried_file_into_a_fresh_project() {
    // Given an empty Rust project
    let project = TempDir::new("init");
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
            keeler::carried_bytes(&source).unwrap(),
            "`{destination}` landed with bytes that are not the carried ones",
        );
    }
    // And the report counts them, so a silent no-op cannot pass for a run
    assert_eq!(report.written, keeler::shipped_files().len());
}

#[test]
fn junk_beside_the_shipped_files_is_not_carried() {
    // include_dir! embeds whatever the directory holds, .gitignore and all.
    // A .DS_Store next to a skill would be compiled in and laid into every
    // adopting project — and the completeness gate above cannot see it,
    // because both sides walk the same filesystem and would agree.
    let carried = keeler::shipped_files();
    let junk: Vec<&str> = carried
        .iter()
        .map(|(source, _)| source.as_str())
        .filter(|source| is_swept_junk(source))
        .collect();
    assert!(
        junk.is_empty(),
        "the binary carries files that are not part of the workflow: {junk:?}",
    );
}

#[test]
fn editing_a_shipped_file_rebuilds_the_binary() {
    // include_dir!'s path tracking is behind a nightly feature this project
    // does not enable, so cargo records no dependency on the swept trees:
    // edit a command, rebuild, and the binary still carries the old text.
    // A build script has to say what to watch, or "the binary is the
    // source" is only true until someone edits a file.
    let build = std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("build.rs"))
        .expect("keeler has no build.rs, so nothing tells cargo when to rebuild");

    for watched in [".claude/commands/keeler", ".claude/skills"] {
        assert!(
            build.contains(watched),
            "the build script does not watch `{watched}` — an edit there would not rebuild",
        );
    }
    assert!(
        build.contains("rerun-if-changed"),
        "the build script names the trees but never tells cargo to watch them",
    );
}
