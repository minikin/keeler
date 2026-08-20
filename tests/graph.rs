//! Spec 06 — graph mode. The reading side: `scripts/keeler-graph.sh`.
//!
//! The parser is shell, driven here as a subprocess the way `install.sh`
//! is. Every test hands it a spec file and reads back one line per task:
//! `<id> <state> [needs...]`, where state is `ready`, `blocked` or `done`.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Output;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// A spec file in its own temp directory, removed on drop.
struct Spec(PathBuf);

impl Spec {
    fn new(name: &str, body: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("keeler-graph-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("{name}.md"));
        std::fs::write(&path, body).unwrap();
        Self(path)
    }

    fn graph(&self) -> Output {
        graph(&self.0)
    }
}

/// Runs the parser on any spec path — the repository's own included,
/// which must not go through `Spec`, whose Drop removes the parent dir.
fn graph(spec: &Path) -> Output {
    std::process::Command::new("bash")
        .arg(repo_root().join("scripts/keeler-graph.sh"))
        .arg(spec)
        .output()
        .expect("failed to run keeler-graph.sh")
}

impl Drop for Spec {
    fn drop(&mut self) {
        if let Some(dir) = self.0.parent() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// The parsed report as `(id, state, needs)`.
fn report(output: &Output) -> Vec<(String, String, Vec<String>)> {
    stdout(output)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let mut parts = line.split_whitespace();
            let id = parts.next().unwrap().to_string();
            let state = parts.next().unwrap().to_string();
            (id, state, parts.map(str::to_string).collect())
        })
        .collect()
}

const FRAME: &str = "# Spec 99 — fixture\n\n**Status:** Approved\n\n## Tasks\n\n";

fn spec(tasks: &str) -> String {
    format!("{FRAME}{tasks}\n---\n\n## Implementation Notes\n\nprose that mentions T1 freely.\n")
}

#[test]
fn the_tasks_stage_emits_a_dependency_graph() {
    // Given a spec whose Tasks section carries ids and needs, some of them
    // wrapped across lines the way a real spec wraps them
    let fixture = Spec::new(
        "wrapped",
        &spec(
            "- [x] **T1 — the root.** Scenarios: _one_. Tests: unit.\n\
             - [ ] **T2 — leans on the root.** Needs: T1. Scenarios: _two_.\n\
             - [ ] **T3 — leans on both, with its dependency list on the\n      \
             second line.** Scenarios: _three_.\n      Needs: T1, T2. Tests: acceptance.\n\
             - [ ] **T4 — another root, no needs at all.**\n",
        ),
    );

    // When the graph is read
    let output = fixture.graph();
    assert!(output.status.success(), "{}", stderr(&output));

    // Then every task carries an id, its needs, and its state — and the
    // needs reference only ids defined in the same spec
    let got = report(&output);
    assert_eq!(
        got,
        vec![
            ("T1".into(), "done".into(), vec![]),
            ("T2".into(), "ready".into(), vec!["T1".into()]),
            (
                "T3".into(),
                "blocked".into(),
                vec!["T1".into(), "T2".into()]
            ),
            ("T4".into(), "ready".into(), vec![]),
        ],
    );
}

#[test]
fn only_the_tasks_section_is_read() {
    // Given a spec whose Implementation Notes quote a task line as an
    // example — exactly what specs/06-graph-mode.md does
    let fixture = Spec::new(
        "boundary",
        &format!(
            "{}\n---\n\n## Implementation Notes\n\nThe line looks like:\n\n\
             ```\n- [ ] **T9 — an example, not a task.** Needs: T1.\n```\n",
            spec("- [ ] **T1 — real.**\n").trim_end_matches(
                "\n---\n\n## Implementation Notes\n\nprose that mentions T1 freely.\n"
            ),
        ),
    );

    // When the graph is read
    let output = fixture.graph();
    assert!(output.status.success(), "{}", stderr(&output));

    // Then the example is not a task
    let ids: Vec<String> = report(&output).into_iter().map(|(id, _, _)| id).collect();
    assert_eq!(
        ids,
        vec!["T1".to_string()],
        "the parser read outside the section"
    );
}

#[test]
fn a_malformed_tasks_section_is_refused_naming_the_line() {
    // Given three ways a Tasks section can be wrong
    let cases = [
        (
            "unknown-need",
            "- [ ] **T1 — a.**\n- [ ] **T2 — b.** Needs: T7.\n",
            "T7",
        ),
        (
            "duplicate-id",
            "- [ ] **T1 — a.**\n- [ ] **T1 — a again.**\n",
            "T1",
        ),
        (
            "two-needs",
            "- [ ] **T1 — a.**\n- [ ] **T2 — b.** Needs: T1. Needs: T1.\n",
            "Needs",
        ),
    ];

    for (name, tasks, expected) in cases {
        let fixture = Spec::new(name, &spec(tasks));

        // When the graph is read
        let output = fixture.graph();

        // Then it fails naming the line and what is wrong, and reports
        // nothing as ready
        assert!(
            !output.status.success(),
            "{name}: a malformed section was accepted"
        );
        let said = stderr(&output);
        assert!(
            said.contains(expected),
            "{name}: the refusal does not name `{expected}`:\n{said}"
        );
        // FRAME is six lines, so the offending item — always the second —
        // sits on line 8. "line" alone is true of every refusal by
        // construction and would let a wrong number through.
        assert!(
            said.contains("line 8"),
            "{name}: the refusal names the wrong line:\n{said}"
        );
        assert!(
            stdout(&output).is_empty(),
            "{name}: a refusal printed a report:\n{}",
            stdout(&output),
        );
    }
}

#[test]
fn what_markdown_reads_as_part_of_an_item_is_part_of_it() {
    // Given a Needs: on a lazy continuation line — unindented, which
    // markdown still renders as part of the item, and which an editor
    // reflow produces
    let fixture = Spec::new(
        "lazy",
        &spec("- [ ] **T1 — a.**\n- [ ] **T2 — b.** Scenarios: _foo_.\nNeeds: T1.\n"),
    );

    // When the graph is read
    let output = fixture.graph();
    assert!(output.status.success(), "{}", stderr(&output));

    // Then the dependency is not lost. Dropping the line silently would
    // report a ready root and fan T2 out in parallel with the task it
    // needs — the exact conflict the graph exists to prevent.
    let got = report(&output);
    assert_eq!(got[1], ("T2".into(), "blocked".into(), vec!["T1".into()]));
}

#[test]
fn an_uppercase_tick_is_a_tick_and_an_unknown_checkbox_is_refused() {
    // Given `- [X]`, which GitHub renders exactly as `- [x]`
    let fixture = Spec::new(
        "upper",
        &spec("- [X] **T1 — a.**\n- [ ] **T2 — b.** Needs: T1.\n"),
    );
    let output = fixture.graph();
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        report(&output)[0].1,
        "done",
        "an uppercase X was not read as ticked"
    );
    assert_eq!(report(&output)[1].1, "ready");

