//! The panel about one task: everything the row had no room for.
//!
//! A row is a line, and a line is a reading taken at a glance. The pane is
//! the other half — what somebody who has stopped at a row wants next: where
//! the work is, what it decided, what it said, and what to do about it. So
//! it opens with a fact block of labelled lines and closes with the run's own
//! words under rules that say which is which.
//!
//! **Every path here is composed, never displayed.** `keeler-status` reports
//! absolute paths, and an absolute path is three quarters somebody's home
//! directory; the branch, the worktree, the session, the log and the record
//! are all derived from the spec's slug and the task's id, which is how every
//! recipe in graph mode derives them. What the pane shows is therefore what
//! the reader would type.
//!
//! **The fact block is ordered by what the row's state makes worth reading.**
//! A live task's is where it is and how to reach it; a landed one's is the
//! record it left and the step after it. That is why the two orders differ:
//! the pane is not a form with fields, it is an answer to the question the
//! state raises.

use ratatui::style::Style;
use ratatui::text::{Line, Span};

use crate::board::{DONE, Row, review_record, run_file, task_branch};
use crate::clock::{Timestamp, format_age, format_elapsed};
use crate::layout::{cut, wide};
use crate::run::RunView;
use crate::theme::{CHROME, DIM, TEXT, Theme};

/// How wide the label column is: the longest of them and two spaces after.
const LABEL: usize = 8;

/// What a landed task's state line says instead of a clock. There is no run
/// to time and no worktree to visit: `keeler-land` took both, and what is
/// left is the record and the tick.
const LANDED: &str = "landed, worktree and branch removed by keeler-land";

/// What a task with no record yet has where a verdict would be.
const UNWRITTEN: &str = "not written yet";

/// The step after the last task lands, which is the one thing a finished
/// board cannot do for itself.
const NEXT: &str = "keeler keeler-land on main";

/// And what that step leaves behind.
const LEAVES: &str = "baseline staged, Status: Implemented";

/// The panel's lines for one row, laid out for the space inside its borders.
///
/// **The pane never outgrows the panel.** Two of its parts are as long as
/// what they hold — the run's own words are five texts of however many lines
/// each, and its branch has however many commits — so a pane that only laid
/// them out would be drawn past the panel's bottom border, taking the
/// section under it with it.
///
/// What is fixed is the fact block, which is the four lines the pane exists
/// for, and the last command, which is one. Everything between them is given
/// what is left in the order it is drawn: the agent's words first, because
/// they are what a watcher stopped at a row to read, and the commits after
/// them — which is why the commits are the ones that say how many they had
/// to leave out.
#[must_use]
pub fn pane(
    row: &Row,
    slug: &str,
    theme: Theme,
    now: Timestamp,
    width: u16,
    height: u16,
) -> Vec<Line<'static>> {
    let mut lines = facts(row, slug, theme, now, width);
    let mut tail = section("last command", theme, width, command(row, theme, width));
    // The command goes too on a panel with room for the fact block and
    // nothing else. Every section is a rule and what is under it, so a panel
    // that cannot hold both holds neither.
    if usize::from(height).saturating_sub(lines.len()) < tail.len() {
        tail.clear();
    }
    // What is left once the fact block and the command have their lines, less
    // the section's own rule — which is why the rule is subtracted here and
    // added by `section`.
    let spare = |taken: usize| {
        usize::from(height)
            .saturating_sub(taken)
            .saturating_sub(tail.len())
            .saturating_sub(1)
    };
    lines.extend(section(
        "agent",
        theme,
        width,
        texts(row, theme, spare(lines.len()), width),
    ));
    lines.extend(section(
        "commits",
        theme,
        width,
        commits(row, theme, spare(lines.len()), width),
    ));
    lines.extend(tail);
    lines
}

