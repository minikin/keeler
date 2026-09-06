//! Spec 09 — Keeler as a plugin. The wrapper: `bin/keeler`.
//!
//! The `Justfile` lives in the plugin now, so nobody can reach a recipe by
//! typing `just` in their project — there is no justfile there to find.
//! `bin/keeler` is the front door: Claude Code puts the plugin's `bin/` on
//! the Bash tool's PATH, a human puts it on theirs, and either way `keeler
//! <recipe>` runs the plugin's `Justfile` against the repository they are
//! standing in.
//!
//! Two kinds of test below. Where the wrapper's own argument list is the
//! thing under test, a stub `just` first on PATH records what it was
//! handed; where the question is whether a real recipe runs, the real
//! `just` runs it against a throwaway repository, as `tests/justfile.rs`
//! does.

mod common;

use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use common::{Repo, repo_root, said};
use proptest::prelude::*;

/// Every name `just` would pick up on its own — the wrapper must leave the
/// project without one of any spelling.
const SPELLINGS: [&str; 4] = ["Justfile", "justfile", ".justfile", ".Justfile"];

/// The wrapper as it ships, run by its path the way PATH would run it.
fn wrapper() -> PathBuf {
    repo_root().join("bin/keeler")
}

/// The `--justfile` argument the wrapper must always pass: the plugin's
/// own file, spelled as a path a human could read back in a message.
fn plugin_justfile() -> String {
    canonical(&repo_root().join("Justfile"))
}

/// A path as the shell will report it: the wrapper resolves symlinks (`cd
/// -P`, `readlink -f`), and a macOS temp directory is `/var` → `/private/var`.
fn canonical(path: &Path) -> String {
    std::fs::canonicalize(path)
        .unwrap_or_else(|why| panic!("cannot resolve {}: {why}", path.display()))
        .display()
        .to_string()
}

/// Records one argument list per run: every argument terminated by `\x1f`,
/// so an empty argument is still a field. Overwrites, because each fixture
/// runs the wrapper once and the property test wants only the last call.
const JUST_STUB: &str = r#"#!/usr/bin/env bash
{ for a in "$@"; do printf '%s\037' "$a"; done; printf '\n'; } > "$KEELER_STUB_JUST_LOG"
"#;

/// A directory holding a stub `just`, first on PATH, and the log it writes.
struct Stub {
    dir: PathBuf,
}

