//! Regression tests for what `install.sh` ships into a user's project.
//!
//! The workflow tests read the installer and the files it copies. The
//! behavioral tests run `install.sh --no-tools` against generated projects
//! whose PATH stubs out `cargo` (logged, not executed) and `curl` (fails
//! loudly), so they stay fast and can never touch the network. The
//! end-to-end installer jobs in CI cover the rest.

use std::path::{Path, PathBuf};

/// Jobs a project that adopts Keeler can actually run: they need nothing but
/// the project's own sources and the tools the installer set up. The last
/// two read only git history, a file the review stage writes, and the
/// project's own `crap-baseline.json` and `Justfile`.
const USER_FACING_JOBS: [&str; 6] = [
    "lints",
    "test",
    "quality",
    "mutants",
    "branch-baseline",
    "review-record",
];

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

/// Every file under `dir`, recursively — the one tree walker every test
/// shares, so traversal policy can never diverge between them. Finder
/// droppings (`.DS_Store`) are nobody's files.
fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(current) = pending.pop() {
        for entry in std::fs::read_dir(&current).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else if path.file_name().is_none_or(|name| name != ".DS_Store") {
                files.push(path);
            }
        }
    }
    files
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

/// A throwaway project for one test run, removed on drop. Its `bin/` holds
/// the PATH stubs: `cargo` logs to `cargo-calls.log` instead of executing,
/// `curl` logs to `network-calls.log` and fails.
struct TempProject {
    dir: PathBuf,
}

impl TempProject {
    fn new(name: &str, manifest: &str) -> Self {
        let project = Self::bare(name);
        std::fs::write(project.dir.join("Cargo.toml"), manifest).unwrap();
        project
    }

