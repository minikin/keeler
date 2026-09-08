//! The wave panel: what the whole wave is doing, in two lines.
//!
//! A watcher of a wave asks, in this order: *is anything stuck? what is
//! running, and how far along? what is left?* The first line answers the
//! first and the third — the healthy counts on the left, and against the
//! right edge the tasks that need a human, which is **empty when there are
//! none**, and that emptiness is itself the reading. The second is one
//! glyph per task, in the order the report gave them, with the keys the
//! board answers to against the same edge.
//!
//! The tasks panel is here too, and it is a panel and not a list: the rows
//! are `frame.rs`'s, but which of them are drawn, on how many lines each,
//! and where the drawing starts are questions about the box they go in.
//! [`tasks`] answers all three — [`collapse`] and [`scrolled`] are the two
//! decisions inside it, and each is a function of numbers with a test of
//! its own. The detail panel is one row in full, which is somewhere else
//! again.
//!
//! **What the lines say is decided apart from what they look like.**
//! [`counts`], [`needing`], [`naming`], [`strip`] and [`hints`] are the
//! reading; the spans built around them are the paint. Each is a function
//! of the rows alone and has a test of its own, because a suite that
//! asserted only on the drawn frame would be reading the arithmetic through
//! the paint. None of them leaves this module — [`wave`], [`tasks`] and
//! [`title`] are the whole of what a frame asks the panels for.

use ratatui::text::{Line, Span};

use crate::board::{Board, Row};
use crate::clock::Timestamp;
use crate::layout::{Columns, wide};
use crate::theme::{BLUE, DIM, GREEN, NEEDS_YOU, TEXT, Theme};

/// What stands between two readings on a header line.
///
/// Three spaces, and not one: two counts a space apart are read as one
/// phrase, and the whole of what this line does is let the eye take them in
/// separately.
const GAP: &str = "   ";

/// How many glyphs of the outcome strip stand together before a space.
///
/// A thirty-task wave is a wall of glyphs with nothing to count from, and
/// fives are what makes "the fourteenth" a thing the eye can find.
const GROUP: usize = 5;

/// The healthy states the counts cover, in the state table's order.
///
/// The word is the state's own, spelled as the header shows it — the theme
/// classifies by the leading word alone, so `not spawned` is one entry here
/// and one row of that table.
const COUNTED: [&str; 6] = [
    "running",
    "passed",
    "ready",
    "blocked",
    "not spawned",
    "done",
];

/// The one state whose reason belongs on the header line.
const FAILED: &str = "failed";

/// What a board with nothing left to run has to say.
const FINISHED: &str = "the feature is finished here — land it on main";

/// What `z` does to a board whose rows are two-line, and what it does to
/// one whose rows are not. One key, and the hint names the half of it the
/// board is not in.
const COMPACT: &str = "compact";
const EXPAND: &str = "expand";

/// The keys the board answers to, and what each of them does.
const HINTS: [(&str, &str); 7] = [
    ("j/k", "move"),
    ("Enter", "attach"),
    ("p", "pause"),
    ("R", "resume"),
    ("r", "refresh"),
    ("z", COMPACT),
    ("q", "quit"),
];

/// Which of them is the one whose word depends on the board it is drawn on.
const TOGGLE: usize = 5;

/// The three a finished board has a use for: every run is over, so there is
/// no session to attach, nothing to pause and nothing to resume.
const LANDED_HINTS: [(&str, &str); 3] = [("j/k", "move"), ("r", "refresh"), ("q", "quit")];

/// The panel's title: which spec the board is about, and the ref the report
/// answered from.
///
/// The word alone carries the panel's colour, and what follows it is dim: a
/// title is read as a label first and as a path second, and a path in the
/// label's colour makes the two one long blue sentence.
#[must_use]
pub fn title(board: &Board, theme: Theme) -> Vec<Span<'static>> {
    vec![
        Span::styled("wave", theme.style(BLUE)),
        Span::styled(
            format!("  {} on {}", board.rel, board.git_ref),
            theme.style(DIM),
        ),
    ]
}

