//! Where a row's columns are, and how wide a thing may be drawn in one.
//!
//! A value computed once per frame and handed to the renderer, so that the
//! arithmetic behind a board is read here rather than counted off a
//! screenshot. Every column is as wide as the shape it holds — the widths
//! are the spec's table — and each is followed by one space, except the
//! marker, which carries its own.
//!
//! **The state is the one column measured rather than fixed.** `incomplete
//! (no review record, box not ticked)` names the two things a task still
//! lacks, and those are the words that say what to do; a column that cut
//! them would drop exactly the reading the row exists to give. So the state
//! takes the widest state on the board when that is wider than the table's
//! seventeen, and every later column moves right by the difference.
//!
//! **A window too narrow for the row drops columns whole.** [`LADDER`] is
//! the order they go in, and [`Columns::live`] takes the first rung that
//! fits — so the bands are cumulative by construction: a rung has given up
//! everything the rungs above it did. Whole, and never clipped where they
//! stand: a column that is gone is visibly gone, while `f5064` in a clipped
//! COMMIT reads as a commit that begins `f5064`. [`bands`] is the same
//! question asked of the whole frame, which is where the height comes in.

/// The columns of a row, in the order they are drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    /// The selection marker, and the space it carries with it.
    Mark,
    /// The task id.
    Task,
    /// The state's glyph and its word, whatever the reason after it.
    State,
    /// How far the run has got.
    Stage,
    /// The bar, the percentage and the flag slot.
    Context,
    /// The branch's head, its distance and its dirt.
    Commit,
    /// What the spec calls the task — or, on a finished board, what landed.
    Title,
}

/// The marker and the space that goes with it.
const MARK: u16 = 2;

/// A task id: `T1` through `T999`.
const TASK: u16 = 4;

/// The state, when no state on the board is wider.
const STATE: u16 = 17;

/// The glyph before a state's word, and the space after the glyph.
const GLYPH: u16 = 2;

/// The stage: `mutants` is the longest of the seven.
const STAGE: u16 = 7;

/// Eight bar cells, a space, the percentage right-aligned in four, and the
/// flag slot.
///
/// Public because the bar is drawn against it: a context column narrower
/// than this is one the bar has been dropped from, and the piece that draws
/// it reads that from the column it was given rather than from a second flag
/// to keep in step.
pub const CONTEXT: u16 = 14;

/// The same column once the bar has gone: ` NN% `, the percentage and its
/// flag slot alone.
const PERCENTAGE: u16 = 5;

/// `<hash> +<ahead> ~<dirty>`.
const COMMIT: u16 = 13;

/// `✓ done`, which is the whole of a finished row's state column.
const LANDED_STATE: u16 = 6;

/// Two spaces after every column but the marker: one leaves a long
/// state running into its stage, and the eye reads the two as one phrase.
pub const GAP: u16 = 2;

/// The narrowest a title is worth drawing in: two words and the mark that
/// says the rest was cut. Below that the column says less than the cells it
/// takes from the row.
const TITLE_MIN: u16 = 12;

/// What a row gives up as the window narrows, in the order it gives it up.
///
/// The title first, because it is the one column that is about the task
/// rather than about the run: what a task is called is in the spec, and what
/// it is doing is only here. Then the bar, which is the percentage beside it
/// drawn a second time. Then the model and the tokens, which are the two a
/// watcher checks rather than watches — and last the share of the window and
/// the commit facts, in that order, since the facts are what says whether a
/// branch is safe to remove. The state, the stage and the id are what is
/// left, and a window too narrow even for those overflows rather than losing
/// them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Given {
    /// The title, and with it the whole of the row's right-hand end.
    Title,
    /// The bar, leaving the percentage it was drawn from.
    Bar,
    /// The percentage that was left when the bar went.
    Context,
    /// The branch's head, its distance and its dirt.
    Commit,
}

