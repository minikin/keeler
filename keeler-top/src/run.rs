//! What a run's stream says the run is doing.
//!
//! Nothing in the stream announces a stage. A run's records do not contain
//! a `Skill` call named `keeler:tdd`, `keeler:qa`, `keeler:review` or
//! `keeler:mutants` — those are slash commands, which reach the agent
//! through its init record and are never tool calls — so the stage here is
//! read from what the agent demonstrably *does*: the files it writes, the
//! recipes it runs, the one skill the pipeline really does call.
//!
//! That evidence is one-way. An agent that runs `just dev` again in the
//! middle of its review has not gone back to qa, so the fold keeps the
//! furthest stage it has seen and never the latest. The price is stated
//! rather than hidden: an agent that does a stage by hand in some other way
//! leaves the row on the stage before it, which is the honest answer.

use std::collections::{HashMap, VecDeque};
use std::path::Path;

use crate::clock::{Timestamp, format_elapsed};
use crate::stream::Record;

/// What a column shows when the run has nothing to say for it: a task that
/// never started, a done task whose worktree is gone, a stream that holds
/// its init record and nothing else.
pub const DASH: &str = "—";

/// How many of the run's own words the detail pane keeps.
const TEXTS_KEPT: usize = 5;

/// The share of its window a run has to reach before the column says so.
const CONTEXT_ALARM: u8 = 80;

/// The context window a model name implies.
///
/// The suffix is the only evidence there is: the init record's model
/// carries it and the assistant records' does not, and nothing in the
/// stream states a window. Anything without it is the ordinary 200k — an
/// unknown model reading full at 200k is the failure that shows, and one
/// reading 20% at a million is the failure that does not.
#[must_use]
pub fn window_for(model: &str) -> u64 {
    if model.contains("[1m]") {
        1_000_000
    } else {
        200_000
    }
}

/// A used-against-window share, rounded half up and never above full.
///
/// Half up rather than to nearest-even because the number is a warning:
/// 79.5% of a window should read 80 and carry the mark, not round down to
/// the quiet side. The arithmetic is `(used * 200 + window) / (window * 2)`
/// — the same as adding a half before truncating, without leaving integers.
#[must_use]
pub fn percent(used: u64, window: u64) -> u8 {
    // A window of nothing is not a division: it is a run whose model said
    // nothing, and whatever it has used fills it.
    let window = u128::from(window.max(1));
    let rounded = (u128::from(used) * 200 + window) / (window * 2);
    match u8::try_from(rounded) {
        Ok(share) if share <= 100 => share,
        _ => 100,
    }
}

/// A token count in the column's own shape: `999`, `1.0k`, `12.1k`, `1.0M`.
///
/// One decimal above a thousand, and the unit steps up before the number
/// reaches four digits — so a count that would print as `1000.0k` prints as
/// `1.0M` instead, and nothing is ever wider than six characters. The
/// ladder runs past `M` because the type says it can: the column has to
/// hold whatever number reaches it, not whatever number is plausible.
#[must_use]
pub fn format_tokens(count: u64) -> String {
    if count < 1_000 {
        return count.to_string();
    }
    #[expect(
        clippy::cast_precision_loss,
        reason = "a count this size is shown to one decimal of a thousand — the digits lost are far below the ones printed"
    )]
    let mut value = count as f64 / 1_000.0;
    let mut unit = "k";
    for next in ["M", "G", "T", "P", "E"] {
        if to_one_decimal(value) < 1_000.0 {
            break;
        }
        value /= 1_000.0;
        unit = next;
    }
    format!("{:.1}{unit}", to_one_decimal(value))
}

/// Rounded to the decimal the column shows, so the step up to the next unit
/// is decided on the number that will be printed rather than on the one
/// behind it.
fn to_one_decimal(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

/// The model as the column names it: the vendor prefix and the hyphens go,
/// and the window suffix stays, because it is the one part of the name that
/// changes what the row beside it means.
fn short_model(model: &str) -> String {
    model
        .strip_prefix("claude-")
        .unwrap_or(model)
        .replace('-', "")
}

/// How far a run has got, in the order the pipeline runs.
///
/// The order is the rule, not a convenience: [`RunView`] keeps the maximum
/// of every signal it has seen, so `Ord` here is what "the stage never moves
/// backwards" means. Anything inserted into this enum must go in pipeline
/// order.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum Stage {
    /// Nothing yet — the run has read files and written none.
    #[default]
    Reading,
    /// It has edited something inside its own worktree.
    Tdd,
    /// It has run one of the gate recipes on its own.
    Qa,
    /// It has called the `code-review` skill, or written its review record.
    Review,
    /// It has run one of the mutation recipes.
    Mutants,
    /// It has run `keeler-branch`, the gate a task branch runs.
    Gate,
    /// Its `.exit` file is there. `ended` rather than `done`, because a
    /// failed task has an exit file too: the stage is over, not finished.
    Ended,
}

