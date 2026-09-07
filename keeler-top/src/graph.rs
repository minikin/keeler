//! Reading the spec's own graph.
//!
//! `keeler-status` has no word for *blocked*: it prints `not spawned` for
//! every task the graph is holding and for every task nobody has got to
//! yet, which are different answers to the question a watcher is asking.
//! The graph knows the difference, and `scripts/keeler-graph.sh` is the
//! parser every recipe already reads it with — so the board runs that
//! script rather than learning the spec's grammar a second time. It costs
//! nothing, so it runs on every tick.
//!
//! The script prints one line per task, in the order the spec lists them:
//!
//! ```text
//! T1 done
//! T2 ready
//! T3 blocked T1 T2
//! ```
//!
//! And it is read against the spec **as the ref holds it**, never the
//! working tree. `keeler-graph`'s own comment says why: answering from the
//! working tree would make the board the one place in graph mode where an
//! uncommitted tick counts, and it fails in the inviting direction — ready
//! for work nothing will accept.

use crate::dispatch::Dispatch;
use std::path::Path;

/// What the graph says about a task, which is not what `keeler-status`
/// says: this is about the spec's ticks and edges alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphState {
    /// Every need is ticked.
    Ready,
    /// Some need is not.
    Blocked,
    /// Its own box is ticked.
    Done,
}

/// One task's line of the script's report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphLine {
    /// The task id, as the spec spells it.
    pub id: String,
    /// Ready, blocked or done.
    pub state: GraphState,
    /// Every task this one declares a `Needs:` on, ticked or not — the
    /// script prints them all, whatever the state.
    pub needs: Vec<String>,
}

/// The board's graph read: the spec as `git_ref` holds it, through the
/// plugin's own parser.
///
/// # Errors
///
/// The spec not being committed on that ref, or the script refusing the
/// graph it found — a cycle, a `Needs:` naming no task. Both are the same
/// answer here: a sentence for the board to show instead of a graph.
pub fn read(
    dispatch: &dyn Dispatch,
    root: &Path,
    git_ref: &str,
    rel: &str,
) -> Result<Vec<GraphLine>, String> {
    let copy = crate::git::spec_from_ref(root, git_ref, rel)?;
    Ok(parse(&dispatch.graph(copy.path())?))
}

/// Reads the script's report.
///
/// A line whose state is none of the three is not one of the script's, and
/// is skipped rather than reported: the board's answer for a task it
/// cannot place is the state alone, and a board that stopped to complain
/// about its own reading would be showing itself instead of the run.
#[must_use]
pub fn parse(report: &str) -> Vec<GraphLine> {
    report.lines().filter_map(line_of).collect()
}

fn line_of(line: &str) -> Option<GraphLine> {
    let mut fields = line.split_whitespace();
    let id = fields.next()?.to_string();
    let state = match fields.next()? {
        "ready" => GraphState::Ready,
        "blocked" => GraphState::Blocked,
        "done" => GraphState::Done,
        _ => return None,
    };
    Some(GraphLine {
        id,
        state,
        needs: fields.map(str::to_string).collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::{GraphLine, GraphState, parse};

    #[test]
    fn the_three_states_are_told_apart_and_the_needs_kept() {
        assert_eq!(
            parse("T1 done\nT2 ready\nT3 blocked T1 T2\n"),
            [
                GraphLine {
                    id: "T1".to_string(),
                    state: GraphState::Done,
                    needs: Vec::new(),
                },
                GraphLine {
                    id: "T2".to_string(),
                    state: GraphState::Ready,
                    needs: Vec::new(),
                },
                GraphLine {
                    id: "T3".to_string(),
                    state: GraphState::Blocked,
                    needs: vec!["T1".to_string(), "T2".to_string()],
                },
            ],
        );
    }

    #[test]
    fn a_done_task_keeps_the_needs_it_declared() {
        // The script prints them whatever the state, and the row that
        // names what a task waited on should not depend on when it is read.
        assert_eq!(
            parse("T3 done T1 T2\n")[0].needs,
            ["T1".to_string(), "T2".to_string()],
        );
    }

    #[test]
    fn a_line_that_is_not_the_scripts_is_no_task() {
        assert!(parse("").is_empty());
        assert!(parse("\n\n").is_empty());
        assert!(parse("T1\n").is_empty(), "a state-less line named a task");
        assert!(
            parse("T1 running\n").is_empty(),
            "keeler-status's vocabulary was read as the script's",
        );
    }
}