/// The fact block: the labelled lines that open the pane.
fn facts(row: &Row, slug: &str, theme: Theme, now: Timestamp, width: u16) -> Vec<Line<'static>> {
    let mut facts = vec![labelled("state", &state(row, theme, now), theme, width)];
    if row.state == DONE {
        // The record is the answer a landed task leaves, so it is read
        // before the archive it left beside it; the step after is last
        // because it is the only line here that asks for anything.
        facts.push(labelled("review", &review(row, slug, theme), theme, width));
        facts.push(labelled("run", &run(row, slug, theme), theme, width));
        facts.push(labelled("next", &next(theme), theme, width));
        return facts;
    }
    if row.worktree.is_some() {
        facts.push(labelled("paths", &paths(row, slug, theme), theme, width));
    }
    facts.push(labelled("run", &run(row, slug, theme), theme, width));
    facts.push(labelled("review", &review(row, slug, theme), theme, width));
    facts
}

/// One fact: its label, and the pieces of its value.
fn labelled(label: &str, value: &[(String, Style)], theme: Theme, width: u16) -> Line<'static> {
    let mut spans = vec![Span::styled(format!("{label:LABEL$}"), theme.style(DIM))];
    spans.extend(laid(value, width.saturating_sub(wide_label()), theme));
    Line::from(spans)
}

/// The label column as cells.
fn wide_label() -> u16 {
    u16::try_from(LABEL).unwrap_or(u16::MAX)
}

/// The state line: the glyph and the word the row shows, and after them
/// whatever that state makes worth saying.
fn state(row: &Row, theme: Theme, now: Timestamp) -> Vec<(String, Style)> {
    let look = theme.look(&row.state, false);
    // Through the theme, as the row above it is: the arrow a blocked task
    // carries is inside its own words, and a pane reading `blocked ← T6`
    // under a row reading `blocked <- T6` would be one board disagreeing
    // with itself on one screen.
    let said = theme.state_text(&row.state);
    let (word, reason) = parted(&said);
    let mut pieces = vec![(format!("{} {word}", look.glyph), look.style)];
    for said in facts_of(row, reason, now) {
        pieces.push((format!(" {} ", theme.separator()), theme.style(DIM)));
        pieces.push((said, theme.style(TEXT)));
    }
    pieces
}

/// What follows the state's own word on that line.
fn facts_of(row: &Row, reason: Option<&str>, now: Timestamp) -> Vec<String> {
    if row.state == DONE {
        return vec![LANDED.to_string()];
    }
    let mut said: Vec<String> = reason.map(str::to_string).into_iter().collect();
    if !row.live() {
        // A closed run's one remaining fact is when it closed, from the file
        // the runner wrote as the turn ended.
        said.extend(
            row.exit
                .and_then(|exit| exit.at)
                .map(|at| format!("ended {} ago", format_elapsed(now.seconds_since(at)))),
        );
        return said;
    }
    said.extend(row.run.as_ref().map(|run| run.stage.to_string()));
    // The clock on the tool belongs to the running row alone, for the reason
    // the second line under a row gives: a paused run's last call was never
    // answered, so timing it would be timing a wait nobody is in.
    if row.running() {
        let elapsed = row.elapsed_column(now);
        if !elapsed.is_empty() {
            said.push(format!("{elapsed} in this tool"));
        }
    }
    said.extend(
        row.spawned_at
            .map(|at| format!("{} since spawn", format_age(now.seconds_since(at)))),
    );
    said
}

/// A state's own word, and the reason the recipe wrote in brackets after it.
///
/// The reason comes out of its brackets because the line it lands on is a
/// list of facts separated by dots, and `failed (exit 2)` inside one of them
/// would be a bracket in a line that has no other punctuation. What is not
/// bracketed stays where it is: `blocked ← T6` is one phrase, and the tasks
/// it names are not a reason to be lifted out of it.
fn parted(state: &str) -> (&str, Option<&str>) {
    let Some((word, rest)) = state.split_once(" (") else {
        return (state, None);
    };
    (word, rest.strip_suffix(')'))
}

/// The paths line: the branch, the worktree beside the repository, and the
/// session to attach to.
fn paths(row: &Row, slug: &str, theme: Theme) -> Vec<(String, Style)> {
    let mut said = vec![task_branch(slug, &row.id)];
    // Named as a sibling of the repository, which is where `keeler-spawn`
    // puts it and how `graph-mode.md` writes it — the report's absolute path
    // is three quarters somebody's home directory.
    said.extend(
        row.worktree
            .as_ref()
            .and_then(|worktree| worktree.file_name())
            .map(|name| format!("../{}", name.to_string_lossy())),
    );
    said.push(format!("tmux {}", row.session(slug)));
    joined(said, theme)
}

