//! Spec 03 — the installer against the wild.
//!
//! These tests drive `scripts/integration-check.sh`, the contract checker,
//! against generated project shapes. CI points the same script at pinned
//! clones of real repositories; here every fixture is local and the tool
//! stubs refuse the network, so the suite stays offline (spec 01).
//!
//! Every invariant is pinned twice: once by a project the checker must
//! accept, and once by a deliberately defective installer it must reject.
//! A check that cannot fail is not a check.

use std::path::{Path, PathBuf};
use std::process::Output;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Content a lived-in project already has. Some of these are files Keeler
/// installs — the project's own copies, which it must never lose — and the
/// rest is content Keeler has no business touching at all. The `.gitignore`
/// deliberately ends without a newline: a real one often does, and appending
/// to it is one of the three edits the installer is allowed to make.
const LIVED_IN_FILES: &[(&str, &str)] = &[
    (".github/workflows/ci.yml", "name: their CI\non: [push]\n"),
    (".gitignore", "/target\ntheir-artifacts/"),
    (
        "README.md",
        "# Their project\n\nNothing to do with Keeler.\n",
    ),
    ("docs/guide.md", "Their documentation.\n"),
    ("Justfile", "their-recipe:\n\techo theirs\n"),
    ("KEELER.md", "Their file that happens to share a name.\n"),
    ("specs/TEMPLATE.md", "Their template.\n"),
    ("rustfmt.toml", "max_width = 79\n"),
    ("CLAUDE.md", "# Their instructions\n"),
];

/// A throwaway directory holding the project under test and the PATH stubs.
///
/// The stubs live *beside* the project, never inside it: the checker
/// inspects the project tree, and harness scaffolding must not look like
/// the project's own content.
struct Fixture {
    root: PathBuf,
}

impl Fixture {
    /// A single-crate library — the shape `dtolnay/anyhow` has, generated.
    fn library(name: &str) -> Self {
        let fixture = Self::empty(name);
        let project = fixture.project();
        std::fs::create_dir_all(project.join("src")).unwrap();
        std::fs::write(
            project.join("Cargo.toml"),
            "[package]\nname = \"wild\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        std::fs::write(
            project.join("src/lib.rs"),
            "pub fn answer() -> u8 {\n    42\n}\n",
        )
        .unwrap();
        fixture
    }

    /// A workspace root with a member crate and no `[package]` of its own —
    /// the shape `serde-rs/serde` has, generated.
    fn workspace(name: &str) -> Self {
        let fixture = Self::empty(name);
        let project = fixture.project();
        std::fs::create_dir_all(project.join("member/src")).unwrap();
        std::fs::write(
            project.join("Cargo.toml"),
            "[workspace]\nmembers = [\"member\"]\nresolver = \"2\"\n",
        )
        .unwrap();
        std::fs::write(
            project.join("member/Cargo.toml"),
            "[package]\nname = \"member\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        std::fs::write(project.join("member/src/lib.rs"), "pub fn member() {}\n").unwrap();
        fixture
    }

    /// A project that has been lived in: its own workflows, documentation,
    /// a `.gitignore` with no final newline, and its own copies of files
    /// Keeler installs — the shape `BurntSushi/ripgrep` has, generated.
    fn lived_in(name: &str) -> Self {
        let fixture = Self::library(name);
        let project = fixture.project();
        for (rel, content) in LIVED_IN_FILES {
            let path = project.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, content).unwrap();
        }
        fixture
    }

    /// A project whose manifest already carries everything Keeler would
    /// add, and which commits its lockfile. Installing into it edits no
    /// manifest, so nothing can explain a lockfile that moves.
    fn settled(name: &str) -> Self {
        let fixture = Self::library(name);
        let project = fixture.project();
        std::fs::write(
            project.join("Cargo.toml"),
            "[package]\nname = \"wild\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n\
             [dev-dependencies]\nproptest = \"1\"\n\n\
             [profile.mutants]\ninherits = \"dev\"\ndebug = 0\n\n\
             [lints.clippy]\npedantic = { level = \"warn\", priority = -1 }\n",
        )
        .unwrap();
        std::fs::write(project.join("Cargo.lock"), "# their pinned build\n").unwrap();
        fixture
    }