/// The rungs, widest first. Cumulative: rung `n` has given up everything the
/// rungs above it gave up.
const LADDER: [Given; 4] = [Given::Title, Given::Bar, Given::Context, Given::Commit];

/// One rung of that ladder: the columns before the title, and whether the
/// title is among them.
struct Band {
    fixed: Vec<(Field, u16)>,
    title: bool,
}

impl Band {
    /// The rung reached once `given` of the ladder's steps have been taken.
    fn new(state: u16, given: usize) -> Self {
        let gone = |what: Given| LADDER[..given.min(LADDER.len())].contains(&what);
        let mut fixed = vec![
            (Field::Mark, MARK),
            (Field::Task, TASK),
            (Field::State, state),
            (Field::Stage, STAGE),
        ];
        if !gone(Given::Context) {
            fixed.push((
                Field::Context,
                if gone(Given::Bar) {
                    PERCENTAGE
                } else {
                    CONTEXT
                },
            ));
        }
        if !gone(Given::Commit) {
            fixed.push((Field::Commit, COMMIT));
        }
        Self {
            fixed,
            title: !gone(Given::Title),
        }
    }

    /// Whether a panel this wide has room for the rung: the columns and
    /// their gaps, and a title worth the name when the rung still has one.
    fn fits(&self, width: u16) -> bool {
        let after = after(&self.fixed);
        if self.title {
            width >= after.saturating_add(TITLE_MIN)
        } else {
            width >= after.saturating_sub(GAP)
        }
    }
}

/// Where the title would start: after the last fixed column and the gap that
/// follows it.
fn after(fixed: &[(Field, u16)]) -> u16 {
    fixed.iter().fold(0, |start, (field, cells)| {
        // The marker carries its own trailing space; everything else is
        // followed by one.
        start.saturating_add(*cells).saturating_add(match field {
            Field::Mark => 0,
            _ => GAP,
        })
    })
}

/// Where one column is: the cell it begins at, and how many it has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Place {
    /// The offset from the panel's inner left edge.
    pub start: u16,
    /// How many cells the column's content has, the gap after it excluded.
    pub width: u16,
}

/// A row's columns, laid out for one panel width.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Columns {
    places: Vec<(Field, Place)>,
    width: u16,
    finished: bool,
}

impl Columns {
    /// The live view's columns: as much of what a running wave has to say as
    /// the panel has room for, with the state as wide as the widest one on
    /// the board.
    ///
    /// The widest rung of [`LADDER`] that fits — and the narrowest whether it
    /// fits or not, because a row of the id, the state and the stage that
    /// runs past the panel's edge still says which task is in what trouble.
    #[must_use]
    pub fn live(width: u16, widest_state: u16) -> Self {
        let state = widest_state.max(STATE);
        let band = (0..LADDER.len())
            .map(|given| Band::new(state, given))
            .find(|band| band.fits(width))
            .unwrap_or_else(|| Band::new(state, LADDER.len()));
        Self::laid_out(width, false, &band.fixed, band.title)
    }

    /// The finished view's: the id, the word, and what landed. The live
    /// columns are about a run, and every run here is over.
    #[must_use]
    pub fn landed(width: u16) -> Self {
        Self::laid_out(
            width,
            true,
            &[
                (Field::Mark, MARK),
                (Field::Task, TASK),
                (Field::State, LANDED_STATE),
            ],
            true,
        )
    }

    /// The columns before the title, placed left to right, and the title —
    /// when the row still has one — taking whatever is left.
    fn laid_out(width: u16, finished: bool, fixed: &[(Field, u16)], title: bool) -> Self {
        let mut places = Vec::with_capacity(fixed.len() + 1);
        let mut start = 0;
        for (field, cells) in fixed {
            places.push((
                *field,
                Place {
                    start,
                    width: *cells,
                },
            ));
            // The marker carries its own trailing space; everything else is
            // followed by one.
            start = start.saturating_add(*cells).saturating_add(match field {
                Field::Mark => 0,
                _ => GAP,
            });
        }
        if title {
            places.push((
                Field::Title,
                Place {
                    start,
                    width: width.saturating_sub(start),
                },
            ));
        }
        Self {
            places,
            width,
            finished,
        }
    }

