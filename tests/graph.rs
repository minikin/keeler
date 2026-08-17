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
        std::process::Command::new("bash")
            .arg(repo_root().join("scripts/keeler-graph.sh"))
            .arg(&self.0)
            .output()
            .expect("failed to run keeler-graph.sh")
    }
}

impl std::ops::Deref for Spec {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
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
        assert!(
            said.contains("line"),
            "{name}: the refusal does not name the line:\n{said}"
        );
        assert!(
            !stdout(&output).contains("ready"),
            "{name}: something was reported ready from a malformed section",
        );
    }
}

#[test]
fn this_spec_is_its_own_fixture() {
    // Given specs/06-graph-mode.md, which describes the format
    let output = std::process::Command::new("bash")
        .arg(repo_root().join("scripts/keeler-graph.sh"))
        .arg(repo_root().join("specs/06-graph-mode.md"))
        .output()
        .unwrap();
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
}
