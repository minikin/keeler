//! Spec 10 — keeler-top. One test per scenario, named after it.
//!
//! The crate's own suite: the board's logic is a library, so its scenarios
//! are driven here rather than through a subprocess. The root harness next
//! door drives shell — `tests/top_recipes.rs` is where the recipe scenarios
//! of this spec live.

use keeler_top::clock::Timestamp;
use keeler_top::run::{RunView, Stage, fold, format_tokens};
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
            Record::Assistant {
                message: serde_json::json!({"id": "m1"}),
                at: None,
            },
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
        vec![Record::Assistant {
            message: serde_json::json!({"id": "m1"}),
            at: None,
        }],
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
        vec![Record::Assistant {
            message: serde_json::json!({"id": "arrival"}),
            at: None,
        }],
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
        view.iter().all(|record| {
            *record
                != Record::Assistant {
                    message: serde_json::json!({"id": "old"}),
                    at: None,
                }
        }),
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
    tool_use_under(None, name, input)
}

/// The same call made by a subagent. The one thing telling it from the
/// main session's is `parent_tool_use_id`, which names the `Task` call that
/// started it — and which sits at the top level of the record, not inside
/// its message.
fn subagent_tool_use(name: &str, input: serde_json::Value) -> String {
    tool_use_under(Some("toolu_the_task_call"), name, input)
}

fn tool_use_under(parent: Option<&str>, name: &str, input: serde_json::Value) -> String {
    let mut record = serde_json::json!({
        "type": "assistant",
        "parent_tool_use_id": parent,
        "message": {
            "id": "m1",
            "content": [{ "type": "tool_use", "id": "toolu_1", "name": name }],
        },
    });
    // Placed rather than written into the literal above: `json!` borrows
    // every value handed to it, and `input` is owned here to be spent.
    record["message"]["content"][0]["input"] = input;
    record.to_string()
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
        tool_use(
            "Read",
            serde_json::json!({ "file_path": in_worktree("keeler.md") }),
        ),
        tool_use(
            "Read",
            serde_json::json!({ "file_path": in_worktree("tests/top.rs") }),
        ),
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
        tool_use(
            "Edit",
            serde_json::json!({ "file_path": in_worktree("tests/top.rs") }),
        ),
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
        tool_use(
            "Bash",
            serde_json::json!({ "command": "just dev 2>&1 | tail -35" }),
        ),
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
        tool_use(
            "Bash",
            serde_json::json!({ "command": "just mutants-diff main" }),
        ),
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
        tool_use(
            "Bash",
            serde_json::json!({ "command": "just keeler-branch 2>&1 | tail -40" }),
        ),
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
            tool_use(
                "Bash",
                serde_json::json!({ "command": "just keeler-branch" }),
            ),
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
            tool_use(
                "Edit",
                serde_json::json!({ "file_path": in_worktree("src/lib.rs") }),
            ),
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
        tool_use(
            "Write",
            serde_json::json!({ "file_path": "/tmp/probe/src/lib.rs" }),
        ),
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
        tool_use(
            "Edit",
            serde_json::json!({ "file_path": in_worktree("Justfile") }),
        ),
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
        tool_use(
            "Bash",
            serde_json::json!({ "command": "grep -c mutants mutants.out" }),
        ),
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
        tool_use(
            "Bash",
            serde_json::json!({ "command": "keeler crap-delta" }),
        ),
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
            tool_use(
                "Edit",
                serde_json::json!({ "file_path": in_worktree("src/run.rs") })
            ),
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
            tool_use(
                "Bash",
                serde_json::json!({ "command": "just mutants-diff main" })
            ),
            Stage::Mutants,
        )),
        Just((
            tool_use(
                "Bash",
                serde_json::json!({ "command": "just keeler-branch" })
            ),
            Stage::Gate,
        )),
        Just((
            tool_use(
                "Read",
                serde_json::json!({ "file_path": in_worktree("keeler.md") })
            ),
            Stage::Reading,
        )),
        Just((
            tool_use(
                "Write",
                serde_json::json!({ "file_path": "/tmp/probe/src/lib.rs" })
            ),
            Stage::Reading,
        )),
        Just((
            subagent_tool_use(
                "Bash",
                serde_json::json!({ "command": "just keeler-branch" })
            ),
            Stage::Reading,
        )),
        Just(("not a record at all".to_string(), Stage::Reading)),
    ]
}

// ── T3

/// The stamp the fixtures' records carry. T2's helpers had no need of one:
/// the stage is read from what a call is, the clock from when it was made.
const STAMP: &str = "2026-09-07T12:00:00.000Z";

