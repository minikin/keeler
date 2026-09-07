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

use std::path::Path;

use crate::stream::Record;

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

/// One `tool_use` block, reduced to the two things the board reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCall {
    /// The tool as the stream names it: `Bash`, `Edit`, `Skill`.
    pub name: String,
    /// The one field of the call's input that says what it did — the
    /// command, the skill, the path. Empty for a tool that has none of
    /// them, which is a tool the board can only name.
    pub detail: String,
}

impl ToolCall {
    /// One content block, or nothing for a block that is not a tool call.
    fn from_block(block: &serde_json::Value) -> Option<Self> {
        if block.get("type")?.as_str()? != "tool_use" {
            return None;
        }
        let name = block.get("name")?.as_str()?.to_string();
        let field = match name.as_str() {
            "Bash" => "command",
            "Skill" => "skill",
            _ => "file_path",
        };
        let detail = block
            .get("input")
            .and_then(|input| input.get(field))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string();
        Some(Self { name, detail })
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
    /// The furthest stage this run has shown a signal for.
    pub stage: Stage,
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

    /// The one way the stage ever changes.
    fn advance(&mut self, stage: Stage) {
        self.stage = self.stage.max(stage);
    }
}

/// Folds one record into the view.
///
/// Only assistant records carry evidence, and the reader has already dropped
/// the ones a subagent made, so what arrives here is the main session's own
/// account of itself.
pub fn fold(view: &mut RunView, record: Record, worktree: &Path) {
    let Record::Assistant(message) = record else {
        return;
    };
    let Some(blocks) = message.get("content").and_then(serde_json::Value::as_array) else {
        return;
    };
    for stage in blocks
        .iter()
        .filter_map(ToolCall::from_block)
        .filter_map(|call| stage_of(&call, worktree))
    {
        view.advance(stage);
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
            name: name.to_string(),
            detail: detail.to_string(),
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
            fold(&mut view, Record::Assistant(message), Path::new(WORKTREE));
        }

        assert_eq!(view.stage, Stage::Reading);
    }

    #[test]
    fn a_record_that_is_not_an_assistants_folds_into_nothing() {
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
            Record::Assistant(serde_json::json!({"content": [
                {"type": "tool_use", "name": "Skill", "input": {"skill": "code-review"}},
                {"type": "tool_use", "name": "Bash", "input": {"command": "just dev"}},
            ]})),
            Path::new(WORKTREE),
        );

        assert_eq!(view.stage, Stage::Review);
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
}
