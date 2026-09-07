//! Spec 10 — keeler-top. One test per scenario, named after it.
//!
//! The crate's own suite: the board's logic is a library, so its scenarios
//! are driven here rather than through a subprocess. The root harness next
//! door drives shell — `tests/top_recipes.rs` is where the recipe scenarios
//! of this spec live.

use keeler_top::clock::Timestamp;
use keeler_top::run::{RunView, Stage, fold, format_tokens};
use keeler_top::stream::{Batch, Record, StreamReader};
use keeler_top::theme::Theme;

/// The theme every board below is built through.
///
/// Said outright and never read from the process: the theme is a value so
/// that a test names the board it wants, and a suite that took the
/// environment's answer would draw a different board on a machine that
/// exports `NO_COLOR`.
const THEME: Theme = Theme::new(true, false);

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

/// A directory of this test's own, under whatever the machine calls
/// temporary.
///
/// The test's name and the process id are not enough between them. A
/// mutation run is 361 mutants of these 165 tests, four at a time, and
/// nextest gives every test a process of its own: tens of thousands of
/// processes in five minutes, which is enough for the pid space to come
/// round, and every fixture below opens by removing whatever is at its
/// path. Two live runs of one test on one name is not two tests sharing a
/// directory — it is one of them deleting the other's files halfway
/// through, which is a failing gate nobody can reproduce. The instant this
/// process started, and a serial that only goes up, are what the pid is
/// missing.
fn fixture_dir(name: &str) -> std::path::PathBuf {
    static SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let started = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "keeler-top-{name}-{}-{started}-{}",
        std::process::id(),
        SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
    ))
}

/// A stream file in its own directory, removed on drop. Named after the
/// test that owns it, so two tests never share one.
struct Stream {
    dir: std::path::PathBuf,
    path: std::path::PathBuf,
}