    // And a checkbox line the grammar does not know is refused, not
    // dropped — a task that vanishes from the graph is never spawned and
    // never counted at land time
    let fixture = Spec::new("star", &spec("- [ ] **T1 — a.**\n* [ ] **T2 — b.**\n"));
    let output = fixture.graph();
    assert!(
        !output.status.success(),
        "an unrecognised checkbox line was silently dropped"
    );
    assert!(stderr(&output).contains("line 8"), "{}", stderr(&output));
}

#[test]
fn a_spec_with_no_tasks_section_is_refused_not_empty() {
    // Given a spec with no `## Tasks` at all
    let missing = Spec::new("no-section", "# Spec\n\n## Context\n\nnothing here\n");
    let output = missing.graph();
    assert!(
        !output.status.success(),
        "a spec with no Tasks section read as zero tasks"
    );
    assert!(stderr(&output).contains("Tasks"), "{}", stderr(&output));

    // And one whose heading is hidden by CRLF endings — a Windows edit or
    // an autocrlf checkout — still parses
    let crlf = Spec::new("crlf", &spec("- [ ] **T1 — a.**\n").replace('\n', "\r\n"));
    let output = crlf.graph();
    assert!(
        output.status.success(),
        "CRLF endings hid the section:\n{}",
        stderr(&output)
    );
    assert_eq!(report(&output)[0].0, "T1");
}