    fn empty(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("keeler-wild-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("bin")).unwrap();
        std::fs::create_dir_all(root.join("project")).unwrap();

        // `cargo` logs instead of executing — `cargo add` would otherwise
        // reach crates.io — except `metadata`, which is local and read-only.
        // `curl` fails loudly, so no case can quietly reach the network.
        let real_cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
        let fixture = Self { root };
        fixture.write_script(
            "bin/cargo",
            &format!(
                "#!/usr/bin/env bash\n\
                 echo \"$@\" >> \"$(dirname \"$0\")/../cargo-calls.log\"\n\
                 if [ \"$1\" = metadata ]; then exec \"{real_cargo}\" \"$@\"; fi\n",
            ),
        );
        fixture.write_script(
            "bin/curl",
            "#!/usr/bin/env bash\n\
             echo \"curl $*\" >> \"$(dirname \"$0\")/../network-calls.log\"\n\
             echo \"harness: network access refused: curl $*\" >&2\nexit 7\n",
        );
        fixture
    }

    /// Makes the stub `cargo` leave a lockfile behind on `cargo add`, the
    /// way a real one does. CI runs with a real cargo, so the reference
    /// install produces artifacts the installer never copied.
    fn cargo_writes_a_lockfile(&self) {
        let real_cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
        self.write_script(
            "bin/cargo",
            &format!(
                "#!/usr/bin/env bash\n\
                 echo \"$@\" >> \"$(dirname \"$0\")/../cargo-calls.log\"\n\
                 if [ \"$1\" = metadata ]; then exec \"{real_cargo}\" \"$@\"; fi\n\
                 if [ \"$1\" = add ]; then echo '# generated' > Cargo.lock; fi\n",
            ),
        );
    }

    fn write_script(&self, rel: &str, body: &str) -> PathBuf {
        let path = self.root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, body).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        path
    }

    fn project(&self) -> PathBuf {
        self.root.join("project")
    }

    /// Runs the checker over the project with the real installer.
    fn check(&self) -> Output {
        self.check_with(None)
    }

    /// Runs the checker with `KEELER_INSTALL_SH` pointed at another script —
    /// the seam that lets a test hand the checker a defective installer.
    fn check_with(&self, installer: Option<&Path>) -> Output {
        let mut command = std::process::Command::new("bash");
        command
            .arg(repo_root().join("scripts/integration-check.sh"))
            .arg(self.project())
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    self.root.join("bin").display(),
                    std::env::var("PATH").unwrap(),
                ),
            );
        if let Some(installer) = installer {
            command.env("KEELER_INSTALL_SH", installer);
        }
        command
            .output()
            .expect("failed to run integration-check.sh")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// Every file under `dir`, relative, excluding `.git`.