    /// A directory with the test scaffolding but no Cargo.toml — not a Rust
    /// project at all.
    fn bare(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("keeler-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("bin")).unwrap();

        // Two stubs sit first on PATH for everything the harness runs:
        // `cargo` logs its invocations instead of executing them — except
        // `metadata`, which is offline and read-only and which the shipped
        // recipes probe for targets — and `curl` fails loudly, so no
        // generated case can quietly reach the network.
        let real_cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
        let cargo_stub = format!(
            "#!/usr/bin/env bash\n\
             echo \"$@\" >> \"$(dirname \"$0\")/../cargo-calls.log\"\n\
             if [ \"$1\" = metadata ]; then exec \"{real_cargo}\" \"$@\"; fi\n",
        );
        let stubs = [
            ("bin/cargo", cargo_stub.as_str()),
            (
                "bin/curl",
                "#!/usr/bin/env bash\n\
                 echo \"curl $*\" >> \"$(dirname \"$0\")/../network-calls.log\"\n\
                 echo \"harness: network access refused: curl $*\" >&2\nexit 7\n",
            ),
        ];
        for (path, script) in stubs {
            let stub = dir.join(path);
            std::fs::write(&stub, script).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
        }
        Self { dir }
    }

    fn path(&self) -> &Path {
        &self.dir
    }

    /// Runs `install.sh <project> --no-tools` with the stub cargo first on
    /// PATH, succeeding or not.
    fn try_install(&self) -> std::process::Output {
        let path_var = std::env::var("PATH").unwrap();
        std::process::Command::new("bash")
            .arg(repo_root().join("install.sh"))
            .arg(&self.dir)
            .arg("--no-tools")
            .env(
                "PATH",
                format!("{}:{path_var}", self.dir.join("bin").display()),
            )
            .output()
            .expect("failed to run install.sh")
    }

    /// Runs the installer and panics if it fails.
    fn install(&self) -> String {
        let output = self.try_install();
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

    /// Every file in the project as `relative path → bytes`, excluding the
    /// test's own scaffolding (the stub bin/ and the call logs). Two equal
    /// snapshots mean two byte-identical trees.
    fn tree_snapshot(&self) -> std::collections::BTreeMap<String, Vec<u8>> {
        let mut snapshot = std::collections::BTreeMap::new();
        for path in files_under(&self.dir) {
            let rel = path
                .strip_prefix(&self.dir)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            if rel.starts_with("bin/") || rel == "cargo-calls.log" || rel == "network-calls.log" {
                continue;
            }
            snapshot.insert(rel, std::fs::read(&path).unwrap());
        }
        snapshot
    }

    /// Runs git in the project directory with a fixed identity, so tests can
    /// build commit history without touching the user's config.
    fn git(&self, args: &[&str]) {
        let status = std::process::Command::new("git")
            .args(["-c", "user.email=probe@keeler", "-c", "user.name=probe"])
            .args(args)
            .current_dir(&self.dir)
            // The developer's own git config must not leak in: a global
            // commit.gpgsign=true would hang the suite waiting for a key.
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .status()
            .expect("failed to run git");
        assert!(status.success(), "git {args:?} failed");
    }

    /// Runs a recipe of the project's installed `Justfile` with the stub
    /// cargo first on PATH, so no real cargo subcommand ever executes.
    fn run_just(&self, recipe: &str) -> std::process::Output {
        self.run_just_args(&[recipe])
    }

    /// `run_just` for the recipes that take arguments — and for `--list`,
    /// which is what an adopter sees when they type `just` with none.
    fn run_just_args(&self, args: &[&str]) -> std::process::Output {
        let path_var = std::env::var("PATH").unwrap();
        std::process::Command::new("just")
            .args(args)
            .current_dir(&self.dir)
            .env(
                "PATH",
                format!("{}:{path_var}", self.dir.join("bin").display()),
            )
            .output()
            .expect("failed to run just")
    }
}

impl Drop for TempProject {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// A manifest whose proptest dependency the installer detects up front, so
/// no test run ever reaches `cargo add` — or the network.
const MANIFEST_WITH_PROPTEST: &str = "[package]\nname = \"probe\"\nversion = \"0.1.0\"\n\
     edition = \"2021\"\n\n[dev-dependencies]\nproptest = \"1\"\n";

#[test]
fn coverage_and_crap_recipes_are_honest_about_a_project_with_no_rust_sources() {
    // Given a project with no src/ directory
    let project = TempProject::new("no-src-recipes", MANIFEST_WITH_PROPTEST);
    project.install();

    for recipe in ["cov", "crap"] {
        // When the coverage recipe or the CRAP recipe runs
        let output = project.run_just(recipe);
        let stdout = String::from_utf8_lossy(&output.stdout);

        // Then it reports that there are no Rust sources to measure
        assert!(
            stdout.contains("no Rust sources"),
            "`just {recipe}` did not report the missing sources:\n{stdout}{}",
            String::from_utf8_lossy(&output.stderr),
        );
        // And it does not fail the build for the absence
        assert!(
            output.status.success(),
            "`just {recipe}` failed on a project with no Rust sources:\n{stdout}{}",
            String::from_utf8_lossy(&output.stderr),
        );
    }
}

#[test]
fn crap_baseline_and_delta_recipes_are_honest_without_rust_sources() {
    // Given a project with no src/ directory
    let project = TempProject::new("no-src-baseline", MANIFEST_WITH_PROPTEST);
    project.install();

    // When the baseline or delta recipe runs, the same honesty applies
    for recipe in ["crap-baseline", "crap-delta"] {
        let output = project.run_just(recipe);
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("no Rust sources"),
            "`just {recipe}` did not report the missing sources:\n{stdout}{}",
            String::from_utf8_lossy(&output.stderr),
        );
        assert!(
            output.status.success(),
            "`just {recipe}` failed on a project with no Rust sources:\n{stdout}{}",
            String::from_utf8_lossy(&output.stderr),
        );
    }
}

#[test]
fn mutation_testing_reports_what_it_did_not_measure() {
    // Given a git project whose only change touches no file under src/
    let project = TempProject::new("mutants-reach", MANIFEST_WITH_PROPTEST);
    project.install();
    project.git(&["init", "-q"]);
    project.git(&["add", "-A"]);
    project.git(&["commit", "-qm", "init"]);
    std::fs::write(project.path().join("NOTES.md"), "docs only\n").unwrap();

    // When the mutation gate runs
    let output = project.run_just("mutants-diff");
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Then it reports that the change lies outside its reach
    assert!(
        stdout.contains("outside the mutation gate"),
        "`just mutants-diff` did not report the change as out of reach:\n{stdout}{}",
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(
        output.status.success(),
        "`just mutants-diff` failed on an out-of-reach change:\n{stdout}{}",
        String::from_utf8_lossy(&output.stderr),
    );
    // And it does not run mutants to report counts as evidence for it.
    // The check is on the invocation, not the substring: the fixture's own
    // directory is called keeler-mutants-reach-<pid>, so any call that
    // mentions a path here contains the word.
    assert!(
        !project
            .cargo_calls()
            .lines()
            .any(|call| call.starts_with("mutants")),
        "mutants ran on a change it cannot measure: {}",
        project.cargo_calls(),
    );
}

#[test]
fn a_statically_detectable_shell_defect_fails_the_gate() {
    // Given the Keeler repository (the templates/ marker is what keys the
    // shell gate) whose install.sh contains an unquoted expansion that can
    // split on whitespace
    let project = TempProject::new("shell-defect", MANIFEST_WITH_PROPTEST);
    project.install();
    std::fs::create_dir_all(project.path().join("templates")).unwrap();
    std::fs::write(project.path().join("templates/keeler.yml"), "").unwrap();
    // The repo shape includes scripts/ — the gate globs it without nullglob.
    std::fs::create_dir_all(project.path().join("scripts")).unwrap();
    std::fs::write(
        project.path().join("scripts/noop.sh"),
        "#!/usr/bin/env bash\ntrue\n",
    )
    .unwrap();
    std::fs::write(
        project.path().join("install.sh"),
        "#!/usr/bin/env bash\nfiles=$1\nls $files\n",
    )
    .unwrap();

    // When the quality gate runs
    let output = project.run_just("lint");
    let report = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    // Then it fails
    assert!(
        !output.status.success(),
        "the lint gate passed a script with an unquoted expansion:\n{report}",
    );
    // And the report names the file and line — shellcheck's own output,
    // not just's echo of the recipe line
    assert!(
        report.contains("install.sh line 3") && report.contains("SC2086"),
        "the report does not name the file and line:\n{report}",
    );
}

#[test]
fn release_scripts_are_gated_like_the_installer() {
    // Given the Keeler repository shape with a defective script under
    // scripts/ (and an install.sh that is itself clean)
    let project = TempProject::new("script-defect", MANIFEST_WITH_PROPTEST);
    project.install();
    std::fs::create_dir_all(project.path().join("templates")).unwrap();
    std::fs::write(project.path().join("templates/keeler.yml"), "").unwrap();
    std::fs::copy(
        repo_root().join("install.sh"),
        project.path().join("install.sh"),
    )
    .unwrap();
    std::fs::create_dir_all(project.path().join("scripts")).unwrap();
    std::fs::write(
        project.path().join("scripts/bad.sh"),
        "#!/usr/bin/env bash\nls $1\n",
    )
    .unwrap();

    // When the lint gate runs in this repository
    let output = project.run_just("lint");
    let report = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    // Then it fails naming the file and line
    assert!(
        !output.status.success(),
        "the lint gate passed a defective release script:\n{report}",
    );
    assert!(
        report.contains("bad.sh line 2") && report.contains("SC2086"),
        "the report does not name the file and line:\n{report}",
    );
}

#[test]
fn the_repositorys_own_installer_passes_shellcheck() {
    // Given the install.sh this repository ships
    // When shellcheck examines it
    let output = std::process::Command::new("shellcheck")
        .arg(repo_root().join("install.sh"))
        .output()
        .expect("failed to run shellcheck — is it installed?");

    // Then it comes out clean
    assert!(
        output.status.success(),
        "install.sh has shellcheck findings:\n{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
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

/// Findings for every reference to a repo-only path in a workflow, each
/// naming the offending file reference and the line it sits on. Empty means
/// the workflow can run outside the Keeler repository.
fn repo_only_references(workflow: &str) -> Vec<String> {
    let mut findings = Vec::new();
    for (index, line) in workflow.lines().enumerate() {
        for path in REPO_ONLY_PATHS {
            if line.contains(path) {
                findings.push(format!(
                    "line {}: references `{path}` in `{}`",
                    index + 1,
                    line.trim(),
                ));
            }
        }
    }
    findings
}

/// Prose that only makes sense inside the Keeler repository. Deliberately
/// prose and not paths: the shipped `Justfile` legitimately names
/// `templates/keeler.yml` (the marker its shellcheck branch is keyed on) and
/// the upgrade URL ending in `install.sh`. What must never ship is *talk
/// about this repository* — an adopter reading their own files should find
/// their project described, not ours.
const REPO_ONLY_PROSE: [&str; 7] = [
    "keeler repository",
    "this repository itself",
    "documented divergence",
    "tests/installer",
    "tests/release",
    "tests/wild",
    "spec 0",
];

/// Findings for every line of `text` that describes this repository, each
/// naming the line and the marker found. Empty means the file speaks only
/// about the project it landed in.
fn repo_only_prose(text: &str) -> Vec<String> {
    let mut findings = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let lowered = line.to_lowercase();
        for marker in REPO_ONLY_PROSE {
            if lowered.contains(marker) {
                findings.push(format!(
                    "line {}: `{marker}` in `{}`",
                    index + 1,
                    line.trim()
                ));
            }
        }
    }
    findings
}

#[test]
fn a_shipped_file_that_talks_about_us_fails_the_gate() {
    // Given a shipped file carrying a comment about this repository — the
    // real one that shipped in the Justfile until it was caught
    let defective = "lint:\n    # In the Keeler repository itself the deliverable is shell\n";

    // When the quality gate examines it
    let findings = repo_only_prose(defective);

    // Then it fails, naming the marker and the line it sits on
    assert!(
        findings
            .iter()
            .any(|f| f.contains("keeler repository") && f.contains("line 2")),
        "the gate did not name the reference and its line: {findings:?}",
    );
}

#[test]
fn a_command_that_needs_a_script_ships_with_it() {
    // Given the installer's file list
    let installer =
        std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("install.sh"))
            .unwrap();

    // Then a command that shells out to a script does not arrive without
    // it: /keeler:graph runs `just keeler-graph`, which runs
    // scripts/keeler-graph.sh, and an adopter who got the first two and
    // not the third has a slash command that exits 127.
    assert!(
        installer.contains(".claude/commands/keeler/graph.md"),
        "the graph command is not installed"
    );
    assert!(
        installer.contains("scripts/keeler-graph.sh"),
        "the graph command is installed but the script it runs is not"
    );
}

#[test]
fn what_adopters_receive_describes_their_project_not_ours() {
    // Given a freshly installed project
    let project = TempProject::new("no-talk-about-us", MANIFEST_WITH_PROPTEST);
    project.install();

    // When the files Keeler put there are read — everything but the
    // project's own manifest, sources and lockfile
    let mut leaks = Vec::new();
    for (rel, bytes) in project.tree_snapshot() {
        if rel == "Cargo.toml" || rel == "Cargo.lock" || rel.starts_with("src/") {
            continue;
        }
        let Ok(text) = String::from_utf8(bytes) else {
            continue;
        };
        for finding in repo_only_prose(&text) {
            leaks.push(format!("{rel}: {finding}"));
        }
    }

    // Then none of them describes the Keeler repository's own internals
    assert!(
        leaks.is_empty(),
        "installed files describe this repository instead of the adopter's:\n{}",
        leaks.join("\n"),
    );
}

#[test]
fn a_defect_in_the_shipped_workflow_fails_the_gate() {
    // Given a shipped workflow that references a file which exists only in
    // the Keeler repository
    let defective = "jobs:\n  installer:\n    steps:\n      - run: ./install.sh probe\n";

    // When the quality gate examines it
    let findings = repo_only_references(defective);

    // Then it fails, naming the offending file and the reference it found
    assert!(
        findings
            .iter()
            .any(|f| f.contains("./install.sh") && f.contains("line 4")),
        "the gate did not name the repo-only reference and its line: {findings:?}",
    );
}

#[test]
fn an_adopting_project_still_receives_every_gate() {
    // Given a fresh Rust project, when Keeler is installed into it
    let workflow = std::fs::read_to_string(shipped_workflow_path()).unwrap();
    let justfile = std::fs::read_to_string(repo_root().join("Justfile")).unwrap();

    // Then the installed workflow runs lints, tests, coverage + CRAP, and
    // mutation testing
    let jobs = job_names(&workflow);
    for gate in USER_FACING_JOBS {
        assert!(
            jobs.iter().any(|job| job == gate),
            "the shipped workflow lost its `{gate}` gate: {jobs:?}",
        );
    }
    // And the installed Justfile still provides every gate recipe
    for recipe in [
        "cov",
        "crap",
        "crap-baseline",
        "crap-delta",
        "mutants FILE",
        "mutants-diff",
    ] {
        assert!(
            justfile.lines().any(|line| line == format!("{recipe}:")),
            "the shipped Justfile lost its `{recipe}` recipe",
        );
    }
}

/// The recipes graph mode adds, as an adopter meets them: the name `just`
/// lists, and the argument list that follows it in the `Justfile`.
const GRAPH_MODE_RECIPES: [(&str, &str); 5] = [
    ("keeler-graph", "keeler-graph SPEC"),
    ("keeler-spawn", "keeler-spawn SPEC TASK"),
    ("keeler-status", "keeler-status SPEC"),
    ("keeler-branch", "keeler-branch"),
    ("keeler-land", "keeler-land"),
];

/// A spec of the shape they all had before graph mode existed: task items
/// with no dependency annotation this parser reads, one of them carrying
/// the `Deps:` prose older specs wrote by hand.
const OLD_FORMAT_SPEC: &str = "\
# Spec 07 — written before graph mode

**Status:** Approved

## Tasks

- [ ] **T1 — The first thing.** Scenarios: _One_. Tests: acceptance.
- [ ] **T2 — The second thing.** Scenarios: _Two_. Tests: unit.
- [ ] **T3 — The third thing.** Scenarios: _Three_. Tests: property.
      Deps: T1.

---

## Implementation Notes
";

/// The description `just --list` shows for `recipe` — the last comment line
/// above it, which is the only one `just` carries into the listing.
fn listed_description(list: &str, recipe: &str) -> Option<String> {
    list.lines().find_map(|line| {
        let (name, description) = line.trim().split_once('#')?;
        (name.split_whitespace().next()? == recipe).then(|| description.trim().to_string())
    })
}

/// Graph mode's file set, in the project the installer just wrote it to.
fn assert_graph_mode_landed(project: &TempProject, justfile: &str) {
    for file in [
        ".claude/commands/keeler/graph.md",
        // The command shells out to it; without it /keeler:graph exits 127.
        "scripts/keeler-graph.sh",
    ] {
        assert!(
            project.path().join(file).is_file(),
            "the install left the project without {file}",
        );
    }
    for (_, signature) in GRAPH_MODE_RECIPES {
        assert!(
            justfile.lines().any(|line| line == format!("{signature}:")),
            "the installed Justfile has no `{signature}` recipe",
        );
    }
    // The private helper too: `keeler-spawn` and `keeler-land` both call it,
    // and a project that got them without it has two recipes that abort.
    assert!(
        justfile.lines().any(|line| line == "_main-ref:"),
        "the installed Justfile has no `_main-ref` helper",
    );
    let workflow =
        std::fs::read_to_string(project.path().join(".github/workflows/keeler.yml")).unwrap();
    let jobs = job_names(&workflow);
    for job in ["branch-baseline", "review-record"] {
        assert!(
            jobs.iter().any(|name| name == job),
            "the installed workflow has no `{job}` job: {jobs:?}",
        );
    }
    // A spawned run writes .keeler/runs/<slug>/; that is machinery, not the
    // project's source, and the installer's ignore list says so.
    let gitignore = std::fs::read_to_string(project.path().join(".gitignore")).unwrap();
    assert!(
        gitignore.lines().any(|line| line.trim() == ".keeler/"),
        "the installed .gitignore does not ignore .keeler/:\n{gitignore}",
    );
}

/// Landing is not arriving: the recipes must be findable where an adopter
/// looks, and the shipped documentation must say what they are for.
fn assert_graph_mode_is_documented(project: &TempProject) {
    // `just` shows the *last* comment line above a recipe and no other, so
    // a rationale paragraph ending mid-sentence is what the listing carries.
    let listing = project.run_just_args(&["--list"]);
    let listing = String::from_utf8_lossy(&listing.stdout).into_owned();
    for (recipe, _) in GRAPH_MODE_RECIPES {
        let description = listed_description(&listing, recipe)
            .unwrap_or_else(|| panic!("`just --list` does not list {recipe}:\n{listing}"));
        assert!(
            description.starts_with("Graph mode:"),
            "`just --list` describes {recipe} with a fragment of the prose above it: {description:?}",
        );
    }
    // /keeler:graph tells the agent to "see .claude/keeler.md" for graph
    // mode; rules that never mention it send the reader to a section that
    // is not there.
    let rules = std::fs::read_to_string(project.path().join(".claude/keeler.md")).unwrap();
    for (recipe, _) in GRAPH_MODE_RECIPES {
        assert!(
            rules.contains(&format!("just {recipe}")),
            "the installed rules never mention `just {recipe}` — an agent reading them stays on the linear road",
        );
    }
    assert!(
        rules.contains("/keeler:graph"),
        "the installed rules never mention the /keeler:graph command",
    );
    // And the guide the installer points humans at when it finishes.
    let guide = std::fs::read_to_string(project.path().join("KEELER.md")).unwrap();
    assert!(
        guide.to_lowercase().contains("graph mode") && guide.contains("just keeler-spawn"),
        "KEELER.md describes the workflow without the parallel road the install just added",
    );
}

/// A spec with no dependency annotation anywhere: every task is a root, so
/// every task is ready.
fn assert_an_old_spec_reads_as_a_graph(project: &TempProject) {
    std::fs::write(project.path().join("specs/07-legacy.md"), OLD_FORMAT_SPEC).unwrap();
    let read = project.run_just_args(&["keeler-graph", "specs/07-legacy.md"]);
    let report = String::from_utf8_lossy(&read.stdout).into_owned();
    assert!(
        read.status.success(),
        "`just keeler-graph` refused a spec in the old format:\n{report}{}",
        String::from_utf8_lossy(&read.stderr),
    );
    let states: Vec<&str> = report
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    assert_eq!(
        states,
        ["T1 ready", "T2 ready", "T3 ready"],
        "the old format did not read as three ready tasks",
    );
}

/// Behaviour, not bytes: the gate an adopter runs is the recipe it was, and
/// /keeler:feature routes through the same six stages in the same order.
fn assert_the_linear_road_is_unchanged(project: &TempProject, justfile: &str) {
    assert!(
        justfile.contains("\ndev: fmt lint test crap\n"),
        "the installed `dev` recipe is no longer `dev: fmt lint test crap`",
    );
    let feature =
        std::fs::read_to_string(project.path().join(".claude/commands/keeler/feature.md")).unwrap();
    let mut at = 0;
    for stage in [
        "/keeler:spec",
        "/keeler:tasks",
        "/keeler:tdd",
        "/keeler:qa",
        "/keeler:review",
        "/keeler:mutants",
    ] {
        let found = feature[at..]
            .find(stage)
            .unwrap_or_else(|| panic!("/keeler:feature no longer routes through {stage} in order"));
        at += found + stage.len();
    }
    // The routing is the numbered stage list, so that is where a detour
    // would have to appear. Prose elsewhere may say graph mode exists —
    // what may not change is which stages `/keeler:feature` runs.
    for step in feature
        .lines()
        .filter(|line| line.starts_with(|c: char| c.is_ascii_digit()))
    {
        for detour in [
            "keeler-spawn",
            "keeler-branch",
            "keeler-graph",
            "keeler:graph",
        ] {
            assert!(
                !step.contains(detour),
                "/keeler:feature now routes through {detour}: {step:?}",
            );
        }
    }
}

#[test]
fn adopters_opt_in_not_out() {
    // Given a fresh Rust project
    let project = TempProject::new("graph-mode-opt-in", MANIFEST_WITH_PROPTEST);

    // When Keeler is installed into it
    project.install();
    let justfile = std::fs::read_to_string(project.path().join("Justfile")).unwrap();

    // Then the graph command, the spawn, status, branch and land recipes,
    // and the review-evidence check land alongside the existing pipeline
    assert_graph_mode_landed(&project, &justfile);
    assert_graph_mode_is_documented(&project);

    // And a spec written in the old format — no Needs: on any task — is
    // read by `just keeler-graph` with every task reported ready
    assert_an_old_spec_reads_as_a_graph(&project);

    // And `just dev` is the recipe it was, and /keeler:feature routes as it did
    assert_the_linear_road_is_unchanged(&project, &justfile);
}

/// Files from the install set a generated project may already contain, with
/// content of its own.
const PREEXISTING_CANDIDATES: &[&str] = &[
    "CLAUDE.md",
    ".claude/keeler.md",
    "KEELER.md",
    "Justfile",
    "specs/TEMPLATE.md",
    ".gitignore",
    "rustfmt.toml",
];

/// Install-set files as `(destination in the project, source in this repo)`
/// — the pairs the conflict convention applies to.
const INSTALL_SET: &[(&str, &str)] = &[
    ("KEELER.md", "KEELER.md"),
    ("Justfile", "Justfile"),
    ("specs/TEMPLATE.md", "specs/TEMPLATE.md"),
    ("clippy.toml", "clippy.toml"),
    ("rustfmt.toml", "rustfmt.toml"),
    (".cargo-mutants.toml", ".cargo-mutants.toml"),
    (
        ".claude/commands/keeler/spec.md",
        ".claude/commands/keeler/spec.md",
    ),
    (".github/workflows/keeler.yml", "templates/keeler.yml"),
];

/// How a generated project starts out relative to one install-set file.
#[derive(Clone, Copy, Debug)]
enum Preexisting {
    Absent,
    ShippedCopy,
    OwnContent,
}

proptest::proptest! {
    // Every case runs the installer twice as a subprocess — keep the count
    // low, per the spec's constraint. Persistence is source-relative because
    // this harness has no lib.rs for proptest's default to anchor on; found
    // counterexamples land in the file tests/installer.proptest-regressions
    // and get committed.
    #![proptest_config(proptest::prelude::ProptestConfig {
        cases: 12,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::WithSource("proptest-regressions"),
        )),
        ..proptest::prelude::ProptestConfig::default()
    })]

    #[test]
    fn a_projects_own_content_is_never_overwritten(
        preexisting in proptest::sample::subsequence(
            PREEXISTING_CANDIDATES.to_vec(),
            1..=PREEXISTING_CANDIDATES.len(),
        ),
        contents in proptest::collection::vec("[ -~]{0,40}", PREEXISTING_CANDIDATES.len()),
    ) {
        // Given a project containing files Keeler installs, with content of
        // its own (the rules file is the documented exception — T8)
        let project = TempProject::new("never-overwrite", MANIFEST_WITH_PROPTEST);
        let own: Vec<(&str, &String)> = preexisting
            .iter()
            .copied()
            .zip(&contents)
            .filter(|(file, _)| {
                // These are modified by design: the rules file is replaced,
                // CLAUDE.md gains an import, .gitignore gains entries.
                !matches!(*file, ".claude/keeler.md" | "CLAUDE.md" | ".gitignore")
            })
            .collect();
        for (file, content) in &own {
            let path = project.path().join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, content).unwrap();
        }

        // When the installer runs
        let report = project.install();

        for (file, content) in &own {
            // Then each file's content is unchanged ...
            let bytes = std::fs::read(project.path().join(file)).unwrap();
            proptest::prop_assert!(
                bytes == content.as_bytes(),
                "{file} was overwritten: {:?} became {:?}",
                content,
                String::from_utf8_lossy(&bytes),
            );
            // ... the version Keeler would have written is alongside it ...
            proptest::prop_assert!(
                project.path().join(format!("{file}.keeler")).exists(),
                "{file}.keeler was not written for a conflicting {file}",
            );
            // ... and the run names the file in its conflict report
            proptest::prop_assert!(
                report.contains(&format!("{file} differs")),
                "the conflict report does not name {file}:\n{report}",
            );
        }
    }

    #[test]
    fn every_conflict_is_reported_by_name(
        states in proptest::collection::vec(
            proptest::prelude::prop_oneof![
                proptest::prelude::Just(Preexisting::Absent),
                proptest::prelude::Just(Preexisting::ShippedCopy),
                proptest::prelude::Just(Preexisting::OwnContent),
            ],
            INSTALL_SET.len(),
        ),
    ) {
        // Given a project where each install-set file is absent, a copy of
        // what Keeler ships, or content of the project's own
        let project = TempProject::new("conflict-report", MANIFEST_WITH_PROPTEST);
        let mut expected: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
        for ((dest, source), state) in INSTALL_SET.iter().zip(&states) {
            let path = project.path().join(dest);
            match state {
                Preexisting::Absent => {}
                Preexisting::ShippedCopy => {
                    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                    std::fs::copy(repo_root().join(source), path).unwrap();
                }
                Preexisting::OwnContent => {
                    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                    std::fs::write(path, "local change\n").unwrap();
                    expected.insert(dest);
                }
            }
        }

        // When the installer runs
        let report = project.install();

        // Then the set of files it names equals the set of .keeler files
        // written, which equals the actual conflicts ...
        let named: std::collections::BTreeSet<&str> = INSTALL_SET
            .iter()
            .map(|(dest, _)| *dest)
            .filter(|dest| report.contains(&format!("{dest} differs")))
            .collect();
        let keeler_files: std::collections::BTreeSet<&str> = INSTALL_SET
            .iter()
            .map(|(dest, _)| *dest)
            .filter(|dest| project.path().join(format!("{dest}.keeler")).exists())
            .collect();
        proptest::prop_assert_eq!(&named, &expected, "report vs actual conflicts:\n{}", report);
        proptest::prop_assert_eq!(&keeler_files, &expected, ".keeler files vs actual conflicts");
        // ... and no conflict line lacks a filename
        for line in report.lines().filter(|line| line.contains("differs — wrote")) {
            let name = line
                .split(" differs")
                .next()
                .unwrap_or_default()
                .trim()
                .trim_start_matches('·')
                .trim();
            proptest::prop_assert!(!name.is_empty(), "a conflict line names no file: {line:?}");
        }
    }

    #[test]
    fn installing_twice_leaves_the_second_run_with_nothing_to_do(
        preexisting in proptest::sample::subsequence(
            PREEXISTING_CANDIDATES.to_vec(),
            0..=PREEXISTING_CANDIDATES.len(),
        ),
        contents in proptest::collection::vec("[ -~]{0,40}", PREEXISTING_CANDIDATES.len()),
    ) {
        // Given any project state the installer accepts
        let project = TempProject::new("idempotence", MANIFEST_WITH_PROPTEST);
        for (file, content) in preexisting.iter().zip(&contents) {
            let path = project.path().join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, content).unwrap();
        }

        // When the installer runs, and then runs again
        project.install();
        let after_first = project.tree_snapshot();
        let second_run = project.install();

        // Then every file is byte-identical to its state after the first run
        let after_second = project.tree_snapshot();
        proptest::prop_assert!(
            after_first == after_second,
            "a second run changed the tree; differing files: {:?}",
            after_first
                .iter()
                .filter(|(path, bytes)| after_second.get(*path) != Some(bytes))
                .map(|(path, _)| path)
                .chain(after_second.keys().filter(|p| !after_first.contains_key(*p)))
                .collect::<Vec<_>>(),
        );
        // And the second run reports that it installed no files
        proptest::prop_assert!(
            second_run.contains("0 file(s) installed"),
            "the second run claims to have installed files:\n{second_run}",
        );
    }
}

