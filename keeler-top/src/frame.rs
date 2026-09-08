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
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};

use crate::board::{Board, PAUSED, Row};
use crate::clock::Timestamp;
use crate::layout::{Columns, Field, cut, right, wide, widest_state};
use crate::run::{DASH, RunView};
use crate::theme::{BORDER, CYAN, DIM, GREEN, ORANGE, RED, TEXT, Theme, YELLOW};

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

/// The fewest lines the detail panel is worth drawing in: its two borders,
/// its fact block, and a few of the run's own words under that.
const DETAIL_MIN: u16 = 6;

/// The lines the wave panel takes: its two, and the borders around them.
const WAVE_ROWS: u16 = 4;

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

/// `text` cut to `width` columns, the last of them an ellipsis saying that
/// something was cut. The detail pane is where the whole of it is.
///
/// The mark is the literal `…` and never the theme's: this is `--once`'s
/// cut, and `--once` is a plain-text surface a script parses. The frame's
/// own rows cut through [`crate::layout::cut`], which is given whichever
/// mark the terminal can draw.
#[must_use]
pub fn truncate(text: &str, width: u16) -> String {
    cut(text, width, "…")
}

/// How far in the second line hangs under the row above it, before the
/// connector: enough to clear the marker and the id, so the connector sits
/// under the state's glyph.
const HANGS: usize = 4;

/// What a paused row has to say under it — the key that starts it again.
const RESUME: &str = "resume: R";

/// One task's row, as the one or two lines it is drawn on.
///
/// Two for a task that is live and has something to say about right now,
/// one for everything else: a closed or waiting task has no tool to name,
/// and a connector under it would be a line saying nothing is happening.
/// `compact` takes that second line away from every row, which is what a
/// panel with more tasks than lines does.
#[must_use]
pub fn lines(
    row: &Row,
    cols: &Columns,
    theme: Theme,
    now: Timestamp,
    selected: bool,
    compact: bool,
) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from(cells_of(row, cols, theme, selected, compact))];
    if !compact {
        lines.extend(under(row, cols, theme, now));
    }
    lines
}

/// The row's first line: every column the layout has, in its place.
fn cells_of(
    row: &Row,
    cols: &Columns,
    theme: Theme,
    selected: bool,
    compact: bool,
) -> Vec<Span<'static>> {
    let look = theme.look(&row.state, cols.finished());
    let mut spans = Vec::new();
    let last = cols.fields().len().saturating_sub(1);
    for (index, (field, place)) in cols.fields().iter().enumerate() {
        let pieces = pieces(
            row,
            *field,
            place.width,
            theme,
            look,
            cols.finished(),
            compact,
            selected,
        );
        spans.extend(column(&pieces, place.width, theme.ellipsis()));
        // The marker carries its own trailing space, and the last column
        // has nothing after it to be separated from.
        if index < last && !matches!(field, Field::Mark) {
            spans.push(Span::raw(" "));
        }
    }
    spans
}

/// What one column of one row holds, in the pieces it is drawn in — each
/// with the style that piece is drawn in, because a column can carry two
/// readings at once: a bar and its track, a hash and the dirt beside it.
#[expect(
    clippy::too_many_arguments,
    reason = "one row's cell is a function of the row, the column, the theme and the three answers the view has already decided — a struct of them would be built per cell and read once"
)]
fn pieces(
    row: &Row,
    field: Field,
    width: u16,
    theme: Theme,
    look: crate::theme::Look,
    finished: bool,
    compact: bool,
    selected: bool,
) -> Vec<(String, Style)> {
    match field {
        // In the row's own colour: the marker says which row is selected,
        // and a marker of one colour on every row would be one more thing
        // to read before the row itself.
        Field::Mark => vec![(marker(theme, selected), look.style)],
        Field::Task => vec![(row.id.clone(), theme.style(TEXT))],
        Field::State => vec![(
            format!("{} {}", look.glyph, theme.state_text(&row.state)),
            look.style,
        )],
        Field::Stage => plain(row.run.as_ref().map(|run| run.stage.to_string()), theme),
        Field::Model => plain(said(row, RunView::model_column), theme),
        Field::Context => context(row, theme),
        // Right-aligned, because it is read beside the number above it.
        Field::Tokens => vec![(
            right(
                &said(row, RunView::tokens_column).unwrap_or_default(),
                width,
                theme.ellipsis(),
            ),
            theme.style(TEXT),
        )],
        Field::Commit => commit(row, theme),
        Field::Title => title(row, theme, finished, compact),
    }
}