fn files_under(dir: &Path, base: &Path, into: &mut std::collections::BTreeSet<String>) {
    for entry in std::fs::read_dir(dir).unwrap().map(Result::unwrap) {
        let path = entry.path();
        if path.file_name().is_some_and(|name| name == ".git") {
            continue;
        }
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

/// What a clean install adds to an empty crate, computed independently of
/// the checker — the oracle its reported count must agree with.
fn files_a_clean_install_adds() -> usize {
    let fixture = Fixture::library("oracle");
    let project = fixture.project();
    let mut before = std::collections::BTreeSet::new();
    files_under(&project, &project, &mut before);

    let output = std::process::Command::new("bash")
        .arg(repo_root().join("install.sh"))
        .arg(&project)
        .arg("--no-tools")
        .env(
            "PATH",
            format!(
                "{}:{}",
                fixture.root.join("bin").display(),
                std::env::var("PATH").unwrap(),
            ),
        )
        .output()
        .expect("failed to run install.sh");
    assert!(output.status.success(), "the reference install failed");

    let mut after = std::collections::BTreeSet::new();
    files_under(&project, &project, &mut after);
    after.difference(&before).count()
}

fn combined(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    )
}

#[test]
fn the_installer_lands_cleanly_on_a_real_library_crate() {
    // Given a single-crate library project — the shape a pinned real-world
    // library clone presents
    let fixture = Fixture::library("library");

    // When Keeler is installed into it under the contract checker
    let output = fixture.check();
    let report = combined(&output);

    // Then the installer exits zero
    assert!(
        output.status.success(),
        "the checker rejected a clean library crate:\n{report}",
    );

    // And every file the completeness guard tracks exists in the project
    assert!(
        report.contains("tracked files present"),
        "the checker did not report what it verified:\n{report}",
    );
    let counted: usize = report
        .split_whitespace()
        .zip(report.split_whitespace().skip(1))
        .find(|(_, next)| *next == "tracked")
        .and_then(|(count, _)| count.parse().ok())
        .unwrap_or_else(|| panic!("the checker reported no file count:\n{report}"));

    // And the set it tracks is exactly what a clean install adds — no more
    // (a file the project already had is not something the installer
    // landed) and no fewer.
    assert_eq!(
        counted,
        files_a_clean_install_adds(),
        "the checker's tracked set disagrees with what a clean install adds:\n{report}",
    );
}

#[test]
fn an_installer_that_skips_a_file_is_caught() {
    // Given an installer that does everything right but drops one file
    let fixture = Fixture::library("skips-a-file");
    let defective = fixture.write_script(
        "bin/defective-install.sh",
        &format!(
            "#!/usr/bin/env bash\n\
             set -euo pipefail\n\
             bash {} \"$@\"\n\
             rm -f \"$1/Justfile\"\n",
            repo_root().join("install.sh").display(),
        ),
    );

    // When the contract checker runs against it
    let output = fixture.check_with(Some(&defective));
    let report = combined(&output);

    // Then the check fails, naming the file that never landed
    assert!(
        !output.status.success(),
        "the checker passed an installer that skipped a file:\n{report}",
    );
    assert!(
        report.contains("Justfile"),
        "the checker did not name the missing file:\n{report}",
    );
}

#[test]
fn the_installer_lands_cleanly_on_a_real_workspace() {
    // Given a workspace root with member crates — the shape a pinned
    // real-world workspace clone presents
    let fixture = Fixture::workspace("workspace");

    // When Keeler is installed into it under the contract checker
    let output = fixture.check();
    let report = combined(&output);

    // Then the installer exits zero
    assert!(
        output.status.success(),
        "the checker rejected a clean workspace root:\n{report}",
    );

    // And it reports the workspace-root manifest as the project's own to
    // manage — the member crates need proptest and the profile themselves
    assert!(
        report.contains("workspace root"),
        "the checker did not confirm the workspace-root report:\n{report}",
    );
}

#[test]
fn an_installer_silent_about_a_workspace_root_is_caught() {
    // Given an installer that installs correctly but never tells a
    // workspace root that its manifest is the project's own to manage
    let fixture = Fixture::workspace("silent-workspace");
    let defective = fixture.write_script(
        "bin/silent-install.sh",
        &format!(
            "#!/usr/bin/env bash\n\
             set -uo pipefail\n\
             bash {} \"$@\" 2>&1 | grep -v 'workspace root'\n\
             exit \"${{PIPESTATUS[0]}}\"\n",
            repo_root().join("install.sh").display(),
        ),
    );

    // When the contract checker runs against it
    let output = fixture.check_with(Some(&defective));
    let report = combined(&output);

    // Then the check fails, saying what it expected to be told
    assert!(
        !output.status.success(),
        "the checker passed an installer that said nothing about the workspace root:\n{report}",
    );
    assert!(
        report.contains("workspace root"),
        "the checker did not name the missing report:\n{report}",
    );
}

#[test]
fn a_lockfile_the_installer_never_copied_is_not_demanded() {
    // Given an environment where installing leaves a lockfile behind — a
    // real cargo does exactly this when the installer adds proptest
    let fixture = Fixture::workspace("lockfile");
    fixture.cargo_writes_a_lockfile();

    // When the checker runs over a workspace root, where the installer
    // skips `cargo add` and so no lockfile appears
    let output = fixture.check();
    let report = combined(&output);

    // Then it does not demand a file the installer never copied: the
    // tracked set is what Keeler installs, not what its tools leave around
    assert!(
        output.status.success(),
        "the checker demanded a tool's side effect:\n{report}",
    );
    assert!(
        !report.contains("Cargo.lock"),
        "the checker treated a generated lockfile as part of the install set:\n{report}",
    );
}

#[test]
fn a_lived_in_project_keeps_every_byte_of_its_own_content() {
    // Given a project with its own workflows, .gitignore and documentation,
    // plus its own copies of files Keeler installs
    let fixture = Fixture::lived_in("lived-in");
    let project = fixture.project();
    let theirs: Vec<(&str, Vec<u8>)> = LIVED_IN_FILES
        .iter()
        .map(|(rel, _)| (*rel, std::fs::read(project.join(rel)).unwrap()))
        .collect();

    // When Keeler is installed into it under the contract checker
    let output = fixture.check();
    let report = combined(&output);

    // Then the checker accepts the install
    assert!(
        output.status.success(),
        "the checker rejected a clean install into a lived-in project:\n{report}",
    );

    // And no pre-existing file's content changed, except the three
    // documented append targets
    for (rel, before) in theirs {
        let after = std::fs::read(project.join(rel)).unwrap();
        if matches!(rel, "CLAUDE.md" | ".gitignore") {
            let before_text = String::from_utf8(before).unwrap();
            let after_text = String::from_utf8(after).unwrap();
            assert!(
                after_text.starts_with(before_text.trim_end_matches('\n')),
                "{rel} was rewritten rather than appended to",
            );
            continue;
        }
        assert_eq!(
            before, after,
            "the installer changed {rel}, which is the project's own",
        );
    }

    // And every conflicting file landed alongside as <name>.keeler
    assert!(
        report.contains("conflict"),
        "the checker did not account for the conflicts it found:\n{report}",
    );
}

proptest::proptest! {
    // Every case runs the installer through the checker as a subprocess —
    // keep the count low. Seeds land beside this file and get committed.
    #![proptest_config(proptest::prelude::ProptestConfig {
        cases: 8,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::WithSource("proptest-regressions"),
        )),
        ..proptest::prelude::ProptestConfig::default()
    })]

    /// No silent pass: whichever of the project's own files an installer
    /// destroys, the checker must notice and name it.
    #[test]
    fn a_clobbered_file_is_never_passed_over(
        victim in proptest::sample::select(
            LIVED_IN_FILES
                .iter()
                .map(|(rel, _)| *rel)
                .filter(|rel| !matches!(*rel, "CLAUDE.md" | ".gitignore"))
                .collect::<Vec<_>>(),
        ),
    ) {
        // Given an installer that overwrites one of the project's own files
        let fixture = Fixture::lived_in("clobbered");
        let defective = fixture.write_script(
            "bin/clobbering-install.sh",
            &format!(
                "#!/usr/bin/env bash\n\
                 set -euo pipefail\n\
                 bash {} \"$@\"\n\
                 printf 'clobbered\\n' > \"$1/{victim}\"\n",
                repo_root().join("install.sh").display(),
            ),
        );

        // When the contract checker runs against it
        let output = fixture.check_with(Some(&defective));
        let report = combined(&output);

        // Then the check fails, naming the file that was destroyed
        proptest::prop_assert!(
            !output.status.success(),
            "the checker passed an installer that overwrote {victim}:\n{report}",
        );
        proptest::prop_assert!(
            report.contains(victim),
            "the checker did not name the file it lost:\n{report}",
        );
    }
}

