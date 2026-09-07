//! Reading a run's `.stream` while it is still being written.
//!
//! The runner tees `claude -p --output-format stream-json` into
//! `.keeler/runs/<slug>/<tid>.stream`, one JSON record per line. Two things
//! about that file shape everything here:
//!
//! - it grows under the reader, so the board reads only the bytes that
//!   arrived since it last looked, and the last line of a read is as often
//!   as not half-written;
//! - it is teed with `tee`, not `tee -a`, so a resume truncates it and
//!   starts a new run in the same path. A file shorter than what has been
//!   read is that, and so is a second `init` record — the record a run
//!   opens with, arriving in a file the reader is already partway through.

use std::io::{Read as _, Seek as _, SeekFrom};
use std::path::PathBuf;

use crate::clock::Timestamp;

/// One record of a run's stream, in the four shapes the board tells apart.
///
/// The payloads of the middle two are the records' `message` objects,
/// unread: the fold that takes the stage, the tool, the clock and the usage
/// out of them arrives with the tasks that need them.
#[derive(Debug, Clone, PartialEq)]
pub enum Record {
    /// The record a run opens with, naming the model it was started with —
    /// and, because there is exactly one per run, marking where a run
    /// begins.
    Init {
        /// The model as the init record spells it, `[1m]` suffix and all.
        model: String,
    },
    /// One content block of one assistant message.
    Assistant {
        /// The record's `message`: its id, its usage and the one content
        /// block this record is.
        message: serde_json::Value,
        /// When the record was written, from the record itself rather than
        /// from its message — the CLI stamps the envelope, and the API's
        /// message object has no such field.
        at: Option<Timestamp>,
    },
    /// A tool's answer coming back. `claude -p` has nobody to type at it,
    /// so every `user` record in the stream is one of these.
    ToolResult(serde_json::Value),
    /// A record the board does not read — a `result`, a system record that
    /// is not the init one, anything a later version of the CLI adds, and
    /// anything a subagent said.
    Other,
}

impl Record {
    /// Whether this record is where a run begins.
    #[must_use]
    pub fn is_init(&self) -> bool {
        matches!(self, Self::Init { .. })
    }
}

/// The stream as it is written, before it is read for what it means.
///
/// A separate type from [`Record`] so the shapes the CLI emits and the
/// shapes the board reads can drift apart without either one bending to
/// the other.
#[derive(serde::Deserialize)]
#[serde(tag = "type")]
enum Wire {
    #[serde(rename = "system")]
    System {
        subtype: String,
        // Defaulted rather than required: an init record with no model is
        // still the start of a run, and the restart rule cares about that
        // far more than any column cares about the name.
        #[serde(default)]
        model: String,
        #[serde(default)]
        parent_tool_use_id: Option<String>,
    },
    #[serde(rename = "assistant")]
    Assistant {
        message: serde_json::Value,
        // The one field that tells a subagent's record from the main
        // session's, and it is here rather than in `message`: it is the
        // record that belongs to a `Task` call, not the message.
        #[serde(default)]
        parent_tool_use_id: Option<String>,
        // Kept as it was written and parsed on the way out: a record whose
        // stamp is missing or in some shape this does not read is a record
        // with no clock, not a record to throw away.
        #[serde(default)]
        timestamp: Option<String>,
    },
    #[serde(rename = "user")]
    User {
        message: serde_json::Value,
        #[serde(default)]
        parent_tool_use_id: Option<String>,
    },
    #[serde(other)]
    Other,
}

impl From<Wire> for Record {
    fn from(wire: Wire) -> Self {
        match wire {
            // The main session's own account of itself, and only it. A run
            // spawns subagents, and their records share the stream with
            // `parent_tool_use_id` set — a third of t10's, on the run this
            // spec was written against. Matching the null here rather than
            // filtering in the fold is what makes "only the main session
            // counts" true of every record shape at once, instead of a rule
            // the stage, the tool, the texts, the usage and the restart
            // signal each remember separately.
            Wire::System {
                subtype,
                model,
                parent_tool_use_id: None,
            } if subtype == "init" => Self::Init { model },
            Wire::Assistant {
                message,
                parent_tool_use_id: None,
                timestamp,
            } => Self::Assistant {
                message,
                at: timestamp.as_deref().and_then(Timestamp::parse),
            },
            Wire::User {
                message,
                parent_tool_use_id: None,
            } => Self::ToolResult(message),
            Wire::System { .. } | Wire::Assistant { .. } | Wire::User { .. } | Wire::Other => {
                Self::Other
            }
        }
    }
}

