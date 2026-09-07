//! The frame: the header, the table, the detail pane — and the same frame
//! as plain text, which is what `--once` prints.
//!
//! Every decision about what a frame looks like is a value here, not a
//! drawing call: the columns' widths, the cut a command takes when it is
//! too long for one, where the panes go and which of them a small terminal
//! does without. [`render`] is what is left once those are decided, which
//! is little enough to read in one screen — and the plain-text frame is the
//! same values joined with spaces, so the two cannot drift apart.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Stylize as _;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;

use crate::board::{Board, Row};
use crate::clock::Timestamp;

/// The table's columns, in the order it shows them.
const HEADINGS: [&str; COLUMNS] = [
    "TASK", "STATE", "STAGE", "TOOL", "ELAPSED", "CTX", "TOKENS", "MODEL", "COMMIT",
];

/// How many columns the table has.
pub const COLUMNS: usize = 9;

/// The narrowest each column may be: the widest shape its values have. The
/// stage is `mutants`, the elapsed `1:02:05`, the context `100%!`, the
/// tokens `999.9k`, the model `opus5[1m]` — all seven characters or nine,
/// and none of them is ever wider without something else being wrong.
const NARROWEST: [u16; COLUMNS] = [4, 5, 7, 10, 7, 5, 6, 9, 10];

/// The one column whose content has no shape of its own: a command is as
/// long as it is, so it takes what the others leave rather than setting the
/// table's width.
const TOOL: usize = 3;

/// The columns a table keeps whatever the terminal: which task, what state
/// it is in, which stage it is on, and what it is running. They are the
/// four the narrow-terminal scenario names, and they are the ones a watcher
/// came for — everything after the tool is a number that can wait for a
/// wider window.
const KEPT: usize = 4;

/// One space between each pair of columns.
const GAP: u16 = 1;

/// The width `--once` composes its table at.
///
/// Fixed rather than the terminal's, because there is no terminal: a table
/// whose columns moved with `$COLUMNS` would make every script that reads
/// it depend on the window it happened to run in.
const ONCE_WIDTH: u16 = 120;

/// The fewest lines the detail pane is worth drawing in: its heading, the
/// command, and a few of the run's own words under them.
const DETAIL_MIN: u16 = 6;

/// One row's cells, in the table's order.
#[must_use]
pub fn cells(row: &Row, now: Timestamp) -> [String; COLUMNS] {
    [
        row.id.clone(),
        row.state.clone(),
        row.stage_column(),
        row.tool_column(),
        row.elapsed_column(now),
        row.context_column(),
        row.tokens_column(),
        row.model_column(),
        row.commit_column(),
    ]
}

/// The table as text, laid out for a terminal of a given width.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grid {
    /// One row of cells per task, in the board's order.
    pub rows: Vec<[String; COLUMNS]>,
    /// How wide each column is on this terminal.
    pub widths: [u16; COLUMNS],
    /// How many of the columns this terminal has room for, counted from
    /// the left.
    pub shown: usize,
}

impl Grid {
    /// The board's rows as cells, with the columns sized for `width`.
    #[must_use]
    pub fn new(board: &Board, now: Timestamp, width: u16) -> Self {
        let rows: Vec<[String; COLUMNS]> = board.rows.iter().map(|row| cells(row, now)).collect();
        let (shown, widths) = columns(&rows, width);
        Self {
            rows,
            widths,
            shown,
        }
    }

    /// The table as lines: the headings, then one line per row.
    #[must_use]
    pub fn lines(&self) -> Vec<String> {
        let headings = HEADINGS.map(str::to_string);
        std::iter::once(&headings)
            .chain(self.rows.iter())
            .map(|cells| line(cells, &self.widths, self.shown))
            .collect()
    }
}