/// The panel's lines, laid out for the width inside its borders.
///
/// Two of them where the window has the rows for both, and on a short one
/// the first alone — which is the half that is about this wave. The second
/// is the strip and the keys: the keys are the same on every board there is,
/// and the strip is the first line's counts task by task. A window that has
/// to choose keeps the reading that cannot be had anywhere else.
#[must_use]
pub fn wave(
    board: &Board,
    theme: Theme,
    now: Timestamp,
    width: u16,
    lines: u16,
) -> Vec<Line<'static>> {
    let mut panel = vec![first(board, theme, now, width)];
    if lines > 1 {
        panel.push(second(board, theme, width));
    }
    panel
}

/// The column heading, which takes a row of the panel like any other.
const HEADING: usize = 1;

/// The tasks panel: the heading, and as much of the board as `height` rows
/// hold, wound so that the watched task is among them.
///
/// `height` is the panel's inner height, the heading's line included and
/// its borders excluded — and it is the tallest the panel could be rather
/// than the height it ends up with. The two are the same number whenever it
/// matters: a panel shorter than that is one the detail pane has already
/// been dropped from, and one taller has room for every line there is.
#[must_use]
pub fn tasks(
    board: &Board,
    cols: &Columns,
    theme: Theme,
    now: Timestamp,
    height: u16,
) -> Vec<Line<'static>> {
    let room = usize::from(height).saturating_sub(HEADING);
    let expanded = drawn(board, cols, theme, now, Collapse::None);
    let collapse = collapse(board.compact, expanded.rows.len(), room);
    let Drawn { rows, watched } = if collapse == Collapse::None {
        expanded
    } else {
        drawn(board, cols, theme, now, collapse)
    };
    let mut lines = vec![Line::styled(
        cols.header(theme.ellipsis()),
        theme.style(DIM),
    )];
    lines.extend(
        rows.into_iter()
            .skip(scrolled(watched.start, watched.end, room))
            .take(room),
    );
    lines
}

/// How many of the live rows give up the line under them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Collapse {
    /// None: every live row says what it is doing underneath itself.
    None,
    /// All but the watched one — the panel's own answer when the rows have
    /// outgrown it. The row somebody is reading keeps its second line
    /// because what it is doing now is why they are reading it.
    Crowded,
    /// Every one of them, which is what `z` asks for: somebody who wants
    /// the whole wave on one screen wants the watched row on one line too.
    Whole,
}

impl Collapse {
    /// Whether one row is drawn on a line of its own.
    const fn takes(self, selected: bool) -> bool {
        match self {
            Self::None => false,
            Self::Crowded => !selected,
            Self::Whole => true,
        }
    }
}

/// The rows as lines, and where among them the watched task's own are.
struct Drawn {
    rows: Vec<Line<'static>>,
    watched: std::ops::Range<usize>,
}

/// Every task's lines, in the order the board draws them, with the watched
/// task's marked and painted.
///
/// The selection is a report index and the rows are in the board's order, so
/// the two are compared here rather than counted: what `j` moved is a task,
/// and where it ends up on the screen is this order's answer.
fn drawn(board: &Board, cols: &Columns, theme: Theme, now: Timestamp, collapse: Collapse) -> Drawn {
    let mut rows: Vec<Line<'static>> = Vec::new();
    let mut watched = 0..0;
    for (index, row) in crate::board::ordered(&board.rows) {
        let selected = index == board.selected;
        let lines = crate::frame::lines(row, cols, theme, now, selected, collapse.takes(selected));
        if selected {
            watched = rows.len()..rows.len().saturating_add(lines.len());
        }
        rows.extend(lines.into_iter().map(|line| {
            if selected {
                // A lighter ground and never reverse video, which would
                // throw away the colours the row was drawn in.
                line.style(theme.selection())
            } else {
                line
            }
        }));
    }
    Drawn { rows, watched }
}