    /// Where a column is, or nothing when this row has none.
    #[must_use]
    pub fn place(&self, field: Field) -> Option<Place> {
        self.places
            .iter()
            .find(|(candidate, _)| *candidate == field)
            .map(|(_, place)| *place)
    }

    /// Every column this row has, in the order they are drawn.
    #[must_use]
    pub fn fields(&self) -> &[(Field, Place)] {
        &self.places
    }

    /// How wide the row is: the panel's inner width.
    #[must_use]
    pub const fn width(&self) -> u16 {
        self.width
    }

    /// Whether these are a finished board's columns.
    #[must_use]
    pub const fn finished(&self) -> bool {
        self.finished
    }

    /// The line above the rows, naming each column that is drawn.
    ///
    /// Composed from the same places the rows are, so a column that moved
    /// took its heading with it — a heading table of its own would be a
    /// second layout to keep in step. And cut by the same mark the rows
    /// under it are cut by: a `…` over columns cutting with `...` would put
    /// the one glyph the ASCII theme exists to avoid at the top of the
    /// board.
    #[must_use]
    pub fn header(&self, ellipsis: &str) -> String {
        let mut header = String::new();
        for (field, place) in &self.places {
            let heading = self.heading(*field, place.width);
            let cell = left(&heading, place.width, ellipsis);
            header.push_str(&cell);
            if !matches!(field, Field::Mark) {
                for _ in 0..GAP {
                    header.push(' ');
                }
            }
        }
        header.trim_end().to_string()
    }

    /// What one column is called, at the width this row gives it.
    fn heading(&self, field: Field, width: u16) -> String {
        match field {
            Field::Mark => String::new(),
            Field::Task => "TASK".to_string(),
            Field::State => "STATE".to_string(),
            Field::Stage => "STAGE".to_string(),
            // The heading follows the column: a `CONTEXT` cut to `CON` over
            // five cells of percentage would name a bar that is not there.
            Field::Context if width < CONTEXT => "CTX".to_string(),
            Field::Context => "CONTEXT".to_string(),
            // The two counts the column carries beside the hash, in the
            // shape they are written: `+4 ~2`.
            Field::Commit => "COMMIT +~".to_string(),
            // What the column holds is the same string either way; what it
            // means is not. On a finished board it is what landed.
            Field::Title if self.finished => "LANDED".to_string(),
            Field::Title => "TITLE".to_string(),
        }
    }
}

/// The two cells a panel's borders take from its width.
const BORDERS: u16 = 2;

/// The narrowest terminal the detail panel is drawn on.
///
/// The pane is a task in full — its paths, its record, its commits — and
/// every one of those lines is a path or a sentence. Below this it is a box
/// of cut strings taking rows the board could be drawing tasks in.
const DETAIL_COLS: u16 = 100;

/// And the shortest. Fewer lines than this and the pane is a heading with
/// two facts under it, which is less than the row it is about already says.
const DETAIL_ROWS: u16 = 24;

/// The shortest terminal the wave panel keeps both its lines on.
///
/// Its first line is what the wave is doing and what needs a human; the
/// second is the strip and the keys, which are the same on every board and
/// in the README. On a window this short a line spent on either is a task
/// the tasks panel cannot draw.
const WAVE_ROWS: u16 = 16;

/// How many lines the wave panel has to itself.
const WAVE_LINES: u16 = 2;

/// What a terminal of a given size has room for.
///
/// One value per frame, and the only place the size of a terminal is turned
/// into a decision: the columns of a row, how many of the wave panel's lines
/// are drawn, and whether there is a detail panel at all. The parts that
/// draw those read the answer rather than the width, so no two of them can
/// disagree about where a band is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bands {
    /// The columns a row is drawn through, inside the panel's borders.
    pub columns: Columns,
    /// How many of the wave panel's two lines are drawn.
    pub wave: u16,
    /// Whether the detail panel is drawn at all. The first thing to go: a
    /// row that has lost its columns is a board that misleads, and a board
    /// without the panel is one that shows less.
    pub detail: bool,
}