#[test]
fn a_needs_list_is_validated_not_guessed_at() {
    // Given the plausible slips: a lowercase token, a stray file name in
    // the list, a self-need, a duplicate need
    let cases = [
        (
            "lowercase",
            "- [ ] **T1 — a.**\n- [ ] **T2 — b.** needs: T1.\n",
            "needs",
        ),
        (
            "stray-token",
            "- [ ] **T1 — a.**\n- [ ] **T2 — b.** Needs: T1, T2.md.\n",
            "T2.md",
        ),
        (
            "self-need",
            "- [ ] **T1 — a.**\n- [ ] **T2 — b.** Needs: T2.\n",
            "itself",
        ),
        (
            "duplicate-need",
            "- [ ] **T1 — a.**\n- [ ] **T2 — b.** Needs: T1, T1.\n",
            "twice",
        ),
    ];
    for (name, tasks, expected) in cases {
        let fixture = Spec::new(name, &spec(tasks));
        let output = fixture.graph();
        // Then each is refused naming the problem, never read as a
        // smaller graph than the one written
        assert!(
            !output.status.success(),
            "{name}: read as a graph nobody wrote"
        );
        assert!(
            stderr(&output).contains(expected),
            "{name}:\n{}",
            stderr(&output)
        );
    }
}

#[test]
fn a_hash_inside_a_fence_is_a_comment_not_a_heading() {
    // Given a fenced example in the section's prose whose lines begin
    // with `#` — a shell comment, which is what an example usually is
    let fixture = Spec::new(
        "fenced-hash",
        &spec(
            "Example:\n\n```\n# how to read it\njust keeler-graph specs/01-foo.md\n```\n\n- [ ] **T1 — a.**\n- [ ] **T2 — b.** Needs: T1.\n",
        ),
    );
    let output = fixture.graph();
    assert!(output.status.success(), "{}", stderr(&output));

    // Then the section did not end at it: inside a fence, `#` is text
    let ids: Vec<String> = report(&output).into_iter().map(|(id, _, _)| id).collect();
    assert_eq!(
        ids,
        vec!["T1".to_string(), "T2".to_string()],
        "a comment in a fenced example truncated the section"
    );
}

#[test]
fn a_sub_heading_inside_the_section_does_not_end_it() {
    // Given a Tasks section organised in phases, as a long spec would be
    let fixture = Spec::new(
        "phases",
        &spec("### Phase 1\n\n- [ ] **T1 — a.**\n\n### Phase 2\n\n- [ ] **T2 — b.** Needs: T1.\n"),
    );
    let output = fixture.graph();
    assert!(output.status.success(), "{}", stderr(&output));

    // Then both tasks are read. A deeper heading is structure *within*
    // the section, and truncating there loses tasks silently — the worst
    // shape this failure can take, because a graph that reports fewer
    // tasks than exist reports every one of them done, and keeler-land
    // marks a spec Implemented on that.
    let ids: Vec<String> = report(&output).into_iter().map(|(id, _, _)| id).collect();
    assert_eq!(
        ids,
        vec!["T1".to_string(), "T2".to_string()],
        "a sub-heading truncated the section"
    );
}

#[test]
fn any_heading_ends_the_section_not_only_a_level_two_one() {
    // Given a checkbox under a later heading of any level — an appendix,
    // a backlog, a note someone kept
    let fixture = Spec::new(
        "appendix",
        &spec("- [ ] **T1 — a.**\n\n# Appendix\n\n- [ ] **T9 — not a task.**\n"),
    );
    let output = fixture.graph();
    assert!(output.status.success(), "{}", stderr(&output));

    // Then it is not a task: the section ended at the heading
    let ids: Vec<String> = report(&output).into_iter().map(|(id, _, _)| id).collect();
    assert_eq!(
        ids,
        vec!["T1".to_string()],
        "a checkbox under a later heading was read as a task"
    );
}

#[test]
fn a_fenced_example_inside_the_section_is_not_a_task() {
    // Given an example task line quoted in a code fence in the intro
    // prose of the Tasks section — where the next example will be pasted
    let fixture = Spec::new(
        "fenced",
        &spec("Example:\n\n```\n- [ ] **T9 — example.** Needs: T1.\n```\n\n- [ ] **T1 — a.**\n"),
    );
    let output = fixture.graph();
    assert!(output.status.success(), "{}", stderr(&output));
    let ids: Vec<String> = report(&output).into_iter().map(|(id, _, _)| id).collect();
    assert_eq!(
        ids,
        vec!["T1".to_string()],
        "the fenced example was read as a task"
    );
}