/// How the live rows are drawn: what the watcher asked for with `z`, and
/// otherwise whether they have outgrown the panel.
///
/// The hand answer wins outright, in both directions. A board collapsed by
/// somebody who wanted the whole wave on one screen must not expand itself
/// when two tasks finish, and one expanded by somebody reading a tool must
/// not collapse when a third task starts — it scrolls instead.
fn collapse(asked: Option<bool>, lines: usize, room: usize) -> Collapse {
    match asked {
        Some(true) => Collapse::Whole,
        None if lines > room => Collapse::Crowded,
        // A board expanded by hand, and one whose rows the panel has room
        // for as they are: the same drawing, from two different answers.
        Some(false) | None => Collapse::None,
    }
}

/// Which row the panel starts drawing at, so that the watched task is on it.
///
/// Derived per frame and never stored: the offset is only ever about where
/// the selection is, and a number kept between frames would be a second
/// answer to disagree with when the rows above it change state and move.
///
/// Below the window, the task's last line is the panel's last row; above
/// it, its first line is the first — which is what the `min` of the two
/// says, and why one line says both.
fn scrolled(first: usize, last: usize, room: usize) -> usize {
    first.min(last.saturating_sub(room))
}

/// How many tasks are in each of the healthy states, in the state table's
/// order, with the states nobody is in left out.
///
/// Left out rather than shown as a zero: the line is read at a glance for
/// what the wave is doing, and six tallies of which four say nothing are
/// four things between the reader and the two that do.
fn counts(rows: &[Row]) -> Vec<(&'static str, usize)> {
    COUNTED
        .into_iter()
        .map(|word| {
            let how_many = rows
                .iter()
                .filter(|row| Theme::rank(&row.state) == Theme::rank(word))
                .count();
            (word, how_many)
        })
        .filter(|(_, how_many)| *how_many > 0)
        .collect()
}

/// The tasks a human has to do something about, in the state table's order
/// and, within one state, in the order the report gave them.
///
/// Which tasks those are is the group's answer, and what order they come in
/// is the table's: three states share group 0, and the header names them one
/// at a time. A state the table has no row for is grouped with the running
/// tasks and so is not among them — the board cannot read the word, and a
/// state it cannot read must not be promoted to the half of the line that
/// means somebody is needed.
fn needing(rows: &[Row]) -> Vec<&Row> {
    let mut needing: Vec<&Row> = rows
        .iter()
        .filter(|row| Theme::group(&row.state) <= NEEDS_YOU)
        .collect();
    needing.sort_by_key(|row| Theme::rank(&row.state));
    needing
}

/// What an exception is called on the header line: the exit code for a task
/// that failed, and the state's own word for every other.
///
/// Only `failed` carries a reason short enough to belong up here, and it is
/// the one that says *which* failure. `incomplete (no review record, box
/// not ticked)` is a sentence, and the row is where it is read.
fn naming(state: &str) -> &str {
    let word = state.split_whitespace().next().unwrap_or_default();
    if word != FAILED {
        return word;
    }
    state
        .split_once('(')
        .and_then(|(_, reason)| reason.strip_suffix(')'))
        .unwrap_or(word)
}

/// The outcome strip: one glyph per task, in the order the report gave
/// them, counted off in fives.
///
/// The report's order and not the board's, deliberately. The rows lead with
/// what needs a human, and they move as states change; the strip is the
/// spec read left to right, and a glyph that stays where it was is what
/// makes a second glance at it worth anything.
fn strip(rows: &[Row], theme: Theme, finished: bool) -> Vec<Span<'static>> {
    let mut spans = Vec::with_capacity(rows.len());
    for (index, row) in rows.iter().enumerate() {
        let look = theme.look(&row.state, finished);
        spans.push(Span::styled(look.glyph, look.style));
        if (index + 1) % GROUP == 0 && index + 1 < rows.len() {
            spans.push(Span::raw(" "));
        }
    }
    spans
}

/// The keys the board answers to, as the second line names them.
fn hints(finished: bool, compact: bool) -> Vec<(&'static str, &'static str)> {
    if finished {
        return LANDED_HINTS.to_vec();
    }
    let mut hints = HINTS.to_vec();
    hints[TOGGLE].1 = if compact { EXPAND } else { COMPACT };
    hints
}