/// What marks the selected row, and the nothing every other row gets.
fn marker(theme: Theme, selected: bool) -> String {
    if selected {
        theme.marker().to_string()
    } else {
        String::new()
    }
}

/// One of the run's columns, or nothing at all where `--once` writes a dash.
///
/// The two surfaces ask the same [`RunView`] and part company on this one
/// answer: `--once` fills an empty cell so that a row of a table is never
/// ambiguous, and the frame leaves it blank so that a board of done rows is
/// not a wall of dashes.
fn said(row: &Row, column: fn(&RunView) -> String) -> Option<String> {
    row.run
        .as_ref()
        .map(column)
        .filter(|said| said != DASH && !said.is_empty())
}

/// A column with one piece in it, in the ordinary text colour.
fn plain(text: Option<String>, theme: Theme) -> Vec<(String, Style)> {
    vec![(text.unwrap_or_default(), theme.style(TEXT))]
}

/// The context column: the bar in its two halves, then the share as a
/// number with the flag slot after it.
fn context(row: &Row, theme: Theme) -> Vec<(String, Style)> {
    let Some(percent) = row.run.as_ref().and_then(RunView::context_percent) else {
        return Vec::new();
    };
    let bar = theme.bar(percent);
    vec![
        (bar.filled, bar.fill),
        (bar.empty, bar.track),
        (
            format!(" {}", Theme::percentage(percent)),
            theme.style(TEXT),
        ),
    ]
}

/// The commit column: the branch's head, how far ahead it is, and what is
/// not committed — the dirty count only when there is one.
fn commit(row: &Row, theme: Theme) -> Vec<(String, Style)> {
    let Some(branch) = &row.branch else {
        return Vec::new();
    };
    let mut pieces = vec![
        (branch.head.clone(), theme.style(YELLOW)),
        (format!(" +{}", branch.ahead), theme.style(TEXT)),
    ];
    if branch.dirty > 0 {
        pieces.push((format!(" ~{}", branch.dirty), theme.style(RED)));
    }
    pieces
}

/// The last column: what the spec calls the task, what landed on a finished
/// board — or, on a row collapsed to one line, the tool that row's second
/// line would have named.
fn title(row: &Row, theme: Theme, finished: bool, compact: bool) -> Vec<(String, Style)> {
    if compact && row.live() {
        return plain(said(row, RunView::tool_column), theme);
    }
    let colour = if finished { TEXT } else { DIM };
    vec![(row.title.clone().unwrap_or_default(), theme.style(colour))]
}

/// The line under a live row: what the run is doing now, and how long it
/// has been doing it.
///
/// Nothing at all for a row with nothing to put there — a closed task, or a
/// running one whose stream has not reached a tool call yet. A connector
/// with an empty line after it would be the board reporting on itself.
fn under(row: &Row, cols: &Columns, theme: Theme, now: Timestamp) -> Option<Line<'static>> {
    // The elapsed belongs to the running row alone. A paused run's last
    // tool call was never answered — the session was killed in the middle
    // of it — so the clock on that call would go on counting beside
    // `resume: R`, timing a wait that nobody is in.
    let (pieces, elapsed) = if row.state == PAUSED {
        (vec![(RESUME.to_string(), theme.style(TEXT))], String::new())
    } else if row.running() {
        (tool(row, theme)?, row.elapsed_column(now))
    } else {
        return None;
    };
    let hangs = format!("{}{} ", " ".repeat(HANGS), theme.connector());
    // What is left once the connector and the elapsed have their cells: the
    // elapsed ends at the panel's inner right edge, so a command long
    // enough to reach it is cut rather than drawn over it.
    let tail = if elapsed.is_empty() {
        0
    } else {
        wide(&elapsed).saturating_add(1)
    };
    let width = cols
        .width()
        .saturating_sub(wide(&hangs))
        .saturating_sub(tail);
    let mut spans = vec![Span::styled(hangs, theme.style(DIM))];
    spans.extend(column(&pieces, width, theme.ellipsis()));
    if !elapsed.is_empty() {
        spans.push(Span::styled(format!(" {elapsed}"), theme.style(DIM)));
    }
    Some(Line::from(spans))
}

