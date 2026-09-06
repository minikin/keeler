//! Spec 09 — Keeler as a plugin. The `Justfile` side: the recipes stop
//! assuming they live in the project they measure.
//!
//! The plugin's `Justfile` is run from wherever Claude Code cached the
//! plugin, against a project that has no justfile of its own — so every
//! recipe-to-recipe call has to name *this* file, and the graph parser has
//! to be found beside it rather than under the project's root. The tests
//! drive the shipped recipes as subprocesses against throwaway git
//! repositories, the way `tests/branch.rs` and `tests/spawn.rs` do, with a
//! stub `just` on PATH where a call's own arguments are the thing under
//! test.

mod common;

use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;
use std::process::{Command, Output};
use std::sync::OnceLock;

use common::{Repo, repo_root, said};
use proptest::prelude::*;

/// Every name `just` would pick up on its own. A recipe that reached one
/// of these in the project would be running someone else's file.
const SPELLINGS: [&str; 4] = ["Justfile", "justfile", ".justfile", ".Justfile"];

/// The task branch shape `keeler-spawn` creates, and the only place
/// `keeler-branch` is ever run.
const BRANCH: &str = "keeler/99-fixture/t4";

/// The absolute path of the real `just`, resolved once against the
/// harness's own PATH — so a stub `just` on a fixture's PATH is only ever
/// reached from inside a recipe body.
fn real_just() -> &'static str {
    static JUST: OnceLock<String> = OnceLock::new();
    JUST.get_or_init(|| {
        let out = Command::new("sh")
            .args(["-c", "command -v just"])
            .output()
            .expect("failed to look for just");
        assert!(out.status.success(), "just is not on PATH");
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    })
}

/// The plugin's `Justfile` — this repository's, which is the plugin root.
fn plugin_justfile() -> String {
    repo_root().join("Justfile").display().to_string()
}

fn justfile_text() -> String {
    std::fs::read_to_string(repo_root().join("Justfile")).unwrap()
}

/// A spec whose Tasks section is the given lines — the smallest thing the
/// graph parser accepts.
fn spec(tasks: &str) -> String {
    format!("# Spec 01 — foo\n\n**Status:** Approved\n\n## Tasks\n\n{tasks}\n---\n")
}

/// Runs a recipe the way the wrapper will: the plugin's `Justfile`, the
/// project as the working directory, and nothing of the project's own on
/// the command line.
fn keeler(dir: &Path, args: &[&str]) -> Output {
    Command::new(real_just())
        .args(["--justfile", &plugin_justfile()])
        .arg("--working-directory")
        .arg(dir)
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .expect("failed to run a recipe from the plugin's Justfile")
}

/// The board line for one task, from a `keeler-graph` report.
fn task_line(report: &str, id: &str) -> String {
    report
        .lines()
        .find(|line| line.split_whitespace().next() == Some(id))
        .unwrap_or_else(|| panic!("no line for {id} in:\n{report}"))
        .to_string()
}

// ---------------------------------------------------------------------------
// Scenario: A recipe runs against a project that has no justfile
// ---------------------------------------------------------------------------

#[test]
fn a_recipe_runs_against_a_project_that_has_no_justfile() {
    // Given a git repository with no justfile of any spelling, and a spec
    // with task T1 committed on HEAD
    let repo = Repo::new("justfile", "no-justfile");
    repo.commit(
        "specs/01-foo.md",
        &spec("- [ ] **T1 — the root.** Scenarios: _one_.\n"),
        "the spec",
    );

    // When a recipe runs from the plugin's Justfile against that crate
    let out = keeler(repo.path(), &["keeler-graph", "specs/01-foo.md"]);

    // Then it exits zero and reports T1 ready
    assert!(
        out.status.success(),
        "a recipe refused a project with no justfile:\n{}",
        said(&out)
    );
    let report = said(&out);
    assert!(
        task_line(&report, "T1").contains("ready"),
        "the graph did not report T1 ready:\n{report}"
    );

    // And no justfile of any spelling has appeared in the crate — the
    // recipes read their own file and write nothing of the kind
    for spelling in SPELLINGS {
        assert!(
            !repo.path().join(spelling).exists(),
            "running a recipe left a {spelling} in the project"
        );
    }
}