/// The first line: the counts, and against the right edge whatever needs a
/// human and how old the report is.
///
/// **The age is the one thing on it that is never given up.** A board whose
/// read has stalled looks exactly like a live one except for that number, so
/// a window too narrow for the line takes its cells from everything else
/// first: the counts, which are work going as it should, and then the names
/// of what needs a human — which are on the strip under this line as glyphs
/// whatever happens to them here.
fn first(board: &Board, theme: Theme, now: Timestamp, width: u16) -> Line<'static> {
    let mut counted = tally(&board.rows, theme, board.finished());
    if board.finished() {
        counted.push(vec![Span::styled(FINISHED, theme.style(GREEN))]);
    }
    let age = Span::styled(board.age(now), theme.style(DIM));
    // The names give up their end of the list rather than their front: they
    // are in the state table's order, most urgent first, so the last of them
    // is the least of what a human is needed for.
    let room = width.saturating_sub(wide(&age.content));
    let mut right = fitting(
        needed(&board.rows, theme),
        room.saturating_sub(wide(GAP)),
        &separator(theme),
        Given::Last,
    );
    if !right.is_empty() {
        right.push(Span::raw(GAP));
    }
    right.push(age);
    let left = fitting(
        counted,
        width
            .saturating_sub(measured(&right))
            .saturating_sub(wide(GAP)),
        &Span::raw(GAP),
        Given::First,
    );
    spread(left, right, width)
}

/// Which end of a list a line that does not fit takes its cells from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Given {
    /// The front of it — which for the counts is the end furthest from the
    /// two readings this line exists for.
    First,
    /// The back — which for the names is the least urgent of them.
    Last,
}

/// As many of `groups` as `room` holds, whole ones, given up from `end`.
///
/// Whole, because half of one is worse than none: half a count is a glyph
/// with no number after it, and half a name is a task id with somebody
/// else's state beside it.
fn fitting(
    mut groups: Vec<Vec<Span<'static>>>,
    room: u16,
    between: &Span<'static>,
    end: Given,
) -> Vec<Span<'static>> {
    while !groups.is_empty() && measured(&joined(&groups, between)) > room {
        match end {
            Given::First => {
                groups.remove(0);
            }
            Given::Last => {
                groups.pop();
            }
        }
    }
    joined(&groups, between)
}

/// The groups as one run of spans, with `between` at each seam.
fn joined(groups: &[Vec<Span<'static>>], between: &Span<'static>) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    for group in groups {
        if !spans.is_empty() {
            spans.push(between.clone());
        }
        spans.extend(group.iter().cloned());
    }
    spans
}

/// The second: the strip, and the hints against the right edge — when there
/// is room for both.
fn second(board: &Board, theme: Theme, width: u16) -> Line<'static> {
    let strip = strip(&board.rows, theme, board.finished());
    // The hint names what the *next press* does, so it reads the same
    // answer the key does and not how the panel below happened to draw the
    // rows. A hint taken from the drawing would offer "z expand" on a board
    // the panel collapsed by itself — where the press expands nothing and
    // takes the watched row's second line instead — and on a board of
    // closed rows, which have no second line for either word to be about.
    let hinted = hinted(theme, board.finished(), board.compact == Some(true));
    // The strip is one cell a task and the hints are the same seven on
    // every board there is: a wave too wide for both keeps the half that is
    // about the wave, and the keys are on the second line of the README.
    if measured(&strip)
        .saturating_add(wide(GAP))
        .saturating_add(measured(&hinted))
        > width
    {
        return Line::from(strip);
    }
    spread(strip, hinted, width)
}

/// The counts, as they are drawn: the glyph in the state's colour, and how
/// many there are in the ordinary text one.
///
/// Only the glyph, because the right half of this line is the one that
/// means *act* and it is coloured throughout — six coloured tallies beside
/// it would spend that signal on work going as it should.
///
/// One entry per count rather than one run of spans, because a narrow window
/// gives them up one at a time and a count is two spans.
fn tally(rows: &[Row], theme: Theme, finished: bool) -> Vec<Vec<Span<'static>>> {
    counts(rows)
        .into_iter()
        .map(|(word, how_many)| {
            let look = theme.look(word, finished);
            vec![
                Span::styled(look.glyph, look.style),
                Span::styled(format!(" {how_many} {word}"), theme.style(TEXT)),
            ]
        })
        .collect()
}