#[test]
fn a_fenced_example_before_the_section_does_not_open_it() {
    // Given a spec whose Context quotes the format — a fenced block
    // holding a `## Tasks` heading and an example item — ahead of the
    // real Tasks section
    let fixture = Spec::new(
        "fenced-before",
        "# Spec 99 — fixture\n\n**Status:** Approved\n\n## Context\n\n\
         The Tasks section looks like this:\n\n\
         ```markdown\n## Tasks\n\n- [x] **T1 — example done.** Scenarios: _none_.\n```\n\n\
         ## Tasks\n\n\
         - [ ] **T1 — the real first task.** Scenarios: _one_.\n\
         - [ ] **T2 — the real second task.** Needs: T1. Scenarios: _two_.\n",
    );

    // When the graph is read
    let output = fixture.graph();
    assert!(output.status.success(), "{}", stderr(&output));

    // Then the quoted heading opened nothing: the graph is the real
    // section's. Reading the example instead reports one ticked task
    // where two unstarted ones live, and `keeler-land` marks a spec
    // Implemented on exactly that.
    assert_eq!(
        report(&output),
        vec![
            ("T1".to_string(), "ready".to_string(), vec![]),
            (
                "T2".to_string(),
                "blocked".to_string(),
                vec!["T1".to_string()]
            ),
        ],
        "a fenced example ahead of the section was read as the graph"
    );
}

#[test]
fn a_fence_left_open_is_refused_rather_than_swallowing_the_rest() {
    // Given a Tasks section with a fence someone never closed
    let fixture = Spec::new(
        "fence-unclosed",
        &spec(
            "- [x] **T1 — done.** Scenarios: _one_.\n\n\
             ```\nan example nobody closed\n\n\
             - [ ] **T2 — never seen.** Scenarios: _two_.\n\
             - [ ] **T3 — never seen either.** Scenarios: _three_.\n",
        ),
    );

    // When the graph is read
    let output = fixture.graph();

    // Then it is refused naming the fence, and nothing is reported —
    // ending at the fence in silence reports one done task and no
    // others, which is a finished spec to everything downstream
    assert!(
        !output.status.success(),
        "an unclosed fence was accepted:\n{}",
        stdout(&output)
    );
    let said = stderr(&output);
    assert!(
        said.contains("fence") && said.contains("line 9"),
        "the refusal did not name the fence and its line: {said}"
    );
    assert!(
        stdout(&output).is_empty(),
        "a refusal printed a report:\n{}",
        stdout(&output)
    );
}

#[test]
fn a_link_bullet_is_not_a_malformed_checkbox() {
    // Given an ordinary markdown link bullet in the Tasks section's prose
    let fixture = Spec::new(
        "link-bullet",
        &spec(
            "- [the design note](notes/design.md) explains the shape.\n\n\
             - [ ] **T1 — a task.** Scenarios: _one_.\n",
        ),
    );

    // When the graph is read
    let output = fixture.graph();

    // Then it is read as prose, not refused as a checkbox the grammar
    // cannot parse: a refusal is total, so one link would block
    // keeler-graph, keeler-spawn, keeler-status and keeler-land for the
    // whole spec
    assert!(
        output.status.success(),
        "a link bullet was refused as a checkbox: {}",
        stderr(&output)
    );
    assert_eq!(
        report(&output),
        vec![("T1".to_string(), "ready".to_string(), vec![])],
        "the link bullet disturbed the graph"
    );
}