impl Stream {
    fn new(name: &str) -> Self {
        let dir = fixture_dir(name);
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

// ── T4

// The trait itself is named under T6's heading, which is where a `dyn
// Dispatch` first appears; this file's own calls to it read from that.
use keeler_top::dispatch::Shell;
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

/// git with a fixed identity and no user config, so a global
/// `commit.gpgsign` cannot hang the suite waiting for a key.
fn git(dir: &Path, args: &[&str]) -> String {
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

fn write(dir: &Path, rel: &str, body: &str) {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}

fn commit(dir: &Path, rel: &str, body: &str, message: &str) {
    write(dir, rel, body);
    git(dir, &["add", rel]);
    git(dir, &["commit", "-qm", message]);
}

/// A synthetic project: the repository, and room beside it for the
/// worktrees a wave cuts — `keeler-spawn` puts them next to the
/// repository, and the board reads them there.
struct Project(PathBuf);

impl Project {
    fn new(name: &str) -> Self {
        let dir = fixture_dir(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("repo")).unwrap();
        let project = Self(dir);
        git(&project.root(), &["init", "-qb", "main"]);
        project
    }

    fn root(&self) -> PathBuf {
        self.0.join("repo")
    }

    /// The worktree and branch a spawn cuts, from wherever HEAD is.
    fn worktree(&self, branch: &str) -> PathBuf {
        let path = self.0.join(branch.replace('/', "-"));
        git(
            &self.root(),
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                branch,
                path.to_str().unwrap(),
            ],
        );
        path
    }

    /// The spec, committed on `feat/01-foo`, which is where every read of
    /// the graph goes.
    fn with_spec_on_the_feature_branch(&self) -> &Self {
        commit(
            &self.root(),
            "specs/01-foo.md",
            &spec_text(false),
            "docs(01-foo): the approved spec",
        );
        git(&self.root(), &["checkout", "-qb", "feat/01-foo"]);
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
    write(&project.root(), "specs/01-foo.md", &spec_text(true));

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
    // A shebang recipe, as `keeler-status` is: under the `-q` the board
    // passes, just gives a linewise recipe a null stdout and this fixture
    // would answer nothing.
    write(
        &plugin,
        "Justfile",
        "keeler-status SPEC:\n    #!/usr/bin/env bash\n    \
         echo \"graph: {{SPEC}} on feat/01-foo\"\n    pwd -P\n",
    );
    write(
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
            "-q".as_ref(),
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

/// Not a scenario of its own: it is why `-q` is in the argv the scenario
/// above asserts. Every refusal the board shows is a recipe's sentence,
/// written for whoever has to act on it, and `just` adds a line of its own
/// about a recipe the reader never named — the same reason `keeler-spawn`
/// passes `-q` to its own preflight.
///
/// The fixture recipe carries a shebang because `keeler-status` does, and
/// under `-q` the two kinds are not alike: just gives a *linewise* recipe
/// a null stderr and swallows its refusal whole, while a shebang recipe is
/// one process whose stderr it leaves alone. A linewise fixture measures
/// the wrong half of that — it fails here about a board that is right.
#[test]
fn a_recipes_refusal_reaches_the_board_without_justs_report_of_it() {
    let project = Project::new("refusal-relayed");
    let plugin = project.0.join("plugin");
    write(
        &plugin,
        "Justfile",
        "keeler-status SPEC:\n    #!/usr/bin/env bash\n    \
         echo \"keeler-status: {{SPEC}} is not committed\" >&2\n    exit 1\n",
    );
    let shell = Shell::new(&plugin, project.root(), "specs/01-foo.md");

    let refused = shell.status().expect_err("the fixture recipe refuses");

    assert_eq!(refused, "keeler-status: specs/01-foo.md is not committed");
}

#[test]
fn the_commit_column_is_the_branchs_head_and_its_distance_from_the_feature_branch() {
    // Given keeler/01-foo/t1 is three commits ahead of feat/01-foo, head f5064b1
    let project = Project::new("commit-column");
    project.with_spec_on_the_feature_branch();
    let worktree = project.worktree("keeler/01-foo/t1");
    for step in ["T1 red", "T1 green", "T1 the review record"] {
        commit(
            &worktree,
            "src/t1.rs",
            step,
            &format!("test(01-foo): {step}"),
        );
    }

    // When the board renders
    let facts = branch_facts(&worktree, "feat/01-foo").expect("the branch and its base are there");

    // Then T1's commit column reads "f5064b1 +3"
    // The head is compared as a prefix of the full hash rather than
    // against a short one of the fixture's own: `core.abbrev` is the
    // machine's, and the two gits — this one, told to ignore the user's
    // config, and the board's, which must not be — would disagree about
    // the length of a hash they agree about.
    assert!(
        git(&worktree, &["rev-parse", "HEAD"]).starts_with(&facts.head),
        "the head named is not this branch's: {}",
        facts.head,
    );
    assert!(facts.head.len() >= 7, "the head is too short to name one");
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
    commit(&worktree, "src/a.rs", "one\n", "feat(01-foo): a");
    commit(&worktree, "src/b.rs", "two\n", "feat(01-foo): b");
    write(&worktree, "src/a.rs", "one, changed\n");
    write(&worktree, "src/b.rs", "two, changed\n");
    write(&worktree, "scratch", "not added\n");

    // When the board renders
    let facts = branch_facts(&worktree, "feat/01-foo").expect("the branch and its base are there");

    // Then T1's commit column ends with "3 dirty"
    assert_eq!(facts.dirty, 3);
    assert_eq!(facts.ahead, 2, "the commits were miscounted alongside it");
}

/// Not a scenario of its own: it is why the count above is asked for
/// explicitly. `git status --porcelain` obeys `status.showUntrackedFiles`,
/// so on a machine set to `no` a worktree whose only uncommitted work is
/// new files reports clean — and a board that said a task had written
/// nothing would be wrong in the direction that gets work thrown away.
#[test]
fn a_repository_that_hides_untracked_files_still_has_them_counted() {
    let project = Project::new("hidden-untracked");
    project.with_spec_on_the_feature_branch();
    let worktree = project.worktree("keeler/01-foo/t1");
    git(&worktree, &["config", "status.showUntrackedFiles", "no"]);
    write(&worktree, "src/new.rs", "written but not added\n");

    let facts = branch_facts(&worktree, "feat/01-foo").expect("the branch and its base are there");

    assert_eq!(facts.dirty, 1);
}

#[test]
fn a_clean_worktree_on_the_base_commit_shows_the_base() {
    // Given keeler/01-foo/t1 equals feat/01-foo and the worktree is clean
    let project = Project::new("on-the-base");
    project.with_spec_on_the_feature_branch();
    let base = git(&project.root(), &["rev-parse", "feat/01-foo"]);
    let worktree = project.worktree("keeler/01-foo/t1");

    // When the board renders
    let facts = branch_facts(&worktree, "feat/01-foo").expect("the branch and its base are there");

    // Then T1's commit column reads the base's short hash and "+0"
    assert!(
        base.starts_with(&facts.head) && facts.head.len() >= 7,
        "the head named is not the base's: {}",
        facts.head,
    );
    assert_eq!(facts.ahead, 0);
    assert_eq!(facts.dirty, 0);
    assert!(facts.commits.is_empty());
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
        // A record stamped with whatever a garbled stream might hold. The
        // clock has to answer for those too: a year of twenty digits
        // multiplied into seconds is the shape that took the board down
        // before the ranges went in.
        "[0-9]{0,20}(-[0-9]{0,4}){0,3}[T ]?[0-9]{0,20}(:[0-9]{0,20}){0,3}Z?".prop_map(|stamp| {
            restamped(
                &stamp,
                &stamped_tool_use(
                    "toolu_1",
                    "Bash",
                    serde_json::json!({ "command": "just dev" }),
                ),
            )
        }),
        "[^\n]{0,12}",
    ]
}

/// The same record with some other stamp.
fn restamped(stamp: &str, line: &str) -> String {
    let mut record: serde_json::Value =
        serde_json::from_str(line).expect("the fixture's own record is not JSON");
    record["timestamp"] = serde_json::Value::String(stamp.to_string());
    record.to_string()
}

// ── T5

use keeler_top::board::{Board, Runs};
use keeler_top::frame::{cells, detail, layout, render};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;

/// The clock every frame below is drawn against. Fixed rather than
/// `Timestamp::now()`: the elapsed and age columns are differences, and a
/// test that read the machine's clock would assert on a number that
/// changes between the two lines that produce it.
const NOON: &str = "2026-09-07T12:00:00.000Z";

/// The run files keeler-status's report points at: a `<tid>.stream` the
/// board reads, and the `<tid>.log` path the report actually names — the
/// board finds the one from the other, so the fixture hands out the log.
struct Runfiles(std::path::PathBuf);

impl Runfiles {
    fn new(name: &str) -> Self {
        let dir = fixture_dir(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    /// The log path for a task, whether or not anything was written beside
    /// it — a report names one for every state that has a run.
    fn log(&self, tid: &str) -> String {
        self.0.join(format!("{tid}.log")).display().to_string()
    }

    /// Writes a task's stream and gives back the log path the report names.
    fn stream(&self, tid: &str, lines: &[String]) -> String {
        let mut body = lines.join("\n");
        body.push('\n');
        std::fs::write(self.0.join(format!("{tid}.stream")), body).unwrap();
        self.log(tid)
    }
}

impl Drop for Runfiles {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A report the recipe could have printed: the header it opens with, and
/// the lines a scenario describes under it.
fn report(lines: &[String]) -> String {
    let mut report = "graph: specs/01-foo.md on feat/01-foo\n".to_string();
    report.push_str(&lines.join("\n"));
    report.push('\n');
    report
}

/// One tick's board, assembled as the binary assembles one: the report
/// keeler-status printed, the graph script's answer beside it, and
/// whatever is on disk under the paths the report names.
fn assemble(report: &str, graph: &str, answered: &str) -> Board {
    let status =
        keeler_top::status::parse(report).expect("the fixture's report opens with a header");
    Board::assemble(
        &status,
        &keeler_top::graph::parse(graph),
        &mut Runs::default(),
        now(answered),
    )
}

/// A frame, drawn on a terminal of the given size and read back as lines
/// of text with the trailing blanks cut.
fn drawn(board: &Board, width: u16, height: u16) -> Vec<String> {
    let mut terminal =
        Terminal::new(TestBackend::new(width, height)).expect("a terminal to draw on");
    terminal
        .draw(|frame| render(frame, board, now(NOON)))
        .expect("the board drew a frame");
    lines_of(&terminal, width, height)
}

/// The drawn line that begins with a task's id, or the whole frame in the
/// failure message — a row that is missing is the thing most of these
/// tests are about.
fn row_of<'a>(frame: &'a [String], id: &str) -> &'a str {
    frame
        .iter()
        .find(|line| line.starts_with(&format!("{id} ")))
        .unwrap_or_else(|| panic!("no row for {id} in:\n{}", frame.join("\n")))
}

/// The board's binary, run the way `keeler keeler-top` runs it: the plugin
/// tree it shells back to, the project it is watching, and the flags and
/// spec after them.
fn keeler_top(plugin: &Path, root: &Path, args: &[&str]) -> std::process::Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_keeler-top"))
        .arg("--plugin-root")
        .arg(plugin)
        .arg("--root")
        .arg(root)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .expect("failed to run the keeler-top binary")
}

/// A plugin tree of the fixture's own: a `keeler-status` recipe printing
/// the report a scenario describes, and the graph script beside it.
///
/// The two refusal scenarios use the real plugin, because there the
/// recipe's own words are the answer. A frame is not: it is a reading of
/// whatever report arrives, and the states a live wave produces —
/// `running`, `died`, `failed (exit 1)` — need a tmux session and four
/// spawned agents to produce. The recipe's format is pinned against the
/// recipe itself, by the round-trip property in `status.rs`.
fn stub_plugin(at: &Path, report: &str, graph: &str) {
    write(at, "report", report);
    write(at, "graph", graph);
    write(
        at,
        "Justfile",
        "keeler-status SPEC:\n    #!/usr/bin/env bash\n    \
         cat \"{{justfile_directory()}}/report\"\n",
    );
    write(
        at,
        "scripts/keeler-graph.sh",
        "#!/usr/bin/env bash\ncat \"$(dirname \"$0\")/../graph\"\n",
    );
}

#[test]
fn outside_a_git_repository_the_board_refuses_with_keeler_statuss_words() {
    // Given a directory that is not a git repository
    let outside = fixture_dir("outside");
    let _ = std::fs::remove_dir_all(&outside);
    std::fs::create_dir_all(&outside).unwrap();

    // When the user runs `keeler-top specs/01-foo.md`
    let output = keeler_top(&plugin_root(), &outside, &["--once", "specs/01-foo.md"]);
    let _ = std::fs::remove_dir_all(&outside);

    // Then it exits 1
    assert!(
        !output.status.success(),
        "the board drew a board it had not"
    );
    // And stderr carries the reason keeler-status gave
    let said = String::from_utf8_lossy(&output.stderr);
    assert!(
        said.contains("graph mode needs a git repository"),
        "the recipe's own reason did not reach the board:\n{said}",
    );
}

#[test]
fn a_spec_not_committed_on_the_feature_branch_is_refused() {
    // Given specs/01-foo.md exists in the working tree but is not committed
    // on feat/01-foo or HEAD
    let project = Project::new("not-committed");
    commit(
        &project.root(),
        "README.md",
        "a project\n",
        "docs: the first commit",
    );
    git(&project.root(), &["checkout", "-qb", "feat/01-foo"]);
    write(&project.root(), "specs/01-foo.md", &spec_text(false));

    // When the user runs `keeler-top specs/01-foo.md`
    let output = keeler_top(
        &plugin_root(),
        &project.root(),
        &["--once", "specs/01-foo.md"],
    );

    // Then it exits 1
    assert!(!output.status.success(), "an uncommitted spec drew a board");
    // And stderr says the spec is not committed on the ref the board reads
    let said = String::from_utf8_lossy(&output.stderr);
    assert!(
        said.contains("specs/01-foo.md is not committed on feat/01-foo"),
        "the refusal names neither the spec nor the ref:\n{said}",
    );
}

#[test]
fn once_prints_one_frame_as_a_table_and_exits() {
    // Given a spec whose T1 is running and T2 is not spawned
    let project = Project::new("once");
    project.with_spec_on_the_feature_branch();
    let runfiles = Runfiles::new("once-runs");
    let plugin = project.0.join("plugin");
    stub_plugin(
        &plugin,
        &format!(
            "graph: specs/01-foo.md on feat/01-foo\n\
             T1     running          log {}  worktree /nowhere\n\
             T2     not spawned\n",
            runfiles.stream(
                "t1",
                &[
                    INIT.to_string(),
                    stamped_tool_use(
                        "toolu_1",
                        "Bash",
                        serde_json::json!({"command": "just dev"})
                    ),
                ],
            ),
        ),
        "T1 ready\nT2 blocked T1\n",
    );

    // When the user runs `keeler-top --once specs/01-foo.md`
    let output = keeler_top(&plugin, &project.root(), &["--once", "specs/01-foo.md"]);

    // Then stdout is a plain table with a header line and one row per task
    assert!(
        output.status.success(),
        "the board refused:\n{}",
        String::from_utf8_lossy(&output.stderr),
    );
    let shown = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = shown.lines().collect();
    assert_eq!(lines.len(), 3, "not a heading and two rows:\n{shown}");
    assert!(lines[0].starts_with("TASK"), "no heading line:\n{shown}");
    assert!(lines[1].starts_with("T1 "), "no row for T1:\n{shown}");
    assert!(
        lines[1].contains("running"),
        "T1's state is missing:\n{shown}"
    );
    assert!(lines[2].starts_with("T2 "), "no row for T2:\n{shown}");

    // And the process exits 0 without entering the alternate screen
    assert!(
        !shown.contains('\u{1b}'),
        "the plain frame carried terminal escapes:\n{shown:?}",
    );
}

#[test]
fn without_a_terminal_the_board_refuses_and_names_once() {
    // Given stdout is a pipe and --once was not given
    let project = Project::new("no-terminal");
    project.with_spec_on_the_feature_branch();
    let plugin = project.0.join("plugin");
    stub_plugin(
        &plugin,
        "graph: specs/01-foo.md on feat/01-foo\nT1     not spawned\nT2     not spawned\n",
        "T1 ready\nT2 blocked T1\n",
    );

    // When the user runs `keeler-top specs/01-foo.md`
    let output = keeler_top(&plugin, &project.root(), &["specs/01-foo.md"]);

    // Then it exits 1
    assert!(!output.status.success(), "a pipe was drawn a live board");
    // And stderr says the board needs a terminal, and that --once prints one frame
    let said = String::from_utf8_lossy(&output.stderr);
    assert!(
        said.contains("terminal") && said.contains("--once"),
        "the refusal names neither the terminal nor the way out:\n{said}",
    );
}

#[test]
fn the_header_names_the_ref_keeler_status_answered_from_and_the_age_of_that_answer() {
    // Given keeler-status printed "graph: specs/01-foo.md on feat/01-foo"
    // four seconds ago
    let board = assemble(
        "graph: specs/01-foo.md on feat/01-foo\nT1     not spawned\n",
        "T1 ready\n",
        "2026-09-07T11:59:56.000Z",
    );

    // When the board renders
    let frame = drawn(&board, 100, 12);

    // Then the header reads "specs/01-foo.md on feat/01-foo" and "status 4s ago"
    assert!(
        frame[0].contains("specs/01-foo.md on feat/01-foo") && frame[0].contains("status 4s ago"),
        "the header names neither the ref nor the age: {:?}",
        frame[0],
    );
}

#[test]
fn every_task_in_the_graph_has_a_row_in_spec_order() {
    // Given a spec with tasks T1..T5
    let lines: Vec<String> = (1..=5)
        .map(|task| format!("T{task}     not spawned"))
        .collect();
    let board = assemble(&report(&lines), "T1 ready\n", NOON);

    // When the board renders
    let frame = drawn(&board, 100, 20);

    // Then it shows five rows, T1 first and T5 last
    let heading = frame
        .iter()
        .position(|line| line.starts_with("TASK"))
        .expect("the table has a heading line");
    let rows = &frame[heading + 1..=heading + 5];
    for (index, task) in (1..=5).enumerate() {
        assert!(
            rows[index].starts_with(&format!("T{task} ")),
            "the rows are not in spec order:\n{}",
            frame.join("\n"),
        );
    }
    assert_eq!(
        board
            .rows
            .iter()
            .map(|row| row.id.as_str())
            .collect::<Vec<_>>(),
        ["T1", "T2", "T3", "T4", "T5"],
    );
}

#[test]
fn the_state_column_is_keeler_statuss_word_for_the_task() {
    // Given keeler-status reports T1 running, T2 died, T3 passed, T4
    // incomplete (no review record), T5 failed (exit 1)
    let runfiles = Runfiles::new("states");
    let states = [
        ("T1", "running"),
        ("T2", "died"),
        ("T3", "passed"),
        ("T4", "incomplete (no review record)"),
        ("T5", "failed (exit 1)"),
    ];
    let lines: Vec<String> = states
        .iter()
        .map(|(id, state)| {
            format!(
                "{id:<6} {state:<16} log {}  worktree /nowhere",
                runfiles.log(&id.to_lowercase()),
            )
        })
        .collect();
    let board = assemble(&report(&lines), "T1 ready\n", NOON);

    // When the board renders
    let frame = drawn(&board, 140, 20);

    // Then each row's state column shows exactly that word, with
    // incomplete's reason and failed's exit code kept
    for (index, (id, state)) in states.into_iter().enumerate() {
        assert_eq!(board.rows[index].state, state);
        assert!(
            row_of(&frame, id).contains(state),
            "{id}'s state was cut: {:?}",
            row_of(&frame, id),
        );
    }
}

#[test]
fn a_not_spawned_task_the_graph_holds_reads_blocked_with_its_needs() {
    // Given keeler-status reports T3 not spawned and keeler-graph.sh
    // reports "T3 blocked T1 T2"
    let board = assemble(
        "graph: specs/01-foo.md on feat/01-foo\nT3     not spawned\n",
        "T3 blocked T1 T2\n",
        NOON,
    );

    // When the board renders
    // Then T3's state column reads "blocked ← T1, T2"
    assert_eq!(board.rows[0].state, "blocked ← T1, T2");
    assert!(
        row_of(&drawn(&board, 100, 12), "T3").contains("blocked ← T1, T2"),
        "the graph's word did not reach the column",
    );
}

#[test]
fn a_not_spawned_task_the_graph_calls_ready_reads_ready() {
    // Given keeler-status reports T1 not spawned and keeler-graph.sh
    // reports "T1 ready"
    let board = assemble(
        "graph: specs/01-foo.md on feat/01-foo\nT1     not spawned\n",
        "T1 ready\n",
        NOON,
    );

    // When the board renders
    // Then T1's state column reads "ready"
    assert_eq!(board.rows[0].state, "ready");
    assert!(row_of(&drawn(&board, 100, 12), "T1").contains("ready"));
}

#[test]
fn a_done_task_with_no_worktree_shows_dashes_for_the_live_columns() {
    // Given T1 is done and ../<repo>-01-foo-t1 no longer exists
    let board = assemble(
        "graph: specs/01-foo.md on feat/01-foo\nT1     done\n",
        "T1 done\n",
        NOON,
    );

    // When the board renders
    // Then T1's state is done
    assert_eq!(board.rows[0].state, "done");

    // And its stage, tool, context, tokens and commit columns show "—"
    let cells = cells(&board.rows[0], now(NOON));
    for column in [2, 3, 5, 6, 8] {
        assert_eq!(cells[column], "—", "column {column} of {cells:?}");
    }
}

#[test]
fn a_landed_feature_whose_branch_is_gone_still_renders() {
    // Given feat/01-foo no longer exists and keeler-status answers from HEAD
    let board = assemble(
        "graph: specs/01-foo.md on HEAD\nT1     done\nT2     done\n",
        "T1 done\nT2 done\n",
        NOON,
    );

    // When the board renders
    let frame = drawn(&board, 100, 16);

    // Then every row is shown and the commit column reads "—" for each
    for (index, id) in ["T1", "T2"].into_iter().enumerate() {
        assert!(row_of(&frame, id).ends_with('—'), "{id} named a commit");
        assert_eq!(cells(&board.rows[index], now(NOON))[8], "—");
    }

    // And the header names HEAD as the ref
    assert!(
        frame[0].contains("specs/01-foo.md on HEAD"),
        "the header does not name the ref: {:?}",
        frame[0],
    );
}

#[test]
fn a_long_command_is_cut_to_the_column_with_an_ellipsis() {
    // Given the last command is longer than the tool column
    let runfiles = Runfiles::new("long-command");
    let command = format!("just dev 2>&1 | tail -35 {}", "#".repeat(64));
    let log = runfiles.stream(
        "t1",
        &[
            INIT.to_string(),
            stamped_tool_use(
                "toolu_1",
                "Bash",
                serde_json::json!({ "command": command.clone() }),
            ),
        ],
    );
    let board = assemble(
        &format!(
            "graph: specs/01-foo.md on feat/01-foo\nT1     running          log {log}  worktree /nowhere\n"
        ),
        "T1 ready\n",
        NOON,
    );

    // When the board renders
    let frame = drawn(&board, 120, 24);

    // Then the column shows its head followed by "…"
    let row = row_of(&frame, "T1");
    assert!(
        row.contains("Bash: just dev 2>&1 | tail -35 #") && row.contains('…'),
        "the command was not cut to the column: {row:?}",
    );
    assert!(
        !row.contains(&command),
        "the whole command was written into the row: {row:?}",
    );

    // And the detail pane shows it whole
    let whole = format!("Bash: {command}");
    assert!(
        frame.contains(&whole),
        "the pane did not show the command whole:\n{}",
        frame.join("\n"),
    );
}

#[test]
fn the_detail_pane_shows_the_selected_tasks_last_five_texts_and_its_last_command() {
    // Given T1's stream holds seven main-session assistant text blocks and
    // the last tool_use is Bash "just dev"
    let runfiles = Runfiles::new("pane-texts");
    let mut lines = vec![INIT.to_string()];
    for word in ["one", "two", "three", "four", "five", "six", "seven"] {
        lines.push(record(None, "m1", None, text(word)));
    }
    lines.push(stamped_tool_use(
        "toolu_1",
        "Bash",
        serde_json::json!({ "command": "just dev" }),
    ));
    let log = runfiles.stream("t1", &lines);
    let board = assemble(
        &format!(
            "graph: specs/01-foo.md on feat/01-foo\nT1     running          log {log}  worktree /nowhere\n"
        ),
        "T1 ready\n",
        NOON,
    );

    // When T1 is selected
    let pane = detail(board.selected_row().expect("the first row is selected"));

    // Then the pane shows the last five texts, oldest first, and the
    // command in full
    let texts: Vec<&String> = pane
        .iter()
        .filter(|line| {
            ["one", "two", "three", "four", "five", "six", "seven"].contains(&line.as_str())
        })
        .collect();
    assert_eq!(texts, ["three", "four", "five", "six", "seven"]);
    assert!(
        pane.contains(&"Bash: just dev".to_string()),
        "the pane does not name the command:\n{}",
        pane.join("\n"),
    );
    // And it is drawn where the pane is.
    let frame = drawn(&board, 120, 24);
    assert!(
        frame.contains(&"Bash: just dev".to_string()),
        "the pane was not drawn:\n{}",
        frame.join("\n"),
    );
}

#[test]
fn the_detail_pane_lists_the_branchs_commits_since_the_feature_branch() {
    // Given keeler/01-foo/t1 holds commits "test(01-foo): T1 red" and
    // "feat(01-foo): T1 green" on top of feat/01-foo
    let project = Project::new("pane-commits");
    project.with_spec_on_the_feature_branch();
    let worktree = project.worktree("keeler/01-foo/t1");
    for step in ["test(01-foo): T1 red", "feat(01-foo): T1 green"] {
        commit(&worktree, "src/t1.rs", step, step);
    }
    let board = assemble(
        &format!(
            "graph: specs/01-foo.md on feat/01-foo\nT1     running          log /nowhere/t1.log  worktree {}\n",
            worktree.display(),
        ),
        "T1 ready\n",
        NOON,
    );

    // When T1 is selected
    let pane = detail(board.selected_row().expect("the first row is selected"));

    // Then the pane lists both subjects with their short hashes, newest first
    let listed: Vec<&String> = pane
        .iter()
        .filter(|line| line.contains("(01-foo): T1 "))
        .collect();
    assert_eq!(listed.len(), 2, "not both commits:\n{}", pane.join("\n"));
    assert!(
        listed[0].ends_with("feat(01-foo): T1 green")
            && listed[1].ends_with("test(01-foo): T1 red"),
        "the commits are not newest first:\n{}",
        pane.join("\n"),
    );
    let head = git(&worktree, &["rev-parse", "HEAD"]);
    let hash = listed[0]
        .split_whitespace()
        .next()
        .expect("a commit line names a hash");
    assert!(
        head.starts_with(hash) && hash.len() >= 7,
        "the hash shown is not the branch's head: {hash}",
    );
}

#[test]
fn a_missing_stream_file_shows_the_state_alone() {
    // Given keeler-status says T1 died but
    // .keeler/runs/01-foo/t1.stream does not exist
    let runfiles = Runfiles::new("no-stream");
    let board = assemble(
        &format!(
            "graph: specs/01-foo.md on feat/01-foo\nT1     died             log {}  worktree /nowhere\n",
            runfiles.log("t1"),
        ),
        "T1 ready\n",
        NOON,
    );

    // When the board renders
    // Then T1's state is died and its live columns show "—"
    assert_eq!(board.rows[0].state, "died");
    assert!(
        board.rows[0].run.is_none(),
        "a stream that is not there was read"
    );
    let cells = cells(&board.rows[0], now(NOON));
    for column in [2, 3, 5, 6, 8] {
        assert_eq!(cells[column], "—", "column {column} of {cells:?}");
    }
    assert!(row_of(&drawn(&board, 100, 12), "T1").contains("died"));
}

#[test]
fn a_narrow_terminal_drops_the_detail_pane_before_it_drops_columns() {
    // Given a terminal 100 columns wide and 12 lines tall
    let runfiles = Runfiles::new("narrow");
    let lines: Vec<String> = (1..=5)
        .map(|task| {
            format!(
                "T{task:<5} running          log {}  worktree /nowhere",
                runfiles.stream(
                    &format!("t{task}"),
                    &[
                        INIT.to_string(),
                        stamped_tool_use(
                            "toolu_1",
                            "Bash",
                            serde_json::json!({"command": "just dev"}),
                        ),
                    ],
                ),
            )
        })
        .collect();
    let board = assemble(&report(&lines), "T1 ready\n", NOON);

    // When the board renders
    let frame = drawn(&board, 100, 12);

    // Then every task row is shown with state, stage and tool
    for task in 1..=5 {
        let row = row_of(&frame, &format!("T{task}"));
        assert!(
            row.contains("running") && row.contains("qa") && row.contains("Bash: just dev"),
            "T{task}'s row lost a column: {row:?}",
        );
        assert!(
            row.chars().count() <= 100,
            "the row runs past the window and is clipped where it stands: {row:?}",
        );
    }

    // And the detail pane is absent
    assert_eq!(layout(Rect::new(0, 0, 100, 12), 5).detail, None);
    assert!(
        !frame.iter().any(|line| line.starts_with("T1 — running")),
        "the pane was drawn on a terminal with no room for it:\n{}",
        frame.join("\n"),
    );
}

// ── T6

use keeler_top::app::{Action, App, Events, StatusFeed, TICK, Woke, looping, on_key, step};
use keeler_top::dispatch::Dispatch;
use keeler_top::terminal::{Guard, Screen};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::collections::VecDeque;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// The two reads, answered from a fixture rather than run.
///
/// What these scenarios are about is *when* the board asks and what it does
/// while it waits, so the recipe's own words are somebody else's test —
/// T5's, which runs the real one. What is counted here is how many reads
/// were started, and the gate is how one of them is made slow.
#[derive(Debug)]
struct Reads {
    report: String,
    graph: String,
    started: Mutex<usize>,
    /// A read waits on this before answering, when a scenario wants one
    /// still running. A gate the test opens rather than a sleep: what is
    /// being asserted is that the board kept moving while a read was out,
    /// and three seconds of sleeping would make the suite slow to say it
    /// and flaky the moment somebody shortened the sleep to make it quick.
    gate: Option<Mutex<Receiver<()>>>,
}

impl Reads {
    fn answering(report: &str) -> Self {
        Self {
            report: report.to_string(),
            graph: String::new(),
            started: Mutex::new(0),
            gate: None,
        }
    }

    /// A read that will not answer until the test says so.
    fn slow(report: &str) -> (Self, Sender<()>) {
        let (opener, gate) = channel();
        (
            Self {
                gate: Some(Mutex::new(gate)),
                ..Self::answering(report)
            },
            opener,
        )
    }

    fn started(&self) -> usize {
        *self.started.lock().expect("the count")
    }
}

impl Dispatch for Reads {
    fn status(&self) -> Result<String, String> {
        // Counted on the way in, not the way out: what the cadence decides
        // is when a read *starts*, and a scenario about a read still running
        // could not see one counted at the end.
        *self.started.lock().expect("the count") += 1;
        if let Some(gate) = &self.gate {
            let _ = gate.lock().expect("the gate").recv();
        }
        Ok(self.report.clone())
    }

    fn graph(&self, _spec: &Path) -> Result<String, String> {
        Ok(self.graph.clone())
    }

    // T6's scenarios are the two reads and the loop's cadences, and none of
    // the keys they press is a lever. A fixture that answered one would be
    // hiding a pass that pulled it; the levers are T7's, and `Levers` below
    // is what answers them.
    fn kill(&self, session: &str) -> Result<(), String> {
        panic!("a board about the cadences killed {session}")
    }

    fn resume(&self, task: &str) -> Result<String, String> {
        panic!("a board about the cadences resumed {task}")
    }

    fn in_tmux(&self) -> bool {
        false
    }

    fn attach(&self, session: &str, _inside: bool) -> Result<(), String> {
        panic!("a board about the cadences attached to {session}")
    }
}

/// A keyboard with a script: each pass takes the next entry, and a script
/// that has run out is a keyboard nobody is at.
///
/// Every wait's timeout is kept, because the tick is a promise about how
/// long the board sits still between reads of the streams.
#[derive(Debug, Default)]
struct Script {
    woke: VecDeque<Woke>,
    waited: Vec<Duration>,
}

impl Script {
    fn of(codes: &[KeyCode]) -> Self {
        Self {
            woke: codes.iter().map(|code| Woke::Key(press(*code))).collect(),
            waited: Vec::new(),
        }
    }

    /// A keyboard that keeps waking the loop with something that is not a
    /// key — a window being dragged across the board.
    fn resizing(times: usize) -> Self {
        Self {
            woke: std::iter::repeat_n(Woke::Other, times).collect(),
            waited: Vec::new(),
        }
    }
}

impl Events for Script {
    fn next(&mut self, timeout: Duration) -> Result<Woke, String> {
        self.waited.push(timeout);
        Ok(self.woke.pop_front().unwrap_or(Woke::Elapsed))
    }
}

/// A keyboard that falls over, which is how a board in the alternate screen
/// comes to panic.
struct Falls;

impl Events for Falls {
    fn next(&mut self, _timeout: Duration) -> Result<Woke, String> {
        panic!("the board fell over");
    }
}

/// A screen that records what was asked of it, into whatever list it was
/// given — the panic scenario hands it the same one the hooks write to, so
/// that the order of all four events is one assertion.
#[derive(Debug, Clone)]
struct Recorder(Arc<Mutex<Vec<&'static str>>>);

impl Screen for Recorder {
    fn enter(&mut self) -> std::io::Result<()> {
        self.0.lock().expect("the recorder").push("enter");
        Ok(())
    }

    fn leave(&mut self) -> std::io::Result<()> {
        self.0.lock().expect("the recorder").push("leave");
        Ok(())
    }
}

fn press(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

/// A terminal to draw the passes on — the same `TestBackend` T5's frames go
/// through, wide enough that no column of these rows is dropped.
fn surface() -> Terminal<TestBackend> {
    Terminal::new(TestBackend::new(BOARD.0, BOARD.1)).expect("a terminal to draw on")
}

/// The size of that terminal, so a scenario reading back what a pass drew
/// asks about the same rectangle it was drawn on.
const BOARD: (u16, u16) = (140, 20);

/// What is on a terminal now, read back as lines with the trailing blanks
/// cut.
///
/// T5's [`drawn`] renders a board and reads it back in one call, which is
/// what a scenario about a frame wants. A scenario about the loop wants the
/// other half of that: what a *pass* put there, on a terminal it did not
/// build itself — otherwise a `draw` that drew nothing reads exactly like
/// one that drew the right thing.
fn lines_of(terminal: &Terminal<TestBackend>, width: u16, height: u16) -> Vec<String> {
    let buffer = terminal.backend().buffer().clone();
    (0..height)
        .map(|y| {
            (0..width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect()
}

/// The board the loop is driven over: the report a scenario describes, and
/// the two reads behind it.
fn app_over(reads: &Arc<Reads>, report: &str) -> App {
    let status = keeler_top::status::parse(report).expect("the fixture's report has a header");
    App::new(
        Arc::clone(reads) as Arc<dyn Dispatch>,
        PathBuf::from("/nowhere"),
        status,
        Vec::new(),
        now(NOON),
        THEME,
    )
}

/// One running task's report line, pointing at the stream the fixture wrote.
fn running(id: &str, log: &str) -> String {
    format!("{id:<6} running          log {log}  worktree /nowhere")
}

/// Passes over the board until the read that is out has come back, so what
/// follows is asserted about a board holding the answer rather than one
/// still waiting for it.
fn settle(app: &mut App, feed: &mut StatusFeed, now: Timestamp) {
    for _ in 0..500 {
        step(&mut surface(), &mut Script::default(), app, feed, now).expect("a pass");
        if !feed.pending() {
            return;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    panic!("keeler-status never answered");
}

impl Runfiles {
    /// Adds a record to a task's stream, as a run writes one while the board
    /// is up.
    fn append(&self, tid: &str, line: &str) {
        use std::io::Write as _;
        let mut stream = std::fs::OpenOptions::new()
            .append(true)
            .open(self.0.join(format!("{tid}.stream")))
            .expect("the fixture's stream");
        writeln!(stream, "{line}").expect("the fixture's stream");
    }
}

#[test]
fn j_and_k_move_the_selection_and_the_detail_pane_follows() {
    // Given the board shows T1..T3 with T1 selected
    let runfiles = Runfiles::new("t6-selection");
    let lines: Vec<String> = (1..=3)
        .map(|task| {
            let log = runfiles.stream(
                &format!("t{task}"),
                &[
                    INIT.to_string(),
                    stamped_tool_use(
                        "toolu_1",
                        "Bash",
                        serde_json::json!({ "command": format!("just test t{task}") }),
                    ),
                ],
            );
            running(&format!("T{task}"), &log)
        })
        .collect();
    let mut app = app_over(&Arc::new(Reads::answering("")), &report(&lines));
    assert_eq!(app.board.selected, 0, "the board opened on some other row");

    // When the user presses j twice then k once
    for code in [KeyCode::Char('j'), KeyCode::Char('j'), KeyCode::Char('k')] {
        assert_eq!(on_key(&mut app, press(code)), Action::Nothing);
    }

    // Then T2 is selected and the detail pane shows T2
    assert_eq!(app.board.selected, 1);
    let frame = drawn(&app.board, 140, 20);
    let pane = |id: &str| {
        frame
            .iter()
            .any(|line| line.starts_with(&format!("{id} —")))
    };
    assert!(
        pane("T2") && !pane("T1") && !pane("T3"),
        "the pane did not follow the selection:\n{}",
        frame.join("\n"),
    );
    // And it is that task's own run in the pane, not the row above's.
    assert!(
        frame.iter().any(|line| line.contains("just test t2")),
        "the pane shows another task's work:\n{}",
        frame.join("\n"),
    );
}

#[test]
fn the_stream_is_re_read_once_a_second_without_input() {
    // Given T1's stream gains a new tool_use record after the board is up
    let runfiles = Runfiles::new("t6-tick");
    let log = runfiles.stream(
        "t1",
        &[
            INIT.to_string(),
            stamped_tool_use(
                "toolu_1",
                "Bash",
                serde_json::json!({"command": "just dev"}),
            ),
        ],
    );
    let reads = Arc::new(Reads::answering(""));
    let mut app = app_over(&reads, &report(&[running("T1", &log)]));
    let mut feed = StatusFeed::new(Arc::clone(&reads) as Arc<dyn Dispatch>, now(NOON));
    assert!(row_of(&drawn(&app.board, 140, 20), "T1").contains("just dev"));
    runfiles.append(
        "t1",
        &stamped_tool_use(
            "toolu_2",
            "Bash",
            serde_json::json!({"command": "just mutants-diff main"}),
        ),
    );

    // When one second passes
    //
    // Two passes, because a pass draws what it has and then waits: the first
    // wait is the second passing, and the frame carrying what arrived in it
    // is the one the pass after draws. Read back from the terminal the
    // passes drew on rather than rendered again here — a `draw` that drew
    // nothing must not read like one that drew the right thing.
    let mut terminal = surface();
    let mut script = Script::default();
    for _ in 0..2 {
        assert_eq!(
            step(&mut terminal, &mut script, &mut app, &mut feed, now(NOON)),
            Ok(true),
        );
    }

    // Then the tool column shows the new command
    assert_eq!(
        script.waited,
        [TICK, TICK],
        "the board did not wait exactly one tick for a key",
    );
    assert_eq!(TICK, Duration::from_secs(1), "the tick is not a second");
    let frame = lines_of(&terminal, BOARD.0, BOARD.1);
    let row = row_of(&frame, "T1").to_string();
    assert!(
        row.contains("just mutants-diff main"),
        "the frame is a second out of date:\n{}",
        frame.join("\n"),
    );
    assert_eq!(app.board.rows[0].stage_column(), "mutants");
}

/// Not a scenario of its own: it is the other half of the one above, and it
/// is what the review found there. A tick is a `git show`, a `bash` and four
/// `git` calls per task, and a board that ticked on every event the terminal
/// sent would run all of that dozens of times a second while somebody
/// dragged a window edge across it. The wait running out is the tick;
/// everything else is worth a frame.
#[test]
fn a_window_being_dragged_is_frames_and_not_ticks() {
    let runfiles = Runfiles::new("t6-resize");
    let log = runfiles.stream("t1", &[INIT.to_string()]);
    let reads = Arc::new(Reads::answering(""));
    let mut app = app_over(&reads, &report(&[running("T1", &log)]));
    let mut feed = StatusFeed::new(Arc::clone(&reads) as Arc<dyn Dispatch>, now(NOON));
    let mut terminal = surface();
    // One more than the passes below, so the pass that does tick ticks
    // because a second has gone by and not because the script ran out.
    let mut resizes = Script::resizing(4);
    runfiles.append(
        "t1",
        &stamped_tool_use(
            "toolu_1",
            "Bash",
            serde_json::json!({"command": "just dev"}),
        ),
    );

    for _ in 0..3 {
        assert_eq!(
            step(&mut terminal, &mut resizes, &mut app, &mut feed, now(NOON)),
            Ok(true),
        );
    }

    assert_eq!(
        app.board.rows[0].stage_column(),
        "reading",
        "a resize read the streams, and with them the graph and four git calls a task",
    );
    // And the second that passes while the window is being dragged is still
    // a tick: the columns must not freeze for as long as somebody holds the
    // mouse down.
    assert_eq!(
        step(
            &mut terminal,
            &mut resizes,
            &mut app,
            &mut feed,
            now("2026-09-07T12:00:01.000Z"),
        ),
        Ok(true),
    );
    assert_eq!(app.board.rows[0].stage_column(), "qa");
}

#[test]
fn keeler_status_is_re_read_every_five_seconds_and_on_r() {
    // Given keeler-status's last answer is two seconds old
    let answer = report(&["T1     not spawned".to_string()]);
    let reads = Arc::new(Reads::answering(&answer));
    let mut app = app_over(&reads, &answer);
    let mut feed = StatusFeed::new(Arc::clone(&reads) as Arc<dyn Dispatch>, now(NOON));
    let two = now("2026-09-07T12:00:02.000Z");
    step(
        &mut surface(),
        &mut Script::default(),
        &mut app,
        &mut feed,
        two,
    )
    .expect("a pass");
    assert_eq!(
        reads.started(),
        0,
        "a two-second-old answer was asked again"
    );

    // When the user presses r
    step(
        &mut surface(),
        &mut Script::of(&[KeyCode::Char('r')]),
        &mut app,
        &mut feed,
        two,
    )
    .expect("a pass");
    settle(&mut app, &mut feed, two);

    // Then keeler-status runs again and the header's age resets
    assert_eq!(reads.started(), 1, "r asked nobody anything");
    assert!(
        app.board.header(two).ends_with("status 0s ago"),
        "the header still shows the age of the answer r replaced: {}",
        app.board.header(two),
    );

    // And without r it runs again when the answer is five seconds old
    let four_seconds_old = now("2026-09-07T12:00:06.000Z");
    step(
        &mut surface(),
        &mut Script::default(),
        &mut app,
        &mut feed,
        four_seconds_old,
    )
    .expect("a pass");
    assert_eq!(
        reads.started(),
        1,
        "the answer was replaced at four seconds"
    );
    let five_seconds_old = now("2026-09-07T12:00:07.000Z");
    step(
        &mut surface(),
        &mut Script::default(),
        &mut app,
        &mut feed,
        five_seconds_old,
    )
    .expect("a pass");
    assert_eq!(
        reads.started(),
        2,
        "a five-second-old answer was left to stand"
    );
}

#[test]
fn a_slow_keeler_status_does_not_stall_the_board() {
    // Given keeler-status takes three seconds to answer
    let runfiles = Runfiles::new("t6-slow");
    let log = runfiles.stream("t1", &[INIT.to_string()]);
    let (slow, opener) = Reads::slow(&report(&[running("T1", &log)]));
    let reads = Arc::new(slow);
    let mut app = app_over(&reads, &report(&[running("T1", &log)]));
    let mut feed = StatusFeed::new(Arc::clone(&reads) as Arc<dyn Dispatch>, now(NOON));
    // Old enough that the cadence asks on the first pass below.
    let later = now("2026-09-07T12:00:30.000Z");

    // When the board is up
    // Then the stream columns keep refreshing every second while it runs
    for (call, command) in ["just dev", "just mutants-diff main", "just keeler-branch"]
        .into_iter()
        .enumerate()
    {
        runfiles.append(
            "t1",
            &stamped_tool_use(
                &format!("toolu_{call}"),
                "Bash",
                serde_json::json!({ "command": command }),
            ),
        );
        step(
            &mut surface(),
            &mut Script::default(),
            &mut app,
            &mut feed,
            later,
        )
        .expect("a pass");
        assert!(
            row_of(&drawn(&app.board, 140, 20), "T1").contains(command),
            "the streams stopped while the report was being waited for",
        );
        assert!(feed.pending(), "the fixture's slow read answered early");
    }
    // And what is out is one read, not one per pass: a machine already too
    // slow to answer in five seconds must not be given a `just` a second.
    assert_eq!(reads.started(), 1, "the board asked a slow recipe again");

    opener.send(()).expect("the read is still waiting");
    settle(&mut app, &mut feed, later);
    assert!(
        app.board.header(later).ends_with("status 0s ago"),
        "the answer arrived and the header did not take it",
    );
}

/// Not a scenario of its own either: the tick reads the graph as well as the
/// streams, and every fixture above stands outside a repository, where that
/// read refuses. This is the other side — a project the read can answer
/// about, so that a tick moving the state column, and a refusal that has
/// gone taking its sentence with it, are both seen at least once.
#[test]
fn a_tick_that_could_read_the_graph_takes_its_word_and_its_refusal_back() {
    let project = Project::new("t6-graph");
    project.with_spec_on_the_feature_branch();
    let reads = Arc::new(Reads {
        graph: "T1 ready\nT2 blocked T1\n".to_string(),
        ..Reads::answering("")
    });
    let status = keeler_top::status::parse(&report(&[
        "T1     not spawned".to_string(),
        "T2     not spawned".to_string(),
    ]))
    .expect("the fixture's report has a header");
    let mut app = App::new(
        Arc::clone(&reads) as Arc<dyn Dispatch>,
        project.root(),
        status,
        Vec::new(),
        now(NOON),
        THEME,
    );
    // A refusal to be taken away: the first tick reads a ref the project
    // does not have, and the second reads the one it does.
    let mut lost = App::new(
        Arc::clone(&reads) as Arc<dyn Dispatch>,
        PathBuf::from("/nowhere"),
        keeler_top::status::parse(&report(&["T1     not spawned".to_string()])).expect("a report"),
        Vec::new(),
        now(NOON),
        THEME,
    );
    lost.tick(now(NOON));
    assert!(
        !lost.board.message.is_empty(),
        "a graph read outside a repository said nothing about it",
    );

    app.tick(now(NOON));

    assert_eq!(
        app.board.message, "",
        "a read that answered still complained"
    );
    assert_eq!(
        app.board
            .rows
            .iter()
            .map(|row| row.state.as_str())
            .collect::<Vec<_>>(),
        ["ready", "blocked ← T1"],
        "the tick did not take the graph's word for the two it has one for",
    );
}

#[test]
fn q_quits_and_restores_the_terminal() {
    // Given the board is up
    let screen = Arc::new(Mutex::new(Vec::new()));
    let answer = report(&["T1     done".to_string(), "T2     done".to_string()]);
    let reads = Arc::new(Reads::answering(&answer));
    let mut app = app_over(&reads, &answer);
    let mut feed = StatusFeed::new(Arc::clone(&reads) as Arc<dyn Dispatch>, now(NOON));
    let mut script = Script::of(&[KeyCode::Char('j'), KeyCode::Char('q')]);

    // When the user presses q
    let ended = {
        let _guard = Guard::new(Recorder(Arc::clone(&screen))).expect("the recorder entered");
        let ended = looping(&mut surface(), &mut script, &mut app, &mut feed);
        assert_eq!(
            *screen.lock().expect("the recorder"),
            ["enter"],
            "the board gave the screen back while it was still drawing on it",
        );
        ended
    };

    // Then the process exits 0 and the terminal is restored to the screen it
    // was on
    assert_eq!(
        ended,
        Ok(()),
        "the board ended on an error rather than on the key",
    );
    assert_eq!(
        *screen.lock().expect("the recorder"),
        ["enter", "leave"],
        "the terminal was left in the board's own screen",
    );
    // And the key before it was acted on rather than skipped past.
    assert_eq!(app.board.selected, 1);
}

#[test]
fn a_panic_restores_the_terminal_before_the_message_is_printed() {
    // Given the board is up in the alternate screen
    //
    // The panic hook is process-wide, and nextest gives every test a process
    // of its own — which is what makes installing one here safe.
    let order = Arc::new(Mutex::new(Vec::new()));
    let printing = Arc::clone(&order);
    std::panic::set_hook(Box::new(move |_| {
        printing.lock().expect("the order").push("printed");
    }));
    let restoring = Arc::clone(&order);
    keeler_top::terminal::restore_on_panic(move || {
        restoring.lock().expect("the order").push("restored");
    });
    let answer = report(&["T1     done".to_string()]);
    let reads = Arc::new(Reads::answering(&answer));
    let mut app = app_over(&reads, &answer);
    let mut feed = StatusFeed::new(Arc::clone(&reads) as Arc<dyn Dispatch>, now(NOON));

    // When the process panics
    let ended = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = Guard::new(Recorder(Arc::clone(&order))).expect("the recorder entered");
        looping(&mut surface(), &mut Falls, &mut app, &mut feed)
    }));

    // Then the terminal leaves raw mode and the alternate screen
    // And the panic message is printed on the restored screen
    assert!(ended.is_err(), "the fixture did not panic");
    // Read out before asserting, never asserted on through the guard: the
    // hooks above lock this list, and a failing `assert_eq!` holding the
    // lock would panic into a hook that waits for it — a deadlock where a
    // test failure should be.
    let seen = order.lock().expect("the order").clone();
    assert_eq!(
        seen,
        ["enter", "restored", "printed"],
        "the message was printed onto a screen about to be thrown away",
    );
    // And the guard unwinding past does not give the same screen back a
    // second time: `ESC[?1049l` twice restores the cursor to where the entry
    // saved it, which is above the message the first one made room for.
}

// ── T7

use keeler_top::app::Live;
use keeler_top::dispatch::{attach_command, handed, kill_command};

/// What the board asked the world for, in the order it asked.
///
/// One list for the levers and the screen together, because half of these
/// scenarios are about that order: the screen is given back *before* tmux
/// takes the terminal and taken again after, and the kill is made *before*
/// the marker that claims it.
#[derive(Debug, Clone, Default)]
struct Tally(Arc<Mutex<Vec<String>>>);

impl Tally {
    fn note(&self, what: &str) {
        self.0.lock().expect("the tally").push(what.to_string());
    }

    fn seen(&self) -> Vec<String> {
        self.0.lock().expect("the tally").clone()
    }
}

impl Screen for Tally {
    fn enter(&mut self) -> std::io::Result<()> {
        self.note("enter");
        Ok(())
    }

    fn leave(&mut self) -> std::io::Result<()> {
        self.note("leave");
        Ok(())
    }
}

/// The three levers, recorded rather than run.
///
/// A pause is a `tmux kill-session`, a resume is a `just`, and an attach is
/// a program that takes the terminal away from the suite running it — none
/// of which a test may actually do. What is watched here is which lever the
/// board pulled and what it did with the answer; the argv each one composes
/// is `Shell`'s, and the scenarios below read that from [`kill_command`],
/// [`attach_command`] and `Shell::resume_command` rather than describing it.
struct Levers {
    tally: Tally,
    /// What `keeler-status` would print now — a closure rather than a
    /// string, because two of these scenarios are about a report that
    /// changes: the recipe reads the marker every time it runs, and so does
    /// this.
    report: Box<dyn Fn() -> String + Send + Sync>,
    kill: Result<(), String>,
    resume: Result<String, String>,
    attach: Result<(), String>,
    /// Whether the board is itself drawing inside tmux.
    inside: bool,
    /// What happens while the board is away, in tmux's place: the run it
    /// attached to goes on writing to its stream, and what it wrote is what
    /// the frame after the attach has to carry.
    meanwhile: Option<Box<dyn Fn() + Send + Sync>>,
}

impl Levers {
    /// Levers that all succeed, over a report that does not change.
    fn answering(tally: &Tally, report: String) -> Self {
        Self {
            tally: tally.clone(),
            report: Box::new(move || report.clone()),
            kill: Ok(()),
            resume: Ok(String::new()),
            attach: Ok(()),
            inside: false,
            meanwhile: None,
        }
    }
}

impl Dispatch for Levers {
    fn status(&self) -> Result<String, String> {
        Ok((self.report)())
    }

    fn graph(&self, _spec: &Path) -> Result<String, String> {
        Ok(String::new())
    }

    fn kill(&self, session: &str) -> Result<(), String> {
        self.tally.note(&format!("kill {session}"));
        self.kill.clone()
    }

    fn resume(&self, task: &str) -> Result<String, String> {
        self.tally.note(&format!("resume {task}"));
        self.resume.clone()
    }

    fn in_tmux(&self) -> bool {
        self.inside
    }

    fn attach(&self, session: &str, inside: bool) -> Result<(), String> {
        self.tally
            .note(&format!("attach {session} inside={inside}"));
        if let Some(meanwhile) = &self.meanwhile {
            meanwhile();
        }
        self.attach.clone()
    }
}

/// One task's line of the report, in whatever state the scenario wants it —
/// the recipe's own `printf`, with the paths every state but two carries.
fn state_line(id: &str, state: &str, log: &str) -> String {
    format!("{id:<6} {state:<16} log {log}  worktree /nowhere")
}

/// The board over one set of answers, and the feed beside it.
fn board_over(levers: &Arc<Levers>) -> (App, StatusFeed) {
    let report = levers.status().expect("the fixture answers");
    let status = keeler_top::status::parse(&report).expect("the fixture's report has a header");
    let app = App::new(
        Arc::clone(levers) as Arc<dyn Dispatch>,
        PathBuf::from("/nowhere"),
        status,
        Vec::new(),
        now(NOON),
        THEME,
    );
    let feed = StatusFeed::new(Arc::clone(levers) as Arc<dyn Dispatch>, now(NOON));
    (app, feed)
}

/// The screen the board holds while these scenarios run: the terminal T5's
/// frames go through, and a guard over a screen that records.
fn held(tally: &Tally) -> Live<Tally, TestBackend> {
    Live::new(
        surface(),
        Guard::new(tally.clone()).expect("the tally entered"),
    )
}

/// One pass of the loop with one key pressed — which is what carries an
/// action out, the key deciding only what it is.
fn pressed(
    screen: &mut Live<Tally, TestBackend>,
    app: &mut App,
    feed: &mut StatusFeed,
    code: KeyCode,
) {
    step(screen, &mut Script::of(&[code]), app, feed, now(NOON)).expect("a pass");
}

/// The board after another read of `keeler-status`: what `r` asks for, and
/// what the cadence asks for by itself five seconds later.
fn refreshed(screen: &mut Live<Tally, TestBackend>, app: &mut App, feed: &mut StatusFeed) {
    pressed(screen, app, feed, KeyCode::Char('r'));
    settle(app, feed, now(NOON));
}

/// Every marker in a run directory, which is what "no file is written"
/// asks about.
fn markers(runfiles: &Runfiles) -> Vec<String> {
    std::fs::read_dir(&runfiles.0)
        .expect("the run directory")
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".paused"))
        .collect()
}

/// The report `keeler-status` prints for a run whose session is gone with
/// no verdict: `died`, or `paused` when the marker is beside the log.
///
/// The recipe's own reading, mirrored — that it reads the file is T8's
/// scenario, pinned against the shipped recipe in `tests/top_recipes.rs`.
/// What these scenarios are about is the board writing the file the recipe
/// reads, and the row that follows from it.
fn as_the_marker_has_it(log: &str) -> Box<dyn Fn() -> String + Send + Sync> {
    let marker = PathBuf::from(log).with_extension("paused");
    let log = log.to_string();
    Box::new(move || {
        let state = if marker.exists() { "paused" } else { "died" };
        report(&[state_line("T1", state, &log)])
    })
}

#[test]
fn p_kills_the_selected_running_task_then_marks_it() {
    // Given T1 is running in tmux session keeler-01-foo-t1 and its row is
    // selected
    let runfiles = Runfiles::new("t7-pause");
    let log = runfiles.stream("t1", &[INIT.to_string()]);
    let marker = PathBuf::from(&log).with_extension("paused");
    let tally = Tally::default();
    let gone = as_the_marker_has_it(&log);
    let running = report(&[state_line("T1", "running", &log)]);
    let levers = Arc::new(Levers {
        // The recipe's answer, before and after: a session that is up is
        // `running` whatever is on disk, and the marker is only read once
        // the session is gone — which is what the kill makes it.
        report: Box::new(move || {
            if marker.exists() {
                gone()
            } else {
                running.clone()
            }
        }),
        ..Levers::answering(&tally, String::new())
    });
    let (mut app, mut feed) = board_over(&levers);
    let mut screen = held(&tally);
    assert_eq!(app.board.rows[0].state, "running");

    // When the user presses p
    pressed(&mut screen, &mut app, &mut feed, KeyCode::Char('p'));

    // Then `tmux kill-session -t =keeler-01-foo-t1` runs
    assert_eq!(tally.seen(), ["enter", "kill keeler-01-foo-t1"]);
    let command = kill_command("keeler-01-foo-t1");
    assert_eq!(command.get_program(), "tmux");
    assert_eq!(
        command.get_args().collect::<Vec<_>>(),
        ["kill-session", "-t", "=keeler-01-foo-t1"],
        "the session is named loosely — `=` is what keeps t1 from matching t10",
    );

    // And .keeler/runs/01-foo/t1.paused is written after it succeeds
    let marker = PathBuf::from(&log).with_extension("paused");
    assert!(
        marker.exists(),
        "the marker keeler-status reads is not beside the log it names",
    );

    // And T1's row reads "paused" on the next refresh
    refreshed(&mut screen, &mut app, &mut feed);
    assert_eq!(app.board.rows[0].state, "paused");
}

#[test]
fn a_kill_that_fails_writes_no_marker() {
    // Given T1's row is selected and the tmux stub fails on kill-session
    let runfiles = Runfiles::new("t7-kill-fails");
    let log = runfiles.stream("t1", &[INIT.to_string()]);
    let tally = Tally::default();
    let levers = Arc::new(Levers {
        kill: Err("can't find session: =keeler-01-foo-t1".to_string()),
        ..Levers::answering(&tally, report(&[state_line("T1", "running", &log)]))
    });
    let (mut app, mut feed) = board_over(&levers);
    let mut screen = held(&tally);

    // When the user presses p
    pressed(&mut screen, &mut app, &mut feed, KeyCode::Char('p'));

    // Then no marker is written and the status line carries tmux's reason
    assert_eq!(
        markers(&runfiles),
        Vec::<String>::new(),
        "the board claimed a pause its kill did not make",
    );
    assert_eq!(app.board.message, "can't find session: =keeler-01-foo-t1");
    // And a tick later it still is — `p` is pressed exactly when something
    // is going wrong, and the answer must outlive the second it was given
    // in.
    step(
        &mut screen,
        &mut Script::default(),
        &mut app,
        &mut feed,
        now("2026-09-07T12:00:01.000Z"),
    )
    .expect("a pass");
    assert_eq!(app.board.message, "can't find session: =keeler-01-foo-t1");
}

#[test]
fn p_on_a_task_that_is_not_running_does_nothing() {
    // Given T2's row is selected and T2 is not spawned
    let runfiles = Runfiles::new("t7-not-running");
    let log = runfiles.stream("t1", &[INIT.to_string()]);
    let tally = Tally::default();
    let levers = Arc::new(Levers::answering(
        &tally,
        report(&[
            state_line("T1", "running", &log),
            "T2     not spawned".to_string(),
        ]),
    ));
    let (mut app, mut feed) = board_over(&levers);
    let mut screen = held(&tally);
    pressed(&mut screen, &mut app, &mut feed, KeyCode::Char('j'));
    assert_eq!(app.board.selected, 1, "the fixture is about the wrong row");

    // When the user presses p
    pressed(&mut screen, &mut app, &mut feed, KeyCode::Char('p'));

    // Then no file is written and the status line says T2 is not running
    assert_eq!(
        tally.seen(),
        ["enter"],
        "a task with no session was killed anyway",
    );
    assert_eq!(markers(&runfiles), Vec::<String>::new());
    assert!(
        app.board.message.contains("T2") && app.board.message.contains("not running"),
        "the status line does not say why nothing happened: {}",
        app.board.message,
    );
}

#[test]
fn a_session_killed_without_the_marker_is_still_died() {
    // Given T1's tmux session is gone, no .exit file and no .paused marker
    let runfiles = Runfiles::new("t7-died");
    let log = runfiles.stream("t1", &[INIT.to_string()]);
    let tally = Tally::default();
    let levers = Arc::new(Levers {
        report: as_the_marker_has_it(&log),
        ..Levers::answering(&tally, String::new())
    });
    let (app, _feed) = board_over(&levers);

    // When the board renders
    // Then T1's state is "died"
    assert_eq!(
        app.board.rows[0].state, "died",
        "a kill that bypassed the board was read as one the board made",
    );
    assert_eq!(markers(&runfiles), Vec::<String>::new());
    assert!(row_of(&drawn(&app.board, 140, 20), "T1").contains("died"));
}

#[test]
fn r_resumes_a_paused_or_died_task() {
    // Given T1 is paused with its marker on disk and its row selected
    let runfiles = Runfiles::new("t7-resume");
    let log = runfiles.stream("t1", &[INIT.to_string()]);
    std::fs::write(PathBuf::from(&log).with_extension("paused"), "").unwrap();
    let tally = Tally::default();
    let watching = tally.clone();
    let paused = log.clone();
    let levers = Arc::new(Levers {
        // The recipe starts the session before it answers, so a report read
        // after the resume is a report about a task that is running.
        report: Box::new(move || {
            let state = if watching.seen().iter().any(|call| call == "resume T1") {
                "running"
            } else {
                "paused"
            };
            report(&[state_line("T1", state, &paused)])
        }),
        resume: Ok(
            "keeler-resume: re-running T1 in the worktree and branch it already has\n  \
             worktree: /w/repo-01-foo-t1\n"
                .to_string(),
        ),
        ..Levers::answering(&tally, String::new())
    });
    let (mut app, mut feed) = board_over(&levers);
    let mut screen = held(&tally);
    assert_eq!(app.board.rows[0].state, "paused");

    // When the user presses R
    pressed(&mut screen, &mut app, &mut feed, KeyCode::Char('R'));

    // Then `keeler-resume specs/01-foo.md T1` runs through the plugin's
    // Justfile
    assert_eq!(tally.seen(), ["enter", "resume T1"]);
    let shell = Shell::new("/p", "/r", "specs/01-foo.md");
    let command = shell.resume_command("T1");
    assert_eq!(command.get_program(), "just");
    assert_eq!(
        command.get_args().collect::<Vec<_>>(),
        [
            "--justfile".as_ref(),
            Path::new("/p/Justfile").as_os_str(),
            "--working-directory".as_ref(),
            Path::new("/r").as_os_str(),
            "-q".as_ref(),
            "keeler-resume".as_ref(),
            "specs/01-foo.md".as_ref(),
            "T1".as_ref(),
        ],
    );
    // And the recipe's own first sentence is what the watcher is told.
    assert_eq!(
        app.board.message,
        "keeler-resume: re-running T1 in the worktree and branch it already has",
    );

    // And T1's row reads "running" on the next refresh
    refreshed(&mut screen, &mut app, &mut feed);
    assert_eq!(app.board.rows[0].state, "running");
}

/// Not a scenario of its own: it is the other half of the argv above. A
/// board that composed the right command and ran something else would pass
/// every assertion made against `resume_command`, so the recipe is reached
/// here for real — the plugin's Justfile, the project as working directory,
/// and the two arguments in the order the recipe takes them.
#[test]
fn the_resume_the_board_asks_for_is_the_plugins_recipe_run_in_the_project() {
    let project = Project::new("t7-resume-recipe");
    let plugin = project.0.join("plugin");
    write(
        &plugin,
        "Justfile",
        "keeler-resume SPEC TASK:\n    #!/usr/bin/env bash\n    \
         echo \"keeler-resume: re-running {{TASK}} of {{SPEC}}\"\n    pwd -P\n",
    );
    let shell = Shell::new(&plugin, project.root(), "specs/01-foo.md");

    let said = shell.resume("T1").expect("the fixture recipe answers");

    assert!(
        said.contains("keeler-resume: re-running T1 of specs/01-foo.md"),
        "the recipe that answered was not the plugin's:\n{said}",
    );
    let ran_in = std::fs::canonicalize(project.root()).unwrap();
    assert!(
        said.contains(&format!("{}\n", ran_in.display())),
        "the recipe ran somewhere other than the project root:\n{said}",
    );
}

#[test]
fn r_on_a_task_that_is_not_resumable_shows_keeler_resumes_refusal() {
    // Given T1 is running and its row selected
    let runfiles = Runfiles::new("t7-not-resumable");
    let log = runfiles.stream("t1", &[INIT.to_string()]);
    let tally = Tally::default();
    let refusal = "keeler-resume: T1 is still running — attach with tmux attach -t \
                   '=keeler-01-foo-t1', or let it finish";
    let levers = Arc::new(Levers {
        resume: Err(refusal.to_string()),
        ..Levers::answering(&tally, report(&[state_line("T1", "running", &log)]))
    });
    let (mut app, mut feed) = board_over(&levers);
    let mut screen = held(&tally);

    // When the user presses R
    pressed(&mut screen, &mut app, &mut feed, KeyCode::Char('R'));

    // Then the status line shows keeler-resume's reason and nothing is
    // spawned
    assert_eq!(app.board.message, refusal);
    assert_eq!(
        tally.seen(),
        ["enter", "resume T1"],
        "the board decided for itself what was resumable",
    );
    assert_eq!(app.board.rows[0].state, "running");

    // And it is still there a second later. Every tick re-assembles the
    // board and the line under it, and a sentence drawn once and blanked
    // before the next second is one nobody read.
    step(
        &mut screen,
        &mut Script::default(),
        &mut app,
        &mut feed,
        now("2026-09-07T12:00:01.000Z"),
    )
    .expect("a pass");
    assert_eq!(app.board.message, refusal, "the tick took the answer away");
}

#[test]
fn enter_attaches_to_the_selected_running_task_and_returns_on_detach() {
    // Given T1 is running, its row selected, and $TMUX is unset
    let runfiles = Runfiles::new("t7-attach");
    let log = runfiles.stream("t1", &[INIT.to_string()]);
    let tally = Tally::default();
    let writing = runfiles.0.join("t1.stream");
    let levers = Arc::new(Levers {
        inside: false,
        // The run goes on working while nobody is reading the board.
        meanwhile: Some(Box::new(move || {
            use std::io::Write as _;
            let mut stream = std::fs::OpenOptions::new()
                .append(true)
                .open(&writing)
                .expect("the fixture's stream");
            writeln!(
                stream,
                "{}",
                stamped_tool_use(
                    "toolu_1",
                    "Bash",
                    serde_json::json!({"command": "just dev"})
                ),
            )
            .expect("the fixture's stream");
        })),
        ..Levers::answering(&tally, report(&[state_line("T1", "running", &log)]))
    });
    let (mut app, mut feed) = board_over(&levers);
    let mut screen = held(&tally);
    assert_eq!(app.board.rows[0].stage_column(), "reading");

    // When the user presses Enter
    pressed(&mut screen, &mut app, &mut feed, KeyCode::Enter);

    // Then the board leaves the alternate screen and runs
    // `tmux attach -t =keeler-01-foo-t1`
    assert_eq!(
        tally.seen(),
        [
            "enter",
            "leave",
            "attach keeler-01-foo-t1 inside=false",
            "enter",
        ],
        "tmux was handed a terminal the board was still drawing on",
    );
    let command = attach_command("keeler-01-foo-t1", false);
    assert_eq!(command.get_program(), "tmux");
    assert_eq!(
        command.get_args().collect::<Vec<_>>(),
        ["attach", "-t", "=keeler-01-foo-t1"],
    );

    // And when tmux exits the board redraws in the alternate screen with its
    // state refreshed
    assert_eq!(
        app.board.rows[0].stage_column(),
        "qa",
        "the board came back showing what the run was doing before it left",
    );
    assert!(row_of(&drawn(&app.board, 140, 20), "T1").contains("just dev"));
}

#[test]
fn inside_tmux_enter_switches_the_client_instead() {
    // Given $TMUX is set and T1 is running with its row selected
    let runfiles = Runfiles::new("t7-switch");
    let log = runfiles.stream("t1", &[INIT.to_string()]);
    let tally = Tally::default();
    let levers = Arc::new(Levers {
        inside: true,
        ..Levers::answering(&tally, report(&[state_line("T1", "running", &log)]))
    });
    let (mut app, mut feed) = board_over(&levers);
    let mut screen = held(&tally);

    // When the user presses Enter
    pressed(&mut screen, &mut app, &mut feed, KeyCode::Enter);

    // Then the board runs `tmux switch-client -t =keeler-01-foo-t1` and
    // stays up
    assert_eq!(
        tally.seen(),
        ["enter", "attach keeler-01-foo-t1 inside=true"],
        "the board gave away a screen tmux was not going to take",
    );
    let command = attach_command("keeler-01-foo-t1", true);
    assert_eq!(command.get_program(), "tmux");
    assert_eq!(
        command.get_args().collect::<Vec<_>>(),
        ["switch-client", "-t", "=keeler-01-foo-t1"],
    );
}

#[test]
fn enter_without_tmux_says_so_in_the_status_line() {
    // Given tmux is not on PATH and T1's row is selected
    //
    // The sentence is not the fixture's: it is what the board's own way of
    // running a program on its terminal gives back when there is no such
    // program. What this scenario is about is that it reaches the status
    // line rather than ending the board.
    let missing = handed(std::process::Command::new("keeler-top-no-such-tmux"))
        .expect_err("there is no such program");
    assert!(
        missing.contains("keeler-top-no-such-tmux") && missing.contains("not installed"),
        "the refusal does not say the program is missing: {missing}",
    );
    let runfiles = Runfiles::new("t7-no-tmux");
    let log = runfiles.stream("t1", &[INIT.to_string()]);
    let tally = Tally::default();
    let levers = Arc::new(Levers {
        attach: Err(missing.replace("keeler-top-no-such-tmux", "tmux")),
        ..Levers::answering(&tally, report(&[state_line("T1", "running", &log)]))
    });
    let (mut app, mut feed) = board_over(&levers);
    let mut screen = held(&tally);

    // When the user presses Enter
    pressed(&mut screen, &mut app, &mut feed, KeyCode::Enter);

    // Then the status line says tmux is not installed and the board stays up
    assert!(
        app.board.message.contains("tmux") && app.board.message.contains("not installed"),
        "the status line does not name what is missing: {}",
        app.board.message,
    );
    assert_eq!(
        tally.seen().last().map(String::as_str),
        Some("enter"),
        "the board did not take its screen back from a tmux that never ran",
    );
}

#[test]
fn enter_on_a_task_with_no_session_says_so() {
    // Given T2 is not spawned and its row selected
    let runfiles = Runfiles::new("t7-no-session");
    let log = runfiles.stream("t1", &[INIT.to_string()]);
    let tally = Tally::default();
    let levers = Arc::new(Levers::answering(
        &tally,
        report(&[
            state_line("T1", "running", &log),
            "T2     not spawned".to_string(),
        ]),
    ));
    let (mut app, mut feed) = board_over(&levers);
    let mut screen = held(&tally);
    pressed(&mut screen, &mut app, &mut feed, KeyCode::Char('j'));

    // When the user presses Enter
    pressed(&mut screen, &mut app, &mut feed, KeyCode::Enter);

    // Then the status line says T2 has no session to attach
    assert!(
        app.board.message.contains("T2") && app.board.message.contains("no session"),
        "the status line does not say why nothing happened: {}",
        app.board.message,
    );
    assert_eq!(
        tally.seen(),
        ["enter"],
        "the board handed the terminal to a session that is not there",
    );
}