/// What needs a human, as it is drawn: each task in its state's colour,
/// because the colour is what says which kind of trouble it is in.
///
/// One entry per task, for the reason [`tally`] gives: a narrow window gives
/// them up one at a time.
fn needed(rows: &[Row], theme: Theme) -> Vec<Vec<Span<'static>>> {
    needing(rows)
        .into_iter()
        .map(|row| {
            let look = theme.look(&row.state, false);
            vec![Span::styled(
                format!("{} {} {}", look.glyph, row.id, naming(&row.state)),
                look.style,
            )]
        })
        .collect()
}

/// The hints, as they are drawn: the key bright and what it does dim, so
/// that a line of them reads as keys with words beside them.
fn hinted(theme: Theme, finished: bool, compact: bool) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    for (key, does) in hints(finished, compact) {
        if !spans.is_empty() {
            spans.push(separator(theme));
        }
        spans.push(Span::styled(key, theme.style(TEXT)));
        spans.push(Span::styled(format!(" {does}"), theme.style(DIM)));
    }
    spans
}

/// One line of the panel: what is read from the left, and what sits against
/// its right edge, with the room between them as the gap.
///
/// One space when they do not both fit, rather than none: two readings run
/// together are one unreadable reading, and the panel cuts what overflows
/// its edge either way.
fn spread(left: Vec<Span<'static>>, right: Vec<Span<'static>>, width: u16) -> Line<'static> {
    let between = width
        .saturating_sub(measured(&left))
        .saturating_sub(measured(&right))
        .max(1);
    let mut spans = left;
    spans.push(Span::raw(" ".repeat(usize::from(between))));
    spans.extend(right);
    Line::from(spans)
}

/// How many cells a run of spans takes.
fn measured(spans: &[Span<'static>]) -> u16 {
    spans
        .iter()
        .fold(0, |total, span| total.saturating_add(wide(&span.content)))
}

/// What stands between two things on one line.
fn separator(theme: Theme) -> Span<'static> {
    Span::styled(format!(" {} ", theme.separator()), theme.style(DIM))
}

#[cfg(test)]
mod tests {
    use super::{Collapse, collapse, counts, hints, naming, needing, scrolled, strip};
    use crate::board::{Board, Row};
    use crate::clock::Timestamp;
    use crate::theme::Theme;
    use ratatui::text::Line;

    /// The theme these are composed through: colours on, glyphs drawable.
    /// Said outright rather than read from the process, which is the whole
    /// reason the theme is a value.
    const THEME: Theme = Theme::new(true, false);

    /// A board of nothing but ids and states, in the report's order.
    fn rows_of(states: &[(&str, &str)]) -> Vec<Row> {
        states
            .iter()
            .map(|(id, state)| Row {
                id: (*id).to_string(),
                state: (*state).to_string(),
                log: None,
                worktree: None,
                run: None,
                branch: None,
                title: None,
                verdict: None,
                exit: None,
                spawned_at: None,
            })
            .collect()
    }

    #[test]
    fn the_counts_are_the_healthy_states_the_board_has_a_task_in() {
        let counted = counts(&rows_of(&[
            ("T1", "running"),
            ("T2", "running"),
            ("T3", "passed"),
            ("T4", "done"),
            ("T5", "done"),
            ("T6", "done"),
        ]));

        assert_eq!(counted, [("running", 2), ("passed", 1), ("done", 3)]);
        // In the state table's order, whatever order the report was in.
        assert_eq!(
            counts(&rows_of(&[("T1", "done"), ("T2", "running")])),
            [("running", 1), ("done", 1)],
        );
        // The reason after a state is not part of what is counted, and the
        // graph's two words are counted like the recipe's own.
        assert_eq!(
            counts(&rows_of(&[
                ("T1", "blocked ← T2, T3"),
                ("T2", "ready"),
                ("T3", "not spawned"),
            ])),
            [("ready", 1), ("blocked", 1), ("not spawned", 1)],
        );
        assert_eq!(counts(&[]), []);
    }