// ---------------------------------------------------------------------------
// Scenario: The project's own justfile is neither read nor written
// ---------------------------------------------------------------------------

#[test]
fn the_projects_own_justfile_is_neither_read_nor_written() {
    // Given a git repository whose own justfile defines `keeler-graph` as
    // a refusal — the recipe that answers is the one that gets read
    let repo = Repo::new("justfile", "own-justfile");
    let theirs = "# the project's own\nkeeler-graph SPEC:\n    exit 1\n";
    repo.commit("justfile", theirs, "the project's own recipes");
    repo.commit(
        "specs/01-foo.md",
        &spec("- [ ] **T1 — the root.** Scenarios: _one_.\n"),
        "the spec",
    );

    // When the plugin's `keeler-graph` runs there
    let out = keeler(repo.path(), &["keeler-graph", "specs/01-foo.md"]);

    // Then it exits zero and reports T1 ready — the plugin's recipe ran,
    // not the project's
    assert!(
        out.status.success(),
        "the project's own justfile answered instead of the plugin's:\n{}",
        said(&out)
    );
    assert!(
        task_line(&said(&out), "T1").contains("ready"),
        "the graph did not report T1 ready:\n{}",
        said(&out)
    );

    // And the project's justfile is byte-identical to before
    assert_eq!(
        std::fs::read_to_string(repo.path().join("justfile")).unwrap(),
        theirs,
        "a recipe rewrote the project's own justfile"
    );
}

// ---------------------------------------------------------------------------
// Scenario: A recipe that calls another recipe calls its own Justfile
// ---------------------------------------------------------------------------

/// Records every argument list it is handed — one field per `\x1f`, one
/// call per line — and stands in for the three gates `keeler-branch`
/// composes, which it tells apart by the *last* argument: a self-call
/// names the recipe after the flags that select this Justfile.
const JUST_STUB: &str = r#"#!/usr/bin/env bash
{ for a in "$@"; do printf '%s\037' "$a"; done; printf '\n'; } >> "$KEELER_STUB_JUST_LOG"
last=""
for a in "$@"; do last="$a"; done
case "$last" in
dev|crap-delta|mutants-diff) exit 0 ;;
esac
exec "$KEELER_REAL_JUST" "$@"
"#;