/// What one [`StreamReader::poll`] found.
#[derive(Debug, Default, PartialEq)]
pub struct Batch {
    /// Set when the stream was replaced by a new run: whatever was folded
    /// from the run that ended is no longer about this file, and must be
    /// discarded before these records are folded onto it.
    pub restarted: bool,
    /// The records this poll read, in stream order.
    pub records: Vec<Record>,
}

/// An incremental reader over one run's `.stream`.
#[derive(Debug)]
pub struct StreamReader {
    path: PathBuf,
    /// How much of the file has been handed to [`Self::consume`] — the
    /// place the next read starts, not the last complete record.
    offset: u64,
    /// The tail of what has been read that had no newline after it. A
    /// record is only parsed once its line is whole.
    partial: Vec<u8>,
    /// Whether the run being read has shown its init record yet.
    seen_init: bool,
}

impl StreamReader {
    /// A reader that has read nothing of `path` yet. The file need not
    /// exist: a task that never started has no stream, and one that is
    /// about to start has none for a moment either.
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            offset: 0,
            partial: Vec::new(),
            seen_init: false,
        }
    }

    /// Reads whatever has arrived since the last call.
    ///
    /// A stream that cannot be read at all — no file yet, no permission —
    /// is an empty batch and not an error: the board's answer for a task
    /// with no stream is the state alone, and it can say that every second
    /// without anything to say about it.
    pub fn poll(&mut self) -> Batch {
        let Ok(length) = std::fs::metadata(&self.path).map(|file| file.len()) else {
            return Batch::default();
        };
        let mut restarted = false;
        if length < self.offset {
            self.rewind();
            restarted = true;
        }
        let had_init = self.seen_init;
        let mut records = self.consume();
        // The second signal: a run that begins in a file the reader is
        // already partway through. An offset into the run that ended says
        // nothing about where the new one is, so the whole file is read
        // again rather than the tail of it.
        //
        // Note what this does *not* catch, though the spec's prose says it
        // does: a resume that truncated and then wrote past the old offset
        // before the board looked. Its init record sits below the offset,
        // so it is never among the records read here. Only the length
        // comparison above catches that resume, and only while the new run
        // is still shorter than the old one was — which, at a poll a
        // second, it always is in practice.
        if had_init && records.iter().any(Record::is_init) {
            self.rewind();
            records = self.consume();
            restarted = true;
        }
        self.seen_init |= records.iter().any(Record::is_init);
        Batch { restarted, records }
    }

    /// Back to the top of the file, with nothing remembered of the run that
    /// was there. `seen_init` goes too: a truncation the board catches
    /// before the new run's init line is written must not then treat that
    /// init — the first of its run — as a second one and restart again.
    fn rewind(&mut self) {
        self.offset = 0;
        self.partial.clear();
        self.seen_init = false;
    }

    /// Reads from the offset to the end of the file and parses every whole
    /// line it now has, keeping the rest for the next call.
    fn consume(&mut self) -> Vec<Record> {
        let mut fresh = Vec::new();
        // The error is dropped rather than branched on, because there is
        // nothing to decide: whatever `read_to_end` appended before it gave
        // up is this file's bytes at this offset, and a read that got
        // nowhere leaves `fresh` empty and the offset where it was. Either
        // way the next poll starts from what was actually read.
        let _ = std::fs::File::open(&self.path).and_then(|mut file| {
            file.seek(SeekFrom::Start(self.offset))?;
            file.read_to_end(&mut fresh)
        });
        self.offset += fresh.len() as u64;
        self.partial.append(&mut fresh);

        // Split at the last newline rather than walking an index forward
        // one line at a time: everything before it is whole lines and
        // everything after it is the tail that has no newline yet. The
        // hand-rolled walk said the same thing, and the mutation gate
        // showed what it cost — every arithmetic slip in the advance is an
        // infinite loop rather than a wrong answer, which a test suite can
        // only report as a hang.
        let Some(end) = self.partial.iter().rposition(|byte| *byte == b'\n') else {
            return Vec::new();
        };
        let records = self.partial[..=end]
            .split(|byte| *byte == b'\n')
            .filter_map(parse_line)
            .collect();
        self.partial.drain(..=end);
        records
    }
}