    #[test]
    fn a_state_the_table_has_no_row_for_is_counted_as_nothing() {
        // The fallback shares its group with the running tasks, so a count
        // taken by group would report a word the board cannot read as work
        // in progress. It is neither: the board does not know what it is.
        let counted = counts(&rows_of(&[("T1", "sulking"), ("T2", "running")]));

        assert_eq!(counted, [("running", 1)]);
    }

    #[test]
    fn what_needs_a_human_is_named_in_the_state_tables_order() {
        // Given the report in some order of its own
        let rows = rows_of(&[
            ("T9", "failed (exit 2)"),
            ("T8", "paused"),
            ("T4", "died"),
            ("T2", "incomplete (no review record)"),
            ("T3", "running"),
            ("T1", "done"),
        ]);

        let named: Vec<&str> = needing(&rows).iter().map(|row| row.id.as_str()).collect();

        assert_eq!(named, ["T9", "T4", "T2", "T8"]);
        // And within one state the report's order holds: `sort_by_key` is
        // stable, and the report's order is the spec's.
        let two = rows_of(&[("T5", "died"), ("T1", "died")]);
        assert_eq!(
            needing(&two)
                .iter()
                .map(|row| row.id.as_str())
                .collect::<Vec<_>>(),
            ["T5", "T1"],
        );
        // Nothing else is on it — a word the table has no row for least of
        // all.
        assert!(needing(&rows_of(&[("T1", "sulking"), ("T2", "passed")])).is_empty());
    }

    #[test]
    fn an_exception_names_the_exit_code_and_otherwise_its_states_own_word() {
        assert_eq!(naming("failed (exit 2)"), "exit 2");
        assert_eq!(naming("failed (exit 137)"), "exit 137");
        // A reason nobody wrote, and a state whose reason is a sentence the
        // row carries rather than the header.
        assert_eq!(naming("failed"), "failed");
        assert_eq!(
            naming("incomplete (no review record, box not ticked)"),
            "incomplete"
        );
        assert_eq!(naming("died"), "died");
        assert_eq!(naming("paused"), "paused");
        assert_eq!(naming(""), "");
    }