/// The board's clock, spelled the way the stream spells a record's.
fn now(stamp: &str) -> Timestamp {
    Timestamp::parse(stamp).expect("the fixture's own timestamp did not parse")
}

/// One assistant record in the shape the CLI writes it: the record carries
/// the stamp and the parent, the message carries its id, its usage and the
/// one content block that record is.
fn record(
    parent: Option<&str>,
    message_id: &str,
    usage: Option<serde_json::Value>,
    block: serde_json::Value,
) -> String {
    let mut record = serde_json::json!({
        "type": "assistant",
        "parent_tool_use_id": parent,
        "timestamp": STAMP,
        "message": { "id": message_id },
    });
    // Placed rather than written into the literal above: `json!` borrows
    // every value handed to it, and both of these are owned here to be spent.
    record["message"]["content"] = serde_json::Value::Array(vec![block]);
    if let Some(usage) = usage {
        record["message"]["usage"] = usage;
    }
    record.to_string()
}

/// A `tool_use` block with an id, so the `tool_result` that closes the call
/// has something to name.
fn call(id: &str, name: &str, input: serde_json::Value) -> serde_json::Value {
    let mut block = serde_json::json!({ "type": "tool_use", "id": id, "name": name });
    block["input"] = input;
    block
}

/// One main-session tool call, stamped [`STAMP`].
fn stamped_tool_use(id: &str, name: &str, input: serde_json::Value) -> String {
    record(None, "m1", None, call(id, name, input))
}

/// The record a tool's answer comes back in. `claude -p` has nobody to type
/// at it, so every `user` record in a run's stream is one of these.
fn tool_result(tool_use_id: &str) -> String {
    serde_json::json!({
        "type": "user",
        "parent_tool_use_id": null,
        "message": { "content": [{ "type": "tool_result", "tool_use_id": tool_use_id }] },
    })
    .to_string()
}

/// A usage whose input side sums to `total`, spread over the three fields
/// the sum is taken from — a board that read only `input_tokens` would show
/// 0% for a session that has been running for an hour.
fn input_usage(total: u64) -> serde_json::Value {
    serde_json::json!({
        "input_tokens": 2,
        "cache_creation_input_tokens": 270,
        "cache_read_input_tokens": total - 272,
        "output_tokens": 0,
    })
}

#[test]
fn the_tool_column_shows_the_last_main_session_tool_call_and_its_command() {
    // Given T1's stream's last main-session tool_use is Bash with command
    // "just dev 2>&1 | tail -35"
    let lines = vec![
        INIT.to_string(),
        stamped_tool_use(
            "toolu_1",
            "Read",
            serde_json::json!({ "file_path": in_worktree("keeler.md") }),
        ),
        stamped_tool_use(
            "toolu_2",
            "Bash",
            serde_json::json!({ "command": "just dev 2>&1 | tail -35" }),
        ),
    ];

    // When the board renders
    // Then T1's tool column reads "Bash: just dev 2>&1 | tail -35"
    assert_eq!(
        fold_stream("tool-last", &lines).tool_column(),
        "Bash: just dev 2>&1 | tail -35",
    );
}

#[test]
fn a_subagents_tool_call_does_not_become_the_rows_tool() {
    // Given T1's main session's last tool_use is Task, and a later tool_use
    // with parent_tool_use_id set is Bash "cargo test"
    let lines = vec![
        INIT.to_string(),
        stamped_tool_use(
            "toolu_1",
            "Task",
            serde_json::json!({ "description": "Explore the crate", "prompt": "…" }),
        ),
        record(
            Some("toolu_1"),
            "m2",
            None,
            call(
                "toolu_2",
                "Bash",
                serde_json::json!({ "command": "cargo test" }),
            ),
        ),
    ];

    // When the board renders
    // Then T1's tool column reads "Task" and its description
    assert_eq!(
        fold_stream("tool-subagent", &lines).tool_column(),
        "Task: Explore the crate",
    );
}

#[test]
fn elapsed_counts_from_the_last_tool_calls_timestamp() {
    // Given T1's last tool_use assistant record is stamped 02:14 before now
    // And no tool_result for its tool_use_id has arrived
    let lines = vec![
        INIT.to_string(),
        stamped_tool_use(
            "toolu_1",
            "Bash",
            serde_json::json!({ "command": "just dev" }),
        ),
    ];

    // When the board renders
    // Then T1's elapsed column reads "02:14"
    assert_eq!(
        fold_stream("elapsed-minutes", &lines).elapsed_column(now("2026-09-07T12:02:14.000Z")),
        "02:14",
    );
}

