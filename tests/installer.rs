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
        self.try_install_args(&["--no-tools"])
    }

    /// `try_install` for the flags a scenario names itself — `--no-ci`, or
    /// no `--no-tools` at all when the tool path is what is under test.
    fn try_install_args(&self, args: &[&str]) -> std::process::Output {
        let path_var = std::env::var("PATH").unwrap();
        std::process::Command::new("bash")
            .arg(repo_root().join("install.sh"))
            .arg(&self.dir)
            .args(args)
            .env(
                "PATH",
                format!("{}:{path_var}", self.dir.join("bin").display()),
            )
            .output()
            .expect("failed to run install.sh")
    }

    /// Runs the installer and panics if it fails.
    fn install(&self) -> String {
        self.install_args(&["--no-tools"])
    }

    /// `install` for the flags a scenario names itself.
    fn install_args(&self, args: &[&str]) -> String {
        let output = self.try_install_args(args);
        assert!(
            output.status.success(),
            "install.sh failed:\n{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    /// Puts Keeler's `Justfile` where `run_just` can reach it. The installer
    /// no longer copies it — the recipes are Keeler's and stay Keeler's —
    /// but the recipe tests below are about the recipes, not about how they
    /// arrive, so the harness stands in for the delivery mechanism.
    fn with_keeler_recipes(&self) {
        std::fs::copy(repo_root().join("Justfile"), self.dir.join("Justfile")).unwrap();
    }

    /// The graph parser `keeler-graph` shells out to, beside the recipes.
    fn with_graph_parser(&self) {
        std::fs::create_dir_all(self.dir.join("scripts")).unwrap();
        std::fs::copy(
            repo_root().join("scripts/keeler-graph.sh"),
            self.dir.join("scripts/keeler-graph.sh"),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                self.dir.join("scripts/keeler-graph.sh"),
                std::fs::Permissions::from_mode(0o755),
            )
            .unwrap();
        }
    }

    /// Every file in the project, without reading any of them — the shape a
    /// scenario means when it says "the files created".
    fn names(&self) -> std::collections::BTreeSet<String> {
        self.tree_snapshot().into_keys().collect()
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
    project.with_keeler_recipes();

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
    project.with_keeler_recipes();

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
    project.with_keeler_recipes();
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
    project.with_keeler_recipes();
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
    project.with_keeler_recipes();
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
fn no_shipped_file_carries_an_unresolved_merge() {
    // A conflict marker is invisible to every gate this project has: the
    // suite reads these files for content, not for shape, and markdown
    // renders `<<<<<<< HEAD` as a line of text. So one rode into main and
    // out to adopters inside .claude/keeler.md — the rules file an agent
    // is told to read first — and it was a spawned agent in a demo
    // project that noticed, not us.
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut wounded = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if name == "target" || name == ".git" || name.starts_with("keeler-") {
                continue;
            }
            if path.is_dir() {
                stack.push(path);
            } else if matches!(
                path.extension().and_then(|e| e.to_str()),
                Some("md" | "rs" | "sh" | "toml" | "yml")
            ) || name == "Justfile"
            {
                let Ok(text) = std::fs::read_to_string(&path) else {
                    continue;
                };
                // At the start of a line and followed by a space or the
                // end of it: `<<<<<<<` inside prose about conflicts, or a
                // shell heredoc, does not look like this.
                for (n, line) in text.lines().enumerate() {
                    let marked = ["<<<<<<< ", ">>>>>>> "].iter().any(|m| line.starts_with(m))
                        || line == "=======" && text.contains("<<<<<<< ");
                    if marked {
                        wounded.push(format!(
                            "{}:{}: {}",
                            path.strip_prefix(&root).unwrap_or(&path).display(),
                            n + 1,
                            line.trim()
                        ));
                    }
                }
            }
        }
    }
    assert!(
        wounded.is_empty(),
        "these files carry an unresolved merge:\n{}",
        wounded.join("\n")
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
    // And the recipes, which an adopter reads too — they reach them through
    // the plugin rather than through the install, so scanning the installed
    // tree alone would leave the file this gate was written for ungated.
    let justfile = std::fs::read_to_string(repo_root().join("Justfile")).unwrap();
    for finding in repo_only_prose(&justfile) {
        leaks.push(format!("Justfile: {finding}"));
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
        "mutants-diff BASE=\"HEAD\"",
    ] {
        assert!(
            justfile.lines().any(|line| line == format!("{recipe}:")),
            "the shipped Justfile lost its `{recipe}` recipe",
        );
    }
}

/// The recipes graph mode adds, as an adopter meets them: the name `just`
/// lists, and the argument list that follows it in the `Justfile`.
const GRAPH_MODE_RECIPES: [(&str, &str); 6] = [
    ("keeler-graph", "keeler-graph SPEC"),
    ("keeler-spawn", "keeler-spawn SPEC TASK"),
    ("keeler-status", "keeler-status SPEC"),
    ("keeler-resume", "keeler-resume SPEC TASK"),
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

/// Graph mode's file set. The command and the parser are Keeler's and stay
/// Keeler's — the install no longer copies them — so they are checked where
/// they live; the workflow and the ignore entry are the adopter's, and
/// those the install still writes.
fn assert_graph_mode_landed(project: &TempProject, justfile: &str) {
    for file in [
        ".claude/commands/keeler/graph.md",
        // The command shells out to it; without it /keeler:graph exits 127.
        "scripts/keeler-graph.sh",
    ] {
        assert!(
            repo_root().join(file).is_file(),
            "Keeler no longer carries {file}",
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
    // /keeler:graph tells the agent to see the rules for graph mode; rules
    // that never mention it send the reader to a section that is not there.
    let rules = std::fs::read_to_string(repo_root().join(".claude/keeler.md")).unwrap();
    for (recipe, _) in GRAPH_MODE_RECIPES {
        assert!(
            rules.contains(&format!("just {recipe}")),
            "the rules never mention `just {recipe}` — an agent reading them stays on the linear road",
        );
    }
    assert!(
        rules.contains("/keeler:graph"),
        "the rules never mention the /keeler:graph command",
    );
    // And the guide the reasoning lives in.
    let guide = std::fs::read_to_string(repo_root().join("KEELER.md")).unwrap();
    assert!(
        guide.to_lowercase().contains("graph mode") && guide.contains("just keeler-spawn"),
        "KEELER.md describes the workflow without the parallel road",
    );
}

/// A spec with no dependency annotation anywhere: every task is a root, so
/// every task is ready.
fn assert_an_old_spec_reads_as_a_graph(project: &TempProject) {
    std::fs::create_dir_all(project.path().join("specs")).unwrap();
    std::fs::write(project.path().join("specs/07-legacy.md"), OLD_FORMAT_SPEC).unwrap();
    // The recipe answers from the spec as committed; with no feature
    // branch here, that is the fallback to HEAD.
    project.git(&["init", "-qb", "main"]);
    project.git(&["add", "specs/07-legacy.md"]);
    project.git(&["commit", "-qm", "a spec from before graph mode"]);
    let read = project.run_just_args(&["keeler-graph", "specs/07-legacy.md"]);
    let report = String::from_utf8_lossy(&read.stdout).into_owned();
    assert!(
        read.status.success(),
        "`just keeler-graph` refused a spec in the old format:\n{report}{}",
        String::from_utf8_lossy(&read.stderr),
    );
    assert!(
        report.contains("graph: specs/07-legacy.md on HEAD"),
        "the recipe does not say which ref it answered from:\n{report}",
    );
    let states: Vec<&str> = report
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.starts_with("graph: "))
        .collect();
    assert_eq!(
        states,
        ["T1 ready", "T2 ready", "T3 ready"],
        "the old format did not read as three ready tasks",
    );
}

/// Behaviour, not bytes: the gate an adopter runs is the recipe it was, and
/// /keeler:feature routes through the same six stages in the same order.
fn assert_the_linear_road_is_unchanged(justfile: &str) {
    assert!(
        justfile.contains("\ndev: fmt lint test crap\n"),
        "the `dev` recipe is no longer `dev: fmt lint test crap`",
    );
    let feature =
        std::fs::read_to_string(repo_root().join(".claude/commands/keeler/feature.md")).unwrap();
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
    // what may not change is which stages `/keeler:feature` runs. The
    // whole list, not only the lines that open a step: a sub-bullet or a
    // wrapped continuation reroutes it just as well, and neither begins
    // with a digit.
    let lines: Vec<&str> = feature.lines().collect();
    let numbered = |line: &&str| line.starts_with(|c: char| c.is_ascii_digit());
    let first = lines
        .iter()
        .position(numbered)
        .expect("/keeler:feature has no numbered stage list at all");
    let last = lines.iter().rposition(numbered).unwrap();
    // The last step ends where its paragraph does, so its continuations
    // are read and the prose after the list is not.
    let end = lines[last..]
        .iter()
        .position(|line| line.trim().is_empty())
        .map_or(lines.len(), |offset| last + offset);
    for step in &lines[first..end] {
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
    project.with_keeler_recipes();
    project.with_graph_parser();
    let justfile = std::fs::read_to_string(repo_root().join("Justfile")).unwrap();

    // Then the graph command, the spawn, status, branch and land recipes,
    // and the review-evidence check land alongside the existing pipeline
    assert_graph_mode_landed(&project, &justfile);
    assert_graph_mode_is_documented(&project);

    // And a spec written in the old format — no Needs: on any task — is
    // read by `just keeler-graph` with every task reported ready
    assert_an_old_spec_reads_as_a_graph(&project);

    // And `just dev` is the recipe it was, and /keeler:feature routes as it did
    assert_the_linear_road_is_unchanged(&justfile);
}

/// Files a generated project may already contain that the installer has an
/// opinion about — the install set, the two files it edits, and two of the
/// names an earlier Keeler left behind, which it must now leave alone.
const PREEXISTING_CANDIDATES: &[&str] = &[
    "CLAUDE.md",
    ".gitignore",
    "clippy.toml",
    "rustfmt.toml",
    ".github/workflows/keeler.yml",
    "Justfile",
    "KEELER.md",
];

/// Install-set files as `(destination in the project, source in this repo)`
/// — the pairs the conflict convention applies to. Three, now that the
/// workflow files are the plugin's: two tool configs that configure the
/// project's own toolchain, and the CI workflow, which GitHub reads from
/// the repository and nowhere else.
const INSTALL_SET: &[(&str, &str)] = &[
    ("clippy.toml", "clippy.toml"),
    ("rustfmt.toml", "rustfmt.toml"),
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
            INSTALL_SET.iter().map(|(dest, _)| *dest).collect::<Vec<_>>(),
            1..=INSTALL_SET.len(),
        ),
        contents in proptest::collection::vec("[ -~]{0,40}", INSTALL_SET.len()),
    ) {
        // Given a project containing files Keeler installs, with content of
        // its own
        let project = TempProject::new("never-overwrite", MANIFEST_WITH_PROPTEST);
        let own: Vec<(&str, &String)> = preexisting.iter().copied().zip(&contents).collect();
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
    project.with_keeler_recipes();

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
    project.with_keeler_recipes();
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
    project.with_keeler_recipes();
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
    project.with_keeler_recipes();
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
    let target = outside.join("theirs.toml");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&target, project.path().join("clippy.toml")).unwrap();

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
        project.path().join("clippy.toml.keeler").is_file(),
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
    // Given a project that already has both its own clippy.toml and its own
    // clippy.toml.keeler — the ordinary state after one upgrade
    let project = TempProject::new("keeler-collision", MANIFEST_WITH_PROPTEST);
    std::fs::write(project.path().join("clippy.toml"), "theirs\n").unwrap();
    std::fs::write(
        project.path().join("clippy.toml.keeler"),
        "from last time\n",
    )
    .unwrap();

    // When Keeler is installed
    project.install();

    // Then last time's copy survives. Overwriting it loses whatever the
    // project had not merged yet, silently and with no conflict reported.
    assert_eq!(
        std::fs::read_to_string(project.path().join("clippy.toml.keeler")).unwrap(),
        "from last time\n",
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
        !project.path().join("clippy.toml").exists(),
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

// --- Spec 09 — the installer stops copying Keeler's files ------------------
//
// What lands in an adopter's repository is what GitHub and the project's own
// toolchain read from there and nowhere else: the CI workflow and the two
// tool configs. The commands, the skills, the rules, the Justfile and the
// graph parser stay Keeler's, reached through the plugin.

/// The three files a fresh install writes, and the two it edits.
const CREATED_IN_A_FRESH_CRATE: [&str; 3] = [
    ".github/workflows/keeler.yml",
    "clippy.toml",
    "rustfmt.toml",
];

/// Names that were part of the install set before spec 09 and must not
/// reappear. `.claude/` and `specs/` are directories, so they are probed as
/// prefixes; the rest are files.
const NO_LONGER_INSTALLED: [&str; 6] = [
    ".claude/",
    "specs/",
    "scripts/",
    "Justfile",
    "KEELER.md",
    ".cargo-mutants.toml",
];

/// The files a run created and the files it changed, as a scenario words it.
fn created_and_modified(
    before: &std::collections::BTreeMap<String, Vec<u8>>,
    after: &std::collections::BTreeMap<String, Vec<u8>>,
) -> (Vec<String>, Vec<String>) {
    let created = after
        .keys()
        .filter(|name| !before.contains_key(*name))
        .cloned()
        .collect();
    let modified = after
        .iter()
        .filter(|(name, bytes)| before.get(*name).is_some_and(|was| was != *bytes))
        .map(|(name, _)| name.clone())
        .collect();
    (created, modified)
}

/// Nothing of the retired install set is anywhere in the project.
fn assert_nothing_of_keelers_landed(project: &TempProject) {
    let present: Vec<String> = project
        .names()
        .into_iter()
        .filter(|name| {
            NO_LONGER_INSTALLED
                .iter()
                .any(|retired| name == retired || name.starts_with(retired))
        })
        .collect();
    assert!(
        present.is_empty(),
        "the install put Keeler's own files in the project: {present:?}",
    );
}

#[test]
fn init_leaves_the_workflow_and_two_tool_configs_in_a_fresh_crate() {
    // Given a fresh crate with a .gitignore
    let project = TempProject::new("fresh-crate", MANIFEST_WITH_PROPTEST);
    std::fs::write(project.path().join(".gitignore"), "/target\n").unwrap();
    let before = project.tree_snapshot();

    // When the installer runs
    project.install();

    // Then the files created are the workflow and the two tool configs ...
    let (created, modified) = created_and_modified(&before, &project.tree_snapshot());
    assert_eq!(
        created, CREATED_IN_A_FRESH_CRATE,
        "a fresh install created something other than the workflow and the tool configs",
    );
    // ... and the only files modified are Cargo.toml and .gitignore
    assert_eq!(
        modified,
        [".gitignore", "Cargo.toml"],
        "the install edited a file that is not its to edit",
    );
    // And none of the files Keeler used to copy is there
    assert_nothing_of_keelers_landed(&project);
}

#[test]
fn a_crate_without_a_gitignore_gets_one() {
    // Given a fresh crate with no .gitignore
    let project = TempProject::new("no-gitignore", MANIFEST_WITH_PROPTEST);
    let before = project.tree_snapshot();

    // When the installer runs
    project.install();

    // Then the .gitignore is created alongside the workflow and the configs
    let (created, _) = created_and_modified(&before, &project.tree_snapshot());
    assert_eq!(
        created,
        [
            ".github/workflows/keeler.yml",
            ".gitignore",
            "clippy.toml",
            "rustfmt.toml",
        ],
    );
}

#[test]
fn init_without_ci_leaves_no_workflow() {
    // Given a fresh crate with a .gitignore
    let project = TempProject::new("no-ci", MANIFEST_WITH_PROPTEST);
    std::fs::write(project.path().join(".gitignore"), "/target\n").unwrap();
    let before = project.tree_snapshot();

    // When the installer runs with --no-ci
    project.install_args(&["--no-tools", "--no-ci"]);

    // Then only the two tool configs are created ...
    let (created, modified) = created_and_modified(&before, &project.tree_snapshot());
    assert_eq!(created, ["clippy.toml", "rustfmt.toml"]);
    // ... no .github/ exists at all ...
    assert!(
        !project.path().join(".github").exists(),
        "--no-ci still left a .github directory behind",
    );
    // ... and the only files modified are Cargo.toml and .gitignore
    assert_eq!(modified, [".gitignore", "Cargo.toml"]);
}

#[test]
fn the_tool_configs_still_land() {
    // Given a fresh crate
    let project = TempProject::new("tool-configs", MANIFEST_WITH_PROPTEST);

    // When the installer runs
    project.install();

    // Then both configs are there, byte-identical to the ones Keeler ships
    for config in ["clippy.toml", "rustfmt.toml"] {
        assert_eq!(
            std::fs::read(project.path().join(config)).unwrap(),
            std::fs::read(repo_root().join(config)).unwrap(),
            "{config} did not land with Keeler's content",
        );
    }
}

#[test]
fn tools_are_skipped_on_request() {
    // Given a fresh crate and a PATH where no cargo tool answers
    let project = TempProject::new("skip-tools", MANIFEST_WITH_PROPTEST);
    cargo_fails_at(&project, "nextest");

    // When the installer runs with --no-tools
    let output = project.try_install_args(&["--no-tools"]);

    // Then it exits zero ...
    assert!(
        output.status.success(),
        "--no-tools failed on a machine without the tools:\n{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    // ... and nothing was installed to make that true
    let calls = project.cargo_calls();
    for reach in ["binstall", "install "] {
        assert!(
            !calls.lines().any(|call| call.starts_with(reach)),
            "--no-tools still reached for the toolchain: {calls}",
        );
    }
}

#[test]
fn the_run_directory_is_ignored() {
    // Given a fresh crate with an empty .gitignore
    let project = TempProject::new("ignore-entries", MANIFEST_WITH_PROPTEST);
    std::fs::write(project.path().join(".gitignore"), "").unwrap();

    // When the installer runs
    project.install();

    // Then every entry the workflow's artifacts need is there
    let gitignore = std::fs::read_to_string(project.path().join(".gitignore")).unwrap();
    for entry in [
        "/target",
        "lcov.info",
        "crap-report.json",
        "mutants.out*/",
        ".keeler/",
    ] {
        assert!(
            gitignore.lines().any(|line| line == entry),
            "the install did not ignore {entry}:\n{gitignore}",
        );
    }
}

#[test]
fn the_adopters_claude_md_is_not_touched() {
    // Given a crate whose CLAUDE.md is its own
    let project = TempProject::new("claude-md-untouched", MANIFEST_WITH_PROPTEST);
    std::fs::write(project.path().join("CLAUDE.md"), "# Mine\n").unwrap();

    // When the installer runs
    project.install();

    // Then not a byte of it moved. The rules reach the agent through the
    // plugin's SessionStart hook, so there is nothing to import and no
    // reason to write into a file that is not Keeler's.
    assert_eq!(
        std::fs::read_to_string(project.path().join("CLAUDE.md")).unwrap(),
        "# Mine\n",
    );
}

#[test]
fn a_crate_without_a_claude_md_does_not_get_one() {
    // Given a crate with no CLAUDE.md
    let project = TempProject::new("no-claude-md", MANIFEST_WITH_PROPTEST);

    // When the installer runs
    project.install();

    // Then none appears
    assert!(
        !project.path().join("CLAUDE.md").exists(),
        "the installer created a CLAUDE.md nobody asked for",
    );
}

// --- Spec 09 — what an earlier install left behind -------------------------
//
// A project installed before spec 09 holds Keeler's commands, skills, rules,
// recipes and graph parser. They are dead weight now — the plugin carries
// them — but removing them is not the installer's call: an edited copy is
// indistinguishable from an untouched one, and a deleted edit is the one
// loss this script has always promised not to cause. So they are named, with
// the command that removes them, and left exactly as they are.

/// The eight traces an earlier install leaves, as `(what a past install
/// wrote, the path the report names it by)`. The two directories are named
/// whole: every file under them was Keeler's, and `git rm -r --` is how they
/// go. `CLAUDE.md` is the odd one — the file is the project's, only the
/// import line is ours, so what the report names is the line.
const STALE_MARKERS: [(&str, &str); 8] = [
    (".claude/commands/keeler/spec.md", ".claude/commands/keeler"),
    (
        ".claude/skills/gherkin-specs/SKILL.md",
        ".claude/skills/gherkin-specs",
    ),
    (".claude/keeler.md", ".claude/keeler.md"),
    ("scripts/keeler-graph.sh", "scripts/keeler-graph.sh"),
    ("KEELER.md", "KEELER.md"),
    (".cargo-mutants.toml", ".cargo-mutants.toml"),
    ("Justfile", "Justfile"),
    ("CLAUDE.md", "@.claude/keeler.md"),
];

/// Files an earlier install would have left with this content. The two that
/// are detected by what is inside them rather than by their name get the
/// content that identifies them; the rest may hold anything.
fn stale_content(path: &str) -> &'static str {
    match path {
        "Justfile" => "default:\n    @just --list\n\nkeeler-spawn SPEC TASK:\n    @echo spawning\n",
        "CLAUDE.md" => "# Theirs\n\n@.claude/keeler.md\n",
        _ => "left by an earlier Keeler\n",
    }
}

/// Writes the fixture for each named marker into the project.
fn leave_behind(project: &TempProject, markers: &[(&str, &str)]) {
    for (file, _) in markers {
        let path = project.path().join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, stale_content(file)).unwrap();
    }
}

/// The paths the report lists as an earlier Keeler's — the bullets under its
/// heading, which run until the blank line before the removal command.
fn stale_bullets(report: &str) -> Vec<String> {
    report
        .lines()
        .skip_while(|line| !line.contains("Left by an earlier Keeler"))
        .skip(1)
        .map_while(|line| line.trim().strip_prefix("· ").map(str::to_string))
        .collect()
}

/// The one `git rm -r --` line the report offers, split into the paths it
/// names. `None` when the report gives no such line at all.
fn removal_line(report: &str) -> Option<Vec<String>> {
    let lines: Vec<&str> = report
        .lines()
        .filter(|line| line.contains("git rm -r --"))
        .collect();
    assert!(
        lines.len() <= 1,
        "the report gives more than one removal line: {lines:?}",
    );
    lines.first().map(|line| {
        line.split_once("git rm -r --")
            .unwrap()
            .1
            .split_whitespace()
            .map(str::to_string)
            .collect()
    })
}

#[test]
fn files_an_earlier_install_left_behind_are_named_and_kept() {
    // Given a crate holding everything a pre-plugin install put there
    let project = TempProject::new("stale-report", MANIFEST_WITH_PROPTEST);
    leave_behind(&project, &STALE_MARKERS);
    let before = project.tree_snapshot();

    // When the installer runs
    let output = project.try_install();
    let report = String::from_utf8_lossy(&output.stdout).into_owned();

    // Then it exits zero
    assert!(
        output.status.success(),
        "the installer refused a project an earlier Keeler had touched:\n{report}{}",
        String::from_utf8_lossy(&output.stderr),
    );
    // And it names each of those paths as left by an earlier Keeler
    let bullets = stale_bullets(&report);
    for (_, named) in STALE_MARKERS {
        if named.starts_with('@') {
            continue;
        }
        assert!(
            bullets.iter().any(|listed| listed == named),
            "the report does not name {named}:\n{report}",
        );
    }
    // And it gives one `git rm -r --` line listing exactly those files
    let removal = removal_line(&report).unwrap_or_else(|| panic!("no removal line:\n{report}"));
    assert_eq!(removal, bullets, "the removal line and the list disagree");
    // And it names the import line in CLAUDE.md as one to delete by hand —
    // the file is the project's, so only the line is ours to point at
    assert!(
        report.contains("@.claude/keeler.md") && report.contains("by hand"),
        "the report does not name the CLAUDE.md import as one to delete by hand:\n{report}",
    );
    assert!(
        !removal.iter().any(|path| path == "CLAUDE.md"),
        "the removal line offers to delete the project's own CLAUDE.md: {removal:?}",
    );
    // And it says where those relative paths are rooted, next to the line
    // itself: the destination is an argument, so the reader is not
    // necessarily standing in it.
    let lines: Vec<&str> = report.lines().collect();
    let at = lines
        .iter()
        .position(|line| line.contains("git rm -r --"))
        .unwrap();
    let dest = project.path().display().to_string();
    assert!(
        lines[..at].iter().rev().take(3).any(|l| l.contains(&dest)),
        "nothing near the removal line says where its paths are rooted:\n{report}",
    );
    // And every one of those files is byte-identical afterwards
    let after = project.tree_snapshot();
    for (file, _) in STALE_MARKERS {
        assert_eq!(
            before.get(file),
            after.get(file),
            "{file} was not left as it was found",
        );
    }
}

#[test]
fn the_traces_the_scenarios_do_not_name_are_found_too() {
    // Given a crate holding the two an earlier install could leave that the
    // scenarios' fixture does not: the second skill, and a justfile under
    // the dotted spelling `just` accepts alongside the plain one
    let project = TempProject::new("stale-remainder", MANIFEST_WITH_PROPTEST);
    let skill = project.path().join(".claude/skills/property-testing");
    std::fs::create_dir_all(&skill).unwrap();
    std::fs::write(skill.join("SKILL.md"), "left behind\n").unwrap();
    std::fs::write(project.path().join(".justfile"), stale_content("Justfile")).unwrap();

    // When the installer runs
    let report = project.install();

    // Then both are named. Neither is reachable through STALE_MARKERS, so
    // without this test the two lines that find them could go unnoticed.
    assert_eq!(
        stale_bullets(&report),
        [".claude/skills/property-testing", ".justfile"],
        "a trace of an earlier install went unreported:\n{report}",
    );
}

#[test]
fn a_projects_own_justfile_is_not_mistaken_for_keelers() {
    // Given a crate whose justfile defines only its own recipes
    // (The fixture's directory name says nothing about recipes: the report
    // prints the destination path, and a name containing "justfile" would
    // satisfy the check below without the installer saying a word.)
    let project = TempProject::new("own-recipes", MANIFEST_WITH_PROPTEST);
    std::fs::write(
        project.path().join("justfile"),
        "build:\n    cargo build\n\nspawn-workers:\n    ./workers.sh\n",
    )
    .unwrap();

    // When the installer runs
    let report = project.install();

    // Then the output does not name it. Sharing a name with Keeler's recipes
    // is what every project with its own justfile does; telling them to
    // `git rm` it would be telling them to delete their own work.
    assert!(
        !report.to_lowercase().contains("justfile"),
        "the installer named a justfile that was never Keeler's:\n{report}",
    );
    assert!(
        removal_line(&report).is_none(),
        "the installer offered to remove files from a project it left nothing in:\n{report}",
    );
}

proptest::proptest! {
    // Each case runs the installer as a subprocess; the count stays low for
    // the same reason the properties above keep theirs low.
    #![proptest_config(proptest::prelude::ProptestConfig {
        cases: 10,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::WithSource("proptest-regressions"),
        )),
        ..proptest::prelude::ProptestConfig::default()
    })]

    #[test]
    fn any_subset_of_stale_files_is_reported_exactly(
        left in proptest::sample::subsequence(STALE_MARKERS.to_vec(), 0..=STALE_MARKERS.len()),
    ) {
        // Given any subset of the traces an earlier install leaves
        let project = TempProject::new("stale-subset", MANIFEST_WITH_PROPTEST);
        leave_behind(&project, &left);
        let before = project.tree_snapshot();

        // When the installer runs
        let report = project.install();

        // Then the paths it names are exactly the subset's, and no other
        let expected: Vec<String> = STALE_MARKERS
            .iter()
            .filter(|(file, named)| !named.starts_with('@') && left.iter().any(|(l, _)| l == file))
            .map(|(_, named)| (*named).to_string())
            .collect();
        proptest::prop_assert_eq!(
            stale_bullets(&report), expected.clone(),
            "report vs what was left:\n{}", report,
        );
        proptest::prop_assert_eq!(
            removal_line(&report), (!expected.is_empty()).then_some(expected),
            "removal line vs what was left:\n{}", report,
        );
        // And the CLAUDE.md import is named when, and only when, it is there
        proptest::prop_assert_eq!(
            report.contains("@.claude/keeler.md"),
            left.iter().any(|(file, _)| *file == "CLAUDE.md"),
            "the CLAUDE.md import was misreported:\n{}", report,
        );

        // And nothing outside the install set differs afterwards
        let after = project.tree_snapshot();
        let touched: Vec<&String> = after
            .iter()
            .filter(|(name, bytes)| before.get(*name).is_none_or(|was| was != *bytes))
            .map(|(name, _)| name)
            .filter(|name| ![
                "Cargo.toml",
                ".gitignore",
                "clippy.toml",
                "rustfmt.toml",
                ".github/workflows/keeler.yml",
            ].contains(&name.as_str()))
            .collect();
        proptest::prop_assert!(touched.is_empty(), "the install touched {:?}", touched);
    }
}
