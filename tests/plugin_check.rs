//! Spec 09 — `plugin-check` holds the plugin to VERSION.
//!
//! Two failures this gate exists for are silent ones. `/plugin update`
//! compares `plugin.json`'s `version`: a release that forgets to bump it
//! ships nothing to everyone who asks for the update, and says so to
//! nobody. The `SessionStart` hook is the same shape — output over the cap
//! is filed away and replaced by a preview, so rules that outgrew it reach
//! the agent partially and without a word.
//!
//! The tests drive the built binary against throwaway repositories, the way
//! `tests/release.rs` drives the release guard: exit codes and messages are
//! the contract CI reads, and a library call could not see either.

mod common;

use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use common::{Repo, real_just, repo_root, said, xtask_bin};

/// The plugin manifest Claude Code reads, at one version.
fn plugin_manifest(version: &str) -> String {
    format!("{{\n  \"name\": \"keeler\",\n  \"version\": \"{version}\"\n}}\n")
}

/// The marketplace, at one version — with the owner block the shipped file
/// carries, whose own `name` must not be mistaken for a plugin's.
fn marketplace_manifest(version: &str) -> String {
    format!(
        "{{\n  \"name\": \"keeler\",\n  \"owner\": {{ \"name\": \"minikin\" }},\n  \
         \"plugins\": [\n    {{ \"name\": \"keeler\", \"source\": \"./\", \
         \"version\": \"{version}\" }}\n  ]\n}}\n"
    )
}

/// A repository shaped like Keeler's own: the two plugin manifests, the
/// rules with their marker, and — because the release guard reads them on
/// the way past — VERSION, a CHANGELOG, a manifest and a spec. Everything
/// in it agrees on one version until a test breaks one thing.
struct Fixture(PathBuf);