/// The run line: where the log is, and what the run ended with.
///
/// The dash is for a task whose run left nothing to name — one nobody has
/// spawned, and a landed one whose run directory went with its worktree.
fn run(row: &Row, slug: &str, theme: Theme) -> Vec<(String, Style)> {
    let log = run_file(slug, &row.id, "log");
    match (row.log.is_some(), row.exit) {
        (_, Some(exit)) => joined(vec![log, format!("exit {}", exit.code)], theme),
        (true, None) => joined(vec![log], theme),
        (false, None) => vec![(theme.dash().to_string(), theme.style(DIM))],
    }
}

/// The review line: the record and the word it settled on, or the fact that
/// nobody has written one.
fn review(row: &Row, slug: &str, theme: Theme) -> Vec<(String, Style)> {
    let Some(verdict) = &row.verdict else {
        return vec![(format!("{} {UNWRITTEN}", theme.dash()), theme.style(DIM))];
    };
    vec![
        (review_record(slug, &row.id), theme.style(DIM)),
        (format!("   Verdict: {verdict}"), theme.style(TEXT)),
    ]
}

/// The next line: the one step a landed task's board cannot take for itself.
fn next(theme: Theme) -> Vec<(String, Style)> {
    vec![
        (NEXT.to_string(), theme.style(TEXT)),
        (format!(" {} ", theme.leads_to()), theme.style(DIM)),
        (LEAVES.to_string(), theme.style(DIM)),
    ]
}

/// Several readings on one line, with the separator between them.
fn joined(said: Vec<String>, theme: Theme) -> Vec<(String, Style)> {
    let mut pieces = Vec::with_capacity(said.len() * 2);
    for one in said {
        if !pieces.is_empty() {
            pieces.push((format!(" {} ", theme.separator()), theme.style(DIM)));
        }
        pieces.push((one, theme.style(TEXT)));
    }
    pieces
}

/// The run's last words, oldest first and without a prefix — the rule above
/// them is what says whose they are.
///
/// The **last** `room` lines of them when there are more than that, for the
/// same reason the view keeps the last five texts rather than the first: what
/// the run said a moment ago is what somebody watching it wants, and a
/// paragraph shown from its opening would push that off the panel.
fn texts(row: &Row, theme: Theme, room: usize, width: u16) -> Vec<Line<'static>> {
    let Some(run) = &row.run else {
        return Vec::new();
    };
    let said: Vec<&str> = run
        .texts
        .iter()
        // A text block can hold several lines, and a pane that wrote the
        // newline as a symbol would show one long line of mojibake where the
        // run's own paragraph should be.
        .flat_map(|text| text.lines())
        .collect();
    said[said.len().saturating_sub(room)..]
        .iter()
        .map(|line| Line::from(plain(line, theme, width)))
        .collect()
}

/// The commits the branch has made since the feature branch, newest first,
/// and how many did not fit.
fn commits(row: &Row, theme: Theme, room: usize, width: u16) -> Vec<Line<'static>> {
    let Some(branch) = &row.branch else {
        return Vec::new();
    };
    // When they do not all fit, one line of the room goes to saying how many
    // are not shown: a list cut at the panel's edge would read as the whole
    // of what the branch has done, which is the one thing the pane is asked
    // this for. Which is also why a section that could hold nothing but that
    // count draws nothing at all — `… +9 more` under a rule and above no
    // commit at all is the pane reporting its own arithmetic.
    let over = branch.commits.len() > room;
    if branch.commits.is_empty() || room == 0 || (over && room < 2) {
        return Vec::new();
    }
    let shown = if over {
        room.saturating_sub(1)
    } else {
        branch.commits.len()
    };
    let mut lines: Vec<Line<'static>> = branch.commits[..shown]
        .iter()
        .map(|commit| {
            Line::from(plain(
                &format!("{} {}", commit.hash, commit.subject),
                theme,
                width,
            ))
        })
        .collect();
    if over {
        lines.push(Line::styled(
            format!(
                "{} +{} more",
                theme.ellipsis(),
                branch.commits.len().saturating_sub(shown)
            ),
            theme.style(DIM),
        ));
    }
    lines
}