#[test]
fn elapsed_over_an_hour_shows_hours() {
    // Given T1's last tool_use is stamped 1 hour 2 minutes 5 seconds before
    // now and has not returned
    let lines = vec![
        INIT.to_string(),
        stamped_tool_use(
            "toolu_1",
            "Bash",
            serde_json::json!({ "command": "just mutants-diff" }),
        ),
    ];

    // When the board renders
    // Then T1's elapsed column reads "1:02:05"
    assert_eq!(
        fold_stream("elapsed-hours", &lines).elapsed_column(now("2026-09-07T13:02:05.000Z")),
        "1:02:05",
    );
}

#[test]
fn a_tool_that_returned_shows_no_elapsed_time() {
    // Given T1's last tool_use has a tool_result carrying its tool_use_id
    // later in the stream
    let lines = vec![
        INIT.to_string(),
        stamped_tool_use(
            "toolu_1",
            "Bash",
            serde_json::json!({ "command": "just dev" }),
        ),
        tool_result("toolu_1"),
    ];

    // When the board renders
    // Then T1's elapsed column is empty
    assert_eq!(
        fold_stream("elapsed-returned", &lines).elapsed_column(now("2026-09-07T12:02:14.000Z")),
        "",
    );
}

#[test]
fn a_skill_call_shows_as_skill_and_the_skills_name() {
    // Given the last tool_use is Skill with skill "code-review"
    let lines = vec![
        INIT.to_string(),
        stamped_tool_use(
            "toolu_1",
            "Skill",
            serde_json::json!({ "skill": "code-review" }),
        ),
    ];

    // When the board renders
    // Then the tool column reads "Skill: code-review"
    assert_eq!(
        fold_stream("tool-skill", &lines).tool_column(),
        "Skill: code-review",
    );
}

#[test]
fn the_model_comes_from_the_init_record() {
    // Given T1's stream's init record says model "claude-opus-5[1m]"
    let lines = vec![INIT.to_string()];

    // When the board renders
    // Then T1's model column reads "opus5[1m]"
    assert_eq!(fold_stream("model", &lines).model_column(), "opus5[1m]");
}

#[test]
fn context_is_the_last_main_session_assistant_records_input_over_the_models_window() {
    // Given the model is "claude-opus-5[1m]"
    // And the last main-session assistant record's usage has input_tokens 2,
    // cache_read_input_tokens 121691 and cache_creation_input_tokens 270
    let lines = vec![
        INIT.to_string(),
        record(
            None,
            "m1",
            Some(serde_json::json!({ "input_tokens": 9, "output_tokens": 1 })),
            serde_json::json!({ "type": "text", "text": "an earlier record, long since overtaken" }),
        ),
        record(
            None,
            "m2",
            Some(serde_json::json!({
                "input_tokens": 2,
                "cache_read_input_tokens": 121_691,
                "cache_creation_input_tokens": 270,
                "output_tokens": 4,
            })),
            serde_json::json!({ "type": "text", "text": "the last word" }),
        ),
    ];

    // When the board renders
    // Then T1's context column reads "12%"
    assert_eq!(fold_stream("context", &lines).context_column(), "12%");
}

#[test]
fn a_half_is_rounded_up() {
    // Given the model is "claude-opus-5[1m]" and the usage sums to 115003
    let lines = vec![
        INIT.to_string(),
        record(None, "m1", Some(input_usage(115_003)), text("…")),
    ];

    // When the board renders
    // Then T1's context column reads "12%"
    assert_eq!(fold_stream("round-up", &lines).context_column(), "12%");

    // And a sum of 114999 reads "11%"
    let below = vec![
        INIT.to_string(),
        record(None, "m1", Some(input_usage(114_999)), text("…")),
    ];
    assert_eq!(fold_stream("round-down", &below).context_column(), "11%");
}

#[test]
fn a_subagents_usage_is_not_the_rows_context() {
    // Given the last main-session assistant record sums to 120000, window
    // 1,000,000 — and the last assistant record in the stream has
    // parent_tool_use_id set and a usage sum of 900000
    let lines = vec![
        INIT.to_string(),
        record(None, "m1", Some(input_usage(120_000)), text("mine")),
        record(
            Some("toolu_1"),
            "m2",
            Some(input_usage(900_000)),
            text("the subagent's"),
        ),
    ];

    // When the board renders
    // Then T1's context column reads "12%"
    assert_eq!(
        fold_stream("context-subagent", &lines).context_column(),
        "12%",
    );
}