#[test]
fn this_spec_is_its_own_fixture() {
    // Given specs/06-graph-mode.md, which describes the format
    let output = graph(&repo_root().join("specs/06-graph-mode.md"));
    assert!(output.status.success(), "{}", stderr(&output));

    // Then it parses to exactly the graph its Tasks section draws
    let got: Vec<(String, Vec<String>)> = report(&output)
        .into_iter()
        .map(|(id, _, needs)| (id, needs))
        .collect();
    let want: Vec<(String, Vec<String>)> = [
        ("T1", vec![]),
        ("T2", vec!["T1"]),
        ("T3", vec!["T1"]),
        ("T4", vec!["T1"]),
        ("T5", vec!["T1"]),
        ("T6", vec!["T5"]),
        ("T7", vec!["T1"]),
        ("T8", vec!["T2", "T3", "T4", "T6", "T7"]),
        ("T9", vec!["T8"]),
        ("T10", vec!["T9"]),
        ("T11", vec!["T3"]),
        ("T12", vec!["T11"]),
        ("T13", vec!["T12"]),
        ("T14", vec!["T13"]),
        ("T15", vec!["T14"]),
    ]
    .into_iter()
    .map(|(id, needs)| {
        (
            id.to_string(),
            needs.into_iter().map(str::to_string).collect(),
        )
    })
    .collect();
    assert_eq!(got, want);
}

#[test]
fn no_shipped_file_still_teaches_that_readiness_comes_from_main() {
    // Every one of these is installed into an adopter's project, and each
    // is read by an agent as instruction. A sentence left behind by an
    // amendment does not merely go stale — it teaches the model the model
    // the tools no longer implement, and the first thing the adopter hits
    // is a refusal their own rules told them could not happen.
    let shipped = [
        ".claude/keeler.md",
        ".claude/commands/keeler/graph.md",
        ".claude/commands/keeler/mutants.md",
        "KEELER.md",
    ];
    let mut stale = Vec::new();
    for name in shipped {
        let text = std::fs::read_to_string(repo_root().join(name)).unwrap();
        for (n, line) in text.lines().enumerate() {
            // Wherever a line says where readiness comes from, it must
            // name the feature branch. Matching "main" anywhere would
            // catch lines that mention main for other true reasons — the
            // baseline, `Status:` — and a gate that fires on correct text
            // gets edited until it stops firing.
            let lower = line.to_lowercase();
            if lower.contains("readiness is read from") && !line.contains("feat/") {
                stale.push(format!("{name}:{}: {}", n + 1, line.trim()));
            }
        }
    }
    assert!(
        stale.is_empty(),
        "these shipped files still say readiness is read from main:\n{}",
        stale.join("\n")
    );

    // And the rules must name the refusal an adopter meets first
    let rules = std::fs::read_to_string(repo_root().join(".claude/keeler.md")).unwrap();
    assert!(
        rules.contains("feat/<spec-slug>"),
        ".claude/keeler.md does not say which branch the graph is read from"
    );
}

#[test]
fn the_template_and_the_tasks_command_carry_the_format() {
    // Given what /keeler:tasks reads to learn the format
    let template = std::fs::read_to_string(repo_root().join("specs/TEMPLATE.md")).unwrap();
    let command =
        std::fs::read_to_string(repo_root().join(".claude/commands/keeler/tasks.md")).unwrap();

    // Then both show a task line with Needs:, and the command carries the
    // rule that two tasks editing one region are not independent
    assert!(
        template.contains("Needs:"),
        "TEMPLATE.md does not show the Needs: annotation"
    );
    assert!(
        command.contains("Needs:"),
        "tasks.md does not tell the stage to emit Needs:"
    );
    assert!(
        command.contains("same region") || command.contains("same lines"),
        "tasks.md does not carry the same-region rule",
    );
    for hot in ["Cargo.lock", "Justfile"] {
        assert!(
            command.contains(hot),
            "tasks.md does not name `{hot}` as a hot file"
        );
    }
}

// ---- T2: /keeler:graph reads readiness ------------------------------------
//
// The human's road to the graph is `just keeler-graph <spec>`, and the
// command file sends the agent down the same road rather than letting it
// read the Tasks section itself. These tests drive the recipe.

/// Runs `just keeler-graph <spec>` from the repository root, the way a
/// human — or /keeler:graph — would.
fn just_graph(spec: &Path) -> Output {
    std::process::Command::new("just")
        .arg("keeler-graph")
        .arg(spec)
        .current_dir(repo_root())
        .output()
        .expect("failed to run just")
}

/// The report line that opens with `<id> `, or a panic naming what was
/// printed instead.
fn line_for<'a>(out: &'a str, id: &str) -> &'a str {
    out.lines()
        .find(|line| line.starts_with(&format!("{id} ")))
        .unwrap_or_else(|| panic!("no line for {id} in:\n{out}"))
}