/// One line into one record, or nothing.
///
/// Nothing covers three things the board must survive without a word: a
/// line that is not UTF-8 (a half-written multi-byte character that landed
/// before its newline somehow), a line that is not JSON, and a line that is
/// JSON but not a record. All three are the same answer — skip it — because
/// the alternative is a board that reports on itself instead of on the run.
fn parse_line(line: &[u8]) -> Option<Record> {
    let line = std::str::from_utf8(line).ok()?;
    serde_json::from_str::<Wire>(line).ok().map(Record::from)
}

#[cfg(test)]
mod tests {
    use super::{Record, StreamReader};

    const INIT: &str = r#"{"type":"system","subtype":"init","model":"claude-opus-5[1m]"}"#;

    fn init() -> Record {
        Record::Init {
            model: "claude-opus-5[1m]".to_string(),
        }
    }

    /// A stream file of its own, removed on drop.
    struct Fixture(std::path::PathBuf);

    impl Fixture {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("keeler-top-unit-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn path(&self) -> std::path::PathBuf {
            self.0.join("t1.stream")
        }

        fn write(&self, body: &str) {
            std::fs::write(self.path(), body).unwrap();
        }

        fn reader(&self) -> StreamReader {
            StreamReader::new(self.path())
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_stream_that_is_not_there_is_an_empty_batch_and_not_an_error() {
        let fixture = Fixture::new("absent");

        assert_eq!(fixture.reader().poll(), super::Batch::default());
    }

    #[test]
    fn a_stream_that_cannot_be_read_is_an_empty_batch_too() {
        // A directory answers `metadata` and refuses to be read, which is
        // the shape of every other way a stream is there but unreadable.
        let fixture = Fixture::new("unreadable");
        std::fs::create_dir(fixture.path()).unwrap();

        assert_eq!(fixture.reader().poll(), super::Batch::default());
    }

    #[test]
    fn a_poll_that_finds_nothing_new_reads_nothing_and_reports_no_restart() {
        let fixture = Fixture::new("nothing-new");
        fixture.write(&format!("{INIT}\n"));
        let mut reader = fixture.reader();
        assert_eq!(reader.poll().records, vec![init()]);

        let second = reader.poll();

        assert!(second.records.is_empty());
        assert!(
            !second.restarted,
            "an unchanged file was read as a new run — the board would clear a live view every second",
        );
    }

    #[test]
    fn a_second_init_record_is_a_new_run_and_the_file_is_read_from_the_start() {
        // The signal the length comparison cannot give: a run that begins
        // in a file the reader is already partway through. It is the whole
        // file that is re-read, not the tail — the offset is the only thing
        // that says where the reader is, and it is what has just been shown
        // to be about the wrong run.
        let fixture = Fixture::new("second-init");
        fixture.write(&format!("{INIT}\n"));
        let mut reader = fixture.reader();
        assert_eq!(reader.poll().records, vec![init()]);

        fixture.write(&format!(
            "{INIT}\n{INIT}\n{{\"type\":\"assistant\",\"message\":{{\"id\":\"m1\"}}}}\n"
        ));
        let batch = reader.poll();

        assert!(batch.restarted, "a second init was read as more of one run");
        assert_eq!(
            batch.records,
            vec![
                init(),
                init(),
                Record::Assistant {
                    message: serde_json::json!({"id": "m1"}),
                    at: None,
                },
            ],
            "the file was read on from the offset rather than from the start",
        );
    }

    #[test]
    fn a_new_run_whose_init_has_not_arrived_yet_does_not_restart_twice() {
        // `tee` truncates when the resumed runner opens the file, and the
        // init record lands a moment later. A board that looked in between
        // has already restarted; the init that follows is the first of its
        // run, not a second one.
        let fixture = Fixture::new("init-late");
        fixture.write(&format!("{INIT}\n{INIT}\n"));
        let mut reader = fixture.reader();
        assert_eq!(reader.poll().records.len(), 2);

        fixture.write("");
        assert!(reader.poll().restarted);

        fixture.write(&format!("{INIT}\n"));
        let batch = reader.poll();

        assert!(
            !batch.restarted,
            "the new run's own init restarted it a second time",
        );
        assert_eq!(batch.records, vec![init()]);
    }

    #[test]
    fn a_partial_line_left_by_the_run_that_ended_is_not_glued_to_the_new_one() {
        let fixture = Fixture::new("partial-across-runs");
        fixture.write(&format!("{INIT}\n{{\"type\":\"assist"));
        let mut reader = fixture.reader();
        assert_eq!(reader.poll().records, vec![init()]);

        fixture.write("");
        assert!(reader.poll().restarted);
        fixture.write(&format!("{INIT}\n"));

        assert_eq!(
            reader.poll().records,
            vec![init()],
            "the half-written tail of the old run was read as part of the new one",
        );
    }

    #[test]
    fn the_four_record_shapes_are_told_apart() {
        assert_eq!(super::parse_line(INIT.as_bytes()), Some(init()));
        assert_eq!(
            super::parse_line(br#"{"type":"assistant","message":{"id":"m1"}}"#),
            Some(Record::Assistant {
                message: serde_json::json!({"id": "m1"}),
                at: None,
            }),
        );
        assert_eq!(
            super::parse_line(
                br#"{"type":"assistant","timestamp":"2026-09-07T12:00:00.000Z","message":{}}"#
            ),
            Some(Record::Assistant {
                message: serde_json::json!({}),
                at: crate::clock::Timestamp::parse("2026-09-07T12:00:00.000Z"),
            }),
            "the record's stamp is read from the record, not from its message",
        );
        assert_eq!(
            super::parse_line(br#"{"type":"assistant","timestamp":"noon","message":{}}"#),
            Some(Record::Assistant {
                message: serde_json::json!({}),
                at: None,
            }),
            "a stamp the clock cannot read cost the whole record",
        );
        assert_eq!(
            super::parse_line(br#"{"type":"user","message":{"content":[]}}"#),
            Some(Record::ToolResult(serde_json::json!({"content": []}))),
        );
        assert_eq!(
            super::parse_line(br#"{"type":"result","subtype":"success"}"#),
            Some(Record::Other),
        );
        assert_eq!(
            super::parse_line(br#"{"type":"system","subtype":"compact_boundary"}"#),
            Some(Record::Other),
            "a system record that is not the init one would restart every run that compacts",
        );
    }

    #[test]
    fn a_record_a_subagent_made_is_one_the_board_does_not_read() {
        // A run spawns subagents, and their records share the stream with
        // `parent_tool_use_id` set. Dropping them here rather than in the
        // fold is what makes "only the main session counts" true of every
        // column at once — the stage, the tool, the texts and the usage —
        // instead of a rule each of them has to remember separately.
        assert_eq!(
            super::parse_line(
                br#"{"type":"assistant","parent_tool_use_id":"toolu_1","message":{"id":"m1"}}"#
            ),
            Some(Record::Other),
        );
        assert_eq!(
            super::parse_line(
                br#"{"type":"user","parent_tool_use_id":"toolu_1","message":{"content":[]}}"#
            ),
            Some(Record::Other),
        );
        assert_eq!(
            super::parse_line(br#"{"type":"assistant","parent_tool_use_id":null,"message":{}}"#),
            Some(Record::Assistant {
                message: serde_json::json!({}),
                at: None,
            }),
            "an explicit null parent is the main session, not a subagent",
        );
        // The init record most of all. One per run is what makes a second
        // one the sign of a resume, so an init a subagent brought with it
        // would rewind the reader and wipe a live view mid-run — the one
        // record where reading a subagent's is worse than useless.
        assert_eq!(
            super::parse_line(
                br#"{"type":"system","subtype":"init","model":"m","parent_tool_use_id":"toolu_1"}"#
            ),
            Some(Record::Other),
        );
    }

    #[test]
    fn a_line_that_is_neither_utf8_nor_json_nor_a_record_is_skipped() {
        assert_eq!(super::parse_line(&[0xff, 0xfe]), None);
        assert_eq!(super::parse_line(b"not json at all"), None);
        assert_eq!(super::parse_line(b""), None);
        assert_eq!(super::parse_line(b"[1, 2, 3]"), None);
    }

    #[test]
    fn an_init_record_without_a_model_still_begins_a_run() {
        assert_eq!(
            super::parse_line(br#"{"type":"system","subtype":"init"}"#),
            Some(Record::Init {
                model: String::new()
            }),
        );
        assert!(init().is_init());
        assert!(!Record::Other.is_init());
    }
}