impl Fixture {
    fn new(name: &str, version: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("keeler-plugin-check-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let fixture = Self(dir);
        fixture.write("VERSION", &format!("{version}\n"));
        fixture.write(
            "keeler.md",
            &format!("<!-- keeler-version: {version} -->\n# rules\n"),
        );
        fixture.write(".claude-plugin/plugin.json", &plugin_manifest(version));
        fixture.write(
            ".claude-plugin/marketplace.json",
            &marketplace_manifest(version),
        );
        fixture.write(
            "CHANGELOG.md",
            &format!("# Changelog\n\n## [{version}] — 2026-09-06\n\n- an entry\n"),
        );
        fixture.write(
            "Cargo.toml",
            &format!("[package]\nname = \"fixture\"\nversion = \"{version}\"\n"),
        );
        fixture.write(
            "specs/01-fixture.md",
            "**Status:** Approved\n\n## Tasks\n\n- [ ] **T1 — Not yet.**\n",
        );
        fixture
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn write(&self, rel: &str, body: &str) {
        let path = self.0.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }

    /// Runs an xtask command from inside this repository — the command
    /// reads the working directory, as CI runs it.
    fn xtask(&self, args: &[&str]) -> Output {
        Command::new(xtask_bin())
            .args(args)
            .current_dir(&self.0)
            .output()
            .expect("failed to run the xtask binary")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Asserts a refusal happened and names everything the reader needs to fix
/// it — a gate that fails without naming the file sends someone hunting.
fn refused_naming(output: &Output, expected: &[&str]) {
    assert!(
        !output.status.success(),
        "the gate passed on a plugin that disagrees with VERSION:\n{}",
        said(output),
    );
    for needle in expected {
        assert!(
            said(output).contains(needle),
            "the refusal does not name `{needle}`:\n{}",
            said(output),
        );
    }
}

#[test]
fn plugin_check_refuses_a_plugin_manifest_that_disagrees() {
    // Given VERSION reads 0.5.0 and the plugin manifest declares 0.4.1
    let fixture = Fixture::new("manifest", "0.5.0");
    fixture.write(".claude-plugin/plugin.json", &plugin_manifest("0.4.1"));

    // When `cargo xtask plugin-check` runs
    let output = fixture.xtask(&["plugin-check"]);

    // Then it exits non-zero, naming the file and both versions: this is
    // the field `/plugin update` compares, so a release over it installs
    // nothing and reports success doing so
    refused_naming(&output, &[".claude-plugin/plugin.json", "0.4.1", "0.5.0"]);
}

#[test]
fn plugin_check_refuses_a_marketplace_entry_that_disagrees() {
    // Given the "keeler" entry in the marketplace declares 0.4.1
    let fixture = Fixture::new("marketplace", "0.5.0");
    fixture.write(
        ".claude-plugin/marketplace.json",
        &marketplace_manifest("0.4.1"),
    );

    // When the gate runs
    let output = fixture.xtask(&["plugin-check"]);

    // Then it exits non-zero, naming that file and both versions
    refused_naming(
        &output,
        &[".claude-plugin/marketplace.json", "0.4.1", "0.5.0"],
    );
}

#[test]
fn plugin_check_refuses_rules_the_hook_would_truncate() {
    // Given a keeler.md of 9,501 bytes — one past the ceiling
    let fixture = Fixture::new("ceiling", "0.5.0");
    let head = "<!-- keeler-version: 0.5.0 -->\n";
    fixture.write(
        "keeler.md",
        &format!("{head}{}", "x".repeat(9501 - head.len())),
    );
    assert_eq!(
        std::fs::read(fixture.path().join("keeler.md"))
            .unwrap()
            .len(),
        9501,
        "the fixture is not the size the scenario is about",
    );

    // When the gate runs
    let output = fixture.xtask(&["plugin-check"]);

    // Then it exits non-zero, naming the file, its size and the ceiling —
    // over the cap the hook files the text away and hands the agent a
    // preview, which is the rules arriving partially and in silence
    refused_naming(&output, &["keeler.md", "9501", "9500"]);
}

#[test]
fn plugin_check_reads_the_rules_marker_from_the_plugin_root() {
    // Given VERSION reads 0.5.0 and the rules at the plugin root carry
    // 0.4.1, with no .claude/keeler.md anywhere
    let fixture = Fixture::new("marker", "0.5.0");
    fixture.write("keeler.md", "<!-- keeler-version: 0.4.1 -->\n# rules\n");
    assert!(
        !fixture.path().join(".claude/keeler.md").exists(),
        "the fixture carries the old rules path — the scenario is about its absence",
    );

    // When the gate runs
    let output = fixture.xtask(&["plugin-check"]);

    // Then it exits non-zero, naming the file it read and both versions:
    // the rules moved to the plugin root, and a gate still looking at
    // .claude/keeler.md would have found nothing and said nothing
    refused_naming(&output, &["keeler.md", "0.4.1", "0.5.0"]);
}

#[test]
fn a_plugin_that_agrees_passes_the_check() {
    // Given a repository where VERSION, both manifests and the marker
    // agree, and the rules fit — the state a release is cut from
    let fixture = Fixture::new("agreeing", "0.5.0");

    // When the gate runs
    let output = fixture.xtask(&["plugin-check"]);

    // Then it passes, and says what it compared: a gate whose message does
    // not name what it read is one nobody notices has stopped reading it
    assert!(
        output.status.success(),
        "the gate refused a truthful plugin:\n{}",
        said(&output),
    );
    for expected in ["0.5.0", "plugin.json", "marketplace.json", "keeler.md"] {
        assert!(
            said(&output).contains(expected),
            "the gate does not say it checked `{expected}`:\n{}",
            said(&output),
        );
    }
}

#[test]
fn the_release_guard_runs_the_plugin_check() {
    // Given VERSION reads 0.5.0 with a CHANGELOG section for it, and a
    // plugin manifest left behind at 0.4.1
    let fixture = Fixture::new("guard", "0.5.0");
    fixture.write(".claude-plugin/plugin.json", &plugin_manifest("0.4.1"));

    // When the release guard runs for the honest tag
    let output = fixture.xtask(&["release-guard", "v0.5.0"]);

    // Then it refuses, naming the manifest: a tag that ships a plugin
    // nobody can update is the release this guard exists to stop
    refused_naming(&output, &[".claude-plugin/plugin.json"]);

    // And a truthful repository still passes the same guard
    let honest = Fixture::new("guard-honest", "0.5.0");
    let output = honest.xtask(&["release-guard", "v0.5.0"]);
    assert!(
        output.status.success(),
        "the guard refused a truthful release:\n{}",
        said(&output),
    );
}

#[test]
fn the_release_guard_reports_the_rules_marker_once() {
    // Given a repository where the marker disagrees with VERSION — the one
    // fact both the guard and the plugin check read
    let fixture = Fixture::new("marker-once", "0.5.0");
    fixture.write("keeler.md", "<!-- keeler-version: 0.4.1 -->\n# rules\n");

    // When the release guard runs
    let output = fixture.xtask(&["release-guard", "v0.5.0"]);

    // Then the marker is refused once. Twice would send the reader looking
    // for a second file to fix, and there is only one.
    refused_naming(&output, &["keeler.md", "0.4.1"]);
    let complaints = said(&output)
        .lines()
        .filter(|line| line.contains("marker"))
        .count();
    assert_eq!(
        complaints,
        1,
        "the marker is reported {complaints} times:\n{}",
        said(&output),
    );
}

/// Stands in for every tool `lint` reaches for — `cargo` and `shellcheck`
/// both — recording the argument list it was handed and returning. What
/// the two lint scenarios are about is whether `xtask plugin-check` is
/// among those lists, which no substring of the `Justfile` could show.
const TOOL_STUB: &str = r#"#!/usr/bin/env bash
printf '%s\n' "$*" >> "$KEELER_STUB_LOG"
"#;

/// A project `lint` runs against, with stub tools first on PATH.
struct Project(Repo);

impl Project {
    fn new(name: &str) -> Self {
        let repo = Repo::new("lint", name);
        std::fs::create_dir_all(repo.path().join("bin")).unwrap();
        for tool in ["cargo", "shellcheck"] {
            let stub = repo.path().join("bin").join(tool);
            std::fs::write(&stub, TOOL_STUB).unwrap();
            std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        Self(repo)
    }

    fn lint(&self) -> Output {
        let dir = self.0.path();
        let path = std::env::var("PATH").unwrap();
        Command::new(real_just())
            .args(["--justfile", repo_root().join("Justfile").to_str().unwrap()])
            .args(["--working-directory", dir.to_str().unwrap()])
            .arg("lint")
            .current_dir(dir)
            .env("PATH", format!("{}:{path}", dir.join("bin").display()))
            .env("KEELER_STUB_LOG", dir.join("tool-calls"))
            .output()
            .expect("failed to run just")
    }

    /// The argument lists the stubs were handed, in order.
    fn calls(&self) -> Vec<String> {
        std::fs::read_to_string(self.0.path().join("tool-calls"))
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }
}

#[test]
fn lint_in_this_repository_runs_the_plugin_check() {
    // Given a checkout holding templates/keeler.yml — the marker only
    // Keeler's own repository has — and stub tools recording their calls
    let project = Project::new("keeler-checkout");
    project.0.write("templates/keeler.yml", "on: push\n");

    // When `lint` runs from the plugin's Justfile
    let output = project.lint();
    assert!(output.status.success(), "{}", said(&output));

    // Then the plugin check ran with the rest of the gate, so a manifest
    // left behind fails here and not at the tag
    assert!(
        project
            .calls()
            .iter()
            .any(|call| call == "xtask plugin-check"),
        "lint never ran the plugin check: {:?}",
        project.calls(),
    );
}

#[test]
fn lint_in_an_adopters_project_does_not() {
    // Given a crate with no templates/keeler.yml — every adopter
    let project = Project::new("adopter");

    // When `lint` runs from the plugin's Justfile
    let output = project.lint();
    assert!(output.status.success(), "{}", said(&output));

    // Then no `cargo xtask` is attempted: the xtask crate is Keeler's own
    // machinery and is not in their repository, so running it would fail
    // their lint over a gate that is none of their business
    let attempted: Vec<String> = project
        .calls()
        .into_iter()
        .filter(|call| call.contains("xtask"))
        .collect();
    assert!(
        attempted.is_empty(),
        "lint ran Keeler's own machinery in an adopter's project: {attempted:?}",
    );
}