/// How many columns a terminal `width` across has room for, and how wide
/// each of them is.
///
/// Every column but the tool's is as wide as the widest thing in it, and
/// never narrower than the shape it exists to hold. The state is the reason
/// this is measured rather than fixed: `incomplete (no review record, box
/// not ticked)` names the two things a task still lacks, and a column that
/// cut it would drop the very words that say what to do.
///
/// Which is also why the table can end up wider than the window, and what
/// this does about it. The columns go from the right, whole, until the tool
/// has its own minimum — never clipped where they stand. The difference is
/// the whole point: a column that is gone is visibly gone, while `f5064` in
/// a clipped COMMIT reads as a commit that begins `f5064`. That is the
/// failure the detail pane is dropped first to avoid, and dropping the pane
/// alone did not avoid it.
#[must_use]
pub fn columns(rows: &[[String; COLUMNS]], width: u16) -> (usize, [u16; COLUMNS]) {
    let mut widths = NARROWEST;
    for (column, heading) in HEADINGS.iter().enumerate() {
        widths[column] = widths[column].max(wide(heading));
        for row in rows {
            widths[column] = widths[column].max(wide(&row[column]));
        }
    }
    let shown = (KEPT..=COLUMNS)
        .rev()
        .find(|shown| width.saturating_sub(fixed(&widths, *shown)) >= NARROWEST[TOOL])
        // Four columns is the floor, and on a window too narrow even for
        // those the table overflows rather than disappearing: a board
        // nobody can read is still better than no board at all.
        .unwrap_or(KEPT);
    widths[TOOL] = width
        .saturating_sub(fixed(&widths, shown))
        .max(NARROWEST[TOOL]);
    (shown, widths)
}

/// What everything but the tool takes up when `shown` columns are drawn,
/// the spaces between them included.
///
/// A gap is added for each column that is not the tool's, which is exactly
/// the `shown - 1` that sit between them: the tool is always among the ones
/// drawn, since it comes before the last column any terminal keeps.
fn fixed(widths: &[u16; COLUMNS], shown: usize) -> u16 {
    widths[..shown]
        .iter()
        .enumerate()
        .filter(|(column, _)| *column != TOOL)
        .fold(0_u16, |total, (_, width)| {
            total.saturating_add(*width).saturating_add(GAP)
        })
}

/// One line of the table: each cell padded or cut to its column, a space
/// between them, and nothing after the last.
fn line(cells: &[String; COLUMNS], widths: &[u16; COLUMNS], shown: usize) -> String {
    let mut line = String::new();
    for (column, (cell, width)) in cells.iter().zip(widths).take(shown).enumerate() {
        if column > 0 {
            line.push(' ');
        }
        let cut = truncate(cell, *width);
        let padding = usize::from(*width).saturating_sub(usize::from(wide(&cut)));
        line.push_str(&cut);
        line.push_str(&" ".repeat(padding));
    }
    line.trim_end().to_string()
}

/// How many of the terminal's columns a string takes up.
///
/// Not its characters: the table is composed as text and padded by hand, so
/// a cell measured in characters and drawn in columns puts every column
/// after it out by one for each double-width character in it. The only cell
/// that can hold one is the command, which is whatever the agent typed.
fn wide(text: &str) -> u16 {
    u16::try_from(unicode_width::UnicodeWidthStr::width(text)).unwrap_or(u16::MAX)
}

/// `text` cut to `width` columns, the last of them an ellipsis saying that
/// something was cut. The detail pane is where the whole of it is.
///
/// Cut by column and not by character, for the reason [`wide`] gives — and
/// a character that would straddle the edge is left out rather than half
/// drawn, so the result can be a column short of its budget. The padding
/// that follows it in [`line`] closes that up.
#[must_use]
pub fn truncate(text: &str, width: u16) -> String {
    if wide(text) <= width {
        return text.to_string();
    }
    // A column with no room for the ellipsis has no room for a head to put
    // it after either.
    let Some(budget) = usize::from(width).checked_sub(1) else {
        return String::new();
    };
    let mut cut = String::new();
    let mut used = 0;
    for character in text.chars() {
        let next = used + unicode_width::UnicodeWidthChar::width(character).unwrap_or(0);
        if next > budget {
            break;
        }
        cut.push(character);
        used = next;
    }
    cut.push('…');
    cut
}

/// Where the board's four parts go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Panes {
    /// The one line naming the spec, the ref and the age of the answer.
    pub header: Rect,
    /// The headings and the rows.
    pub table: Rect,
    /// The selected task in full — absent on a terminal with no room for
    /// it. It is the first thing to go: a row that has lost its columns is
    /// a board that misleads, and a board without the pane is one that
    /// shows less.
    pub detail: Option<Rect>,
    /// The line under everything: what the last keypress did.
    pub status: Rect,
}

