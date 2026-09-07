//! Spec 10 — keeler-top. One test per scenario, named after it.
//!
//! The crate's own suite: the board's logic is a library, so its scenarios
//! are driven here rather than through a subprocess. The root harness next
//! door drives shell — `tests/top_recipes.rs` is where the recipe scenarios
//! of this spec live.

use keeler_top::stream::{Batch, Record, StreamReader};

// ── T1

/// A run's opening record, in the shape `claude -p --output-format
/// stream-json` writes it.
const INIT: &str =
    r#"{"type":"system","subtype":"init","model":"claude-opus-5[1m]","session_id":"s"}"#;

/// One assistant record, distinguishable by the message id it carries —
/// the property below compares whole records, so two of them must be
/// tellable apart.
fn assistant(id: &str) -> String {
    format!(r#"{{"type":"assistant","parent_tool_use_id":null,"message":{{"id":"{id}"}}}}"#)
}

/// A stream file in its own directory, removed on drop. Named after the
/// test that owns it, so two tests never share one.
struct Stream {
    dir: std::path::PathBuf,
    path: std::path::PathBuf,
}

impl Stream {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("keeler-top-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t1.stream");
        Self { dir, path }
    }

    /// Writes `bytes` over whatever was there, the way the runner's `tee`
    /// does on every resume.
    fn write(&self, bytes: &[u8]) {
        std::fs::write(&self.path, bytes).unwrap();
    }

    fn append(&self, bytes: &[u8]) {
        use std::io::Write as _;
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&self.path)
            .unwrap();
        file.write_all(bytes).unwrap();
    }

    /// Overwrites the first `bytes.len()` bytes in place, leaving the
    /// file's length untouched. A reader that re-read what it had already
    /// consumed would see this; one that reads only new bytes cannot.
    fn scribble_over_the_head(&self, bytes: &[u8]) {
        use std::io::{Seek as _, SeekFrom, Write as _};
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .open(&self.path)
            .unwrap();
        file.seek(SeekFrom::Start(0)).unwrap();
        file.write_all(bytes).unwrap();
    }

    fn reader(&self) -> StreamReader {
        StreamReader::new(&self.path)
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// The board's use of a batch: a restart throws away everything folded from
/// the run that ended, and the new run's records are folded onto nothing.
fn accumulate(batch: Batch, into: &mut Vec<Record>) {
    if batch.restarted {
        into.clear();
    }
    into.extend(batch.records);
}

#[test]
fn a_malformed_stream_line_is_skipped() {
    // Given T1's stream holds a line that is not JSON between two valid records
    let stream = Stream::new("malformed");
    stream.write(format!("{INIT}\nnot json at all\n{}\n", assistant("m1")).as_bytes());

    // When the board renders
    let batch = stream.reader().poll();

    // Then the two valid records are counted and the board shows no error
    assert_eq!(
        batch.records,
        vec![
            Record::Init {
                model: "claude-opus-5[1m]".to_string()
            },
            Record::Assistant(serde_json::json!({"id": "m1"})),
        ],
    );
    assert!(!batch.restarted, "a malformed line was read as a new run");
}

#[test]
fn a_half_written_last_line_waits_for_its_rest() {
    // Given T1's stream ends mid-record without a newline
    let stream = Stream::new("half-written");
    let tail = assistant("m1");
    let (head, rest) = tail.split_at(tail.len() / 2);
    stream.write(format!("{INIT}\n{head}").as_bytes());
    let mut reader = stream.reader();

    // When the board renders
    let first = reader.poll();

    // Then the partial line is not parsed
    assert_eq!(
        first.records,
        vec![Record::Init {
            model: "claude-opus-5[1m]".to_string()
        }],
    );

    // And it is parsed on the refresh after the newline arrives
    stream.append(format!("{rest}\n").as_bytes());
    assert_eq!(
        reader.poll().records,
        vec![Record::Assistant(serde_json::json!({"id": "m1"}))],
    );
}

#[test]
fn only_the_streams_new_bytes_are_read_on_refresh() {
    // Given T1's stream is 1 MB and the board has already parsed it
    let stream = Stream::new("new-bytes");
    let filler = assistant("filler");
    let mut written = format!("{INIT}\n");
    while written.len() < 1_000_000 {
        written.push_str(&filler);
        written.push('\n');
    }
    stream.write(written.as_bytes());
    let mut reader = stream.reader();
    let already = reader.poll().records.len();
    assert!(already > 1, "the fixture parsed nothing to speak of");

    // The head is replaced by junk of the same length: a reader that went
    // back over it would find no records there, and the length is untouched
    // so nothing looks like a new run.
    stream.scribble_over_the_head(&b"x".repeat(written.len() / 2));

    // When 200 bytes are appended
    let mut arrival = format!("{}\n", assistant("arrival"));
    arrival.push_str(&"#".repeat(200 - arrival.len() - 1));
    arrival.push('\n');
    assert_eq!(arrival.len(), 200);
    stream.append(arrival.as_bytes());

    // Then the refresh reads those bytes and no earlier ones
    let batch = reader.poll();
    assert_eq!(
        batch.records,
        vec![Record::Assistant(serde_json::json!({"id": "arrival"}))],
    );
    assert!(!batch.restarted, "an append was read as a new run");
}

#[test]
fn a_resumed_tasks_stream_is_read_from_the_start() {
    // Given the board has parsed 1 MB of T1's stream
    let stream = Stream::new("resumed");
    let filler = assistant("old");
    let mut written = format!("{INIT}\n");
    while written.len() < 1_000_000 {
        written.push_str(&filler);
        written.push('\n');
    }
    stream.write(written.as_bytes());
    let mut reader = stream.reader();
    let mut view = Vec::new();
    accumulate(reader.poll(), &mut view);
    assert!(view.len() > 1, "the fixture parsed nothing to speak of");

    // When the file is replaced by one 4 kB long
    let mut resumed = format!("{INIT}\n");
    let fresh = assistant("new");
    while resumed.len() < 4_000 {
        resumed.push_str(&fresh);
        resumed.push('\n');
    }
    stream.write(resumed.as_bytes());

    // Then the board discards what it parsed and reads the new file from offset 0
    let batch = reader.poll();
    assert!(batch.restarted, "the replacement was read as an append");
    accumulate(batch, &mut view);
    assert_eq!(
        view.first(),
        Some(&Record::Init {
            model: "claude-opus-5[1m]".to_string()
        }),
    );
    assert!(
        view.iter()
            .all(|record| *record != Record::Assistant(serde_json::json!({"id": "old"}))),
        "the run that ended is still in the view",
    );
    // And the row shows the new run's stage and tool — every record of the
    // new file, and only those.
    assert_eq!(view.len(), resumed.lines().count());
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig {
        cases: 256,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::WithSource("proptest-regressions"),
        )),
        ..proptest::prelude::ProptestConfig::default()
    })]

    /// Given any byte sequence of newline-separated records, when it is fed
    /// to the reader whole, and again in two pieces split at any byte, the
    /// records produced are identical.
    ///
    /// "Produced" is the view the board keeps, not the raw batches: a
    /// restart is the reader saying *discard what you had*, so a split that
    /// puts a resume's init in the second piece rebuilds the same view by a
    /// different route. The split is a byte index, not a character one —
    /// the file arrives as bytes and a cut through a multi-byte character
    /// is a cut the reader has to survive.
    #[test]
    fn any_byte_stream_reads_the_same_at_any_split(
        lines in proptest::collection::vec(stream_line(), 0..8),
        newline_terminated in proptest::prelude::any::<bool>(),
        cut in proptest::prelude::any::<usize>(),
    ) {
        let mut bytes = lines.join("\n").into_bytes();
        if newline_terminated && !bytes.is_empty() {
            bytes.push(b'\n');
        }
        let cut = cut % (bytes.len() + 1);

        let whole = Stream::new(&format!("split-whole-{cut}"));
        whole.write(&bytes);
        let mut whole_view = Vec::new();
        accumulate(whole.reader().poll(), &mut whole_view);

        let split = Stream::new(&format!("split-parts-{cut}"));
        split.write(&bytes[..cut]);
        let mut reader = split.reader();
        let mut split_view = Vec::new();
        accumulate(reader.poll(), &mut split_view);
        split.append(&bytes[cut..]);
        accumulate(reader.poll(), &mut split_view);

        proptest::prop_assert_eq!(whole_view, split_view);
    }
}