/// The tool a running row names under it: what was called, and what it was
/// called on.
fn tool(row: &Row, theme: Theme) -> Option<Vec<(String, Style)>> {
    let call = row.run.as_ref()?.last_tool.as_ref()?;
    let mut pieces = vec![(call.name.clone(), theme.style(CYAN))];
    if !call.detail.is_empty() {
        pieces.push((format!(": {}", call.detail), theme.style(TEXT)));
    }
    Some(pieces)
}

/// One column's pieces, laid into `width` cells: each in its own style, cut
/// where the column ends and padded where its pieces do not fill it.
///
/// The cut runs across the pieces rather than inside one of them: a hash
/// and the counts beside it are one column, and a column that cut each
/// piece to its own share would show a short hash rather than a cut one.
fn column(pieces: &[(String, Style)], width: u16, ellipsis: &str) -> Vec<Span<'static>> {
    let mut spans = Vec::with_capacity(pieces.len() + 1);
    let mut left = width;
    for (text, style) in pieces {
        if left == 0 {
            break;
        }
        let piece = cut(text, left, ellipsis);
        left = left.saturating_sub(wide(&piece));
        spans.push(Span::styled(piece, *style));
    }
    if left > 0 {
        spans.push(Span::raw(" ".repeat(usize::from(left))));
    }
    spans
}

/// Where the board's four parts go.
///
/// Three of them are panels — a box with its title on the top border — and
/// the fourth is the one line that is not: a message drawn inside a box
/// would be a sentence about the board sitting among the things the board
/// is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Panes {
    /// The panel about the wave: the counts, what needs a human, the
    /// outcome strip and the keys.
    pub wave: Rect,
    /// The panel the rows are drawn in, their column heading included.
    pub tasks: Rect,
    /// The selected task in full — absent on a terminal with no room for
    /// it. It is the first thing to go: a row that has lost its columns is
    /// a board that misleads, and a board without the panel is one that
    /// shows less.
    pub detail: Option<Rect>,
    /// The line under everything: what the last keypress did.
    pub footer: Rect,
}

/// Where the parts of a frame go on a terminal of this size.
///
/// `lines` is how many lines the table wants, its heading included — not
/// how many tasks there are. A live task's row carries a second line under
/// it, so the two numbers stopped being the same one. What the tasks panel
/// asks for is that plus its own two borders.
#[must_use]
pub fn layout(area: Rect, lines: usize) -> Panes {
    let wanted = u16::try_from(lines)
        .unwrap_or(u16::MAX)
        .saturating_add(BORDERS);
    // What is left once the wave panel and the footer have their lines.
    let body = area
        .height
        .saturating_sub(WAVE_ROWS.saturating_add(FOOTER_ROWS));
    let roomy = body >= wanted.saturating_add(DETAIL_MIN);
    let [wave, tasks, detail, footer] = Layout::vertical([
        Constraint::Length(WAVE_ROWS),
        Constraint::Length(if roomy { wanted } else { body }),
        Constraint::Min(0),
        Constraint::Length(FOOTER_ROWS),
    ])
    .areas(area);
    Panes {
        wave,
        tasks,
        detail: roomy.then_some(detail),
        footer,
    }
}

/// The one line the footer takes.
const FOOTER_ROWS: u16 = 1;