fn write_stub(path: &Path, body: &str) {
    std::fs::write(path, body).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// The argument lists the stub `just` recorded, in order.
fn recorded(log: &Path) -> Vec<Vec<String>> {
    std::fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .map(|call| {
            call.split('\u{1f}')
                .filter(|field| !field.is_empty())
                .map(str::to_string)
                .collect()
        })
        .collect()
}

#[test]
fn a_recipe_that_calls_another_recipe_calls_its_own_justfile() {
    // Given a crate on a task branch whose own justfile defines `dev` as a
    // refusal, and a stub `just` first on PATH recording every call
    let repo = Repo::new("justfile", "self-calls");
    std::fs::create_dir_all(repo.path().join("bin")).unwrap();
    write_stub(&repo.path().join("bin/just"), JUST_STUB);
    repo.write("justfile", "# the project's own\ndev:\n    exit 1\n");
    repo.write("crap-baseline.json", "{\"functions\":[]}\n");
    repo.write(".gitignore", "/bin/\n/just-calls\n");
    repo.git(&["add", "-A"]);
    repo.git(&["commit", "-qm", "main"]);
    repo.git(&["checkout", "-qb", BRANCH]);
    repo.commit("src/lib.rs", "pub fn t4() {}\n", "feat: t4");

    // When `keeler-branch` runs from the plugin's Justfile, by the real
    // just's absolute path so the stub is reached only from inside a body
    let log = repo.path().join("just-calls");
    let path = std::env::var("PATH").unwrap();
    let out = Command::new(real_just())
        .args(["--justfile", &plugin_justfile()])
        .arg("--working-directory")
        .arg(repo.path())
        .arg("keeler-branch")
        .current_dir(repo.path())
        .env(
            "PATH",
            format!("{}:{path}", repo.path().join("bin").display()),
        )
        .env("KEELER_STUB_JUST_LOG", &log)
        .env("KEELER_REAL_JUST", real_just())
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .expect("failed to run keeler-branch");
    assert!(
        out.status.success(),
        "the branch gate failed — the project's own `dev` may have answered:\n{}",
        said(&out)
    );

    // Then each recorded call named this Justfile and the project as its
    // working directory
    let calls = recorded(&log);
    assert!(!calls.is_empty(), "the branch gate called nothing");
    for call in &calls {
        assert_eq!(
            call.get(..4).map(<[String]>::to_vec).unwrap_or_default(),
            vec![
                "--justfile".to_string(),
                plugin_justfile(),
                "--working-directory".to_string(),
                ".".to_string(),
            ],
            "a self-call did not name the plugin's Justfile: {call:?}"
        );
    }

    // And the recipes it called are dev, crap-delta and mutants-diff, in
    // that order
    let recipes: Vec<String> = calls
        .iter()
        .map(|call| call.last().cloned().unwrap_or_default())
        .collect();
    assert_eq!(
        recipes,
        ["dev", "crap-delta", "mutants-diff"],
        "the branch gate did not run its three gates in order"
    );
}

// ---------------------------------------------------------------------------
// Scenario: Every self-call in the Justfile names its own file
// ---------------------------------------------------------------------------

/// One line of a recipe body: the recipe it belongs to, its 1-based line
/// number, the text, and whether it sits inside a here-doc.
struct BodyLine {
    recipe: String,
    number: usize,
    text: String,
    in_heredoc: bool,
}

/// Whether a top-level line opens a recipe, and under what name. A recipe
/// header is a name at the left margin, its parameters, then a colon —
/// which an assignment (`x := y`), a setting or a comment is not.
fn recipe_header(line: &str) -> Option<String> {
    if line.starts_with(char::is_whitespace) || line.trim().is_empty() {
        return None;
    }
    let (head, _) = line.split_once(':')?;
    if head.contains(":=") || head.contains('#') {
        return None;
    }
    let name = head.split_whitespace().next()?;
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return None;
    }
    Some(name.to_string())
}

/// Every line of every recipe body, with the here-doc it sits inside — if
/// any — already worked out. Nested openers are ignored: what a here-doc
/// holds is text until its own terminator, whatever that text looks like.
fn body_lines(justfile: &str) -> Vec<BodyLine> {
    let mut lines = Vec::new();
    let mut recipe: Option<String> = None;
    let mut terminator: Option<String> = None;
    for (index, text) in justfile.lines().enumerate() {
        let indented = text.starts_with(char::is_whitespace);
        if !indented && !text.trim().is_empty() {
            recipe = recipe_header(text);
            terminator = None;
            continue;
        }
        let Some(name) = recipe.clone() else { continue };
        let closing = terminator.as_ref().is_some_and(|end| text.trim() == end);
        lines.push(BodyLine {
            recipe: name,
            number: index + 1,
            text: text.to_string(),
            in_heredoc: terminator.is_some() && !closing,
        });
        if closing {
            terminator = None;
        } else if terminator.is_none() {
            terminator = heredoc_terminator(text);
        }
    }
    lines
}

/// The word a `<<WORD` here-doc on this line ends at, quoted or not.
fn heredoc_terminator(line: &str) -> Option<String> {
    let rest = line.split_once("<<")?.1.trim_start();
    let rest = rest.strip_prefix('-').unwrap_or(rest);
    let word: String = rest
        .trim_start_matches(['\'', '"'])
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    (!word.is_empty()).then_some(word)
}

/// Where a command can begin: nothing before it, or a shell operator, or a
/// keyword that introduces one.
fn is_command_start(before: &str) -> bool {
    let before = before.trim_end();
    if before.is_empty() {
        return true;
    }
    let last = before.chars().next_back().expect("a non-empty prefix");
    if "|&;({!".contains(last) {
        return true;
    }
    matches!(
        before.split_whitespace().next_back(),
        Some("if" | "then" | "else" | "elif" | "do" | "while" | "until")
    )
}