impl Stage {
    /// The word the board's stage column shows.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Reading => "reading",
            Self::Tdd => "tdd",
            Self::Qa => "qa",
            Self::Review => "review",
            Self::Mutants => "mutants",
            Self::Gate => "gate",
            Self::Ended => "ended",
        }
    }
}

impl std::fmt::Display for Stage {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// One `tool_use` block, reduced to what the board reads of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCall {
    /// The call's own id, which is what a `tool_result` names when the call
    /// comes back.
    pub id: String,
    /// The tool as the stream names it: `Bash`, `Edit`, `Skill`.
    pub name: String,
    /// The one field of the call's input that says what it did — the
    /// command, the skill, the path. Empty for a tool that has none of
    /// them, which is a tool the board can only name.
    pub detail: String,
    /// When the record carrying it was written, from the record rather than
    /// the block. Absent for a stamp the clock could not read, which costs
    /// the elapsed column and nothing else.
    pub at: Option<Timestamp>,
}

impl ToolCall {
    /// One content block, or nothing for a block that is not a tool call.
    fn from_block(block: &serde_json::Value, at: Option<Timestamp>) -> Option<Self> {
        if block.get("type")?.as_str()? != "tool_use" {
            return None;
        }
        let name = block.get("name")?.as_str()?.to_string();
        // Which field says what the call did depends on the tool: the
        // command for a shell, the skill for a skill, the description for a
        // subagent — whose input is a whole prompt, and whose one line worth
        // a column is the description beside it — and the path for the rest.
        let field = match name.as_str() {
            "Bash" => "command",
            "Skill" => "skill",
            "Task" => "description",
            _ => "file_path",
        };
        let detail = block
            .get("input")
            .and_then(|input| input.get(field))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string();
        let id = block
            .get("id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string();
        Some(Self {
            id,
            name,
            detail,
            at,
        })
    }

    /// The tool column's text: what was called, and what it was called on.
    fn column(&self) -> String {
        if self.detail.is_empty() {
            self.name.clone()
        } else {
            format!("{}: {}", self.name, self.detail)
        }
    }
}

/// The stage one tool call is evidence of, if any.
///
/// `worktree` is the path `keeler-status` reports for the task. Paths in a
/// `Write`/`Edit` input are absolute, and this prefix is the whole
/// difference between the task's own work and a scratch file in `/tmp`.
#[must_use]
pub fn stage_of(call: &ToolCall, worktree: &Path) -> Option<Stage> {
    match call.name.as_str() {
        "Bash" => recipe_of(&call.detail).and_then(stage_of_recipe),
        "Write" | "Edit" => stage_of_edit(Path::new(&call.detail), worktree),
        "Skill" => stage_of_skill(&call.detail),
        _ => None,
    }
}

/// The recipe a command runs: the first word after `just` or `keeler`, at
/// the very start of the command and nowhere else.
///
/// Exact, and that is the point. `cat mutants.out` names no recipe and
/// neither does `grep mutants`, while `just dev 2>&1 | tail -35` names
/// `dev` — a rule that went looking for the word anywhere in the line would
/// read a run's own grep as the stage it was grepping for.
fn recipe_of(command: &str) -> Option<&str> {
    let mut words = command.split_whitespace();
    match words.next()? {
        "just" | "keeler" => words.next(),
        _ => None,
    }
}

/// Which recipes are evidence of which stage. Every other recipe — the
/// spawning and landing ones, `fmt`, anything a project adds — is none.
fn stage_of_recipe(recipe: &str) -> Option<Stage> {
    match recipe {
        // `crap-delta` is half of `keeler-branch`, and run under its own
        // name it is the qa stage rather than the gate.
        "dev" | "ci" | "cov" | "crap" | "crap-baseline" | "crap-delta" | "lint" | "test" => {
            Some(Stage::Qa)
        }
        "mutants" | "mutants-all" | "mutants-diff" => Some(Stage::Mutants),
        "keeler-branch" => Some(Stage::Gate),
        _ => None,
    }
}

/// An edit is the task's work when it lands inside the task's worktree, and
/// the review stage when it lands in the review record — which is inside it
/// too, so the order of these two questions is the rule.
fn stage_of_edit(path: &Path, worktree: &Path) -> Option<Stage> {
    let relative = path.strip_prefix(worktree).ok()?;
    if relative.starts_with("reviews") {
        Some(Stage::Review)
    } else {
        Some(Stage::Tdd)
    }
}

/// `code-review` is the one skill a Keeler run really calls, from the second
/// half of `/keeler:review`. The `keeler:*` names are here so that a plugin
/// which one day surfaces its commands as skills is read without a change.
fn stage_of_skill(skill: &str) -> Option<Stage> {
    match skill {
        "code-review" | "keeler:review" => Some(Stage::Review),
        "keeler:tdd" => Some(Stage::Tdd),
        "keeler:qa" => Some(Stage::Qa),
        "keeler:mutants" => Some(Stage::Mutants),
        _ => None,
    }
}

/// What one run's stream says about it, folded record by record.
///
/// A restart throws the whole view away rather than reconciling it — the
/// reader says when — so nothing here has to be undoable.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunView {
    /// The model the init record named, spelled as it spelled it — the
    /// `[1m]` suffix included, since the window is read from it.
    pub model: Option<String>,
    /// The furthest stage this run has shown a signal for.
    pub stage: Stage,
    /// The last tool the main session called.
    pub last_tool: Option<ToolCall>,
    /// Whether that call's answer has come back. A call that has returned
    /// is not a wait, so the elapsed column has nothing to count.
    pub last_tool_returned: bool,
    /// The input side of the last assistant record's usage: everything the
    /// model was shown, which is what the context column is a share of.
    pub context_used: Option<u64>,
    /// Output tokens by message id. One message arrives as one record per
    /// content block, each repeating the whole message's usage, so the
    /// counts are kept by id and summed once rather than added up.
    pub output_by_message: HashMap<String, u64>,
    /// The run's last few words, oldest first — what the detail pane shows.
    pub texts: VecDeque<String>,
}