impl Stub {
    fn new(name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("keeler-wrapper-stub-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        let just = dir.join("bin/just");
        std::fs::write(&just, JUST_STUB).unwrap();
        std::fs::set_permissions(&just, std::fs::Permissions::from_mode(0o755)).unwrap();
        Self { dir }
    }

    fn log(&self) -> PathBuf {
        self.dir.join("just-calls")
    }

    /// Runs the wrapper — by the given path, from the given directory —
    /// with this stub ahead of the real `just`.
    ///
    /// The log goes first: a run that never reached `just` would otherwise
    /// be read as the previous run's arguments, which in a property test is
    /// a case passing or failing on its neighbour's evidence.
    fn run(&self, wrapper: &Path, from: &Path, args: &[&str]) -> Output {
        let _ = std::fs::remove_file(self.log());
        let path = std::env::var("PATH").unwrap();
        Command::new(wrapper)
            .args(args)
            .current_dir(from)
            .env("PATH", format!("{}:{path}", self.dir.join("bin").display()))
            .env("KEELER_STUB_JUST_LOG", self.log())
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .output()
            .expect("failed to run the wrapper")
    }

    /// The argument list the stub recorded.
    fn recorded(&self) -> Vec<String> {
        let call = std::fs::read_to_string(self.log())
            .unwrap_or_else(|why| panic!("the stub `just` recorded nothing: {why}"));
        let mut fields: Vec<String> = call
            .trim_end_matches('\n')
            .split('\u{1f}')
            .map(str::to_string)
            .collect();
        // Every argument is *terminated* by the separator, so the split
        // leaves one empty field past the last one.
        fields.pop();
        fields
    }
}

impl Drop for Stub {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// A directory that is not a git repository, removed on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("keeler-wrapper-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A spec whose Tasks section is the given lines — the smallest thing the
/// graph parser accepts.
fn spec(tasks: &str) -> String {
    format!("# Spec 01 — foo\n\n**Status:** Approved\n\n## Tasks\n\n{tasks}\n---\n")
}

/// The board line for one task, from a `keeler-graph` report.
fn task_line(report: &str, id: &str) -> String {
    report
        .lines()
        .find(|line| line.split_whitespace().next() == Some(id))
        .unwrap_or_else(|| panic!("no line for {id} in:\n{report}"))
        .to_string()
}

/// The fixed prefix every call carries: the plugin's file, the project as
/// the working directory.
fn prefix(root: &str) -> Vec<String> {
    vec![
        "--justfile".to_string(),
        plugin_justfile(),
        "--working-directory".to_string(),
        root.to_string(),
    ]
}

// ---------------------------------------------------------------------------
// Scenario: The wrapper runs the plugin's Justfile in the repository root
// ---------------------------------------------------------------------------

#[test]
fn the_wrapper_runs_the_plugins_justfile_in_the_repository_root() {
    // Given a git repository with a one-task spec committed on HEAD, and a
    // subdirectory to stand in
    let repo = Repo::new("wrapper", "repo-root");
    repo.commit(
        "specs/01-foo.md",
        &spec("- [ ] **T1 — the root.** Scenarios: _one_.\n"),
        "the spec",
    );
    std::fs::create_dir_all(repo.path().join("src")).unwrap();

    // When the wrapper runs a recipe from that subdirectory
    let out = Command::new(wrapper())
        .args(["keeler-graph", "specs/01-foo.md"])
        .current_dir(repo.path().join("src"))
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .expect("failed to run the wrapper");

    // Then it exits zero and reports T1 ready — the recipe ran with the
    // repository root as its working directory, not the subdirectory, so
    // the spec's relative path resolved
    assert!(
        out.status.success(),
        "the wrapper did not run the recipe from a subdirectory:\n{}",
        said(&out)
    );
    assert!(
        task_line(&said(&out), "T1").contains("ready"),
        "the graph did not report T1 ready:\n{}",
        said(&out)
    );

    // And no justfile of any spelling has appeared in the crate
    for spelling in SPELLINGS {
        assert!(
            !repo.path().join(spelling).exists(),
            "the wrapper left a {spelling} in the project"
        );
    }
}

// ---------------------------------------------------------------------------
// Scenario: The wrapper passes every argument through
// ---------------------------------------------------------------------------

#[test]
fn the_wrapper_passes_every_argument_through() {
    // Given a stub `just` first on PATH that records its argument list, and
    // a crate
    let stub = Stub::new("arguments");
    let repo = Repo::new("wrapper", "arguments");

    // When the wrapper runs a recipe with its arguments from the crate root
    let out = stub.run(
        &wrapper(),
        repo.path(),
        &["keeler-spawn", "specs/01-foo.md", "T1"],
    );
    assert!(out.status.success(), "the wrapper failed:\n{}", said(&out));

    // Then the recorded list is the fixed prefix and the arguments as given
    let mut expected = prefix(&canonical(repo.path()));
    expected.extend(["keeler-spawn", "specs/01-foo.md", "T1"].map(str::to_string));
    assert_eq!(
        stub.recorded(),
        expected,
        "the wrapper did not hand `just` the recipe and its arguments"
    );
}

// ---------------------------------------------------------------------------
// Scenario: The wrapper passes any argument list through unchanged
// ---------------------------------------------------------------------------

/// The wrapper is a pass-through, and the arguments that prove it are the
/// awkward ones: a recipe argument with a space in it (a spec title on a
/// `keeler-status` line), an empty one, and one starting with `--` that a
/// careless `$*` would let `just` read as the wrapper's own.
///
/// Written against `TestRunner` rather than through `proptest!` so the stub
/// and the directory are built once for the whole test: a case is a pair of
/// process spawns, and rebuilding the fixture around each one made the test
/// forty times its own cost.
#[test]
fn the_wrapper_passes_any_argument_list_through_unchanged() {
    // Given a stub `just` first on PATH, and a directory to run in
    let stub = Stub::new("argv");
    let scratch = Scratch::new("argv");
    let root = canonical(scratch.path());

    let mut runner = proptest::test_runner::TestRunner::new(ProptestConfig {
        // A pair of subprocesses per case: enough cases to reach the shapes
        // that break naive quoting, few enough to stay a test one waits for.
        cases: 32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::WithSource("proptest-regressions"),
        )),
        // `proptest!` sets this from `file!()`; a runner built by hand has
        // to say it, and without it `WithSource` has no source to sit
        // beside and a found counterexample is never written down.
        source_file: Some(file!()),
        ..ProptestConfig::default()
    });

    // When the wrapper runs with any list of arguments
    let lists = prop::collection::vec(
        prop_oneof!["[a-zA-Z0-9 ._/=-]{0,10}", "--[a-z][a-z-]{0,8}"],
        0..5,
    );
    runner
        .run(&lists, |args| {
            let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
            let out = stub.run(&wrapper(), scratch.path(), &borrowed);
            prop_assert!(
                out.status.success(),
                "the wrapper failed on {:?}:\n{}",
                args,
                said(&out)
            );

            // Then the recorded list is the fixed prefix followed by the
            // list verbatim — same arguments, same order, nothing split on
            // a space and nothing read as the wrapper's own flag
            let mut expected = prefix(&root);
            expected.extend(args.iter().cloned());
            prop_assert_eq!(stub.recorded(), expected);
            Ok(())
        })
        .unwrap();
}

// ---------------------------------------------------------------------------
// Scenario: The wrapper finds its Justfile through a symlink
// ---------------------------------------------------------------------------