#[test]
fn graph_status_names_what_is_unblocked() {
    // Given a spec with tasks T1..T4 where T2 and T3 need T1, and T1 is
    // checked off — and T4 needs all three, so one of its needs is met
    // and two are not
    let fixture = Spec::new(
        "unblocked",
        &spec(
            "- [x] **T1 — root.**\n\
             - [ ] **T2 — leans on T1.** Needs: T1.\n\
             - [ ] **T3 — also leans on T1.** Needs: T1.\n\
             - [ ] **T4 — leans on all of them.** Needs: T1, T2, T3.\n",
        ),
    );

    // When the graph recipe runs against the spec
    let output = just_graph(&fixture.0);
    assert!(output.status.success(), "{}", stderr(&output));
    let out = stdout(&output);

    // Then it reports T2 and T3 as ready and T4 as blocked with its unmet
    // needs — T2 and T3, not the T1 that is already done
    assert!(line_for(&out, "T1").contains("done"), "{out}");
    assert!(line_for(&out, "T2").contains("ready"), "{out}");
    assert!(line_for(&out, "T3").contains("ready"), "{out}");
    let t4 = line_for(&out, "T4");
    assert!(t4.contains("blocked"), "{out}");
    assert!(
        t4.contains("T2") && t4.contains("T3"),
        "T4's unmet needs are not named: {t4}"
    );
    assert!(
        !t4.contains("T1"),
        "T4's line names T1, which is done — a met need is not an unmet one: {t4}"
    );

    // And it reports nothing as ready when the graph is complete
    let complete = Spec::new(
        "complete",
        &spec(
            "- [x] **T1 — root.**\n\
             - [x] **T2 — leans on T1.** Needs: T1.\n\
             - [x] **T3 — also leans on T1.** Needs: T1.\n\
             - [x] **T4 — leans on all of them.** Needs: T1, T2, T3.\n",
        ),
    );
    let output = just_graph(&complete.0);
    assert!(output.status.success(), "{}", stderr(&output));
    let out = stdout(&output);
    assert!(
        !out.contains("ready"),
        "a complete graph reported something as ready:\n{out}"
    );
    assert_eq!(
        out.lines().filter(|line| line.contains("done")).count(),
        4,
        "a complete graph does not report every task as done:\n{out}"
    );
}

#[test]
fn a_cycle_is_refused_loudly() {
    // Given a spec whose Tasks section contains T1 needing T2 and T2
    // needing T1 — and a third task that needs neither, so a report that
    // leaked would have something to call ready
    let fixture = Spec::new(
        "cycle",
        &spec(
            "- [ ] **T1 — a.** Needs: T2.\n\
             - [ ] **T2 — b.** Needs: T1.\n\
             - [ ] **T3 — c.**\n",
        ),
    );

    // When the graph recipe runs against the spec
    let output = just_graph(&fixture.0);

    // Then it fails naming the cycle
    assert!(!output.status.success(), "a cycle was read as a graph");
    let said = stderr(&output);
    assert!(
        said.contains("cycle"),
        "the refusal does not say cycle:\n{said}"
    );
    assert!(
        said.contains("T1") && said.contains("T2"),
        "the refusal does not name the tasks in the cycle:\n{said}"
    );

    // And no task is reported as ready
    let out = stdout(&output);
    assert!(
        !out.contains("ready"),
        "a refused graph still reported something as ready:\n{out}"
    );
    assert!(out.is_empty(), "a refusal printed a report:\n{out}");
}