#[test]
fn a_model_without_the_1m_suffix_has_a_200k_window() {
    // Given the model is "claude-sonnet-5"
    // And the last main-session assistant usage sums to 100000 input-side tokens
    let lines = vec![
        r#"{"type":"system","subtype":"init","model":"claude-sonnet-5"}"#.to_string(),
        record(None, "m1", Some(input_usage(100_000)), text("…")),
    ];

    // When the board renders
    // Then T1's context column reads "50%"
    assert_eq!(fold_stream("window-200k", &lines).context_column(), "50%");
}

#[test]
fn context_at_80_percent_or_more_is_marked() {
    // Given the last main-session assistant usage puts context at 79% of the window
    let below = vec![
        INIT.to_string(),
        record(None, "m1", Some(input_usage(790_000)), text("…")),
    ];

    // When the board renders
    // Then T1's context column reads "79%"
    assert_eq!(fold_stream("context-79", &below).context_column(), "79%");

    // And at 80% it reads "80%!"
    let at_the_mark = vec![
        INIT.to_string(),
        record(None, "m1", Some(input_usage(800_000)), text("…")),
    ];
    assert_eq!(
        fold_stream("context-80", &at_the_mark).context_column(),
        "80%!",
    );
}

#[test]
fn tokens_is_the_runs_output_summed_once_per_message() {
    // Given T1's stream holds three records of one message id with
    // output_tokens 300, and one record of another with 1200 — one message
    // arrives as one record per content block, each carrying the whole
    // message's usage.
    let mut lines = vec![INIT.to_string()];
    for block in 0..3 {
        lines.push(record(
            None,
            "m1",
            Some(serde_json::json!({ "input_tokens": 1, "output_tokens": 300 })),
            text(&format!("block {block}")),
        ));
    }
    lines.push(record(
        None,
        "m2",
        Some(serde_json::json!({ "input_tokens": 1, "output_tokens": 1200 })),
        text("the other message"),
    ));

    // When the board renders
    // Then T1's tokens column reads "1.5k"
    assert_eq!(fold_stream("tokens-dedupe", &lines).tokens_column(), "1.5k");
}

#[test]
fn token_counts_print_in_the_stated_form() {
    // Given output sums of 999, 1000, 12100, 999999 and 12100000
    // When each is formatted
    // Then they read "999", "1.0k", "12.1k", "1.0M" and "12.1M" — the fourth
    // because a value that would round to "1000.0k" prints as "1.0M".
    assert_eq!(format_tokens(999), "999");
    assert_eq!(format_tokens(1_000), "1.0k");
    assert_eq!(format_tokens(12_100), "12.1k");
    assert_eq!(format_tokens(999_999), "1.0M");
    assert_eq!(format_tokens(12_100_000), "12.1M");
}