#[test]
fn a_piped_upgrade_never_mistakes_the_project_for_the_source() {
    // Given a project that already contains the files Keeler ships
    let project = TempProject::new("piped-upgrade", MANIFEST_WITH_PROPTEST);
    project.install();

    // When the installer runs piped from stdin inside that project
    // (BASH_SOURCE is unset, exactly like `curl … | bash -s .`)
    let output = std::process::Command::new("bash")
        .args(["-s", ".", "--no-tools"])
        .stdin(std::fs::File::open(repo_root().join("install.sh")).unwrap())
        .current_dir(project.path())
        .env(
            "PATH",
            format!(
                "{}:{}",
                project.path().join("bin").display(),
                std::env::var("PATH").unwrap(),
            ),
        )
        .output()
        .unwrap();

    // Then it fetches the Keeler source — here the stubbed curl refuses —
    // and does not treat the project's own files as the thing to install
    let network =
        std::fs::read_to_string(project.path().join("network-calls.log")).unwrap_or_default();
    assert!(
        network.contains("codeload.github.com"),
        "the piped run never reached for the source:\n{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(
        !output.status.success(),
        "the piped run reported success while silently installing the project into itself",
    );
}

#[test]
fn a_workspace_with_rust_targets_is_still_measured() {
    // Given a workspace whose member crate defines a library target
    let project = TempProject::new(
        "workspace-measured",
        "[workspace]\nmembers = [\"member\"]\nresolver = \"2\"\n",
    );
    std::fs::create_dir_all(project.path().join("member/src")).unwrap();
    std::fs::write(
        project.path().join("member/Cargo.toml"),
        "[package]\nname = \"member\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    std::fs::write(
        project.path().join("member/src/lib.rs"),
        "pub fn one() -> u32 { 1 }\n",
    )
    .unwrap();
    project.install();

    // When the coverage recipe runs
    let output = project.run_just("cov");
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Then it measures the members (the stub records the invocation) ...
    assert!(
        project.cargo_calls().contains("llvm-cov"),
        "the coverage recipe skipped a workspace it can measure:\n{stdout}",
    );
    // ... and does not report the sources as missing
    assert!(
        !stdout.contains("no Rust sources"),
        "a measurable workspace was reported as having nothing to measure:\n{stdout}",
    );
    assert!(output.status.success());

    // And the CRAP gate asks cargo where the sources are instead of
    // assuming a src/ at the root: a workspace root has none, and the
    // hard-coded path failed with "path does not exist" rather than
    // measuring the members.
    let crap = project.run_just("crap");
    let calls = project.cargo_calls();
    assert!(
        calls
            .lines()
            .any(|call| call.starts_with("crap ") && call.contains("--workspace")),
        "the CRAP gate does not ask cargo for the workspace members:\n{calls}",
    );
    assert!(
        !calls.contains("--path src"),
        "the CRAP gate still assumes a src/ at the project root:\n{calls}",
    );
    assert!(crap.status.success());
}

