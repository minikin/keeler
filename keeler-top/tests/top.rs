//! Spec 10 — keeler-top. One test per scenario, named after it.
//!
//! The crate's own suite: the board's logic is a library, so its scenarios
//! are driven here rather than through a subprocess. The root harness next
//! door drives shell — `tests/top_recipes.rs` is where the recipe scenarios
//! of this spec live.

use keeler_top::run::{RunView, Stage, fold};
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

// ── T2

/// The worktree `keeler-status` reports for T1 of `specs/01-foo.md`. Every
/// path in a `Write`/`Edit` input is absolute, so this prefix is what says
/// whether an edit was this task's work or something else the agent
/// touched. Nothing here is on disk: stripping a prefix asks the
/// filesystem nothing.
const WORKTREE: &str = "/Users/k/GitHub/keeler-01-foo-t1";

/// One assistant record carrying one `tool_use` block, as the CLI writes
/// it: one record per content block, `parent_tool_use_id` null for the
/// session the board is watching.
fn tool_use(name: &str, input: serde_json::Value) -> String {
    tool_use_under(serde_json::Value::Null, name, input)
}

/// The same call made by a subagent. The one thing telling it from the
/// main session's is `parent_tool_use_id`, which names the `Task` call that
/// started it — and which sits at the top level of the record, not inside
/// its message.
fn subagent_tool_use(name: &str, input: serde_json::Value) -> String {
    tool_use_under(serde_json::json!("toolu_the_task_call"), name, input)
}

fn tool_use_under(parent: serde_json::Value, name: &str, input: serde_json::Value) -> String {
    serde_json::json!({
        "type": "assistant",
        "parent_tool_use_id": parent,
        "message": {
            "id": "m1",
            "content": [{ "type": "tool_use", "id": "toolu_1", "name": name, "input": input }],
        },
    })
    .to_string()
}

/// An absolute path inside T1's worktree, the way the stream spells one.
fn in_worktree(relative: &str) -> String {
    format!("{WORKTREE}/{relative}")
}

/// Every line through the reader and the fold, the way the board does it.
///
/// The parser is part of the answer, not a step before it: a subagent's
/// record is told from the main session's at the top level of the record,
/// so a test that folded hand-built records would be asking a different
/// question from the one the board asks.
fn fold_stream(name: &str, lines: &[String]) -> RunView {
    let stream = Stream::new(name);
    let mut body = lines.join("\n");
    body.push('\n');
    stream.write(body.as_bytes());

    let mut view = RunView::default();
    for record in stream.reader().poll().records {
        fold(&mut view, record, std::path::Path::new(WORKTREE));
    }
    view
}

/// The stage a stream leaves the board on, as the word the column shows.
fn stage_of_stream(name: &str, lines: &[String]) -> String {
    fold_stream(name, lines).stage.to_string()
}

#[test]
fn before_any_signal_the_stage_is_reading() {
    // Given T1's stream holds an init record and Read tool calls only
    let lines = vec![
        INIT.to_string(),
        tool_use("Read", serde_json::json!({ "file_path": in_worktree("keeler.md") })),
        tool_use("Read", serde_json::json!({ "file_path": in_worktree("tests/top.rs") })),
    ];

    // When the board renders
    // Then T1's stage is "reading" — reading a file in the worktree is the
    // one thing the row is named after, and it is not an edit.
    assert_eq!(stage_of_stream("stage-reading", &lines), "reading");
}

#[test]
fn an_edit_under_tests_moves_the_stage_to_tdd() {
    // Given T1's stream holds an Edit tool_use on tests/top.rs
    let lines = vec![
        INIT.to_string(),
        tool_use("Edit", serde_json::json!({ "file_path": in_worktree("tests/top.rs") })),
    ];

    // When the board renders
    // Then T1's stage is "tdd"
    assert_eq!(stage_of_stream("stage-tdd", &lines), "tdd");
}

#[test]
fn a_gate_recipe_moves_the_stage_to_qa() {
    // Given T1's stream holds a Bash tool_use with command "just dev 2>&1 | tail -35"
    let lines = vec![
        INIT.to_string(),
        tool_use("Bash", serde_json::json!({ "command": "just dev 2>&1 | tail -35" })),
    ];

    // When the board renders
    // Then T1's stage is "qa"
    assert_eq!(stage_of_stream("stage-qa", &lines), "qa");
}