/// What `width` by `height` cells have room for, with the state as wide as
/// the widest one on the board.
///
/// `width` and `height` are the terminal's, borders and all — the bands are
/// stated in the size of a window somebody has dragged, and the panels take
/// their own borders off it.
#[must_use]
pub fn bands(width: u16, height: u16, widest_state: u16, finished: bool) -> Bands {
    let inner = width.saturating_sub(BORDERS);
    Bands {
        columns: if finished {
            Columns::landed(inner)
        } else {
            Columns::live(inner, widest_state)
        },
        wave: if height < WAVE_ROWS { 1 } else { WAVE_LINES },
        detail: width >= DETAIL_COLS && height >= DETAIL_ROWS,
    }
}

/// How many cells a state's column needs: the glyph, the space after it,
/// and the word with whatever reason the recipe wrote after that.
#[must_use]
pub fn state_cells(state: &str) -> u16 {
    GLYPH.saturating_add(wide(state))
}

/// The widest state on a board, never below the table's seventeen.
#[must_use]
pub fn widest_state<'a>(states: impl IntoIterator<Item = &'a str>) -> u16 {
    states
        .into_iter()
        .map(state_cells)
        .fold(STATE, std::cmp::Ord::max)
}

/// How many of the terminal's columns a string takes up.
///
/// Not its characters: a row is composed as text and padded by hand, so a
/// cell measured in characters and drawn in columns puts every column after
/// it out by one for each double-width character in it.
#[must_use]
pub fn wide(text: &str) -> u16 {
    u16::try_from(unicode_width::UnicodeWidthStr::width(text)).unwrap_or(u16::MAX)
}