/// Where the parts of a frame go on a terminal of this size.
#[must_use]
pub fn layout(area: Rect, tasks: usize) -> Panes {
    let table = u16::try_from(tasks).unwrap_or(u16::MAX).saturating_add(1);
    // What is left once the header and the status line have their line.
    let body = area.height.saturating_sub(2);
    let roomy = body >= table.saturating_add(DETAIL_MIN);
    let [header, rows, detail, status] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(if roomy { table } else { body }),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(area);
    Panes {
        header,
        table: rows,
        detail: roomy.then_some(detail),
        status,
    }
}

/// The detail pane's lines for one row: which task and what it is doing,
/// the command whole rather than cut to a column, the run's last words
/// oldest first, and the commits its branch has made.
#[must_use]
pub fn detail(row: &Row) -> Vec<String> {
    let mut lines = vec![format!("{} — {}", row.id, row.state)];
    if let Some(run) = &row.run {
        lines.push(run.tool_column());
        lines.push(String::new());
        // A text block can hold several lines, and a pane that wrote the
        // newline as a symbol would show one long line of mojibake where
        // the run's own paragraph should be.
        lines.extend(
            run.texts
                .iter()
                .flat_map(|text| text.lines())
                .map(str::to_string),
        );
    }
    if let Some(branch) = &row.branch {
        lines.push(String::new());
        lines.extend(
            branch
                .commits
                .iter()
                .map(|commit| format!("{} {}", commit.hash, commit.subject)),
        );
    }
    lines
}

/// The same frame as plain text — the table alone, which is what a script
/// reading `--once` wants.
#[must_use]
pub fn once(board: &Board, now: Timestamp) -> String {
    let mut text = Grid::new(board, now, ONCE_WIDTH).lines().join("\n");
    text.push('\n');
    text
}

/// Draws one frame.
pub fn render(frame: &mut ratatui::Frame, board: &Board, now: Timestamp) {
    let area = frame.area();
    let panes = layout(area, board.rows.len());
    frame.render_widget(Paragraph::new(board.header(now)), panes.header);
    frame.render_widget(table(board, now, area.width), panes.table);
    if let Some(pane) = panes.detail {
        let lines = board.selected_row().map(detail).unwrap_or_default();
        frame.render_widget(Paragraph::new(lines.join("\n")), pane);
    }
    frame.render_widget(Paragraph::new(board.message.clone()), panes.status);
}

/// The table, with the selected row marked.
fn table(board: &Board, now: Timestamp, width: u16) -> Paragraph<'static> {
    // The headings are the first line, so the selected row is the one after
    // it.
    let selected = board.selected.saturating_add(1);
    let lines: Vec<Line> = Grid::new(board, now, width)
        .lines()
        .into_iter()
        .enumerate()
        .map(|(index, line)| {
            if index == selected {
                Line::from(line).reversed()
            } else {
                Line::from(line)
            }
        })
        .collect();
    Paragraph::new(lines)
}

#[cfg(test)]
mod tests {
    use super::{COLUMNS, Grid, HEADINGS, KEPT, NARROWEST, TOOL, columns, layout, truncate};
    use crate::board::{Board, Row, Runs};
    use crate::clock::Timestamp;
    use crate::status::parse;
    use ratatui::layout::Rect;

    fn cells(state: &str, tool: &str) -> [String; COLUMNS] {
        let mut cells = HEADINGS.map(|_| String::new());
        cells[0] = "T1".to_string();
        cells[1] = state.to_string();
        cells[TOOL] = tool.to_string();
        cells
    }

    /// The grid a terminal of this width composes, so a test can read the
    /// lines rather than the arithmetic behind them.
    fn grid(rows: Vec<[String; COLUMNS]>, width: u16) -> Grid {
        let (shown, widths) = columns(&rows, width);
        Grid {
            rows,
            widths,
            shown,
        }
    }