/// The arguments of every `just` invocation this line starts — an
/// occurrence of the bare word `just` where a command can begin. `just`
/// inside a word (`justfile()`, `--justfile`) or as an argument (`echo
/// just dev`) is not one.
fn self_calls(line: &str) -> Vec<Vec<String>> {
    let mut calls = Vec::new();
    for (at, _) in line.match_indices("just") {
        let before = &line[..at];
        let after = &line[at + "just".len()..];
        if !before
            .chars()
            .next_back()
            .is_none_or(|c| c.is_whitespace() || "|&;({!".contains(c))
        {
            continue;
        }
        if !after.starts_with(char::is_whitespace) || !is_command_start(before) {
            continue;
        }
        calls.push(after.split_whitespace().map(str::to_string).collect());
    }
    calls
}

#[test]
fn every_self_call_in_the_justfile_names_its_own_file() {
    // Given every recipe body in the plugin's Justfile
    let justfile = justfile_text();
    let mut found = 0;

    // When a token `just` starts a command in one
    for line in body_lines(&justfile) {
        for call in self_calls(&line.text) {
            found += 1;

            // Then the next argument names this very file
            assert_eq!(
                call.get(..2).map(<[String]>::to_vec).unwrap_or_default(),
                vec!["--justfile".to_string(), "\"{{justfile()}}\"".to_string()],
                "{} line {} calls `just` without naming this Justfile:\n{}",
                line.recipe,
                line.number,
                line.text.trim()
            );
        }
    }

    // And the scan saw calls at all — a checker that matches nothing
    // passes over a file that has stopped composing recipes entirely
    assert!(
        found >= 8,
        "only {found} self-calls found; the scan is looking in the wrong place"
    );
}

/// Where a `just` call can begin — every shape the Justfile's own recipes
/// use, and the ones a future recipe would reach for.
const COMMAND_STARTS: [&str; 12] = [
    "", "if ", "if ! ", "! ", "then ", "while ", "x=$(", "x=\"$(", "a | ", "a && ", "a || ", "a; ",
];

/// What can follow the recipe name without hiding the call.
const TRAILERS: [&str; 4] = ["", "; then", " 2>/dev/null)", " \"$spec\""];

/// Words that end where a command's arguments begin — a `just` after one
/// of these is text, not a call.
const NOT_STARTS: [&str; 5] = ["echo ", "printf ", "#     ", "bash ", "# run "];

proptest! {
    #![proptest_config(ProptestConfig {
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::WithSource("proptest-regressions"),
        )),
        ..ProptestConfig::default()
    })]

    /// The guard above is only as good as what it can see: a self-call
    /// written into a condition, a pipeline or a command substitution is
    /// the same call, and one the scan must not walk past.
    #[test]
    fn a_self_call_is_seen_wherever_a_command_can_begin(
        indent in 0usize..12,
        start in prop::sample::select(&COMMAND_STARTS[..]),
        recipe in "[a-z][a-z-]{2,20}",
        trailer in prop::sample::select(&TRAILERS[..]),
    ) {
        let line = format!("{}{start}just {recipe}{trailer}", " ".repeat(indent));
        prop_assert_eq!(
            self_calls(&line).len(),
            1,
            "a self-call went unseen in: {}",
            line
        );
    }

    /// And it is a scan for calls, not for the word: the recipe names in
    /// this file's own prose would otherwise all read as self-calls, and
    /// the guard would be a list of exceptions instead of a rule.
    #[test]
    fn the_word_just_as_an_argument_is_not_a_self_call(
        indent in 0usize..12,
        start in prop::sample::select(&NOT_STARTS[..]),
        recipe in "[a-z][a-z-]{2,20}",
    ) {
        let line = format!("{}{start}just {recipe}", " ".repeat(indent));
        prop_assert!(
            self_calls(&line).is_empty(),
            "the word `just` in an argument read as a call: {}",
            line
        );
    }
}

// ---------------------------------------------------------------------------
// Scenario: The graph is read with the script beside the Justfile
// ---------------------------------------------------------------------------