/// `text` cut to `width` columns, the last of them saying that something
/// was cut.
///
/// The mark is the caller's because the theme owns it: a terminal that
/// cannot draw `…` is given `...`, which is three cells and not one, so the
/// budget is measured against whatever is actually going to be drawn.
///
/// Cut by column and not by character, for the reason [`wide`] gives — and
/// a character that would straddle the edge is left out rather than half
/// drawn, so the result can be a column short of its budget. The padding
/// that follows it in a row closes that up.
#[must_use]
pub fn cut(text: &str, width: u16, ellipsis: &str) -> String {
    if wide(text) <= width {
        return text.to_string();
    }
    // A column with no room for the mark has no room for a head to put it
    // after either.
    let Some(budget) = usize::from(width).checked_sub(usize::from(wide(ellipsis))) else {
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
    cut.push_str(ellipsis);
    cut
}

/// `text` in a column of `width`, cut with `ellipsis` if it does not fit and
/// padded with spaces if it does not fill.
#[must_use]
pub fn left(text: &str, width: u16, ellipsis: &str) -> String {
    let cell = cut(text, width, ellipsis);
    let padding = usize::from(width).saturating_sub(usize::from(wide(&cell)));
    format!("{cell}{}", " ".repeat(padding))
}

#[cfg(test)]
mod tests {
    use super::{Columns, Field, Place, cut, state_cells, widest_state};

    /// Where every column of a row begins and how wide it is, in the order
    /// the spec's table lists them.
    fn places(columns: &Columns) -> Vec<(Field, u16, u16)> {
        columns
            .fields()
            .iter()
            .map(|(field, place)| (*field, place.start, place.width))
            .collect()
    }

    #[test]
    fn the_live_columns_are_where_the_spec_puts_them() {
        // A 120-column terminal, less the two cells the panel's borders
        // take: the row is 118 cells across and the title has what is left.
        let columns = Columns::live(118, widest_state(["running"]));

        assert_eq!(
            places(&columns),
            [
                (Field::Mark, 0, 2),
                (Field::Task, 2, 4),
                (Field::State, 8, 17),
                (Field::Stage, 27, 7),
                (Field::Context, 36, 14),
                (Field::Commit, 52, 13),
                (Field::Title, 67, 51),
            ],
        );
        assert_eq!(columns.width(), 118);
        assert!(!columns.finished());
    }

    #[test]
    fn a_state_wider_than_the_table_moves_every_later_column_right_by_the_difference() {
        // Given T2 is incomplete (no review record, box not ticked) — 45
        // cells of words, and 47 with the glyph before them, which is 30
        // more than the table's seventeen.
        let state = "incomplete (no review record, box not ticked)";
        let columns = Columns::live(138, widest_state(["running", state, "done"]));

        assert_eq!(state_cells(state), 47);
        assert_eq!(
            columns.place(Field::State),
            Some(Place {
                start: 8,
                width: 47
            })
        );
        for (field, start) in [
            (Field::Stage, 57),
            (Field::Context, 66),
            (Field::Commit, 82),
            (Field::Title, 97),
        ] {
            assert_eq!(
                columns.place(field).map(|place| place.start),
                Some(start),
                "{field:?} did not move with the state",
            );
        }
    }

    #[test]
    fn the_finished_view_is_the_id_the_word_and_what_landed() {
        let columns = Columns::landed(118);

        assert_eq!(
            places(&columns),
            [
                (Field::Mark, 0, 2),
                (Field::Task, 2, 4),
                (Field::State, 8, 6),
                (Field::Title, 16, 102),
            ],
        );
        assert!(columns.finished());
        // And the live columns are not there to be asked for.
        assert_eq!(columns.place(Field::Commit), None);
    }

    #[test]
    fn the_header_names_the_columns_that_are_drawn() {
        // Given a finished board
        // Then the tasks header reads "  TASK  STATE   LANDED"
        assert_eq!(Columns::landed(118).header("…"), "  TASK  STATE   LANDED");
        // And a live one names every column, the tokens right-aligned over
        // the numbers under them and the commit column saying which two
        // counts it carries.
        let header = Columns::live(118, 17).header("…");
        assert_eq!(
            header,
            "  TASK  STATE              STAGE    CONTEXT         COMMIT +~      TITLE",
        );
        for (field, heading) in [
            (Field::Task, "TASK"),
            (Field::State, "STATE"),
            (Field::Stage, "STAGE"),
            (Field::Context, "CONTEXT"),
            (Field::Commit, "COMMIT +~"),
            (Field::Title, "TITLE"),
        ] {
            let place = Columns::live(118, 17)
                .place(field)
                .expect("a live row has every column");
            let start = usize::from(place.start);
            let cell = &header[start..start + heading.len()];
            assert_eq!(cell, heading, "{field:?} is not above its column");
        }
    }

    #[test]
    fn the_header_is_cut_with_the_mark_the_terminal_can_draw() {
        // The heading over a column is cut like everything else in it, and
        // by the same mark: a heading that cut with `…` above rows cutting
        // with `...` would put the one glyph the ASCII theme exists to
        // avoid at the top of the board.
        //
        // A finished board's, because the live view's bands keep every
        // heading a column it fits in: a window with no room for a title
        // worth reading has no title at all. The landed columns have no
        // bands — there is one column after the word, and it is what landed.
        let columns = Columns::landed(20);

        assert_eq!(columns.header("…").split_whitespace().last(), Some("LAN…"));
        assert_eq!(
            columns.header("...").split_whitespace().last(),
            Some("L...")
        );
    }

    #[test]
    fn a_value_wider_than_its_column_is_cut_with_whatever_says_so() {
        assert_eq!(cut("just dev", 8, "…"), "just dev");
        assert_eq!(cut("just dev", 7, "…"), "just d…");
        assert_eq!(cut("just dev", 1, "…"), "…");
        assert_eq!(cut("just dev", 0, "…"), "");
        // The mark is measured, not counted: the ASCII one is three cells,
        // so it takes three off the budget rather than one.
        assert_eq!(cut("just dev", 7, "..."), "just...");
        assert_eq!(cut("just dev", 2, "..."), "");
        // Columns, not characters: two of these fill four cells.
        assert_eq!(cut("更新", 3, "…"), "更…");
        assert_eq!(super::wide(&cut("更新更", 4, "…")), 3);
    }

    /// Every column is read from its left edge — the two that were read
    /// from the right were the tokens and their heading, and both have left
    /// the row.
    #[test]
    fn a_cell_is_padded_to_the_width_of_the_column_it_is_read_in() {
        let place = Place { start: 0, width: 6 };

        assert_eq!(super::left("12.1M", place.width, "…"), "12.1M ");
        assert_eq!(super::left("", place.width, "…"), "      ");
    }

    // ── 11-T7

    use super::bands;

    /// Which columns a window this wide draws, and how many cells each has.
    ///
    /// Read through [`bands`] rather than [`Columns::live`], so the numbers
    /// here are the ones the spec's bands are stated in: a window somebody
    /// has dragged, borders and all.
    fn columns_at(width: u16, state: u16) -> Vec<(Field, u16)> {
        bands(width, 40, state, false)
            .columns
            .fields()
            .iter()
            .map(|(field, place)| (*field, place.width))
            .collect()
    }

    /// The same, as the names alone — which is what a band drops.
    pub(super) fn drawn_at(width: u16, state: u16) -> Vec<Field> {
        columns_at(width, state)
            .into_iter()
            .map(|(field, _)| field)
            .collect()
    }

    /// Everything a live row can hold, in the order it is drawn.
    const EVERYTHING: [Field; 7] = [
        Field::Mark,
        Field::Task,
        Field::State,
        Field::Stage,
        Field::Context,
        Field::Commit,
        Field::Title,
    ];

    #[test]
    fn the_row_gives_up_a_column_at_a_time_as_the_window_narrows() {
        // The bands the column table works out to, each named by the
        // narrowest window that still has it.
        assert_eq!(drawn_at(81, 17), EVERYTHING);
        assert_eq!(drawn_at(200, 17), EVERYTHING);
        // From 58 the row is whole without a title, and from 65 its bar is
        // the percentage alone.
        assert_eq!(drawn_at(80, 17), EVERYTHING[..6]);
        assert_eq!(drawn_at(58, 17), EVERYTHING[..6]);
        // From 51 the percentage goes, and from 50 the commit facts. What
        // is left is which task is in what trouble and how far it got.
        assert_eq!(
            drawn_at(57, 17),
            [
                Field::Mark,
                Field::Task,
                Field::State,
                Field::Stage,
                Field::Commit
            ],
        );
        assert_eq!(drawn_at(50, 17), EVERYTHING[..4]);
        // And a window too narrow even for those overflows rather than
        // dropping them: a row nobody can read whole still says which task
        // is in what trouble.
        assert_eq!(drawn_at(44, 17), EVERYTHING[..4]);
        assert_eq!(drawn_at(0, 17), EVERYTHING[..4]);
    }

    #[test]
    fn the_bar_goes_before_the_percentage_it_was_drawn_from() {
        let context = |width| {
            columns_at(width, 17)
                .into_iter()
                .find(|(field, _)| *field == Field::Context)
                .map(|(_, cells)| cells)
        };

        assert_eq!(context(67), Some(super::CONTEXT));
        assert_eq!(context(66), Some(super::PERCENTAGE));
        assert_eq!(context(58), Some(super::PERCENTAGE));
        assert_eq!(context(57), None);
        // And the heading follows the column it is over.
        assert_eq!(
            Columns::live(60, 17).header("…"),
            "  TASK  STATE              STAGE    CTX    COMMIT +~",
        );
    }

    #[test]
    fn a_wider_state_moves_every_band_by_the_difference() {
        // `incomplete (no review record)` and its glyph are 31 cells, which
        // is fourteen more than the table's seventeen — so every window
        // above is fourteen columns wider than it was.
        let state = state_cells("incomplete (no review record)");
        assert_eq!(state, 31);

        for width in [93, 80, 71, 54, 48, 34] {
            assert_eq!(
                drawn_at(width + 14, state),
                drawn_at(width, 17),
                "the band at {width} did not move with the state",
            );
            assert_eq!(
                drawn_at(width + 13, state),
                drawn_at(width - 1, 17),
                "the band below {width} did not move with the state",
            );
        }
    }

    #[test]
    fn the_frame_gives_up_the_pane_first_and_then_the_waves_second_line() {
        // The pane's two bands, each named by the smallest terminal that
        // still has it.
        assert!(bands(100, 24, 17, false).detail);
        assert!(!bands(99, 24, 17, false).detail);
        assert!(!bands(100, 23, 17, false).detail);
        // The wave panel's, which is a band further down: a window that has
        // already given up the pane gives up the strip and the keys next,
        // and what is left of the panel is what this wave is doing.
        assert_eq!(bands(100, 16, 17, false).wave, 2);
        assert_eq!(bands(100, 15, 17, false).wave, 1);
        assert_eq!(bands(0, 0, 17, false).wave, 1);
        // And a finished board's rows are the landed columns, whatever the
        // window: there is no run left for a band to take a number off.
        assert!(bands(40, 40, 17, true).columns.finished());
        assert!(!bands(200, 40, 17, false).columns.finished());
    }

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig {
            failure_persistence: Some(Box::new(
                proptest::test_runner::FileFailurePersistence::WithSource("proptest-regressions"),
            )),
            ..proptest::prelude::ProptestConfig::default()
        })]

        /// The bands are cumulative and drop whole: whatever the window and
        /// whatever the widest state, a narrower panel draws none of the
        /// columns a wider one had already dropped, draws none of them wider
        /// than it did, and — until there is nothing left to give up — draws
        /// no row past its own right edge.
        #[test]
        fn a_narrower_panel_never_draws_what_a_wider_one_gave_up(
            width in 0_u16..200,
            state in 17_u16..64,
        ) {
            let narrow = Columns::live(width, state);
            let wide = Columns::live(width.saturating_add(1), state);

            for (field, place) in narrow.fields() {
                let same = wide.place(*field).ok_or_else(|| proptest::test_runner::TestCaseError::fail(
                    format!("{field:?} came back on a narrower panel"),
                ))?;
                proptest::prop_assert!(
                    place.width <= same.width,
                    "{field:?} is wider on the narrower panel",
                );
            }
            // Whole rather than clipped: the row ends inside the panel, or
            // it is the last band, which overflows rather than giving up the
            // id, the state or the stage.
            let (_, last) = narrow.fields().last().expect("a row has columns");
            proptest::prop_assert!(
                last.start.saturating_add(last.width) <= width || narrow.fields().len() == 4,
                "the row runs past the panel with columns left to drop",
            );
        }

        /// Given any set of state words of any length, every state's word
        /// is drawn whole and every later column starts after the widest.
        #[test]
        fn any_set_of_states_is_never_cut(
            states in proptest::collection::vec("[ a-zA-Z0-9(),←-]{0,60}", 0..8),
        ) {
            let widest = widest_state(states.iter().map(String::as_str));
            let columns = Columns::live(200, widest);
            let state = columns.place(Field::State).expect("a live row has a state");

            for word in &states {
                proptest::prop_assert!(
                    state_cells(word) <= state.width,
                    "{word:?} would be cut to {}",
                    state.width,
                );
            }
            proptest::prop_assert!(state.width >= 17);
            for (field, place) in columns.fields() {
                if place.start > state.start {
                    proptest::prop_assert!(
                        place.start > state.start + state.width,
                        "{field:?} starts inside the state column",
                    );
                }
            }
        }
    }
}