    /// The strip as characters, which is what its shape shows up in.
    fn glyphs(states: &[(&str, &str)], finished: bool) -> String {
        strip(&rows_of(states), THEME, finished)
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    #[test]
    fn the_strip_is_one_glyph_a_task_counted_off_in_fives() {
        // Given one task in each state, in the report's order
        let ten = glyphs(
            &[
                ("T1", "done"),
                ("T2", "done"),
                ("T3", "running"),
                ("T4", "died"),
                ("T5", "passed"),
                ("T6", "paused"),
                ("T7", "blocked ← T5"),
                ("T8", "not spawned"),
                ("T9", "ready"),
                ("T10", "failed (exit 2)"),
            ],
            false,
        );

        assert_eq!(ten, "✓✓●⊘◐ ‖○·◇✗");
        // A space after every fifth glyph and none after the last, however
        // the fives come out.
        assert_eq!(glyphs(&[("T1", "done")], false), "✓");
        assert_eq!(
            glyphs(
                &[
                    ("T1", "done"),
                    ("T2", "done"),
                    ("T3", "done"),
                    ("T4", "done"),
                    ("T5", "done"),
                ],
                false,
            ),
            "✓✓✓✓✓",
        );
        assert_eq!(glyphs(&[], false), "");
    }

    #[test]
    fn a_finished_boards_strip_is_the_finished_views_green() {
        let live = strip(&rows_of(&[("T1", "done")]), THEME, false);
        let landed = strip(&rows_of(&[("T1", "done")]), THEME, true);

        assert_eq!(live[0].content, landed[0].content);
        assert_eq!(
            landed[0].style,
            ratatui::style::Style::new().fg(crate::theme::GREEN),
        );
        assert_ne!(live[0].style, landed[0].style);
    }

    #[test]
    fn a_finished_board_offers_only_the_keys_it_has_a_use_for() {
        // Every run is over: there is no session to attach to, nothing to
        // pause and nothing to resume. `z` still works — it is the hint
        // that goes, not the key.
        assert_eq!(
            hints(true, false)
                .iter()
                .map(|(key, _)| *key)
                .collect::<Vec<_>>(),
            ["j/k", "r", "q"],
        );
        assert_eq!(
            hints(false, false)
                .iter()
                .map(|(key, _)| *key)
                .collect::<Vec<_>>(),
            ["j/k", "Enter", "p", "R", "r", "z", "q"],
        );
    }

    #[test]
    fn the_toggles_hint_names_the_half_of_it_the_board_is_not_in() {
        // `z` is one key and two answers, and the hint is what says which of
        // them the next press gives: a board already collapsed offering
        // "z compact" would be naming what the watcher is looking at.
        assert!(hints(false, false).contains(&("z", "compact")));
        assert!(hints(false, true).contains(&("z", "expand")));
        // And a finished board offers neither: every run is over, so there
        // is no second line for either half of the toggle to be about.
        assert!(!hints(true, true).iter().any(|(key, _)| *key == "z"));
        // Nothing else on the line moves with it.
        assert!(hints(false, true).contains(&("p", "pause")));
    }

    #[test]
    fn the_rows_collapse_when_they_outgrow_the_panel_and_when_the_watcher_says_so() {
        // The panel's own rows are the room: a board whose rows fill it
        // exactly has not outgrown anything.
        assert_eq!(collapse(None, 7, 7), Collapse::None);
        assert_eq!(collapse(None, 8, 7), Collapse::Crowded);
        assert_eq!(collapse(None, 0, 0), Collapse::None);
        // And `z` is the answer when it has been given, whichever way the
        // arithmetic would have gone: it is the watcher's board.
        assert_eq!(collapse(Some(true), 1, 7), Collapse::Whole);
        assert_eq!(collapse(Some(false), 99, 7), Collapse::None);
    }

    #[test]
    fn a_panel_that_collapsed_the_rows_itself_spares_the_one_being_read() {
        // The two collapses differ in exactly one row. The panel's own is a
        // board making room; the watcher's is a board being asked for the
        // whole wave at once, and that includes the row they are on.
        assert!(!Collapse::None.takes(false));
        assert!(!Collapse::None.takes(true));
        assert!(Collapse::Crowded.takes(false));
        assert!(
            !Collapse::Crowded.takes(true),
            "the watched row lost its line"
        );
        assert!(Collapse::Whole.takes(false));
        assert!(Collapse::Whole.takes(true));
    }

    /// A board of those rows, answered a minute before the clock the lines
    /// below are drawn against.
    fn board_of(states: &[(&str, &str)]) -> Board {
        Board {
            rel: "specs/01-foo.md".to_string(),
            git_ref: "feat/01-foo".to_string(),
            answered: Timestamp::from_epoch_seconds(0),
            rows: rows_of(states),
            selected: 0,
            compact: None,
            message: String::new(),
        }
    }

    /// One line as text, which is what its arithmetic shows up in.
    fn text(line: &Line<'static>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    #[test]
    fn the_status_age_stands_whatever_else_the_first_line_gives_up() {
        // Given more tasks needing a human than the panel has cells for
        // their names — ten failures are 137 cells of them, and the panel
        // inside a 120-column terminal is 118.
        let failing: Vec<(String, &str)> = (1..=10)
            .map(|task| (format!("T{task}"), "failed (exit 2)"))
            .collect();
        let board = board_of(
            &failing
                .iter()
                .map(|(id, state)| (id.as_str(), *state))
                .collect::<Vec<_>>(),
        );

        let line = text(&super::first(
            &board,
            THEME,
            Timestamp::from_epoch_seconds(60),
            118,
        ));

        // Then the age is still there, and the line is still the panel's.
        assert!(line.ends_with("status 60s ago"), "{line:?}");
        assert_eq!(crate::layout::wide(&line), 118, "{line:?}");
        // And what it gave up is the least of what needs a human: the names
        // are in the state table's order, most urgent first, so the end of
        // that list is the end that goes — whole, never half a name.
        assert!(line.contains("✗ T1 exit 2 · ✗ T2 exit 2"), "{line:?}");
        assert!(!line.contains("T8"), "{line:?}");
        // A board whose line fits keeps every one of them.
        let two = text(&super::first(
            &board_of(&[("T1", "failed (exit 2)"), ("T2", "died")]),
            THEME,
            Timestamp::from_epoch_seconds(60),
            118,
        ));
        assert!(
            two.contains("✗ T1 exit 2 · ⊘ T2 died   status 60s ago"),
            "{two:?}"
        );
    }

    #[test]
    fn a_first_line_that_does_not_fit_gives_up_whole_readings_from_one_end() {
        // The counts as the line draws them, and what a window with room for
        // some of them keeps.
        let tallied = || super::tally(&rows_of(&[("T1", "running"), ("T2", "done")]), THEME, false);
        let gap = ratatui::text::Span::raw(super::GAP);
        let text = |room, end| {
            super::fitting(tallied(), room, &gap, end)
                .iter()
                .map(|span| span.content.to_string())
                .collect::<String>()
        };

        assert_eq!(
            text(u16::MAX, super::Given::First),
            "● 1 running   ✓ 1 done"
        );
        // A window a cell short of both gives up one whole, from the end it
        // was told: the counts from the left, so that what stands at the
        // line's right-hand end keeps its cells, and the names from the
        // right, so that the most urgent of them is the last to go.
        assert_eq!(text(21, super::Given::First), "✓ 1 done");
        assert_eq!(text(8, super::Given::First), "✓ 1 done");
        assert_eq!(text(21, super::Given::Last), "● 1 running");
        assert_eq!(text(7, super::Given::First), "");
        assert_eq!(text(0, super::Given::Last), "");
        assert!(super::fitting(Vec::new(), 40, &gap, super::Given::First).is_empty());
    }

    #[test]
    fn the_panel_winds_to_keep_the_watched_task_in_view() {
        // Below the window: the task's last line is the panel's last row.
        assert_eq!(scrolled(11, 13, 7), 6);
        // Whole inside it: nothing moves, and the rows are drawn from the
        // first — a panel that scrolled while its rows fitted would move
        // the board under somebody reading it.
        assert_eq!(scrolled(0, 2, 7), 0);
        assert_eq!(scrolled(5, 7, 7), 0, "the last row that fits scrolled");
        assert_eq!(scrolled(6, 8, 7), 1);
        // Above it: the task's first line is the panel's first row.
        assert_eq!(scrolled(2, 4, 1), 2);
        // And a panel with room for nothing is laid out rather than
        // panicking.
        assert_eq!(scrolled(0, 0, 0), 0);
    }

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig {
            failure_persistence: Some(Box::new(
                proptest::test_runner::FileFailurePersistence::WithSource("proptest-regressions"),
            )),
            ..proptest::prelude::ProptestConfig::default()
        })]

        /// The one thing the offset exists for: wherever the watched task's
        /// lines are among the rows, and however short the panel, they are
        /// the lines that are drawn.
        #[test]
        fn a_task_no_taller_than_the_panel_is_always_drawn_whole(
            first in 0_usize..200,
            span in 1_usize..3,
            room in 1_usize..40,
        ) {
            let last = first + span;
            let from = scrolled(first, last, room.max(span));
            proptest::prop_assert!(from <= first, "the panel scrolled past the task's first line");
            proptest::prop_assert!(
                last <= from + room.max(span),
                "the task's last line is below the panel's last row",
            );
            // And it never winds further than it has to: rows that fit are
            // drawn from the first.
            if last <= room.max(span) {
                proptest::prop_assert_eq!(from, 0);
            }
        }
    }
}