#[test]
fn the_code_review_skill_or_a_write_to_the_review_record_means_review() {
    // Given T1's stream holds a Skill tool_use with skill "code-review"
    let skill = vec![
        INIT.to_string(),
        tool_use("Skill", serde_json::json!({ "skill": "code-review" })),
    ];

    // When the board renders
    // Then T1's stage is "review"
    assert_eq!(stage_of_stream("stage-review-skill", &skill), "review");

    // And a Write tool_use on reviews/01-foo/t1.md alone gives the same answer
    let record = vec![
        INIT.to_string(),
        tool_use(
            "Write",
            serde_json::json!({ "file_path": in_worktree("reviews/01-foo/t1.md") }),
        ),
    ];
    assert_eq!(stage_of_stream("stage-review-record", &record), "review");
}

#[test]
fn a_mutants_command_means_mutants() {
    // Given T1's stream holds a Bash tool_use with command "just mutants-diff main"
    let lines = vec![
        INIT.to_string(),
        tool_use("Bash", serde_json::json!({ "command": "just mutants-diff main" })),
    ];

    // When the board renders
    // Then T1's stage is "mutants"
    assert_eq!(stage_of_stream("stage-mutants", &lines), "mutants");
}

#[test]
fn a_keeler_branch_command_sets_the_stage_to_gate() {
    // Given T1's stream holds a Bash tool_use whose command starts with "just keeler-branch"
    let lines = vec![
        INIT.to_string(),
        tool_use("Bash", serde_json::json!({ "command": "just keeler-branch 2>&1 | tail -40" })),
    ];

    // When the board renders
    // Then T1's stage is "gate"
    assert_eq!(stage_of_stream("stage-gate", &lines), "gate");
}

#[test]
fn the_stage_does_not_move_backwards() {
    // Given T1's stream holds a Skill code-review followed by a Bash "just dev"
    let lines = vec![
        INIT.to_string(),
        tool_use("Skill", serde_json::json!({ "skill": "code-review" })),
        tool_use("Bash", serde_json::json!({ "command": "just dev" })),
    ];

    // When the board renders
    // Then T1's stage is still "review"
    assert_eq!(stage_of_stream("stage-monotone", &lines), "review");
}

#[test]
fn a_keeler_skill_call_if_one_appears_names_its_stage() {
    // Given T1's stream holds a Skill tool_use with skill "keeler:mutants"
    let lines = vec![
        INIT.to_string(),
        tool_use("Skill", serde_json::json!({ "skill": "keeler:mutants" })),
    ];

    // When the board renders
    // Then T1's stage is "mutants"
    assert_eq!(stage_of_stream("stage-keeler-skill", &lines), "mutants");
}

#[test]
fn a_subagents_tool_calls_do_not_move_the_stage() {
    // Given T1's stream holds a Bash "just dev" whose record has
    // parent_tool_use_id set, and nothing else
    let lines = vec![
        INIT.to_string(),
        subagent_tool_use("Bash", serde_json::json!({ "command": "just dev" })),
    ];

    // When the board renders
    // Then T1's stage is "reading"
    assert_eq!(stage_of_stream("stage-subagent", &lines), "reading");
}

#[test]
fn a_task_with_an_exit_file_shows_ended_as_its_stage() {
    // Given T1's .exit file holds 0 and keeler-status says passed — a run
    // that reached its gate and came back green.
    let mut passed = fold_stream(
        "stage-ended-passed",
        &[
            INIT.to_string(),
            tool_use("Bash", serde_json::json!({ "command": "just keeler-branch" })),
        ],
    );
    passed.ended();

    // And T2's .exit file holds 1 and keeler-status says failed (exit 1).
    // What the file holds never reaches the board: `ended` takes no
    // argument, so 0 and 1 cannot answer differently.
    let mut failed = fold_stream(
        "stage-ended-failed",
        &[
            INIT.to_string(),
            tool_use("Edit", serde_json::json!({ "file_path": in_worktree("src/lib.rs") })),
        ],
    );
    failed.ended();

    // When the board renders
    // Then both stages read "ended"
    assert_eq!(passed.stage.to_string(), "ended");
    assert_eq!(failed.stage.to_string(), "ended");
}

#[test]
fn an_edit_outside_the_worktree_is_not_tdd() {
    // Given T1's stream holds a Write tool_use on /tmp/probe/src/lib.rs and nothing else
    let lines = vec![
        INIT.to_string(),
        tool_use("Write", serde_json::json!({ "file_path": "/tmp/probe/src/lib.rs" })),
    ];

    // When the board renders
    // Then T1's stage is "reading"
    assert_eq!(stage_of_stream("stage-outside", &lines), "reading");
}