#[test]
fn the_wrapper_finds_its_justfile_through_a_symlink() {
    // Given a symlink elsewhere pointing at the wrapper — how a human puts
    // `keeler` on their PATH without putting the plugin's whole bin/ there
    let stub = Stub::new("symlink");
    let elsewhere = Scratch::new("symlink");
    let link = elsewhere.path().join("keeler");
    std::os::unix::fs::symlink(wrapper(), &link).unwrap();

    // When it runs through the symlink
    let out = stub.run(&link, elsewhere.path(), &["--list"]);
    assert!(
        out.status.success(),
        "the wrapper failed through a symlink:\n{}",
        said(&out)
    );

    // Then the recorded list starts with the plugin's Justfile — resolved
    // through the link, not from the directory the link sits in
    assert_eq!(
        stub.recorded().get(..2).map(<[String]>::to_vec),
        Some(vec!["--justfile".to_string(), plugin_justfile()]),
        "the wrapper looked for its Justfile beside the symlink"
    );
}

// ---------------------------------------------------------------------------
// Scenario: Outside a repository the wrapper uses the current directory
// ---------------------------------------------------------------------------

#[test]
fn outside_a_repository_the_wrapper_uses_the_current_directory() {
    // Given a directory that is not inside a git repository
    let stub = Stub::new("no-repo");
    let scratch = Scratch::new("no-repo");
    let toplevel = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(scratch.path())
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .expect("failed to run git");
    assert!(
        !toplevel.status.success(),
        "the fixture directory is inside a git repository, so the scenario \
         it stands for cannot happen here"
    );

    // When the wrapper runs there
    let out = stub.run(&wrapper(), scratch.path(), &["--list"]);
    assert!(
        out.status.success(),
        "the wrapper refused a directory outside a repository:\n{}",
        said(&out)
    );

    // Then the working directory is that directory: a recipe run where
    // there is no repository runs where the human is standing
    let mut expected = prefix(&canonical(scratch.path()));
    expected.push("--list".to_string());
    assert_eq!(
        stub.recorded(),
        expected,
        "the wrapper did not fall back to the current directory"
    );
}

// ---------------------------------------------------------------------------
// Scenario: In a linked worktree the wrapper stays in the worktree
// ---------------------------------------------------------------------------

#[test]
fn in_a_linked_worktree_the_wrapper_stays_in_the_worktree() {
    // Given a repository with a linked worktree — the shape graph mode puts
    // every spawned agent in
    let stub = Stub::new("worktree");
    let repo = Repo::new("wrapper", "worktree");
    repo.commit("README.md", "# fixture\n", "the root commit");
    // Outside the repository and inside something that cleans itself up:
    // an assertion below may never return, and a leaked worktree would
    // then be a stale entry the next run's `worktree add` refuses.
    let elsewhere = Scratch::new("worktree-holder");
    let worktree = elsewhere.path().join("linked");
    repo.git(&[
        "worktree",
        "add",
        "-q",
        worktree.to_str().unwrap(),
        "-b",
        "keeler/99-fixture/t4",
    ]);
    std::fs::create_dir_all(worktree.join("src")).unwrap();

    // When the wrapper runs from a subdirectory of the worktree
    let out = stub.run(&wrapper(), &worktree.join("src"), &["--list"]);
    assert!(
        out.status.success(),
        "the wrapper failed in a linked worktree:\n{}",
        said(&out)
    );

    // Then the working directory is the worktree root and not the main
    // checkout — the task's branch is what the recipe must measure
    let mut expected = prefix(&canonical(&worktree));
    expected.push("--list".to_string());
    assert_eq!(
        stub.recorded(),
        expected,
        "the wrapper left the worktree for the main checkout"
    );
}

// ---------------------------------------------------------------------------
// Scenario: The wrapper is executable as committed
// ---------------------------------------------------------------------------

#[test]
fn the_wrapper_is_executable_as_committed() {
    // Given the repository's git index — the modes a clone and Claude Code's
    // plugin cache both get, whatever the checkout's umask was
    let listed = Command::new("git")
        .args([
            "ls-files",
            "-s",
            "--",
            "bin/keeler",
            "hooks/session-start.sh",
            "scripts/keeler-graph.sh",
        ])
        .current_dir(repo_root())
        .output()
        .expect("failed to run git ls-files");
    assert!(listed.status.success(), "git ls-files failed");
    let index = String::from_utf8_lossy(&listed.stdout);

    // Then each of the three scripts is recorded executable
    for script in [
        "bin/keeler",
        "hooks/session-start.sh",
        "scripts/keeler-graph.sh",
    ] {
        let entry = index
            .lines()
            .find(|line| line.ends_with(script))
            .unwrap_or_else(|| panic!("{script} is not tracked:\n{index}"));
        assert!(
            entry.starts_with("100755"),
            "{script} is committed non-executable, so nothing on PATH can run it: {entry}"
        );
    }
}