    #[test]
    fn a_column_is_as_wide_as_the_widest_thing_in_it_and_never_narrower_than_its_shape() {
        let (_, narrow) = columns(&[cells("running", "Bash: just dev")], 120);
        assert_eq!(narrow[1], 7, "the state column did not fit its value");
        assert_eq!(
            narrow[2], NARROWEST[2],
            "an empty column shrank below its shape"
        );

        let (_, wide) = columns(
            &[cells("incomplete (no review record, box not ticked)", "x")],
            120,
        );
        assert_eq!(
            wide[1], 45,
            "the state was cut, and with it the words that say what to do",
        );
    }

    #[test]
    fn a_heading_is_never_wider_than_the_column_that_carries_it() {
        // `ELAPSED` and `TOKENS` are both wider than anything they hold,
        // and a table whose headings ran into each other would read as one
        // column with a long name.
        let (_, widths) = columns(&[], 120);
        for (column, heading) in HEADINGS.iter().enumerate() {
            assert!(
                widths[column] >= super::wide(heading),
                "{heading} does not fit its own column",
            );
        }
    }

    #[test]
    fn the_tool_takes_what_is_left_and_never_less_than_its_own_minimum() {
        let rows = [cells("running", &"x".repeat(400))];
        let (_, roomy) = columns(&rows, 200);
        let (_, cramped) = columns(&rows, 40);

        assert!(
            roomy[TOOL] > cramped[TOOL],
            "the tool column did not follow the terminal's width",
        );
        // Its own minimum is a floor and not a target: the columns to the
        // right of it go first, so the tool keeps whatever they leave.
        assert!(cramped[TOOL] >= NARROWEST[TOOL]);
        assert_eq!(
            columns(&rows, 0).1[TOOL],
            NARROWEST[TOOL],
            "a window with nothing in it squeezed the tool column out of existence",
        );
        // And a command four hundred characters long must not be what sets
        // the table's width: every other column was measured against its
        // values, and this one against the room left over.
        assert!(roomy[TOOL] < 400);
    }

    #[test]
    fn a_table_wider_than_the_window_drops_columns_whole_rather_than_clipping_them() {
        // The state that fills a column: at a hundred columns the other
        // eight and their gaps come to more than the window, and a table
        // that only floored the tool column would have run off the right
        // edge with COMMIT half drawn.
        let rows = vec![cells(
            "incomplete (no review record, box not ticked)",
            "just dev",
        )];

        let grid = grid(rows, 100);

        assert_eq!(grid.shown, 8, "the table kept a column it had no room for");
        for line in grid.lines() {
            assert!(
                super::wide(&line) <= 100,
                "the line runs past the window: {line:?}",
            );
        }
        // And what it kept is what a watcher came for.
        assert!(grid.lines()[1].contains("incomplete (no review record, box not ticked)"));
        assert!(grid.lines()[1].contains("just dev"));
        assert!(!grid.lines()[0].contains("COMMIT"));
    }

    #[test]
    fn four_columns_is_the_floor_and_a_window_below_it_is_not_an_empty_board() {
        let rows = vec![cells("running", "Bash: just dev")];

        for width in [0, 1, 20] {
            let grid = grid(rows.clone(), width);
            assert_eq!(grid.shown, KEPT, "at {width} columns");
            assert!(
                grid.lines()[1].starts_with("T1 "),
                "the row lost the task it is about at {width} columns",
            );
        }
        // The columns come back one at a time as the window widens, and all
        // nine are there when there is room for them.
        assert_eq!(grid(rows.clone(), 40).shown, 5);
        assert_eq!(grid(rows, 200).shown, COLUMNS);
    }

    #[test]
    fn a_cell_is_padded_to_its_column_and_the_line_ends_where_the_last_one_does() {
        let lines = grid(vec![cells("running", "Bash: just dev")], 120).lines();

        assert!(lines[0].starts_with("TASK "), "{:?}", lines[0]);
        assert!(lines[1].starts_with("T1   running"), "{:?}", lines[1]);
        assert!(
            !lines[1].ends_with(' '),
            "the line carried its padding to the end: {:?}",
            lines[1],
        );
        assert_eq!(lines.len(), 2, "a heading and one row");
    }