#[test]
fn a_stream_with_no_assistant_record_yet_shows_dashes() {
    // Given T1's stream holds only the init record
    let view = fold_stream("no-assistant", &[INIT.to_string()]);

    // When the board renders
    // Then T1's context and tokens columns show "—"
    assert_eq!(view.context_column(), "—");
    assert_eq!(view.tokens_column(), "—");

    // And its model column shows the init record's model
    assert_eq!(view.model_column(), "opus5[1m]");
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig {
        cases: 256,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::WithSource("proptest-regressions"),
        )),
        ..proptest::prelude::ProptestConfig::default()
    })]

    /// Given any sequence of stream records, valid or malformed, when they
    /// are folded all at once, and separately in two halves at any split,
    /// neither run panics and both produce the same view.
    ///
    /// The split is a byte index into the file, as T1's is: the fold is fed
    /// by the reader, so a split that cuts a record in half is the one the
    /// board actually has to survive.
    #[test]
    fn any_record_sequence_folds_without_panic_and_in_pieces_as_in_one(
        lines in proptest::collection::vec(fold_line(), 0..10),
        cut in proptest::prelude::any::<usize>(),
    ) {
        let mut bytes = lines.join("\n").into_bytes();
        bytes.push(b'\n');
        let cut = cut % (bytes.len() + 1);

        let whole = Stream::new(&format!("fold-whole-{cut}"));
        whole.write(&bytes);
        let mut whole_view = RunView::default();
        fold_batch(whole.reader().poll(), &mut whole_view);

        let split = Stream::new(&format!("fold-parts-{cut}"));
        split.write(&bytes[..cut]);
        let mut reader = split.reader();
        let mut split_view = RunView::default();
        fold_batch(reader.poll(), &mut split_view);
        split.append(&bytes[cut..]);
        fold_batch(reader.poll(), &mut split_view);

        proptest::prop_assert_eq!(whole_view, split_view);
    }

    /// Given any two usages a and b with a's sum no greater than b's, and
    /// any window, when each is turned into a percentage, both lie in
    /// 0..=100 and a's is no greater than b's.
    #[test]
    fn any_usage_yields_a_context_percentage_within_bounds_and_monotone(
        first in proptest::prelude::any::<u64>(),
        second in proptest::prelude::any::<u64>(),
        window in proptest::prelude::any::<u64>(),
    ) {
        let (lower, upper) = (first.min(second), first.max(second));

        let low = keeler_top::run::percent(lower, window);
        let high = keeler_top::run::percent(upper, window);

        proptest::prop_assert!(low <= 100, "{low} is not a percentage");
        proptest::prop_assert!(high <= 100, "{high} is not a percentage");
        proptest::prop_assert!(
            low <= high,
            "{lower} read {low}% and the larger {upper} read {high}%",
        );
    }

    /// Given any count, when it is formatted, it is at most six characters,
    /// and the number it shows is within half a unit of its last digit from
    /// the count.
    #[test]
    fn any_token_count_formats_within_the_stated_shape(count in proptest::prelude::any::<u64>()) {
        let shown = format_tokens(count);

        proptest::prop_assert!(
            shown.chars().count() <= 6,
            "{count} formatted as {shown}, which is wider than the column",
        );

        let (number, unit) = shown.split_at(
            shown.find(|c: char| c.is_ascii_alphabetic()).unwrap_or(shown.len()),
        );
        let multiplier = match unit {
            "" => 1.0_f64,
            "k" => 1e3,
            "M" => 1e6,
            "G" => 1e9,
            "T" => 1e12,
            "P" => 1e15,
            _ => 1e18,
        };
        let meant = number.parse::<f64>().expect("the number shown does not parse") * multiplier;
        // Half a unit of the last digit: the number carries one decimal
        // above a thousand, so that half-unit is a twentieth of the unit,
        // and below a thousand the count is shown whole.
        let tolerance = if unit.is_empty() { 0.0 } else { multiplier / 20.0 };
        #[expect(
            clippy::cast_precision_loss,
            reason = "the comparison is about the printed value, which is a rounded float already"
        )]
        let count = count as f64;
        proptest::prop_assert!(
            (meant - count).abs() <= tolerance,
            "{count} formatted as {shown}, which means {meant}",
        );
    }
}

/// One text block, the shape the detail pane's ring is filled from.
fn text(body: &str) -> serde_json::Value {
    serde_json::json!({ "type": "text", "text": body })
}

/// The board's use of a batch, folded: a restart throws away everything the
/// run that ended left behind, and the new run's records are folded onto a
/// fresh view.
fn fold_batch(batch: Batch, into: &mut RunView) {
    if batch.restarted {
        *into = RunView::default();
    }
    for record in batch.records {
        fold(into, record, std::path::Path::new(WORKTREE));
    }
}

/// One line of a stream, in the shapes the fold reads: the init record, a
/// tool call, a tool's answer, a text block, a message's usage, a
/// subagent's record — and junk, which is a line the board must survive.
fn fold_line() -> impl proptest::prelude::Strategy<Value = String> {
    use proptest::prelude::{Just, Strategy as _, prop_oneof};
    prop_oneof![
        Just(INIT.to_string()),
        ("toolu_[0-9]", "[a-z]{1,4}").prop_map(|(id, command)| stamped_tool_use(
            &id,
            "Bash",
            serde_json::json!({ "command": format!("just {command}") }),
        )),
        "toolu_[0-9]".prop_map(|id| stamped_tool_use(
            &id,
            "Edit",
            serde_json::json!({ "file_path": in_worktree("src/run.rs") }),
        )),
        "toolu_[0-9]".prop_map(|id| tool_result(&id)),
        ("m[0-9]", proptest::prelude::any::<u32>()).prop_map(|(id, tokens)| record(
            None,
            &id,
            Some(serde_json::json!({
                "input_tokens": tokens,
                "cache_read_input_tokens": tokens,
                "output_tokens": tokens,
            })),
            text("…"),
        )),
        "m[0-9]".prop_map(|id| record(Some("toolu_1"), &id, Some(input_usage(900_000)), text("…"))),
        "[^\n]{0,12}",
    ]
}