/// The last call the run made, in full — the row's own column has to cut it
/// to fit, and this is where the whole of it is.
fn command(row: &Row, theme: Theme, width: u16) -> Vec<Line<'static>> {
    row.run
        .as_ref()
        .filter(|run| run.last_tool.is_some())
        .map(|run| vec![Line::from(plain(&RunView::tool_column(run), theme, width))])
        .unwrap_or_default()
}

/// One of the pane's parts: a rule naming it, and what it holds — and
/// nothing at all when it holds nothing, because a rule over an empty space
/// is the pane reporting on itself.
fn section(name: &str, theme: Theme, width: u16, body: Vec<Line<'static>>) -> Vec<Line<'static>> {
    if body.is_empty() {
        return Vec::new();
    }
    let mut lines = vec![rule(name, theme, width)];
    lines.extend(body);
    lines
}

/// A section's rule, drawn to the pane's inner edge.
///
/// The whole width and not the name's own: the rules are what divides the
/// pane into parts, and a short one would read as a heading with something
/// beside it.
fn rule(name: &str, theme: Theme, width: u16) -> Line<'static> {
    let opening = format!("{0}{0} {name} ", theme.rule());
    let filled = usize::from(width.saturating_sub(wide(&opening)));
    Line::styled(
        format!("{opening}{}", theme.rule().repeat(filled)),
        theme.style(CHROME),
    )
}

/// A line of the run's own text, cut where the pane ends.
fn plain(text: &str, theme: Theme, width: u16) -> Span<'static> {
    Span::styled(cut(text, width, theme.ellipsis()), theme.style(TEXT))
}