#[test]
fn a_conflict_left_out_of_the_report_is_caught() {
    // Given an installer that keeps a Keeler copy alongside the project's
    // own file but never says so
    let fixture = Fixture::lived_in("silent-conflict");
    let defective = fixture.write_script(
        "bin/quiet-install.sh",
        &format!(
            "#!/usr/bin/env bash\n\
             set -euo pipefail\n\
             bash {} \"$@\"\n\
             printf 'ours\\n' > \"$1/README.md.keeler\"\n",
            repo_root().join("install.sh").display(),
        ),
    );

    // When the contract checker runs against it
    let output = fixture.check_with(Some(&defective));
    let report = combined(&output);

    // Then the check fails, naming the conflict nobody was told about
    assert!(
        !output.status.success(),
        "the checker passed an unreported conflict:\n{report}",
    );
    assert!(
        report.contains("README.md"),
        "the checker did not name the unreported conflict:\n{report}",
    );
}

#[test]
fn a_second_run_over_a_real_project_changes_nothing() {
    // Given a lived-in project that Keeler was just installed into
    let fixture = Fixture::lived_in("second-run");

    // When the installer runs again under the contract checker
    let output = fixture.check();
    let report = combined(&output);

    // Then the working tree is byte-identical to its state after the first
    // run
    assert!(
        output.status.success(),
        "the checker rejected an idempotent install:\n{report}",
    );
    assert!(
        report.contains("second run changed nothing"),
        "the checker never re-ran the installer:\n{report}",
    );
}