/// The binary the recipe will run, before it has a board to show. It is
/// here so the crate has the shape the rest of the spec builds on; what it
/// must not do meanwhile is exit zero having shown nobody anything.
#[test]
fn the_binary_refuses_rather_than_reporting_a_board_it_did_not_draw() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_keeler-top"))
        .output()
        .expect("failed to run the keeler-top binary");

    assert!(!output.status.success(), "an empty board exited zero");
    assert!(output.stdout.is_empty(), "a refusal went to stdout");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("keeler-top"),
        "the refusal does not name the command that made it",
    );
}

/// One line of a stream: the record shapes the board reads, plus the junk
/// it has to ignore — a truncated write, a log line that got in, an empty
/// line.
fn stream_line() -> impl proptest::prelude::Strategy<Value = String> {
    use proptest::prelude::{Just, Strategy as _, prop_oneof};
    prop_oneof![
        Just(INIT.to_string()),
        "[a-z]{1,4}".prop_map(|id| assistant(&id)),
        Just(r#"{"type":"user","message":{"content":[{"type":"tool_result"}]}}"#.to_string()),
        Just(r#"{"type":"result","subtype":"success"}"#.to_string()),
        "[^\n]{0,12}",
    ]
}

// ── T4

use keeler_top::dispatch::{Dispatch as _, Shell};
use keeler_top::git::branch_facts;
use keeler_top::graph::{GraphLine, GraphState};
use std::path::{Path, PathBuf};

/// The plugin's own tree: this crate is a member of it, so the Justfile
/// and `scripts/keeler-graph.sh` the board shells back to are one
/// directory up. The scenario that reads a graph reads it through the
/// shipped parser, not a description of it.
fn plugin_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crate is a workspace member")
        .to_path_buf()
}

/// A spec with two tasks, the second needing the first — the smallest
/// graph in which a tick changes anybody's answer.
fn spec_text(t1_ticked: bool) -> String {
    let ticked = if t1_ticked { "x" } else { " " };
    format!(
        "# Spec 01 — foo\n\n**Status:** Approved\n\n## Tasks\n\n\
         - [{ticked}] **T1 — the root.** Scenarios: _one_.\n\
         - [ ] **T2 — the dependent.** Needs: T1. Scenarios: _two_.\n"
    )
}

/// A synthetic project: the repository, and room beside it for the
/// worktrees a wave cuts — `keeler-spawn` puts them next to the
/// repository, and the board reads them there.
struct Project(PathBuf);

impl Project {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("keeler-top-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("repo")).unwrap();
        let project = Self(dir);
        project.git(&project.root(), &["init", "-qb", "main"]);
        project
    }

    fn root(&self) -> PathBuf {
        self.0.join("repo")
    }

    /// git with a fixed identity and no user config, so a global
    /// `commit.gpgsign` cannot hang the suite waiting for a key.
    fn git(&self, dir: &Path, args: &[&str]) -> String {
        let output = std::process::Command::new("git")
            .args(["-c", "user.email=probe@keeler", "-c", "user.name=probe"])
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .output()
            .expect("failed to run git");
        assert!(
            output.status.success(),
            "git {args:?} failed:\n{}",
            String::from_utf8_lossy(&output.stderr),
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    fn write(&self, dir: &Path, rel: &str, body: &str) {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }

    fn commit(&self, dir: &Path, rel: &str, body: &str, message: &str) {
        self.write(dir, rel, body);
        self.git(dir, &["add", rel]);
        self.git(dir, &["commit", "-qm", message]);
    }

    /// The worktree and branch a spawn cuts, from wherever HEAD is.
    fn worktree(&self, branch: &str) -> PathBuf {
        let path = self.0.join(branch.replace('/', "-"));
        self.git(
            &self.root(),
            &["worktree", "add", "-q", "-b", branch, path.to_str().unwrap()],
        );
        path
    }

    /// The spec, committed on `feat/01-foo`, which is where every read of
    /// the graph goes.
    fn with_spec_on_the_feature_branch(&self) -> &Self {
        self.commit(
            &self.root(),
            "specs/01-foo.md",
            &spec_text(false),
            "docs(01-foo): the approved spec",
        );
        self.git(&self.root(), &["checkout", "-qb", "feat/01-foo"]);
        self
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        // The worktrees hold no locks worth unwinding — the repository
        // they belong to goes with them in the same call.
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn the_graph_is_read_from_the_ref_not_the_working_tree() {
    // Given T1's box is ticked in the working tree but not committed on feat/01-foo
    let project = Project::new("graph-from-ref");
    project.with_spec_on_the_feature_branch();
    project.write(&project.root(), "specs/01-foo.md", &spec_text(true));

    // When the board renders
    let shell = Shell::new(plugin_root(), project.root(), "specs/01-foo.md");
    let graph = keeler_top::graph::read(&shell, &project.root(), "feat/01-foo", "specs/01-foo.md")
        .expect("the spec is committed on feat/01-foo");

    // Then T1 is not done, and T2 (Needs: T1) reads "blocked ← T1"
    assert_eq!(
        graph,
        vec![
            GraphLine {
                id: "T1".to_string(),
                state: GraphState::Ready,
                needs: Vec::new(),
            },
            GraphLine {
                id: "T2".to_string(),
                state: GraphState::Blocked,
                needs: vec!["T1".to_string()],
            },
        ],
        "the working tree's tick was counted — the one place in graph mode where it must not be",
    );
}

#[test]
fn the_board_shells_back_through_the_justfile_it_was_launched_from() {
    // Given the binary was launched with plugin root P
    let project = Project::new("shells-back");
    project.with_spec_on_the_feature_branch();
    let plugin = project.0.join("plugin");
    // A plugin tree of its own, so the answers below can only have come
    // from it: a board that ran the project's own recipes, or the real
    // plugin's, would say something else.
    project.write(
        &plugin,
        "Justfile",
        "keeler-status SPEC:\n    @echo \"graph: {{SPEC}} on feat/01-foo\"\n    @pwd -P\n",
    );
    project.write(
        &plugin,
        "scripts/keeler-graph.sh",
        "#!/usr/bin/env bash\nprintf '%s\\n' \"$1\" > \"$(dirname \"$0\")/../argv\"\n\
         printf 'T1 ready\\nT2 blocked T1\\n'\n",
    );
    let shell = Shell::new(&plugin, project.root(), "specs/01-foo.md");

    // When it refreshes state
    // Then it runs `just --justfile P/Justfile --working-directory <root> keeler-status <spec>`
    let command = shell.status_command();
    assert_eq!(command.get_program(), "just");
    assert_eq!(
        command.get_args().collect::<Vec<_>>(),
        [
            "--justfile".as_ref(),
            plugin.join("Justfile").as_os_str(),
            "--working-directory".as_ref(),
            project.root().as_os_str(),
            "keeler-status".as_ref(),
            "specs/01-foo.md".as_ref(),
        ],
    );
    let report = shell.status().expect("the fixture recipe answers");
    assert!(
        report.contains("graph: specs/01-foo.md on feat/01-foo"),
        "the recipe that answered was not P's:\n{report}",
    );
    let ran_in = std::fs::canonicalize(project.root()).unwrap();
    assert!(
        report.contains(&format!("{}\n", ran_in.display())),
        "the recipe ran somewhere other than the project root:\n{report}",
    );

    // And `bash P/scripts/keeler-graph.sh` for the graph
    let spec = project.root().join("specs/01-foo.md");
    let command = shell.graph_command(&spec);
    assert_eq!(command.get_program(), "bash");
    assert_eq!(
        command.get_args().collect::<Vec<_>>(),
        [
            plugin.join("scripts/keeler-graph.sh").as_os_str(),
            spec.as_os_str(),
        ],
    );
    let graph = shell.graph(&spec).expect("the fixture script answers");
    assert_eq!(graph, "T1 ready\nT2 blocked T1\n");
    assert_eq!(
        std::fs::read_to_string(plugin.join("argv")).unwrap().trim(),
        spec.to_str().unwrap(),
        "the script was not handed the spec to read",
    );
}

#[test]
fn the_commit_column_is_the_branchs_head_and_its_distance_from_the_feature_branch() {
    // Given keeler/01-foo/t1 is three commits ahead of feat/01-foo, head f5064b1
    let project = Project::new("commit-column");
    project.with_spec_on_the_feature_branch();
    let worktree = project.worktree("keeler/01-foo/t1");
    for step in ["T1 red", "T1 green", "T1 the review record"] {
        project.commit(&worktree, "src/t1.rs", step, &format!("test(01-foo): {step}"));
    }

    // When the board renders
    let facts = branch_facts(&worktree, "feat/01-foo").expect("the branch and its base are there");

    // Then T1's commit column reads "f5064b1 +3"
    assert_eq!(
        facts.head,
        project.git(&worktree, &["rev-parse", "--short", "HEAD"]),
    );
    assert_eq!(facts.ahead, 3);
    assert_eq!(facts.dirty, 0);
    // The same read carries what the detail pane lists: the branch's own
    // commits, newest first.
    assert_eq!(
        facts
            .commits
            .iter()
            .map(|commit| commit.subject.as_str())
            .collect::<Vec<_>>(),
        [
            "test(01-foo): T1 the review record",
            "test(01-foo): T1 green",
            "test(01-foo): T1 red",
        ],
    );
    assert_eq!(facts.commits[0].hash, facts.head);
}

#[test]
fn uncommitted_changes_in_the_worktree_are_counted() {
    // Given T1's worktree has two modified files and one untracked
    let project = Project::new("dirty");
    project.with_spec_on_the_feature_branch();
    let worktree = project.worktree("keeler/01-foo/t1");
    project.commit(&worktree, "src/a.rs", "one\n", "feat(01-foo): a");
    project.commit(&worktree, "src/b.rs", "two\n", "feat(01-foo): b");
    project.write(&worktree, "src/a.rs", "one, changed\n");
    project.write(&worktree, "src/b.rs", "two, changed\n");
    project.write(&worktree, "scratch", "not added\n");

    // When the board renders
    let facts = branch_facts(&worktree, "feat/01-foo").expect("the branch and its base are there");

    // Then T1's commit column ends with "3 dirty"
    assert_eq!(facts.dirty, 3);
    assert_eq!(facts.ahead, 2, "the commits were miscounted alongside it");
}

#[test]
fn a_clean_worktree_on_the_base_commit_shows_the_base() {
    // Given keeler/01-foo/t1 equals feat/01-foo and the worktree is clean
    let project = Project::new("on-the-base");
    project.with_spec_on_the_feature_branch();
    let base = project.git(&project.root(), &["rev-parse", "--short", "feat/01-foo"]);
    let worktree = project.worktree("keeler/01-foo/t1");

    // When the board renders
    let facts = branch_facts(&worktree, "feat/01-foo").expect("the branch and its base are there");

    // Then T1's commit column reads the base's short hash and "+0"
    assert_eq!(facts.head, base);
    assert_eq!(facts.ahead, 0);
    assert_eq!(facts.dirty, 0);
    assert!(facts.commits.is_empty());
}
