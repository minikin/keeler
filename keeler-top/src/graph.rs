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
use std::collections::HashMap;
use std::path::Path;

/// What opens a task's title, after the id: the em dash the Tasks section
/// writes, spaced.
const OPENS: &str = " — ";

/// What closes it. The full stop is part of the marker rather than part of
/// the title: every task line ends its title with one, and a column that
/// kept it would read `The theme.` beside states that carry no punctuation.
const CLOSES: &str = ".**";

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

/// The spec as the board reads it: what the script said about each task,
/// and what the Tasks section calls them.
///
/// One value because it is one read. The script and the titles come from
/// the same copy of the same spec at the same ref, and a board that fetched
/// them separately could show a title from one commit beside a state from
/// another.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Graph {
    /// One line per task, in the order the spec lists them.
    pub lines: Vec<GraphLine>,
    /// The title each task line gives, by task id — and no entry at all for
    /// a line that does not give one.
    pub titles: HashMap<String, String>,
}

impl Graph {
    /// What the script said about one task, if it named it.
    #[must_use]
    pub fn line(&self, id: &str) -> Option<&GraphLine> {
        self.lines.iter().find(|line| line.id == id)
    }

    /// What the spec calls one task.
    #[must_use]
    pub fn title(&self, id: &str) -> Option<String> {
        self.titles.get(id).cloned()
    }
}

/// The board's graph read: the spec as `git_ref` holds it, through the
/// plugin's own parser, and the titles from that same copy.
///
/// A copy that cannot be read back costs the titles and nothing else. The
/// graph is the answer this read exists for, and a board that refused
/// altogether over a column of prose would show no states either.
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
) -> Result<Graph, String> {
    let copy = crate::git::spec_from_ref(root, git_ref, rel)?;
    let lines = parse(&dispatch.graph(copy.path())?);
    let titles = titles(&std::fs::read_to_string(copy.path()).unwrap_or_default());
    Ok(Graph { lines, titles })
}

/// The title each task line gives, read from the spec's own text.
///
/// The Tasks section writes one task per line, and the title is the text
/// between `**Tn — ` and the `.**` that closes it. Nothing else on the line
/// is read: what follows the title is the module list, the scenarios and
/// the `Needs:`, none of which belongs in a column two dozen cells wide.
#[must_use]
pub fn titles(spec: &str) -> HashMap<String, String> {
    let mut titles = HashMap::new();
    for (id, title) in spec.lines().filter_map(title_of) {
        // The first line per task wins, as it does for `keeler-graph.sh`:
        // two boards disagreeing about which of two titles is T1's would be
        // worse than either of them alone.
        titles.entry(id).or_insert(title);
    }
    titles
}

/// One task line's id and title, or nothing for a line that is not one.
fn title_of(line: &str) -> Option<(String, String)> {
    let (_, bolded) = line.split_once("**")?;
    let (id, rest) = bolded.split_once(OPENS)?;
    if !task_id(id) {
        return None;
    }
    let (title, _) = rest.split_once(CLOSES)?;
    Some((id.to_string(), title.to_string()))
}

/// Whether a word is a task id: `T` and a number, which is how every task
/// in every spec is named and how every path in graph mode is composed.
///
/// Asked because a spec bolds other things — an Implementation Note's
/// heading, a term being defined — and a title read off one of those would
/// put a stranger's words in a row nobody could trace them from.
fn task_id(word: &str) -> bool {
    word
        .strip_prefix('T')
        .is_some_and(|number| !number.is_empty() && number.chars().all(|digit| digit.is_ascii_digit()))
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
    use super::{GraphLine, GraphState, parse, titles};

    /// The Tasks section as a spec writes one, with the two shapes a title
    /// can arrive in: the form the board reads, and a line that has not got
    /// it.
    const TASKS: &str = "\
## Tasks

- [x] **T1 — The crate exists and reads a stream incrementally.** `keeler-top/src/stream.rs`.
- [ ] **T2 — Rows as lines, in order.** Needs: T1.
- [ ] T4 the fold reads the tool. Needs: T2.
";

    #[test]
    fn a_task_line_gives_up_the_title_between_the_dash_and_the_full_stop() {
        // Given the spec's Tasks section
        // When the titles are read
        let titles = titles(TASKS);

        // Then each task line's title is the text between "**Tn — " and
        // the ".**" that closes it, whatever follows on the line
        assert_eq!(
            titles.get("T1").map(String::as_str),
            Some("The crate exists and reads a stream incrementally"),
        );
        assert_eq!(
            titles.get("T2").map(String::as_str),
            Some("Rows as lines, in order"),
        );
        // And a task line that lacks the form gives no title rather than a
        // wrong one — the board leaves the column blank and says nothing.
        assert_eq!(titles.get("T4"), None);
    }

    #[test]
    fn only_a_task_id_names_a_title() {
        // The Tasks section is not the only place a spec bolds a phrase,
        // and a heading read as a task would put a stranger's words in a
        // row's title column.
        assert!(
            titles("**Order and scroll.** `board.rs` orders by group.\n").is_empty(),
            "a bolded phrase that names no task was read as one",
        );
        assert!(titles("- [ ] **T — no number.** x\n").is_empty());
        assert!(titles("- [ ] **T1x — not an id.** x\n").is_empty());
        // And the closing ".**" is what ends a title: a line that opens the
        // form and never closes it is not one.
        assert!(titles("- [ ] **T1 — never closed\n").is_empty());
        assert!(titles("").is_empty());
    }

    #[test]
    fn a_spec_that_repeats_a_task_line_keeps_the_first_of_them() {
        // The Tasks section is the graph, and `keeler-graph.sh` reads the
        // first line per task too — two boards disagreeing about which of
        // two titles is T1's would be worse than either answer.
        assert_eq!(
            titles("- [ ] **T1 — first.** x\n- [ ] **T1 — second.** x\n")
                .get("T1")
                .map(String::as_str),
            Some("first"),
        );
    }

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