/// The detail pane's lines for one row: which task and what it is doing,
/// the command whole rather than cut to a column, the run's last words
/// oldest first, and the commits its branch has made.
///
/// The state goes through the theme for the reason the row above it does:
/// the arrow a blocked task carries is inside its words, and a pane that
/// wrote `blocked ← T6` under a row reading `blocked <- T6` would be the
/// same board disagreeing with itself on one screen. The `—` is not the
/// theme's — it is spec 10's separator, and T6's fact block is what
/// replaces it.
#[must_use]
pub fn detail(row: &Row, theme: Theme) -> Vec<String> {
    let mut lines = vec![format!("{} — {}", row.id, theme.state_text(&row.state))];
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

/// The two cells a panel's borders take from its width.
///
/// Every offset in the spec's column table is measured from the panel's
/// inner left edge, so the rows are composed for the width inside the
/// borders — which makes a row the same row whether the box around it has
/// been drawn yet or not.
const BORDERS: u16 = 2;

/// Draws one frame.
///
/// The rows and the wave panel's lines are composed for one width and drawn
/// inside panels of that same width, from one binding: a table laid out for
/// one number and drawn in a rectangle of another would line up by luck for
/// as long as the difference happened to be slack.
pub fn render(frame: &mut ratatui::Frame, board: &Board, theme: Theme, now: Timestamp) {
    let area = frame.area();
    let width = area.width.saturating_sub(BORDERS);
    let rows = table(board, theme, now, width);
    let panes = layout(area, rows.len());
    frame.render_widget(
        Paragraph::new(crate::panels::wave(board, theme, now, width))
            .block(panel(theme, crate::panels::title(board, theme))),
        panes.wave,
    );
    frame.render_widget(
        Paragraph::new(rows).block(panel(theme, named("tasks", theme.style(ORANGE)))),
        panes.tasks,
    );
    // Both, or neither. A spec whose tasks are still to be written has no
    // task to show in full, and a panel drawn about one would be an
    // untitled empty box — the board drawing its own furniture.
    if let (Some(pane), Some(row)) = (panes.detail, board.selected_row()) {
        frame.render_widget(
            Paragraph::new(detail(row, theme).join("\n"))
                .block(panel(theme, named(&row.id, theme.style(GREEN)))),
            pane,
        );
    }
    // Yellow, whatever the sentence: every line the board writes here is
    // something that refused — a lever, or one of the two reads — relayed
    // in the words whoever has to act on it needs.
    frame.render_widget(
        Paragraph::new(Line::styled(board.message.clone(), theme.style(YELLOW))),
        panes.footer,
    );
}

/// The box one part of the board is drawn in: neutral borders, and its
/// title on the top one in the panel's own colour.
///
/// A space either side of the title, so that the box does not run into the
/// word it is naming.
fn panel(theme: Theme, title: Vec<Span<'static>>) -> Block<'static> {
    let mut spans = vec![Span::raw(" ")];
    spans.extend(title);
    spans.push(Span::raw(" "));
    Block::bordered()
        .border_set(theme.border())
        .border_style(theme.style(BORDER))
        .title_top(Line::from(spans))
}

/// A title that is one word: the panel's name and nothing after it.
fn named(name: &str, style: Style) -> Vec<Span<'static>> {
    vec![Span::styled(name.to_string(), style)]
}

/// The tasks table: the heading, then every task's lines, in the order the
/// board puts its rows in.
#[must_use]
pub fn table(board: &Board, theme: Theme, now: Timestamp, width: u16) -> Vec<Line<'static>> {
    let cols = columns_of(board, theme, width);
    let mut drawn = vec![Line::styled(
        cols.header(theme.ellipsis()),
        theme.style(DIM),
    )];
    drawn.extend(rows(board, &cols, theme, now));
    drawn
}

/// Every task's lines, in the order the board draws them, with the selected
/// task's marked.
///
/// The selection is a report index and the rows are in the board's order,
/// so the two are compared here rather than counted: what `j` moved is a
/// task, and where it ends up on the screen is this order's answer.
fn rows(board: &Board, cols: &Columns, theme: Theme, now: Timestamp) -> Vec<Line<'static>> {
    crate::board::ordered(&board.rows)
        .into_iter()
        .flat_map(|(index, row)| lines(row, cols, theme, now, index == board.selected, false))
        .collect()
}