    #[test]
    fn a_double_width_character_is_two_columns_and_the_padding_knows_it() {
        // The command is the one cell that holds whatever the agent typed,
        // and a cell measured in characters and drawn in columns puts every
        // column after it out by one for each of these. The two commands
        // below are the same width and different lengths, which is the
        // whole difference between the two ways of measuring.
        let ascii = commit_row("Bash: xxxx");
        let doubled = commit_row("Bash: 更新");

        let ascii_line = grid(vec![ascii], 120).lines().remove(1);
        let doubled_line = grid(vec![doubled], 120).lines().remove(1);

        assert_eq!(
            super::wide(&ascii_line),
            super::wide(&doubled_line),
            "the two rows do not end in the same column:\n{ascii_line}\n{doubled_line}",
        );
        assert_ne!(
            ascii_line.chars().count(),
            doubled_line.chars().count(),
            "the fixture does not tell the two measurements apart",
        );
    }

    /// A row whose last column is not empty, so the trailing space a line
    /// is trimmed of cannot hide a padding mistake behind it.
    fn commit_row(tool: &str) -> [String; COLUMNS] {
        let mut cells = cells("running", tool);
        cells[COLUMNS - 1] = "f5064b1 +3".to_string();
        cells
    }

    #[test]
    fn a_value_wider_than_its_column_is_cut_with_an_ellipsis() {
        assert_eq!(truncate("just dev", 8), "just dev");
        assert_eq!(truncate("just dev", 9), "just dev");
        assert_eq!(truncate("just dev", 7), "just d…");
        assert_eq!(truncate("just dev", 1), "…");
        // A column with no room for the ellipsis has no room for anything.
        assert_eq!(truncate("just dev", 0), "");
        assert_eq!(truncate("", 0), "");
        // Columns, not bytes: the dash and the ellipsis are three bytes
        // each, and a cut counted in bytes would slice one in half.
        assert_eq!(truncate("————", 3), "——…");
        // And columns, not characters: two of these fill four of them, so
        // three columns hold one and the ellipsis.
        assert_eq!(truncate("更新", 3), "更…");
        assert_eq!(truncate("更新", 4), "更新");
        // A character that would straddle the edge is left out rather than
        // half drawn — which leaves the answer a column short, and the
        // padding that follows it closes that up.
        assert_eq!(super::wide(&truncate("更新更", 4)), 3);
    }

    #[test]
    fn the_detail_pane_is_the_first_thing_a_small_terminal_does_without() {
        // Five tasks want six lines, and the header and status line take
        // one each: the pane needs six more on top of those eight.
        assert_eq!(layout(Rect::new(0, 0, 100, 12), 5).detail, None);
        assert!(layout(Rect::new(0, 0, 100, 14), 5).detail.is_some());
        // And the rows never give way to it — on the terminal that has no
        // room, the table gets everything between the two single lines.
        let cramped = layout(Rect::new(0, 0, 100, 12), 5);
        assert_eq!(cramped.table.height, 10);
        assert_eq!(cramped.header.height, 1);
        assert_eq!(cramped.status.height, 1);
    }

    #[test]
    fn a_terminal_too_small_for_any_of_it_is_laid_out_rather_than_panicking() {
        // Nothing the board can do about a window this size, but ending is
        // not one of the things it may do about it.
        for height in 0..=3 {
            let panes = layout(Rect::new(0, 0, 20, height), 3);
            assert!(panes.table.height <= height);
        }
        assert_eq!(layout(Rect::new(0, 0, 0, 0), 0).detail, None);
    }

    #[test]
    fn the_plain_frame_is_the_drawn_one_composed_at_a_width_of_its_own() {
        let status = parse("graph: s.md on HEAD\nT1     done\n").expect("a report");
        let board = Board::assemble(&status, &[], &mut Runs::default(), Timestamp::default());

        let text = super::once(&board, Timestamp::default());

        assert_eq!(
            text,
            format!(
                "{}\n",
                Grid::new(&board, Timestamp::default(), 120)
                    .lines()
                    .join("\n"),
            ),
        );
        assert!(text.ends_with('\n'), "the last row had no line ending");
    }

    #[test]
    fn the_pane_shows_a_task_that_has_neither_a_run_nor_a_branch() {
        let row = Row {
            id: "T1".to_string(),
            state: "not spawned".to_string(),
            run: None,
            branch: None,
        };

        assert_eq!(super::detail(&row), ["T1 — not spawned"]);
    }
}