#[test]
fn an_edit_to_the_worktrees_justfile_is_tdd() {
    // Given T1's stream holds an Edit tool_use on <worktree>/Justfile —
    // t10 of spec 09 spent its whole red-green cycle here, and a rule that
    // knew only src/ and tests/ would have shown "reading" throughout.
    let lines = vec![
        INIT.to_string(),
        tool_use("Edit", serde_json::json!({ "file_path": in_worktree("Justfile") })),
    ];

    // When the board renders
    // Then T1's stage is "tdd"
    assert_eq!(stage_of_stream("stage-justfile", &lines), "tdd");
}

#[test]
fn a_command_that_merely_mentions_mutants_is_not_the_mutants_stage() {
    // Given T1's stream holds a Bash tool_use with command "grep -c mutants mutants.out"
    let lines = vec![
        INIT.to_string(),
        tool_use("Bash", serde_json::json!({ "command": "grep -c mutants mutants.out" })),
    ];

    // When the board renders
    // Then T1's stage is "reading"
    assert_eq!(stage_of_stream("stage-mentions-mutants", &lines), "reading");
}

#[test]
fn crap_delta_is_qa() {
    // Given T1's stream holds a Bash tool_use with command "keeler crap-delta"
    let lines = vec![
        INIT.to_string(),
        tool_use("Bash", serde_json::json!({ "command": "keeler crap-delta" })),
    ];

    // When the board renders
    // Then T1's stage is "qa" — half of keeler-branch, run on its own, is
    // the qa stage and not the gate.
    assert_eq!(stage_of_stream("stage-crap-delta", &lines), "qa");
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig {
        cases: 256,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::WithSource("proptest-regressions"),
        )),
        ..proptest::prelude::ProptestConfig::default()
    })]

    /// Given any sequence of stage signals, when they are folded, every
    /// intermediate stage is no earlier than the one before it.
    ///
    /// And, since a fold that never moved at all would satisfy that on its
    /// own, the stage the sequence ends on is the furthest signal in it —
    /// which is the same rule stated from the other side.
    #[test]
    fn any_stage_sequence_never_moves_backwards(
        signals in proptest::collection::vec(stage_signal(), 0..12),
    ) {
        let stream = Stream::new("stage-sequence");
        let mut body = format!("{INIT}\n");
        for (line, _) in &signals {
            body.push_str(line);
            body.push('\n');
        }
        stream.write(body.as_bytes());

        let mut view = RunView::default();
        let mut previous = Stage::default();
        for record in stream.reader().poll().records {
            fold(&mut view, record, std::path::Path::new(WORKTREE));
            proptest::prop_assert!(
                view.stage >= previous,
                "the stage went from {previous} back to {}",
                view.stage,
            );
            previous = view.stage;
        }

        let furthest = signals
            .iter()
            .map(|(_, stage)| *stage)
            .max()
            .unwrap_or_default();
        proptest::prop_assert_eq!(view.stage, furthest);
    }
}

/// One stage signal and the stage it stands for: the five a tool call can
/// name, and the four that name none — a read, an edit somewhere else, a
/// subagent's gate run, and a line that is not a record at all.
fn stage_signal() -> impl proptest::prelude::Strategy<Value = (String, Stage)> {
    use proptest::prelude::{Just, prop_oneof};
    prop_oneof![
        Just((
            tool_use("Edit", serde_json::json!({ "file_path": in_worktree("src/run.rs") })),
            Stage::Tdd,
        )),
        Just((
            tool_use("Bash", serde_json::json!({ "command": "keeler dev" })),
            Stage::Qa,
        )),
        Just((
            tool_use("Skill", serde_json::json!({ "skill": "code-review" })),
            Stage::Review,
        )),
        Just((
            tool_use("Bash", serde_json::json!({ "command": "just mutants-diff main" })),
            Stage::Mutants,
        )),
        Just((
            tool_use("Bash", serde_json::json!({ "command": "just keeler-branch" })),
            Stage::Gate,
        )),
        Just((
            tool_use("Read", serde_json::json!({ "file_path": in_worktree("keeler.md") })),
            Stage::Reading,
        )),
        Just((
            tool_use("Write", serde_json::json!({ "file_path": "/tmp/probe/src/lib.rs" })),
            Stage::Reading,
        )),
        Just((
            subagent_tool_use("Bash", serde_json::json!({ "command": "just keeler-branch" })),
            Stage::Reading,
        )),
        Just(("not a record at all".to_string(), Stage::Reading)),
    ]
}