#[test]
fn the_graph_is_read_with_the_script_beside_the_justfile() {
    // Given a git repository with a two-task spec committed on HEAD and no
    // scripts/ directory of its own
    let repo = Repo::new("justfile", "parser-beside");
    repo.commit(
        "specs/01-foo.md",
        &spec(
            "- [ ] **T1 — the root.** Scenarios: _one_.\n\
             - [ ] **T2 — the dependent.** Needs: T1. Scenarios: _two_.\n",
        ),
        "the spec",
    );
    assert!(
        !repo.path().join("scripts").exists(),
        "the fixture has a scripts/ directory of its own"
    );

    // When `keeler-graph` runs from the plugin's Justfile
    let out = keeler(repo.path(), &["keeler-graph", "specs/01-foo.md"]);

    // Then it exits zero
    assert!(
        out.status.success(),
        "the graph was not read at all:\n{}",
        said(&out)
    );

    // And the board reports T1 ready and T2 blocked on T1
    let report = said(&out);
    assert!(
        task_line(&report, "T1").contains("ready"),
        "T1 is not reported ready:\n{report}"
    );
    let blocked = task_line(&report, "T2");
    assert!(
        blocked.contains("blocked") && blocked.contains("T1"),
        "T2 is not reported blocked on T1:\n{report}"
    );
}

// ---------------------------------------------------------------------------
// Scenario: Recipe outputs name the wrapper, not a project recipe
// ---------------------------------------------------------------------------

/// Every recipe the Justfile defines — the names a printed line could
/// wrongly tell someone to run with `just`.
fn recipe_names(justfile: &str) -> Vec<String> {
    justfile.lines().filter_map(recipe_header).collect()
}

/// Whether this line puts text in front of a human: an `echo` or `printf`
/// command, or a line of a here-doc being written to a generated file.
fn prints_text(line: &BodyLine) -> bool {
    line.in_heredoc
        || ["echo", "printf"].iter().any(|command| {
            line.text
                .match_indices(*command)
                .any(|(at, _)| is_command_start(&line.text[..at]))
        })
}

#[test]
fn recipe_outputs_name_the_wrapper_not_a_project_recipe() {
    // Given the plugin's Justfile
    let justfile = justfile_text();
    let recipes = recipe_names(&justfile);

    // Then no line that prints tells a human to run `just <recipe>` —
    // comments and the recipes' own self-calls are out of scope, the
    // second because the scenario above owns them
    for line in body_lines(&justfile) {
        if line.text.trim_start().starts_with('#')
            || !self_calls(&line.text).is_empty()
            || !prints_text(&line)
        {
            continue;
        }
        for recipe in &recipes {
            assert!(
                !line.text.contains(&format!("just {recipe}")),
                "{} line {} tells a human to run `just {recipe}`, which an \
                 adopter has no justfile for:\n{}",
                line.recipe,
                line.number,
                line.text.trim()
            );
        }
    }

    // And the two lines that name the way onward name the wrapper
    assert!(
        justfile.contains(r#"echo "  board:    keeler keeler-status $rel""#),
        "keeler-spawn's board line does not read `  board:    keeler keeler-status <spec>`"
    );
    assert!(
        justfile.contains("KEELER_FAN_OUT_YES=1 keeler keeler-fan-out $spec"),
        "keeler-fan-out's prompt hint does not read \
         `KEELER_FAN_OUT_YES=1 keeler keeler-fan-out <spec>`"
    );
}

// ---------------------------------------------------------------------------
// Scenario: The upgrade recipe is gone
// ---------------------------------------------------------------------------

#[test]
fn the_upgrade_recipe_is_gone() {
    // Given the plugin's Justfile
    // When `just --justfile <plugin>/Justfile --list` runs
    let out = keeler(&repo_root(), &["--list"]);
    assert!(
        out.status.success(),
        "the Justfile does not list at all:\n{}",
        said(&out)
    );

    // Then the output has no `keeler-upgrade` recipe: /plugin update is
    // the upgrade, and a recipe that re-ran install.sh would put the files
    // this spec removed straight back
    assert!(
        !said(&out).contains("keeler-upgrade"),
        "keeler-upgrade is still a recipe:\n{}",
        said(&out)
    );
}