impl RunView {
    /// The run's `.exit` file is there, whatever it holds.
    ///
    /// Not a stream signal: the runner writes that file after the stream is
    /// closed, so it reaches the view from the caller that looked for it.
    /// It arrives through the same one-way door as every other signal, and
    /// [`Stage::Ended`] is the last one, so calling this twice says the same
    /// thing as calling it once.
    pub fn ended(&mut self) {
        self.advance(Stage::Ended);
    }

    /// The model column: the run's model, short.
    #[must_use]
    pub fn model_column(&self) -> String {
        self.model
            .as_deref()
            .map_or_else(|| DASH.to_string(), short_model)
    }

    /// The tool column: the last call the main session made.
    #[must_use]
    pub fn tool_column(&self) -> String {
        self.last_tool
            .as_ref()
            .map_or_else(|| DASH.to_string(), ToolCall::column)
    }

    /// The elapsed column: how long the last call has been running.
    ///
    /// Empty rather than dashed, and empty for three different runs — one
    /// that has called nothing, one whose call has come back, one whose
    /// record carried no readable stamp. None of them is waiting on
    /// anything, and a column that said so three ways would be reporting on
    /// the board rather than on the run.
    #[must_use]
    pub fn elapsed_column(&self, now: Timestamp) -> String {
        if self.last_tool_returned {
            return String::new();
        }
        self.last_tool
            .as_ref()
            .and_then(|call| call.at)
            .map_or_else(String::new, |at| format_elapsed(now.seconds_since(at)))
    }

    /// The context column: the share of its window the run has used, marked
    /// once it is close enough to the end to matter.
    #[must_use]
    pub fn context_column(&self) -> String {
        let Some(used) = self.context_used else {
            return DASH.to_string();
        };
        let share = percent(used, window_for(self.model.as_deref().unwrap_or_default()));
        if share >= CONTEXT_ALARM {
            format!("{share}%!")
        } else {
            format!("{share}%")
        }
    }