#[test]
fn the_graph_command_runs_the_recipe_rather_than_reading_the_spec() {
    // Given the shipped /keeler:graph command
    let command = std::fs::read_to_string(repo_root().join(".claude/commands/keeler/graph.md"))
        .expect("cannot read .claude/commands/keeler/graph.md");

    // Then it instructs running the recipe — so a cycle is refused by a
    // program and not judged by an agent — and reports the three states
    assert!(
        command.contains("just keeler-graph"),
        "graph.md does not instruct running `just keeler-graph`"
    );
    for state in ["ready", "blocked", "done"] {
        assert!(
            command.contains(state),
            "graph.md does not tell the agent to report what is {state}"
        );
    }
    assert!(
        command.contains("cycle"),
        "graph.md does not say what to do when the recipe refuses a cycle"
    );
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig {
        cases: 32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::WithSource("proptest-regressions"),
        )),
        ..proptest::prelude::ProptestConfig::default()
    })]

    /// Over any acyclic graph — edges only from later ids to earlier ones,
    /// which is what makes it acyclic by construction — every task is
    /// reported exactly once, and its state follows from its needs and
    /// its checkbox: done if ticked, ready if every need is done, blocked
    /// otherwise.
    #[test]
    fn every_task_is_reported_exactly_once_with_the_state_its_needs_imply(
        n in 1usize..7,
        edges in proptest::collection::vec((0usize..7, 0usize..7), 0..12),
        ticked in proptest::collection::vec(proptest::bool::ANY, 7),
    ) {
        let mut needs: Vec<Vec<usize>> = vec![Vec::new(); n];
        for (a, b) in edges {
            let (a, b) = (a % n, b % n);
            if b < a && !needs[a].contains(&b) {
                needs[a].push(b);
            }
        }
        let mut body = String::new();
        for i in 0..n {
            let _ = write!(
                body,
                "- [{}] **T{} — task {}.**",
                if ticked[i] { 'x' } else { ' ' }, i + 1, i + 1,
            );
            if !needs[i].is_empty() {
                let list: Vec<String> = needs[i].iter().map(|j| format!("T{}", j + 1)).collect();
                let _ = write!(body, " Needs: {}.", list.join(", "));
            }
            body.push('\n');
        }
        let fixture = Spec::new("property", &spec(&body));

        let output = fixture.graph();
        proptest::prop_assert!(output.status.success(), "{}", stderr(&output));
        let got = report(&output);
        proptest::prop_assert_eq!(got.len(), n, "some task was reported twice or not at all");

        for (i, (id, state, _)) in got.iter().enumerate() {
            proptest::prop_assert_eq!(id, &format!("T{}", i + 1));
            let expected = if ticked[i] {
                "done"
            } else if needs[i].iter().all(|j| ticked[*j]) {
                "ready"
            } else {
                "blocked"
            };
            proptest::prop_assert_eq!(state, expected, "T{} with needs {:?}", i + 1, needs[i]);
        }
    }

    /// Over any graph that contains a cycle — T1 -> T2 -> ... -> Tc -> T1
    /// for some c ≥ 2, plus whatever other edges and ticks — the spec is
    /// refused naming a cycle, and nothing is reported. Not only the
    /// two-task cycle the scenario draws: a cycle of any length, buried
    /// under any number of honest edges, and ticked or not.
    #[test]
    fn a_graph_with_a_cycle_anywhere_in_it_is_refused(
        n in 2usize..7,
        cycle_len in 2usize..7,
        edges in proptest::collection::vec((0usize..7, 0usize..7), 0..12),
        ticked in proptest::collection::vec(proptest::bool::ANY, 7),
    ) {
        let cycle_len = cycle_len.min(n);
        let mut needs: Vec<Vec<usize>> = vec![Vec::new(); n];
        for (i, list) in needs.iter_mut().enumerate().take(cycle_len) {
            list.push((i + 1) % cycle_len);
        }
        for (a, b) in edges {
            let (a, b) = (a % n, b % n);
            if a != b && !needs[a].contains(&b) {
                needs[a].push(b);
            }
        }
        let mut body = String::new();
        for i in 0..n {
            let list: Vec<String> = needs[i].iter().map(|j| format!("T{}", j + 1)).collect();
            let _ = write!(
                body,
                "- [{}] **T{} — task {}.**",
                if ticked[i] { 'x' } else { ' ' }, i + 1, i + 1,
            );
            if !list.is_empty() {
                let _ = write!(body, " Needs: {}.", list.join(", "));
            }
            body.push('\n');
        }
        let fixture = Spec::new("cyclic-property", &spec(&body));

        let output = fixture.graph();
        proptest::prop_assert!(!output.status.success(), "a cyclic graph was accepted:\n{}", stdout(&output));
        let said = stderr(&output);
        proptest::prop_assert!(said.contains("cycle") && said.contains("->"), "{said}");
        proptest::prop_assert!(stdout(&output).is_empty(), "a refusal printed a report:\n{}", stdout(&output));
    }
}