/// A fact's pieces, cut where the pane ends — across the pieces rather than
/// inside one of them, as a row's columns are.
fn laid(pieces: &[(String, Style)], width: u16, theme: Theme) -> Vec<Span<'static>> {
    let mut spans = Vec::with_capacity(pieces.len());
    let mut left = width;
    for (text, style) in pieces {
        if left == 0 {
            break;
        }
        let piece = cut(text, left, theme.ellipsis());
        left = left.saturating_sub(wide(&piece));
        spans.push(Span::styled(piece, *style));
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::{pane, parted};
    use crate::board::Row;
    use crate::clock::Timestamp;
    use crate::dispatch::Exit;
    use crate::git::{BranchFacts, Commit};
    use crate::run::{RunView, Stage, ToolCall};
    use crate::theme::Theme;
    use ratatui::text::Line;

    /// The theme these panes are drawn through, said outright rather than
    /// read from the process — which is the whole reason the theme is a
    /// value.
    const THEME: Theme = Theme::new(true, false);

    /// The spec every fixture below is a task of, and the clock every pane
    /// is drawn against.
    const SLUG: &str = "10-keeler-top";
    const NOON: i64 = 1_788_782_400;

    /// How wide the panel's inside is: enough for every line whole, so what
    /// a scenario reads back is the pane rather than a cut of it.
    const WIDE: u16 = 100;

    fn now() -> Timestamp {
        Timestamp::from_epoch_seconds(NOON)
    }

    /// A pane's lines as text, which is what its arithmetic shows up in.
    fn shown(row: &Row, height: u16) -> Vec<String> {
        pane(row, SLUG, THEME, now(), WIDE, height)
            .iter()
            .map(text)
            .collect()
    }

    fn text(line: &Line<'_>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    /// A task in a state, with nothing behind it — the shape of a row the
    /// report carries no paths for.
    fn row(id: &str, state: &str) -> Row {
        Row {
            id: id.to_string(),
            state: state.to_string(),
            log: None,
            worktree: None,
            run: None,
            branch: None,
            title: None,
            verdict: None,
            exit: None,
            spawned_at: None,
        }
    }

    /// T3 as the live scenarios describe it: running in its gate stage,
    /// seven minutes and fifty-two seconds into a `just dev`, spawned
    /// forty-one minutes ago, with a review record already written.
    fn t3() -> Row {
        Row {
            log: Some(std::path::PathBuf::from(
                "/r/.keeler/runs/10-keeler-top/t3.log",
            )),
            worktree: Some(std::path::PathBuf::from("/w/keeler-10-keeler-top-t3")),
            run: Some(RunView {
                stage: Stage::Gate,
                last_tool: Some(ToolCall {
                    id: "toolu_1".to_string(),
                    name: "Bash".to_string(),
                    detail: "just dev".to_string(),
                    at: Some(Timestamp::from_epoch_seconds(NOON - 472)),
                }),
                ..RunView::default()
            }),
            verdict: Some("pass".to_string()),
            spawned_at: Some(Timestamp::from_epoch_seconds(NOON - 41 * 60)),
            ..row("T3", "running")
        }
    }

    #[test]
    fn the_live_pane_opens_with_a_fact_block() {
        // Given T3 is running, stage gate, in its tool for 07:52, spawned 41
        // minutes ago, with a review record carrying Verdict: pass
        // When T3 is selected
        let pane = shown(&t3(), 24);

        // Then the pane's first lines read the four facts, in the order a
        // live task raises them
        assert_eq!(
            pane[..4],
            [
                "state   ● running · gate · 07:52 in this tool · 41m since spawn",
                "paths   keeler/10-keeler-top/t3 · ../keeler-10-keeler-top-t3 · tmux keeler-10-keeler-top-t3",
                "run     .keeler/runs/10-keeler-top/t3.log",
                "review  reviews/10-keeler-top/t3.md   Verdict: pass",
            ],
        );
    }

    #[test]
    fn a_review_record_not_yet_written_says_so() {
        // Given T3 has no reviews/<slug>/t3.md on its branch
        let row = Row {
            verdict: None,
            ..t3()
        };

        // When T3 is selected
        let pane = shown(&row, 24);

        // Then the review line says nobody has written one, rather than
        // naming a file that is not there
        assert_eq!(pane[3], "review  — not written yet");
    }

    #[test]
    fn a_failed_tasks_state_line_carries_the_exit_code() {
        // Given T9 failed (exit 2) and its turn ended 04:11 ago
        let row = Row {
            exit: Some(Exit {
                code: 2,
                at: Some(Timestamp::from_epoch_seconds(NOON - 251)),
            }),
            ..row("T9", "failed (exit 2)")
        };

        // When T9 is selected
        let pane = shown(&row, 24);

        // Then the state line reads the word, the code and when it ended
        assert_eq!(pane[0], "state   ✗ failed · exit 2 · ended 04:11 ago");
        // And a closed run is not timed against a tool it is no longer in.
        assert!(!pane[0].contains("in this tool"));
    }

    /// T3 with the run's own words, a branch behind it, and the tool it is
    /// in — the row every section of the pane has something to say about.
    fn t3_saying(texts: &[&str], commits: usize) -> Row {
        let run = RunView {
            texts: texts.iter().map(|said| (*said).to_string()).collect(),
            ..t3().run.expect("the fixture has a run")
        };
        Row {
            run: Some(run),
            branch: Some(BranchFacts {
                head: "b33e05f".to_string(),
                ahead: commits,
                dirty: 0,
                commits: (0..commits)
                    .map(|which| Commit {
                        hash: format!("b33e05{which}"),
                        subject: format!("feat(10-keeler-top): T3 step {which}"),
                    })
                    .collect(),
            }),
            ..t3()
        }
    }

    #[test]
    fn the_panes_sections_are_ruled_and_ordered_agent_commits_last_command() {
        // Given T3 has seven texts, four commits since the feature branch
        // and a last command — the view keeps five of the texts, which is
        // what the pane is handed
        let row = t3_saying(&["three", "four", "five", "six", "seven"], 4);

        // When T3 is selected
        let pane = shown(&row, 24);

        // Then after the fact block come the agent's last five texts, oldest
        // first and without a prefix
        assert!(pane[4].starts_with("── agent ──"), "{:?}", pane[4]);
        assert_eq!(pane[5..10], ["three", "four", "five", "six", "seven"]);
        // And then the commits, newest first, the first hash equal to the
        // row's COMMIT hash
        assert!(pane[10].starts_with("── commits ──"), "{:?}", pane[10]);
        assert_eq!(pane[11], "b33e050 feat(10-keeler-top): T3 step 0");
        assert_eq!(pane[14], "b33e053 feat(10-keeler-top): T3 step 3");
        // And then the command in full
        assert!(pane[15].starts_with("── last command ──"), "{:?}", pane[15]);
        assert_eq!(pane[16], "Bash: just dev");
        // And every rule spans the pane's inner width
        for rule in [&pane[4], &pane[10], &pane[15]] {
            assert_eq!(
                unicode_width::UnicodeWidthStr::width(rule.as_str()),
                usize::from(WIDE),
                "a rule stops short of the pane's edge: {rule:?}",
            );
        }
    }

    #[test]
    fn commits_that_do_not_fit_end_with_a_count() {
        // Given T3 has 9 commits since the feature branch and the pane holds
        // 3 commit lines — the fact block is four, the agent's rule and its
        // one text two, the commits' own rule one, and the last command two
        let row = t3_saying(&["one"], 9);

        // When T3 is selected
        let pane = shown(&row, 12);

        // Then the third commit line says how many were not drawn
        assert!(pane[6].starts_with("── commits ──"), "{:?}", pane[6]);
        assert_eq!(pane[7], "b33e050 feat(10-keeler-top): T3 step 0");
        assert_eq!(pane[8], "b33e051 feat(10-keeler-top): T3 step 1");
        assert_eq!(pane[9], "… +7 more");
        // And it is drawn dim, because it is the pane talking about the list
        // rather than another line of it.
        let drawn = pane_of(&row, 12);
        assert_eq!(
            drawn[9].style,
            ratatui::style::Style::new().fg(crate::theme::DIM),
        );
        // And the command is still under it: the commits are what gives way,
        // not the section after them.
        assert_eq!(pane[11], "Bash: just dev");
        assert_eq!(pane.len(), 12, "the pane outgrew the panel drawing it");
    }

    /// The pane as lines rather than as text, for the scenarios that ask
    /// what something is drawn in.
    fn pane_of(row: &Row, height: u16) -> Vec<Line<'static>> {
        pane(row, SLUG, THEME, now(), WIDE, height)
    }

    #[test]
    fn a_paragraph_longer_than_the_panel_keeps_the_words_that_came_last() {
        // A text block is one of the run's turns and can hold a paragraph,
        // so the agent's section is as long as what it holds unless
        // something bounds it. The review found the pane drawn past the
        // panel's bottom border, taking the command under it with it.
        let row = t3_saying(&["one\ntwo\nthree\nfour\nfive\nsix\nseven\neight"], 0);

        let pane = shown(&row, 10);

        assert_eq!(pane.len(), 10, "the pane outgrew the panel");
        assert!(pane[4].starts_with("── agent ──"), "{:?}", pane[4]);
        // The last of them, as the view keeps the last five texts: what the
        // run said a moment ago is what somebody watching it came for.
        assert_eq!(pane[5..8], ["six", "seven", "eight"]);
        // And the section under it is still drawn, which is the whole point
        // of bounding the one above.
        assert!(pane[8].starts_with("── last command ──"), "{:?}", pane[8]);
        assert_eq!(pane[9], "Bash: just dev");
    }

    #[test]
    fn a_pane_with_no_room_for_a_commit_draws_no_commits_at_all() {
        // Not a scenario of its own: it is what the one above does not say
        // about a panel with nothing left over. A rule with no list under it
        // would be the pane reporting on its own arithmetic.
        let row = t3_saying(&["one"], 9);

        let pane = shown(&row, 8);

        assert!(
            !pane.iter().any(|line| line.starts_with("── commits")),
            "a section was ruled off with nothing in it:\n{}",
            pane.join("\n"),
        );
        assert_eq!(pane[6], "── last command ──".to_string() + &"─".repeat(82));
    }

    /// T1 as the landed scenarios describe it: done, its worktree gone, its
    /// record and its exit file still where they were written.
    fn t1_done() -> Row {
        Row {
            verdict: Some("pass".to_string()),
            exit: Some(Exit {
                code: 0,
                at: Some(Timestamp::from_epoch_seconds(NOON - 900)),
            }),
            ..row("T1", "done")
        }
    }

    #[test]
    fn a_done_tasks_pane_shows_its_record_its_run_and_the_next_step() {
        // Given T1 is done and its worktree is gone
        // When T1 is selected
        let pane = shown(&t1_done(), 24);

        // Then its lines read the state, the record, the run and the step
        // after — and no paths line, because there is nothing left to visit
        assert_eq!(
            pane,
            [
                "state   ✓ done · landed, worktree and branch removed by keeler-land",
                "review  reviews/10-keeler-top/t1.md   Verdict: pass",
                "run     .keeler/runs/10-keeler-top/t1.log · exit 0",
                "next    keeler keeler-land on main → baseline staged, Status: Implemented",
            ],
        );
    }

    #[test]
    fn a_done_task_whose_run_files_are_gone_shows_dashes_for_them() {
        // Given T1 is done and .keeler/runs/<slug>/t1.exit does not exist —
        // which is what `keeler-land` removing the worktree leaves behind
        let row = Row {
            exit: None,
            ..t1_done()
        };

        // When T1 is selected
        let pane = shown(&row, 24);

        // Then the run line names no file it cannot open
        assert_eq!(pane[2], "run     —");
    }

    #[test]
    fn a_fact_too_long_for_a_narrow_panel_is_cut_where_the_panel_ends() {
        // Across the pieces rather than inside one of them, as a row's
        // columns are: a fact is several readings on one line, and a piece
        // that starts past the edge is not drawn at all rather than drawn
        // half a cell wide.
        let lines = pane(&t3(), SLUG, THEME, now(), 24, 24);

        for line in &lines {
            assert!(
                unicode_width::UnicodeWidthStr::width(text(line).as_str()) <= 24,
                "a line ran past the panel: {:?}",
                text(line),
            );
        }
        // The line ends where the panel does, and what did not fit is gone
        // whole: "07:52 in this tool" begins past the edge and is not drawn
        // at all.
        assert_eq!(text(&lines[0]), "state   ● running · gate");
    }

    #[test]
    fn a_state_the_recipe_wrote_a_reason_after_is_read_in_two_halves() {
        // The reason comes out of its brackets because the line it lands on
        // separates its facts with dots; what is not bracketed stays whole.
        assert_eq!(parted("failed (exit 2)"), ("failed", Some("exit 2")));
        assert_eq!(
            parted("incomplete (no review record, box not ticked)"),
            ("incomplete", Some("no review record, box not ticked")),
        );
        assert_eq!(parted("running"), ("running", None));
        assert_eq!(parted("blocked ← T6, T4"), ("blocked ← T6, T4", None));
        assert_eq!(parted("not spawned"), ("not spawned", None));
        // A bracket the recipe never closed is not a reason to be lifted
        // out of anything.
        assert_eq!(parted("failed (exit 2"), ("failed", None));
    }

    #[test]
    fn a_task_nobody_has_spawned_has_a_pane_and_says_what_it_has_not_got() {
        // No run, no branch, no worktree and no files: every section is
        // silent and the fact block says why rather than showing paths to
        // things that are not there.
        let pane = shown(&row("T5", "ready"), 24);

        assert_eq!(
            pane,
            ["state   ◇ ready", "run     —", "review  — not written yet",],
        );
    }

    #[test]
    fn a_paused_run_is_not_timed_against_the_tool_it_was_killed_in() {
        // Its last call was never answered — the session was killed in the
        // middle of it — so the clock on that call is not a clock on the
        // pause, exactly as the second line under the row has it.
        let row = Row { ..t3() };
        let paused = Row {
            state: "paused".to_string(),
            ..row
        };

        let pane = shown(&paused, 24);

        assert_eq!(
            pane[0], "state   ‖ paused · gate · 41m since spawn",
            "a timer was left running beside a task nobody is waiting on",
        );
    }

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig {
            failure_persistence: Some(Box::new(
                proptest::test_runner::FileFailurePersistence::WithSource("proptest-regressions"),
            )),
            ..proptest::prelude::ProptestConfig::default()
        })]

        /// However many commits a branch has and however much room the panel
        /// leaves them, the list never outgrows that room, what it shows is
        /// the newest commits in order, and every commit is either drawn or
        /// counted. A list that overflowed would be drawn over the section
        /// under it, and one that lied about the count would be worse than
        /// showing none.
        ///
        /// Nothing at all is the answer in three cases, and only those: a
        /// branch with no commits, no room, and room for nothing but the
        /// count — where a rule over one line reading `… +9 more` would be
        /// the pane reporting its own arithmetic.
        #[test]
        fn a_commit_list_fits_its_room_and_says_what_it_left_out(
            how_many in 0_usize..20,
            room in 0_usize..12,
        ) {
            let row = t3_saying(&[], how_many);
            let branch = row.branch.clone().expect("the fixture has a branch");

            let lines: Vec<String> = super::commits(&row, THEME, room, WIDE)
                .iter()
                .map(text)
                .collect();

            proptest::prop_assert!(lines.len() <= room);
            if lines.is_empty() {
                proptest::prop_assert!(
                    how_many == 0 || room == 0 || (how_many > room && room < 2),
                    "the list was dropped with room for {} of {}",
                    room,
                    how_many,
                );
                return Ok(());
            }
            let (shown, left) = match lines.last() {
                Some(last) if last.starts_with('…') => (
                    lines.len() - 1,
                    last.trim_start_matches("… +")
                        .trim_end_matches(" more")
                        .parse::<usize>()
                        .expect("the count is a number"),
                ),
                _ => (lines.len(), 0),
            };
            proptest::prop_assert_eq!(shown + left, how_many);
            for (line, commit) in lines[..shown].iter().zip(&branch.commits) {
                proptest::prop_assert!(line.starts_with(&commit.hash), "{}", line);
            }
        }

        /// Whatever a run has said and whatever its branch has done, the pane
        /// fits the panel it is drawn in. Two of its parts are as long as
        /// what they hold — a text block can be a paragraph and a branch can
        /// have thirty commits — and a pane that outgrew its panel would be
        /// drawn over the border and take the section below it with it.
        ///
        /// Four lines is the floor rather than none: the fact block is the
        /// pane's reason to exist and is never cut, and `frame::layout` draws
        /// no detail panel with fewer rows inside it than that.
        #[test]
        fn a_pane_fits_the_panel_whatever_the_run_has_said(
            texts in proptest::collection::vec("[a-z]{1,8}(\n[a-z]{1,8}){0,6}", 0..5),
            commits in 0_usize..20,
            height in 4_u16..30,
        ) {
            let said: Vec<&str> = texts.iter().map(String::as_str).collect();
            let row = t3_saying(&said, commits);

            let lines = pane(&row, SLUG, THEME, now(), WIDE, height);

            proptest::prop_assert!(
                lines.len() <= usize::from(height),
                "{} lines in a panel {} rows tall",
                lines.len(),
                height,
            );
            // And the fact block is always there: it is what a short panel
            // keeps, not what it gives up.
            proptest::prop_assert!(text(&lines[0]).starts_with("state   "));
        }
    }

    #[test]
    fn the_pane_is_drawn_in_the_glyphs_the_terminal_can_draw() {
        // Every glyph the pane owns — the separator, the dash, the arrow in
        // a blocked state's own words, the rules and the mark that says a
        // list was cut — comes from the theme, so a terminal that cannot
        // draw them gets a pane out of the ASCII set whole.
        let saying = Row {
            verdict: None,
            ..t3_saying(&["one"], 9)
        };
        let blocked = Row {
            state: "blocked ← T6".to_string(),
            ..saying.clone()
        };
        let ascii = Theme::new(true, true);

        for fixture in [saying, blocked, t1_done()] {
            for line in pane(&fixture, SLUG, ascii, now(), WIDE, 12) {
                let drawn = text(&line);
                assert!(
                    drawn.is_ascii(),
                    "the pane drew a glyph this terminal cannot: {drawn:?}",
                );
            }
        }
        // And the state's own words are swapped there as they are in the row
        // above: one board must not disagree with itself on one screen.
        let blocked = Row {
            state: "blocked ← T6".to_string(),
            ..row("T7", "blocked ← T6")
        };
        assert_eq!(
            text(&pane(&blocked, SLUG, ascii, now(), WIDE, 12)[0]),
            "state   - blocked <- T6",
        );
    }
}