    /// The tokens column: everything the run has written, summed once per
    /// message.
    #[must_use]
    pub fn tokens_column(&self) -> String {
        if self.output_by_message.is_empty() {
            return DASH.to_string();
        }
        format_tokens(self.output_by_message.values().sum())
    }

    /// One assistant record: what it called, what it said, and what it cost.
    fn absorb(&mut self, message: &serde_json::Value, at: Option<Timestamp>, worktree: &Path) {
        self.absorb_usage(message);
        let Some(blocks) = message.get("content").and_then(serde_json::Value::as_array) else {
            return;
        };
        for block in blocks {
            if let Some(text) = text_of(block) {
                self.remember(text);
            }
            if let Some(call) = ToolCall::from_block(block, at) {
                if let Some(stage) = stage_of(&call, worktree) {
                    self.advance(stage);
                }
                self.last_tool = Some(call);
                self.last_tool_returned = false;
            }
        }
    }

    /// A message's usage: the input side is the context as it stands, the
    /// output side is this message's contribution to the run's total.
    fn absorb_usage(&mut self, message: &serde_json::Value) {
        let Some(usage) = message.get("usage") else {
            return;
        };
        // The three input fields are one number split by where the tokens
        // came from — fresh, freshly cached, read from cache. What fills a
        // window is their sum; a board that read only `input_tokens` would
        // show 0% for a session an hour into its work.
        self.context_used = Some(
            field(usage, "input_tokens")
                + field(usage, "cache_creation_input_tokens")
                + field(usage, "cache_read_input_tokens"),
        );
        if let Some(id) = message.get("id").and_then(serde_json::Value::as_str) {
            self.output_by_message
                .insert(id.to_string(), field(usage, "output_tokens"));
        }
    }

    /// A tool's answer: it closes the call the row is waiting on, and says
    /// nothing about any earlier one.
    fn absorb_tool_result(&mut self, message: &serde_json::Value) {
        let Some(call) = self.last_tool.as_ref() else {
            return;
        };
        let Some(blocks) = message.get("content").and_then(serde_json::Value::as_array) else {
            return;
        };
        self.last_tool_returned |= blocks.iter().any(|block| {
            block.get("tool_use_id").and_then(serde_json::Value::as_str) == Some(call.id.as_str())
        });
    }

    /// One of the run's own lines, with the oldest dropped once there are
    /// more than the pane shows.
    fn remember(&mut self, text: &str) {
        self.texts.push_back(text.to_string());
        while self.texts.len() > TEXTS_KEPT {
            self.texts.pop_front();
        }
    }

    /// The one way the stage ever changes.
    fn advance(&mut self, stage: Stage) {
        self.stage = self.stage.max(stage);
    }
}

/// One count of a usage, absent or unreadable meaning none.
fn field(usage: &serde_json::Value, name: &str) -> u64 {
    usage
        .get(name)
        .and_then(serde_json::Value::as_u64)
        .unwrap_or_default()
}

/// The words of a text block, or nothing for a block that is not one.
fn text_of(block: &serde_json::Value) -> Option<&str> {
    if block.get("type")?.as_str()? != "text" {
        return None;
    }
    block.get("text")?.as_str()
}