#[test]
fn an_adopters_install_sh_is_not_keelers_to_gate() {
    // Given an adopting project with a script of its own named install.sh
    let project = TempProject::new("adopter-script", MANIFEST_WITH_PROPTEST);
    project.install();
    std::fs::write(
        project.path().join("install.sh"),
        "#!/usr/bin/env bash\nls $1\n",
    )
    .unwrap();

    // When the lint recipe runs there
    let output = project.run_just("lint");

    // Then shellcheck is not applied to it
    assert!(
        output.status.success(),
        "the lint gate judged a script Keeler does not own:\n{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

/// True when the mutation gate actually ran over a diff. Order-independent:
/// what matters is that `mutants` was invoked with `--in-diff`, not which
/// flags happen to sit between them.
fn ran_in_diff(calls: &str) -> bool {
    calls
        .lines()
        .any(|call| call.starts_with("mutants ") && call.contains("--in-diff"))
}

#[test]
fn committed_src_changes_on_a_branch_stay_measured() {
    // Given a branch whose earlier commits changed src/ and whose latest
    // commit did not
    let project = TempProject::new("branch-mutants", MANIFEST_WITH_PROPTEST);
    project.install();
    std::fs::create_dir_all(project.path().join("src")).unwrap();
    std::fs::write(
        project.path().join("src/lib.rs"),
        "pub fn a() -> u32 { 1 }\n",
    )
    .unwrap();
    project.git(&["init", "-qb", "main"]);
    project.git(&["add", "-A"]);
    project.git(&["commit", "-qm", "init"]);
    project.git(&["checkout", "-qb", "feature"]);
    std::fs::write(
        project.path().join("src/lib.rs"),
        "pub fn a() -> u32 { 2 }\n",
    )
    .unwrap();
    project.git(&["commit", "-aqm", "src change"]);
    std::fs::write(project.path().join("NOTES.md"), "docs\n").unwrap();
    project.git(&["add", "NOTES.md"]);
    project.git(&["commit", "-qm", "docs"]);

    // When the mutation gate runs
    let output = project.run_just("mutants-diff");

    // Then it mutates the changed lines against the branch base
    assert!(
        ran_in_diff(&project.cargo_calls()),
        "src changes on the branch were never measured:\n{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(output.status.success());
}

#[test]
fn mutants_diff_survives_untracked_paths_with_spaces() {
    // Given an untracked src file whose name contains a space
    let project = TempProject::new("spaced-untracked", MANIFEST_WITH_PROPTEST);
    project.install();
    std::fs::create_dir_all(project.path().join("src")).unwrap();
    std::fs::write(
        project.path().join("src/lib.rs"),
        "pub fn a() -> u32 { 1 }\n",
    )
    .unwrap();
    project.git(&["init", "-qb", "main"]);
    project.git(&["add", "-A"]);
    project.git(&["commit", "-qm", "init"]);
    std::fs::write(
        project.path().join("src/extra file.rs"),
        "pub fn b() -> u32 { 2 }\n",
    )
    .unwrap();

    // When the mutation gate runs
    let output = project.run_just("mutants-diff");

    // Then the gate neither splits the path nor aborts
    assert!(
        output.status.success(),
        "mutants-diff broke on a spaced path:\n{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(
        ran_in_diff(&project.cargo_calls()),
        "the untracked src file was never measured",
    );
}

#[test]
fn the_property_suite_runs_without_network_access() {
    // Given a generated project state — every one the harness makes carries
    // a curl stub that fails loudly, so no case can quietly reach the network
    let project = TempProject::new("offline", MANIFEST_WITH_PROPTEST);

    // The stub actually intercepts: under the project's PATH, curl refuses
    let intercepted = std::process::Command::new("bash")
        .args(["-c", "curl --version"])
        .env(
            "PATH",
            format!(
                "{}:{}",
                project.path().join("bin").display(),
                std::env::var("PATH").unwrap(),
            ),
        )
        .output()
        .unwrap();
    assert!(
        !intercepted.status.success(),
        "the harness does not stub out curl — a case could reach the network",
    );
    let log = project.path().join("network-calls.log");
    let after_probe = std::fs::read_to_string(&log).unwrap_or_default();

    // When the installer runs, every case completes ...
    project.install();
    // ... with the network never touched
    assert_eq!(
        std::fs::read_to_string(&log).unwrap_or_default(),
        after_probe,
        "the installer tried to reach the network",
    );
}

#[test]
fn the_installer_refuses_a_directory_that_is_not_a_rust_project() {
    // Given a directory with no Cargo.toml
    let project = TempProject::bare("not-rust");
    std::fs::write(project.path().join("notes.txt"), "just files\n").unwrap();
    let before = project.tree_snapshot();

    // When the installer runs against it
    let output = project.try_install();

    // Then it exits with a non-zero status
    assert!(
        !output.status.success(),
        "the installer accepted a directory that is not a Rust project",
    );
    // And it explains that the directory is not a Rust project
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("not a Rust project"),
        "no explanation was given:\n{}",
        String::from_utf8_lossy(&output.stderr),
    );
    // And it creates no files there
    assert_eq!(
        before,
        project.tree_snapshot(),
        "the refused directory was modified",
    );
}

#[test]
fn the_rules_file_is_replaced_and_the_replaced_text_is_kept() {
    // Given a project whose .claude/keeler.md differs from the shipped rules
    let project = TempProject::new("rules-file", MANIFEST_WITH_PROPTEST);
    let rules = project.path().join(".claude/keeler.md");
    std::fs::create_dir_all(rules.parent().unwrap()).unwrap();
    std::fs::write(&rules, "my own edits\n").unwrap();

    // When the installer runs
    project.install();

    // Then .claude/keeler.md matches the shipped rules
    let shipped = std::fs::read(repo_root().join(".claude/keeler.md")).unwrap();
    assert_eq!(
        std::fs::read(&rules).unwrap(),
        shipped,
        "the rules file was not replaced with the shipped rules",
    );
    // And the text it replaced is available as .claude/keeler.md.bak
    assert_eq!(
        std::fs::read_to_string(project.path().join(".claude/keeler.md.bak")).unwrap(),
        "my own edits\n",
        "the replaced text was not kept as .bak",
    );
    // And no .claude/keeler.md.keeler is left behind
    assert!(
        !project.path().join(".claude/keeler.md.keeler").exists(),
        "the rules file wrongly got the conflict-file treatment",
    );
}

/// Pins the counterexample the idempotence property found: a .gitignore
/// whose final line has no newline glued the first appended entry onto it,
/// so the already-present check never matched and every run appended again.
#[test]
fn a_gitignore_missing_its_final_newline_still_converges() {
    let project = TempProject::new("gitignore-newline", MANIFEST_WITH_PROPTEST);
    std::fs::write(project.path().join(".gitignore"), "0").unwrap();

    project.install();
    let after_first = project.tree_snapshot();
    project.install();

    assert_eq!(
        after_first,
        project.tree_snapshot(),
        "the second run kept appending",
    );
    let gitignore = String::from_utf8(after_first[".gitignore"].clone()).unwrap();
    assert!(
        gitignore.lines().any(|line| line == "0"),
        "the project's own final line was damaged:\n{gitignore}",
    );
    assert!(
        gitignore.lines().any(|line| line == "/target"),
        "the entry was not appended cleanly:\n{gitignore}",
    );
}

/// Guards the installer's hard-coded file list behaviorally: every file of
/// every kind under the command and skill trees must actually land in an
/// installed project — a path merely mentioned in a comment of install.sh
/// does not count, and a skill's non-Markdown companion assets count too.
#[test]
fn every_workflow_file_in_the_repository_is_installed() {
    let project = TempProject::new("ship-everything", MANIFEST_WITH_PROPTEST);
    project.install();
    let installed = project.tree_snapshot();

    let mut missing = Vec::new();
    for dir in [".claude/commands/keeler", ".claude/skills"] {
        for path in files_under(&repo_root().join(dir)) {
            let rel = path
                .strip_prefix(repo_root())
                .unwrap()
                .to_string_lossy()
                .into_owned();
            if !installed.contains_key(&rel) {
                missing.push(rel);
            }
        }
    }
    assert!(
        missing.is_empty(),
        "these files exist in the repository but never land in an installed project: {missing:?}",
    );
}

#[test]
fn the_release_workflow_is_not_shipped_to_adopters() {
    // Given a fresh Rust project, when Keeler is installed into it
    let project = TempProject::new("no-release-workflow", MANIFEST_WITH_PROPTEST);
    project.install();

    // Then no release workflow lands in the project
    let installed = project.tree_snapshot();
    let foreign: Vec<&String> = installed
        .keys()
        .filter(|path| {
            path.starts_with(".github/workflows/") && *path != ".github/workflows/keeler.yml"
        })
        .collect();
    assert!(
        foreign.is_empty(),
        "the installer shipped workflows beyond keeler.yml: {foreign:?}",
    );
}

#[test]
fn the_repository_presents_no_placeholder_to_replace() {
    // Given a clone of the Keeler repository, when its Rust sources are
    // listed, then there is no source file whose only purpose is to be
    // measured ...
    assert!(
        !repo_root().join("src").exists(),
        "src/ still holds a placeholder crate for the gates to measure",
    );
    assert!(
        !repo_root().join("tests/acceptance.rs").exists(),
        "tests/acceptance.rs still tests the placeholder",
    );
    // A baseline is no longer evidence of a placeholder: the xtask crate is
    // real code with a real job — the release runs on it — so there is
    // something to baseline and the file is back, deliberately (spec 04).
    // What the scenario forbids is a source file that exists only to be
    // measured, and the check for that is the absence of src/ above.
    // ... and Cargo.toml describes a test harness for the installer
    let manifest = std::fs::read_to_string(repo_root().join("Cargo.toml")).unwrap();
    assert!(
        manifest.contains("test harness") && manifest.contains("install.sh"),
        "Cargo.toml does not describe the crate as the installer's test harness",
    );
}

#[test]
fn a_change_touching_no_rust_is_still_measured() {
    // Given a change that edits only install.sh and the files it ships
    let ci = std::fs::read_to_string(repo_root().join(".github/workflows/ci.yml")).unwrap();

    // When the quality gate runs, the installer test suite runs against it
    // (through `just test`, whose recipe knows this crate has no library)
    assert!(
        ci.contains("run: just test"),
        "the CI test job no longer runs the installer test suite",
    );
    // And no gate reports a pass for code it did not examine — the jobs
    // that measured only the placeholder are gone
    let jobs = job_names(&ci);
    for placeholder_gate in ["quality", "mutants"] {
        assert!(
            !jobs.iter().any(|job| job == placeholder_gate),
            "the `{placeholder_gate}` job still reports green by measuring a placeholder",
        );
    }
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
    let findings = repo_only_references(&workflow);
    assert!(
        findings.is_empty(),
        "{} references files a user's project does not have:\n{}",
        path.display(),
        findings.join("\n"),
    );
    // ... and it runs no command that only some projects can answer
    for fragile in NOT_EVERY_PROJECT {
        assert!(
            !workflow.contains(fragile),
            "{} runs `{fragile}` directly — use the guarded `just` recipe instead",
            path.display(),
        );
    }
}

#[test]
fn a_manifest_that_inherits_its_lints_is_left_able_to_build() {
    // Given the standard modern idiom: a package that takes its lints from
    // the workspace
    let project = TempProject::new(
        "inherited-lints",
        "[package]\nname = \"a\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n\
         [lints]\nworkspace = true\n\n\
         [workspace]\nmembers = []\n\n\
         [workspace.lints.clippy]\npedantic = \"warn\"\n",
    );
    std::fs::create_dir_all(project.path().join("src")).unwrap();
    std::fs::write(project.path().join("src/lib.rs"), "pub fn a() {}\n").unwrap();

    // When Keeler is installed
    project.install();

    // Then cargo can still read the manifest. Appending [lints.clippy]
    // beside an inherited [lints] is not a merge — cargo refuses the file
    // outright, every command in the project fails, and re-running the
    // installer does not repair it because the grep matches the second
    // time and skips.
    let metadata =
        std::process::Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
            .args([
                "metadata",
                "--no-deps",
                "--format-version",
                "1",
                "--manifest-path",
            ])
            .arg(project.path().join("Cargo.toml"))
            .output()
            .expect("failed to run cargo metadata");
    assert!(
        metadata.status.success(),
        "the installer left a manifest cargo cannot read:\n{}",
        String::from_utf8_lossy(&metadata.stderr),
    );
}

#[test]
fn a_symlink_is_the_projects_own_content_not_a_place_to_write() {
    // Given a project where one of the installed names is a symlink
    // pointing outside it, at a file that does not exist yet
    let project = TempProject::new("symlink", MANIFEST_WITH_PROPTEST);
    let outside = project.path().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    let target = outside.join("theirs.md");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&target, project.path().join("KEELER.md")).unwrap();

    // When Keeler is installed
    let report = project.install();

    // Then nothing is written through the link. `-e` is false for a
    // dangling symlink, so a plain existence test writes outside the
    // project — the one boundary the installer promises to keep.
    assert!(
        !target.exists(),
        "the installer wrote outside the project, through a symlink",
    );
    assert!(
        project.path().join("KEELER.md.keeler").is_file(),
        "the symlink was not treated as the project's own content:\n{report}",
    );
}

/// Replaces the stub `cargo` so a chosen subcommand fails.
fn cargo_fails_at(project: &TempProject, subcommand: &str) {
    let stub = project.path().join("bin/cargo");
    let real = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    std::fs::write(
        &stub,
        format!(
            "#!/usr/bin/env bash\n\
             echo \"$@\" >> \"$(dirname \"$0\")/../cargo-calls.log\"\n\
             if [ \"$1\" = metadata ]; then exec \"{real}\" \"$@\"; fi\n\
             if [ \"$1\" = \"{subcommand}\" ]; then echo 'cargo: boom' >&2; exit 101; fi\n"
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

#[test]
fn a_failed_dependency_add_is_not_reported_as_success() {
    // Given a project where adding proptest will fail
    let project = TempProject::new(
        "add-fails",
        "[package]\nname = \"a\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    cargo_fails_at(&project, "add");

    // When Keeler is installed
    let output = project.try_install();

    // Then the run fails and names what went wrong. `&&` swallows the
    // failure under `set -e`, so the script used to print "Keeler
    // installed" and exit 0 over a project whose first `just dev` cannot
    // compile.
    let report = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(
        !output.status.success(),
        "a failed add reported success:\n{report}"
    );
    assert!(
        report.contains("proptest"),
        "the failure does not name it:\n{report}"
    );
}

#[test]
fn a_projects_own_conflict_file_is_not_overwritten() {
    // Given a project that already has both its own KEELER.md and its own
    // KEELER.md.keeler — the ordinary state after one upgrade
    let project = TempProject::new("keeler-collision", MANIFEST_WITH_PROPTEST);
    std::fs::write(project.path().join("KEELER.md"), "theirs\n").unwrap();
    std::fs::write(project.path().join("KEELER.md.keeler"), "from last time\n").unwrap();

    // When Keeler is installed
    project.install();

    // Then last time's copy survives. Overwriting it loses whatever the
    // project had not merged yet, silently and with no conflict reported.
    assert_eq!(
        std::fs::read_to_string(project.path().join("KEELER.md.keeler")).unwrap(),
        "from last time\n",
    );
}

#[test]
fn an_upgrade_does_not_destroy_the_previous_upgrades_backup() {
    // Given a project upgraded once already, whose rules were edited again
    let project = TempProject::new("bak-collision", MANIFEST_WITH_PROPTEST);
    std::fs::create_dir_all(project.path().join(".claude")).unwrap();
    std::fs::write(project.path().join(".claude/keeler.md"), "second edit\n").unwrap();
    std::fs::write(
        project.path().join(".claude/keeler.md.bak"),
        "the original\n",
    )
    .unwrap();

    // When Keeler is installed
    project.install();

    // Then the first backup is still there. The whole point of keeping the
    // replaced text is that it is not lost; a second upgrade overwriting
    // the first one's .bak loses the original for good.
    assert_eq!(
        std::fs::read_to_string(project.path().join(".claude/keeler.md.bak")).unwrap(),
        "the original\n",
    );
}

#[test]
fn a_dependency_declared_elsewhere_is_not_mistaken_for_a_dev_dependency() {
    // Given a package whose workspace declares proptest, but which does
    // not depend on it itself
    let project = TempProject::new(
        "workspace-dep",
        "[package]\nname = \"a\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n\
         [workspace]\nmembers = []\n\n[workspace.dependencies]\nproptest = \"1\"\n",
    );
    project.install();

    // Then proptest is still added to this package: a declaration in
    // another table is not one the package's tests can use, and the grep
    // that matched it left the project unable to compile them.
    assert!(
        project
            .cargo_calls()
            .lines()
            .any(|call| call.starts_with("add ")),
        "proptest was never added:\n{}",
        project.cargo_calls(),
    );
}

#[test]
fn proptest_inherited_from_the_workspace_is_already_declared() {
    // Given the idiomatic workspace-member form
    let project = TempProject::new(
        "inherited-proptest",
        "[package]\nname = \"a\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n\
         [dev-dependencies]\nproptest.workspace = true\n",
    );
    project.install();

    // Then nothing is added — and nothing reaches the registry to find
    // that out
    assert!(
        !project
            .cargo_calls()
            .lines()
            .any(|call| call.starts_with("add ")),
        "proptest was added over an inherited declaration:\n{}",
        project.cargo_calls(),
    );
}

#[test]
fn help_works_the_way_the_documentation_says_to_run_it() {
    // Given the documented entry point: the script piped into bash, where
    // it has no file of its own to read
    let output = std::process::Command::new("bash")
        .arg("-c")
        .arg(format!(
            "cat {} | bash -s -- --help",
            repo_root().join("install.sh").display()
        ))
        .output()
        .expect("failed to run install.sh");

    // Then it prints its usage. Reading the script back from
    // BASH_SOURCE[0] aborts under `set -u` when there is no file.
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(output.status.success(), "--help failed when piped:\n{said}");
    assert!(said.contains("--no-tools"), "the usage is missing:\n{said}");
}

#[test]
fn a_mistyped_flag_is_refused_rather_than_taken_for_a_path() {
    // Given a typo in a flag
    let project = TempProject::new("mistyped", MANIFEST_WITH_PROPTEST);
    let output = std::process::Command::new("bash")
        .arg(repo_root().join("install.sh"))
        .arg(project.path())
        .arg("--no-tool")
        .output()
        .expect("failed to run install.sh");

    // Then it is refused as an option. Taken as a path it silently
    // replaces the destination the user actually gave.
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(
        !output.status.success(),
        "a mistyped flag was accepted:\n{said}"
    );
    assert!(said.contains("--no-tool"), "{said}");
    assert!(
        !project.path().join("KEELER.md").exists(),
        "it installed anyway"
    );
}

#[test]
fn a_pinned_version_is_fetched_even_from_inside_a_clone() {
    // Given a piped run whose working directory happens to be a Keeler
    // clone, and an explicit pin
    let project = TempProject::new("pin-honoured", MANIFEST_WITH_PROPTEST);
    let output = std::process::Command::new("bash")
        .arg("-c")
        .arg(format!(
            "cd {} && cat install.sh | bash -s -- {} --no-tools",
            repo_root().display(),
            project.path().display(),
        ))
        .env("KEELER_REF", "v0.0.0-does-not-exist")
        .env(
            "PATH",
            format!(
                "{}:{}",
                project.path().join("bin").display(),
                std::env::var("PATH").unwrap(),
            ),
        )
        .output()
        .expect("failed to run install.sh");

    // Then the pin is honoured rather than silently ignored. Piped, the
    // script has no file of its own, so SRC becomes the working directory
    // — and a Keeler clone looks exactly like an unpacked tarball. The run
    // then installs whatever is checked out, while the caller believes
    // they pinned a version.
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(
        !output.status.success(),
        "an unfetchable pin was silently ignored and the checkout used instead:\n{said}",
    );
}