#[test]
fn an_installer_that_drifts_on_every_run_is_caught() {
    // Given an installer that appends to the project every time it runs
    let fixture = Fixture::lived_in("drifting");
    let defective = fixture.write_script(
        "bin/drifting-install.sh",
        &format!(
            "#!/usr/bin/env bash\n\
             set -euo pipefail\n\
             bash {} \"$@\"\n\
             printf 'one-more-line\\n' >> \"$1/.gitignore\"\n",
            repo_root().join("install.sh").display(),
        ),
    );

    // When the contract checker runs against it
    let output = fixture.check_with(Some(&defective));
    let report = combined(&output);

    // Then the check fails, naming the file that drifted
    assert!(
        !output.status.success(),
        "the checker passed a non-idempotent installer:\n{report}",
    );
    assert!(
        report.contains(".gitignore"),
        "the checker did not name the drifting path:\n{report}",
    );
}

/// The block of `ci.yml` belonging to one job, by name.
fn job_block(workflow: &str, job: &str) -> String {
    let mut block = String::new();
    let mut inside = false;
    for line in workflow.lines() {
        let entry = line.strip_prefix("  ").map(str::trim_end);
        if let Some(entry) =
            entry.filter(|e| !e.starts_with(' ') && !e.starts_with('#') && e.ends_with(':'))
        {
            inside = entry.trim_end_matches(':') == job;
        }
        if inside {
            block.push_str(line);
            block.push('\n');
        }
    }
    block
}

#[test]
fn the_pins_are_exact() {
    // Given the integration job's project list
    let workflow = std::fs::read_to_string(repo_root().join(".github/workflows/ci.yml")).unwrap();
    let job = job_block(&workflow, "installer-real-world");
    assert!(
        !job.is_empty(),
        "ci.yml has no installer-real-world job — nothing points the checker at real projects",
    );

    // Then the three projects are there, by different authors
    for repo in ["dtolnay/anyhow", "serde-rs/serde", "BurntSushi/ripgrep"] {
        assert!(
            job.contains(repo),
            "the real-world matrix does not pin {repo}"
        );
    }

    // And every ref is a full commit SHA, never a branch head: a third
    // party's push must not change what CI tests
    let shas: Vec<&str> = job
        .lines()
        .filter_map(|line| line.split_once("sha:"))
        .map(|(_, sha)| sha.trim().trim_matches('"'))
        .collect();
    assert_eq!(
        shas.len(),
        3,
        "expected one pinned sha per project: {shas:?}"
    );
    for sha in &shas {
        assert!(
            sha.len() == 40
                && sha
                    .chars()
                    .all(|c| c.is_ascii_hexdigit() && !c.is_uppercase()),
            "{sha} is not a full lowercase commit SHA — a pin must be exact",
        );
    }

    // And the clone is a shallow fetch of that SHA, with no branch anywhere
    assert!(
        job.contains("--depth 1") && job.contains("matrix.sha"),
        "the clone step does not shallow-fetch the pinned SHA:\n{job}",
    );
    for branchy in ["--branch", "origin/main", "origin/master", "clone https"] {
        assert!(
            !job.contains(branchy),
            "the clone step reaches for `{branchy}` — that is a moving target, not a pin",
        );
    }

    // And each entry is judged by the contract checker
    assert!(
        job.contains("scripts/integration-check.sh"),
        "the real-world job does not run the contract checker:\n{job}",
    );

    // And the job says how a pin gets bumped, since nothing automatic will
    assert!(
        job.to_lowercase().contains("bump"),
        "nothing tells a reader how to move a pin deliberately:\n{job}",
    );
}