/// Folds one record into the view.
///
/// The reader has already dropped the records a subagent made, so what
/// arrives here is the main session's own account of itself: what it was
/// started as, what it has done, and what that has cost.
pub fn fold(view: &mut RunView, record: Record, worktree: &Path) {
    match record {
        Record::Init { model } => view.model = Some(model),
        Record::Assistant { message, at } => view.absorb(&message, at, worktree),
        Record::ToolResult(message) => view.absorb_tool_result(&message),
        Record::Other => {}
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{RunView, Stage, ToolCall, fold, stage_of};
    use crate::stream::Record;

    const WORKTREE: &str = "/w/repo-01-foo-t1";

    fn call(name: &str, detail: &str) -> ToolCall {
        ToolCall {
            id: "toolu_1".to_string(),
            name: name.to_string(),
            detail: detail.to_string(),
            at: None,
        }
    }

    fn stage(name: &str, detail: &str) -> Option<Stage> {
        stage_of(&call(name, detail), Path::new(WORKTREE))
    }

    #[test]
    fn every_qa_recipe_is_qa_under_either_launcher() {
        for recipe in [
            "dev",
            "ci",
            "cov",
            "crap",
            "crap-baseline",
            "crap-delta",
            "lint",
            "test",
        ] {
            assert_eq!(
                stage("Bash", &format!("just {recipe}")),
                Some(Stage::Qa),
                "just {recipe}",
            );
            assert_eq!(
                stage("Bash", &format!("keeler {recipe}")),
                Some(Stage::Qa),
                "keeler {recipe}",
            );
        }
    }

    #[test]
    fn every_mutation_recipe_is_the_mutants_stage() {
        for recipe in ["mutants", "mutants-all", "mutants-diff"] {
            assert_eq!(
                stage("Bash", &format!("just {recipe}")),
                Some(Stage::Mutants),
                "just {recipe}",
            );
        }
    }

    #[test]
    fn a_command_that_names_no_recipe_is_no_signal() {
        // `just` with nothing after it lists the recipes; a launcher that
        // is not the first word is some other program's argument; and a
        // recipe this table does not know — a spawn, a landing, a project's
        // own — says nothing about a stage either.
        assert_eq!(stage("Bash", "just"), None);
        assert_eq!(stage("Bash", ""), None);
        assert_eq!(stage("Bash", "cargo nextest run --workspace"), None);
        assert_eq!(stage("Bash", "echo just dev"), None);
        assert_eq!(stage("Bash", "just keeler-status specs/01-foo.md"), None);
    }

    #[test]
    fn a_tool_the_table_does_not_name_is_no_signal() {
        assert_eq!(stage("Read", &format!("{WORKTREE}/src/lib.rs")), None);
        assert_eq!(stage("Grep", "mutants"), None);
        assert_eq!(stage("Skill", "algorithmic-art"), None);
    }

    #[test]
    fn every_keeler_skill_names_the_stage_it_is_called_from() {
        assert_eq!(stage("Skill", "keeler:tdd"), Some(Stage::Tdd));
        assert_eq!(stage("Skill", "keeler:qa"), Some(Stage::Qa));
        assert_eq!(stage("Skill", "keeler:review"), Some(Stage::Review));
        assert_eq!(stage("Skill", "keeler:mutants"), Some(Stage::Mutants));
    }

    #[test]
    fn the_review_record_is_told_from_a_directory_that_merely_starts_the_same_way() {
        assert_eq!(
            stage("Write", &format!("{WORKTREE}/reviews/01-foo/t1.md")),
            Some(Stage::Review),
        );
        assert_eq!(
            stage("Edit", &format!("{WORKTREE}/reviewsomething/x.rs")),
            Some(Stage::Tdd),
            "a path is matched by its components, not by its bytes",
        );
    }

    #[test]
    fn a_worktree_that_merely_prefixes_another_is_not_a_match() {
        // Sibling worktrees are `<repo>-<slug>-t1`, `<repo>-<slug>-t10`:
        // one path is a byte prefix of the other, and an edit in t10's tree
        // is not evidence about t1.
        assert_eq!(
            stage("Edit", &format!("{WORKTREE}0/src/lib.rs")),
            None,
            "t10's worktree was read as t1's",
        );
    }

    #[test]
    fn every_stage_has_its_word() {
        assert_eq!(Stage::default(), Stage::Reading);
        for (stage, word) in [
            (Stage::Reading, "reading"),
            (Stage::Tdd, "tdd"),
            (Stage::Qa, "qa"),
            (Stage::Review, "review"),
            (Stage::Mutants, "mutants"),
            (Stage::Gate, "gate"),
            (Stage::Ended, "ended"),
        ] {
            assert_eq!(stage.as_str(), word);
            assert_eq!(stage.to_string(), word);
        }
    }

    #[test]
    fn the_stages_are_ordered_as_the_pipeline_runs_them() {
        let pipeline = [
            Stage::Reading,
            Stage::Tdd,
            Stage::Qa,
            Stage::Review,
            Stage::Mutants,
            Stage::Gate,
            Stage::Ended,
        ];
        let mut sorted = pipeline;
        sorted.sort_unstable();

        assert_eq!(sorted, pipeline);
    }

    #[test]
    fn a_block_that_is_not_a_tool_call_folds_into_nothing() {
        let mut view = RunView::default();
        for message in [
            // An assistant record whose content is text, not a call.
            serde_json::json!({"content": [{"type": "text", "text": "just dev"}]}),
            // A tool call the board cannot name.
            serde_json::json!({"content": [{"type": "tool_use", "input": {"command": "just dev"}}]}),
            // A block that is not an object at all, and one with no type.
            serde_json::json!({"content": ["just dev", {"name": "Bash"}]}),
            // A message with no content array.
            serde_json::json!({"id": "m1"}),
        ] {
            fold(
                &mut view,
                Record::Assistant { message, at: None },
                Path::new(WORKTREE),
            );
        }

        assert_eq!(view.stage, Stage::Reading);
    }

    #[test]
    fn a_record_that_is_not_an_assistants_moves_no_stage() {
        let mut view = RunView::default();
        fold(
            &mut view,
            Record::Init {
                model: "claude-opus-5[1m]".to_string(),
            },
            Path::new(WORKTREE),
        );
        fold(&mut view, Record::Other, Path::new(WORKTREE));
        fold(
            &mut view,
            Record::ToolResult(serde_json::json!({"content": []})),
            Path::new(WORKTREE),
        );

        assert_eq!(view.stage, Stage::Reading);
        // The init record is not nothing — it names the model, and the
        // window the context column divides by comes from that name.
        assert_eq!(view.model.as_deref(), Some("claude-opus-5[1m]"));
    }

    #[test]
    fn one_record_carrying_several_calls_takes_the_furthest_of_them() {
        // The CLI writes one record per content block, so this shape is not
        // one the board meets today — but the record's own schema is a list,
        // and a fold that read only the first block would be a silent
        // regression the day that changes.
        let mut view = RunView::default();
        fold(
            &mut view,
            Record::Assistant {
                message: serde_json::json!({"content": [
                    {"type": "tool_use", "name": "Skill", "input": {"skill": "code-review"}},
                    {"type": "tool_use", "name": "Bash", "input": {"command": "just dev"}},
                ]}),
                at: None,
            },
            Path::new(WORKTREE),
        );

        assert_eq!(view.stage, Stage::Review);
        // The stage is the furthest of them and the tool column is the last
        // of them: one is a record of where the run has got to, the other of
        // what it is doing now.
        assert_eq!(view.tool_column(), "Bash: just dev");
    }

    #[test]
    fn the_exit_file_ends_a_run_wherever_it_had_got_to() {
        let mut fresh = RunView::default();
        fresh.ended();
        assert_eq!(fresh.stage, Stage::Ended);

        let mut twice = RunView::default();
        twice.ended();
        twice.ended();
        assert_eq!(twice.stage, Stage::Ended);
    }

    // ── T3

    use super::{DASH, format_tokens, percent, window_for};
    use crate::clock::Timestamp;

    /// An assistant record's message, folded the way the reader hands it
    /// over: the record's stamp travels beside the message, not inside it.
    fn absorb(view: &mut RunView, message: serde_json::Value, at: Option<Timestamp>) {
        fold(view, Record::Assistant { message, at }, Path::new(WORKTREE));
    }

    fn at(text: &str) -> Option<Timestamp> {
        Timestamp::parse(text)
    }

    #[test]
    fn the_window_is_the_suffix_and_nothing_else_in_the_name() {
        assert_eq!(window_for("claude-opus-5[1m]"), 1_000_000);
        assert_eq!(window_for("claude-sonnet-5"), 200_000);
        assert_eq!(window_for(""), 200_000);
        assert_eq!(
            window_for("claude-opus-5"),
            200_000,
            "the same family without the suffix is the ordinary window",
        );
    }

    #[test]
    fn a_share_of_a_window_is_a_percentage_at_both_ends() {
        assert_eq!(percent(0, 200_000), 0);
        assert_eq!(percent(200_000, 200_000), 100);
        // A window smaller than what has been used is a model the board
        // guessed wrong about, not a row that reads 450%.
        assert_eq!(percent(900_000, 200_000), 100);
        // And a window of nothing is not a division.
        assert_eq!(percent(1, 0), 100);
        assert_eq!(percent(0, 0), 0);
        // The rounding is half up, on the digit the column shows.
        assert_eq!(percent(1_005, 100_000), 1);
        assert_eq!(percent(2_500, 5_000), 50);
    }

    #[test]
    fn the_unit_climbs_before_the_number_reaches_four_digits() {
        assert_eq!(format_tokens(0), "0");
        assert_eq!(format_tokens(999_949), "999.9k");
        assert_eq!(format_tokens(999_950), "1.0M");
        assert_eq!(format_tokens(1_000_000_000), "1.0G");
        assert_eq!(format_tokens(1_000_000_000_000), "1.0T");
        assert_eq!(format_tokens(1_000_000_000_000_000), "1.0P");
        assert_eq!(format_tokens(1_000_000_000_000_000_000), "1.0E");
        assert_eq!(format_tokens(u64::MAX), "18.4E");
    }

    #[test]
    fn the_model_column_keeps_the_suffix_and_drops_the_vendor() {
        let mut view = RunView::default();
        assert_eq!(view.model_column(), DASH);

        for (model, shown) in [
            ("claude-opus-5[1m]", "opus5[1m]"),
            ("claude-sonnet-5", "sonnet5"),
            ("some-other-model", "someothermodel"),
        ] {
            view.model = Some(model.to_string());
            assert_eq!(view.model_column(), shown);
        }
    }

    #[test]
    fn a_tool_the_board_can_only_name_is_named() {
        let mut view = RunView::default();
        assert_eq!(view.tool_column(), DASH, "a run that has called nothing");

        // A tool whose input holds none of the fields the column reads —
        // the name is the whole of what can honestly be shown.
        absorb(
            &mut view,
            serde_json::json!({"content": [
                {"type": "tool_use", "id": "toolu_1", "name": "TodoWrite", "input": {"todos": []}},
            ]}),
            None,
        );

        assert_eq!(view.tool_column(), "TodoWrite");
    }

    #[test]
    fn only_the_call_the_row_is_waiting_on_closes_the_wait() {
        let mut view = RunView::default();
        absorb(
            &mut view,
            serde_json::json!({"content": [
                {"type": "tool_use", "id": "toolu_1", "name": "Bash", "input": {"command": "just dev"}},
            ]}),
            at("2026-09-07T12:00:00Z"),
        );
        let now = Timestamp::parse("2026-09-07T12:02:14Z").unwrap();
        assert_eq!(view.elapsed_column(now), "02:14");

        // Some earlier call coming back says nothing about this one.
        fold(
            &mut view,
            Record::ToolResult(serde_json::json!({"content": [
                {"type": "tool_result", "tool_use_id": "toolu_0"},
            ]})),
            Path::new(WORKTREE),
        );
        assert_eq!(view.elapsed_column(now), "02:14");

        fold(
            &mut view,
            Record::ToolResult(serde_json::json!({"content": [
                {"type": "tool_result", "tool_use_id": "toolu_1"},
            ]})),
            Path::new(WORKTREE),
        );
        assert_eq!(view.elapsed_column(now), "");
    }

    #[test]
    fn a_tool_result_with_nothing_to_close_is_not_an_answer() {
        let mut view = RunView::default();
        // An answer arriving before any call — the runner's stream opens
        // with the prompt, and `claude -p` has nobody to type at it, so the
        // prompt is a `user` record like every tool's answer.
        fold(
            &mut view,
            Record::ToolResult(
                serde_json::json!({"content": [{"type": "tool_result", "tool_use_id": "toolu_1"}]}),
            ),
            Path::new(WORKTREE),
        );
        assert!(!view.last_tool_returned);

        absorb(
            &mut view,
            serde_json::json!({"content": [
                {"type": "tool_use", "id": "toolu_1", "name": "Bash", "input": {"command": "just dev"}},
            ]}),
            None,
        );
        // And a `user` record whose content is not a list of blocks: it
        // names no call, so the call outstanding is still outstanding.
        fold(
            &mut view,
            Record::ToolResult(serde_json::json!({"content": "the prompt, as text"})),
            Path::new(WORKTREE),
        );

        assert!(!view.last_tool_returned);
    }

    #[test]
    fn a_new_call_is_a_new_wait() {
        let mut view = RunView::default();
        let call = |id: &str| {
            serde_json::json!({"content": [
                {"type": "tool_use", "id": id, "name": "Bash", "input": {"command": "just dev"}},
            ]})
        };
        absorb(&mut view, call("toolu_1"), at("2026-09-07T12:00:00Z"));
        fold(
            &mut view,
            Record::ToolResult(serde_json::json!({"content": [
                {"type": "tool_result", "tool_use_id": "toolu_1"},
            ]})),
            Path::new(WORKTREE),
        );
        assert!(view.last_tool_returned);

        absorb(&mut view, call("toolu_2"), at("2026-09-07T12:01:00Z"));

        assert!(!view.last_tool_returned);
        assert_eq!(
            view.elapsed_column(Timestamp::parse("2026-09-07T12:01:30Z").unwrap()),
            "00:30",
        );
    }

    #[test]
    fn a_call_whose_record_carried_no_readable_stamp_is_not_timed() {
        let mut view = RunView::default();
        absorb(
            &mut view,
            serde_json::json!({"content": [
                {"type": "tool_use", "id": "toolu_1", "name": "Bash", "input": {"command": "just dev"}},
            ]}),
            None,
        );

        assert_eq!(view.tool_column(), "Bash: just dev");
        assert_eq!(view.elapsed_column(Timestamp::now()), "");
    }

    #[test]
    fn the_context_is_the_last_usage_and_a_record_without_one_does_not_clear_it() {
        let mut view = RunView {
            model: Some("claude-opus-5[1m]".to_string()),
            ..RunView::default()
        };
        assert_eq!(view.context_column(), DASH);

        absorb(
            &mut view,
            serde_json::json!({"id": "m1", "usage": {"input_tokens": 500_000}}),
            None,
        );
        assert_eq!(view.context_column(), "50%");

        // A record with no usage at all — the shape a later CLI could write
        // — leaves the last answer standing rather than blanking the column.
        absorb(&mut view, serde_json::json!({"id": "m2"}), None);
        assert_eq!(view.context_column(), "50%");
    }

    #[test]
    fn the_context_column_marks_the_share_it_cannot_leave_unmarked() {
        let mut view = RunView::default();
        for (used, shown) in [
            (0_u64, "0%"),
            (158_000, "79%"),
            (160_000, "80%!"),
            (200_000, "100%!"),
        ] {
            view.context_used = Some(used);
            assert_eq!(view.context_column(), shown);
        }
    }

    #[test]
    fn output_is_summed_once_per_message_and_a_message_without_an_id_is_not_counted() {
        let mut view = RunView::default();
        assert_eq!(view.tokens_column(), DASH);

        for _ in 0..3 {
            absorb(
                &mut view,
                serde_json::json!({"id": "m1", "usage": {"output_tokens": 300}}),
                None,
            );
        }
        absorb(
            &mut view,
            serde_json::json!({"id": "m2", "usage": {"output_tokens": 1200}}),
            None,
        );
        assert_eq!(view.tokens_column(), "1.5k");

        // A record whose message has no id cannot be deduplicated, and
        // counting it would make the total grow with every content block.
        absorb(
            &mut view,
            serde_json::json!({"usage": {"output_tokens": 9_000}}),
            None,
        );
        assert_eq!(view.tokens_column(), "1.5k");
    }

    #[test]
    fn the_pane_keeps_the_last_five_texts_oldest_first() {
        let mut view = RunView::default();
        for word in ["one", "two", "three", "four", "five", "six"] {
            absorb(
                &mut view,
                serde_json::json!({"content": [{"type": "text", "text": word}]}),
                None,
            );
        }

        assert_eq!(
            view.texts.iter().map(String::as_str).collect::<Vec<_>>(),
            ["two", "three", "four", "five", "six"],
        );
    }

    #[test]
    fn a_block_that_is_not_a_text_block_is_not_one_of_the_texts() {
        let mut view = RunView::default();
        absorb(
            &mut view,
            serde_json::json!({"content": [
                {"type": "thinking", "thinking": "not for the pane"},
                {"type": "text"},
                {"type": "tool_use", "id": "toolu_1", "name": "Bash", "input": {"command": "just dev"}},
                {"type": "text", "text": "for the pane"},
            ]}),
            None,
        );

        assert_eq!(
            view.texts.iter().map(String::as_str).collect::<Vec<_>>(),
            ["for the pane"],
        );
    }
}
