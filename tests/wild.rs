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