#[test]
fn the_local_suite_stays_offline() {
    // Given the integration checks
    let checker =
        std::fs::read_to_string(repo_root().join("scripts/integration-check.sh")).unwrap();

    // Then the checker itself never reaches for a repository: cloning is the
    // workflow's job, which is what keeps this suite runnable with no network
    for reach in ["git clone", "git fetch", "ls-remote", "curl ", "wget "] {
        assert!(
            !checker.contains(reach),
            "integration-check.sh reaches for `{reach}` — the local suite would need network",
        );
    }

    // And when the harness runs it, the network guarantee of spec 01 holds:
    // the curl stub intercepts, and nothing calls it
    let fixture = Fixture::lived_in("offline");
    let log = fixture.root.join("network-calls.log");
    let intercepted = std::process::Command::new("bash")
        .args(["-c", "curl --version"])
        .env(
            "PATH",
            format!(
                "{}:{}",
                fixture.root.join("bin").display(),
                std::env::var("PATH").unwrap(),
            ),
        )
        .output()
        .unwrap();
    assert!(
        !intercepted.status.success(),
        "the fixture does not stub out curl — a case could reach the network",
    );
    let after_probe = std::fs::read_to_string(&log).unwrap_or_default();

    let output = fixture.check();
    assert!(
        output.status.success(),
        "the checker failed:\n{}",
        combined(&output),
    );
    assert_eq!(
        std::fs::read_to_string(&log).unwrap_or_default(),
        after_probe,
        "the contract checker tried to reach the network",
    );
}

#[test]
fn a_lockfile_refreshed_by_a_manifest_edit_is_allowed() {
    // Given a project that commits its lockfile — every binary crate does
    let fixture = Fixture::lived_in("committed-lockfile");
    std::fs::write(
        fixture.project().join("Cargo.lock"),
        "# their pinned build\n",
    )
    .unwrap();
    // And a cargo that refreshes the lockfile when the manifest gains a
    // dependency, the way a real one does
    fixture.cargo_writes_a_lockfile();

    // When Keeler is installed into it under the contract checker
    let output = fixture.check();
    let report = combined(&output);

    // Then the refreshed lockfile is not held against the installer: it is
    // cargo's record of an edit the contract already permits
    assert!(
        output.status.success(),
        "the checker rejected a lockfile refresh that followed a manifest edit:\n{report}",
    );
}

#[test]
fn a_lockfile_moved_with_no_manifest_edit_is_caught() {
    // Given a project whose manifest already has everything Keeler adds, so
    // the installer edits no manifest at all
    let fixture = Fixture::settled("unexplained-lockfile");
    // And an installer that rewrites the lockfile anyway
    let defective = fixture.write_script(
        "bin/lockfile-install.sh",
        &format!(
            "#!/usr/bin/env bash\n\
             set -euo pipefail\n\
             bash {} \"$@\"\n\
             printf 'not theirs\\n' > \"$1/Cargo.lock\"\n",
            repo_root().join("install.sh").display(),
        ),
    );

    // When the contract checker runs against it
    let output = fixture.check_with(Some(&defective));
    let report = combined(&output);

    // Then the exemption does not cover it — nothing explains the change
    assert!(
        !output.status.success(),
        "the checker excused a lockfile change with no manifest edit behind it:\n{report}",
    );
    assert!(
        report.contains("Cargo.lock"),
        "the checker did not name the lockfile it lost:\n{report}",
    );
}