/// The columns this board's rows are drawn through: the finished view's
/// when every task has landed, and otherwise the live ones, as wide in the
/// state as the widest state on the board.
fn columns_of(board: &Board, theme: Theme, width: u16) -> Columns {
    if board.finished() {
        return Columns::landed(width);
    }
    // Measured through the theme, because the words it measures are the ones
    // that will be drawn: `blocked <- T1` is a cell wider than `blocked ← T1`
    // on the terminal that gets it, and a column sized from the other set
    // would cut the last thing a blocked task is waiting on.
    let states: Vec<String> = board
        .rows
        .iter()
        .map(|row| theme.state_text(&row.state))
        .collect();
    Columns::live(width, widest_state(states.iter().map(String::as_str)))
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
    fn the_detail_panel_is_the_first_thing_a_small_terminal_does_without() {
        // A table of five lines is a panel of seven; the wave panel is four
        // and the footer one, so the detail panel's own six need eighteen.
        assert_eq!(layout(Rect::new(0, 0, 100, 17), 5).detail, None);
        assert!(layout(Rect::new(0, 0, 100, 18), 5).detail.is_some());
        // And the rows never give way to it — on the terminal that has no
        // room, the tasks panel gets everything between the wave and the
        // footer.
        let cramped = layout(Rect::new(0, 0, 100, 17), 5);
        assert_eq!(cramped.tasks.height, 12);
        assert_eq!(cramped.wave.height, 4);
        assert_eq!(cramped.footer.height, 1);
        // A roomy one gives the tasks panel what it asked for and no more:
        // the rows are drawn at the top of the board, not spread down it.
        let roomy = layout(Rect::new(0, 0, 100, 40), 5);
        assert_eq!(roomy.tasks.height, 7);
        assert_eq!(roomy.detail.map(|pane| pane.height), Some(28));
    }

    #[test]
    fn a_terminal_too_small_for_any_of_it_is_laid_out_rather_than_panicking() {
        // Nothing the board can do about a window this size, but ending is
        // not one of the things it may do about it.
        for height in 0..=6 {
            let panes = layout(Rect::new(0, 0, 20, height), 3);
            assert!(panes.tasks.height <= height);
            assert!(panes.wave.height <= height);
        }
        assert_eq!(layout(Rect::new(0, 0, 0, 0), 0).detail, None);
    }

    #[test]
    fn the_plain_frame_is_the_drawn_one_composed_at_a_width_of_its_own() {
        let status = parse("graph: s.md on HEAD\nT1     done\n").expect("a report");
        let board = Board::assemble(
            &status,
            &crate::graph::Graph::default(),
            &mut Runs::default(),
            Timestamp::default(),
        );

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

    // ── 11-T2

    use crate::layout::Columns;
    use crate::run::RunView;
    use crate::theme::Theme;
    use ratatui::style::Style;

    /// The theme the rows below are composed through: colours on, glyphs
    /// drawable. Said outright rather than read from the process, which is
    /// the whole reason the theme is a value.
    const THEME: Theme = Theme::new(true, false);

    /// What a line reads as text, which is what a column's arithmetic shows
    /// up in.
    fn text(line: &ratatui::text::Line<'_>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    /// The style one cell of a line is drawn in.
    fn style_at(line: &ratatui::text::Line<'_>, column: usize) -> Style {
        let mut seen = 0;
        line.spans
            .iter()
            .find_map(|span| {
                seen += usize::from(super::wide(&span.content));
                (seen > column).then_some(span.style)
            })
            .expect("the cell asked about is inside the line")
    }

    #[test]
    fn a_column_is_padded_when_it_is_short_and_cut_when_it_is_long() {
        let plain = |text: &str| (text.to_string(), Style::default());

        // Padded to its place, so the column after it begins where the
        // table says whatever this one holds.
        assert_eq!(
            super::column(&[plain("qa")], 7, "…")
                .iter()
                .map(|span| span.content.to_string())
                .collect::<Vec<_>>(),
            ["qa", "     "],
        );
        // And cut where the column ends, with the mark the theme owns —
        // across as many pieces as it takes, so a piece that starts past
        // the edge is not drawn at all.
        let cut = super::column(&[plain("b33e05f"), plain(" +4 ~2")], 9, "…");
        assert_eq!(
            cut.iter()
                .map(|span| span.content.to_string())
                .collect::<Vec<_>>(),
            ["b33e05f", " …"],
        );
        assert_eq!(
            super::column(&[plain("b33e05f"), plain(" +4")], 7, "...")
                .iter()
                .map(|span| span.content.to_string())
                .collect::<Vec<_>>(),
            ["b33e05f"],
            "a column that ends exactly where a piece does has nothing to cut",
        );
    }

    /// One row's first line, drawn through the live columns for a board
    /// this wide.
    fn drawn(row: &Row, width: u16) -> String {
        let cols = Columns::live(width, crate::layout::widest_state([row.state.as_str()]));
        text(&super::lines(row, &cols, THEME, Timestamp::default(), false, false)[0])
    }

    #[test]
    fn a_row_with_no_run_shows_empty_live_columns() {
        // Given T5 is ready
        let row = Row {
            id: "T5".to_string(),
            state: "ready".to_string(),
            log: None,
            run: None,
            branch: None,
            title: None,
        };

        // When the board renders
        let line = drawn(&row, 118);

        // Then its STAGE, MODEL, CONTEXT, TOKENS and COMMIT columns are blank
        assert_eq!(line.trim_end(), "  T5   ◇ ready");
        assert_eq!(
            super::wide(&line),
            118,
            "the row is not the width of the panel it is drawn in",
        );
        // And nothing was written where a run's columns would have been —
        // the dash is `--once`'s answer, and a board of done rows padded
        // with it is what this spec was written against.
        assert!(!line.contains('—'));
    }

    #[test]
    fn a_state_the_board_does_not_recognise_still_gets_a_row() {
        // Given keeler-status printed a word the theme has no entry for
        let row = Row {
            id: "T1".to_string(),
            state: "sulking".to_string(),
            log: None,
            run: None,
            branch: None,
            title: None,
        };

        // When the board renders
        let cols = Columns::live(118, 17);
        let line = &super::lines(&row, &cols, THEME, Timestamp::default(), false, false)[0];

        // Then the row shows "?" and the word, in the text colour
        assert!(text(line).starts_with("  T1   ? sulking"));
        assert_eq!(style_at(line, 7), Style::new().fg(crate::theme::TEXT));
    }

    #[test]
    fn a_paused_tasks_second_line_says_how_to_resume() {
        // Given T8 is paused — with the run its report still names, whose
        // last tool call was never answered because the session was killed
        // in the middle of it. The clock on that call is not a clock on the
        // pause: the run is not waiting on the tool, it is not running at
        // all, and a timer ticking beside "resume: R" would say it is.
        let row = Row {
            id: "T8".to_string(),
            state: "paused".to_string(),
            log: None,
            run: Some(RunView {
                last_tool: Some(crate::run::ToolCall {
                    id: "toolu_1".to_string(),
                    name: "Bash".to_string(),
                    detail: "just dev".to_string(),
                    at: Some(Timestamp::from_epoch_seconds(1_000)),
                }),
                ..RunView::default()
            }),
            branch: None,
            title: None,
        };

        // When the board renders
        let cols = Columns::live(60, 17);
        let lines = super::lines(
            &row,
            &cols,
            THEME,
            Timestamp::from_epoch_seconds(1_134),
            false,
            false,
        );

        // Then the line under T8's row reads "    └─ resume: R"
        assert_eq!(lines.len(), 2);
        assert_eq!(text(&lines[1]).trim_end(), "    └─ resume: R");
        assert_eq!(
            super::wide(&text(&lines[1])),
            60,
            "the line is not the width of the panel",
        );
    }

    /// A row with the run a scenario describes, and the state it is in.
    fn row_of(state: &str, run: Option<RunView>, title: Option<&str>) -> Row {
        Row {
            id: "T3".to_string(),
            state: state.to_string(),
            log: None,
            run,
            branch: None,
            title: title.map(str::to_string),
        }
    }

    /// A run whose last call is one the board can name.
    fn calling() -> RunView {
        RunView {
            last_tool: Some(crate::run::ToolCall {
                id: "toolu_1".to_string(),
                name: "Bash".to_string(),
                detail: "just dev".to_string(),
                at: None,
            }),
            ..RunView::default()
        }
    }

    #[test]
    fn only_a_live_rows_tool_takes_the_place_of_its_title() {
        // Collapsing is what a row gives up its second line to; a row that
        // never had one has nothing to move into the title's column, and a
        // done task showing the last tool of a run that ended would be the
        // board answering a question nobody asked of it.
        let compacted = |row: &Row| {
            let cols = Columns::live(118, 17);
            text(&super::lines(row, &cols, THEME, Timestamp::default(), false, true)[0])
                .trim_end()
                .to_string()
        };

        assert!(
            compacted(&row_of("running", Some(calling()), Some("the title")))
                .ends_with("Bash: just dev"),
        );
        // Paused is live too: it is stopped, not closed, and what it was in
        // the middle of is what somebody deciding whether to resume reads.
        assert!(
            compacted(&row_of("paused", Some(calling()), Some("the title")))
                .ends_with("Bash: just dev"),
        );
        // And a closed row keeps its title, whatever its run last said.
        assert!(
            compacted(&row_of("done", Some(calling()), Some("the title"))).ends_with("the title"),
            "a closed row showed the tool of a run that is over",
        );
        assert!(
            compacted(&row_of("passed", Some(calling()), Some("the title"))).ends_with("the title")
        );
    }

    #[test]
    fn the_commit_column_names_the_dirt_only_when_there_is_some() {
        // A `~0` on every clean row would spend the column saying nothing,
        // and the count is read for one decision — whether a branch is safe
        // to remove — which a zero never enters into.
        let commit = |dirty| {
            let row = Row {
                branch: Some(crate::git::BranchFacts {
                    head: "b33e05f".to_string(),
                    ahead: 4,
                    dirty,
                    commits: Vec::new(),
                }),
                ..row_of("running", None, None)
            };
            super::column(&super::commit(&row, THEME), 13, "…")
                .iter()
                .map(|span| span.content.to_string())
                .collect::<String>()
                .trim_end()
                .to_string()
        };

        assert_eq!(commit(2), "b33e05f +4 ~2");
        assert_eq!(commit(1), "b33e05f +4 ~1");
        assert_eq!(commit(0), "b33e05f +4", "a clean worktree was called dirty");
    }

    #[test]
    fn a_row_collapsed_to_one_line_shows_its_tool_where_its_title_goes() {
        // The second line is what a compact row gives up, and the tool is
        // what it was carrying — so it moves into the title's column rather
        // than off the board. Which rows are collapsed, and when, is the
        // panel's question and not this one's.
        let row = Row {
            id: "T3".to_string(),
            state: "running".to_string(),
            log: None,
            run: Some(RunView {
                last_tool: Some(crate::run::ToolCall {
                    id: "toolu_1".to_string(),
                    name: "Bash".to_string(),
                    detail: "just dev".to_string(),
                    at: None,
                }),
                ..RunView::default()
            }),
            branch: None,
            title: Some("The fold reads the tool".to_string()),
        };
        let cols = Columns::live(118, 17);

        let compact = super::lines(&row, &cols, THEME, Timestamp::default(), false, true);

        assert_eq!(compact.len(), 1, "a collapsed row kept its second line");
        assert!(
            text(&compact[0]).trim_end().ends_with("Bash: just dev"),
            "{:?}",
            text(&compact[0]),
        );
        // And expanded it is the title again, with the tool underneath.
        let expanded = super::lines(&row, &cols, THEME, Timestamp::default(), false, false);
        assert_eq!(expanded.len(), 2);
        assert!(
            text(&expanded[0])
                .trim_end()
                .ends_with("The fold reads the tool"),
        );
    }

    #[test]
    fn a_live_row_that_has_nothing_to_say_underneath_says_nothing() {
        // A running task whose stream holds its init record and nothing
        // else — the connector alone would be a line about the board rather
        // than about the run.
        let row = row_of("running", Some(RunView::default()), None);

        let cols = Columns::live(118, 17);
        let lines = super::lines(&row, &cols, THEME, Timestamp::default(), false, false);

        assert_eq!(lines.len(), 1);
        // And the columns that run has nothing to say for are blank, not
        // dashed: the dash is `--once`'s answer, and a run that has started
        // is exactly where the two surfaces part company.
        assert_eq!(
            text(&lines[0]).trim_end(),
            "  T3   ● running         reading"
        );
    }

    #[test]
    fn the_pane_shows_a_task_that_has_neither_a_run_nor_a_branch() {
        let row = Row {
            id: "T1".to_string(),
            state: "not spawned".to_string(),
            log: None,
            run: None,
            branch: None,
            title: None,
        };

        assert_eq!(super::detail(&row, THEME), ["T1 — not spawned"]);
    }

    // ── 11-T3

    #[test]
    fn the_pane_says_a_blocked_task_in_the_glyphs_the_row_above_it_uses() {
        // The arrow reaches the pane inside the state's own words, as it
        // reaches the row: a pane reading `blocked ← T6` under a row reading
        // `blocked <- T6` would be one board disagreeing with itself on one
        // screen — and mojibake on the terminal the ASCII set exists for.
        let row = row_of("blocked ← T6", None, None);

        assert_eq!(
            super::detail(&row, Theme::new(true, true)),
            ["T3 — blocked <- T6"],
        );
        assert_eq!(super::detail(&row, THEME), ["T3 — blocked ← T6"]);
    }
}
