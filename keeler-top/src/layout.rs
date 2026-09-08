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
    /// What the run was started with.
    Model,
    /// The bar, the percentage and the flag slot.
    Context,
    /// Everything the run has written.
    Tokens,
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

/// The model: `opus5[1m]`.
const MODEL: u16 = 9;

/// Eight bar cells, a space, the percentage right-aligned in four, and the
/// flag slot.
const CONTEXT: u16 = 14;

/// The tokens, right-aligned: `999.9k`.
const TOKENS: u16 = 6;

/// `<hash> +<ahead> ~<dirty>`.
const COMMIT: u16 = 13;

/// `✓ done`, which is the whole of a finished row's state column.
const LANDED_STATE: u16 = 6;

/// One space after every column but the marker.
const GAP: u16 = 1;

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
    /// The live view's columns: everything a running wave has to say, with
    /// the state as wide as the widest one on the board.
    #[must_use]
    pub fn live(width: u16, widest_state: u16) -> Self {
        Self::laid_out(
            width,
            false,
            &[
                (Field::Mark, MARK),
                (Field::Task, TASK),
                (Field::State, widest_state.max(STATE)),
                (Field::Stage, STAGE),
                (Field::Model, MODEL),
                (Field::Context, CONTEXT),
                (Field::Tokens, TOKENS),
                (Field::Commit, COMMIT),
            ],
        )
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
        )
    }

    /// The columns before the last one, placed left to right, and the last
    /// taking whatever is left.
    fn laid_out(width: u16, finished: bool, fixed: &[(Field, u16)]) -> Self {
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
        places.push((
            Field::Title,
            Place {
                start,
                width: width.saturating_sub(start),
            },
        ));
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
    /// second layout to keep in step.
    #[must_use]
    pub fn header(&self) -> String {
        let mut header = String::new();
        for (field, place) in &self.places {
            let heading = self.heading(*field);
            let cell = if matches!(field, Field::Tokens) {
                right(&heading, place.width)
            } else {
                left(&heading, place.width)
            };
            header.push_str(&cell);
            if !matches!(field, Field::Mark) {
                header.push(' ');
            }
        }
        header.trim_end().to_string()
    }

    /// What one column is called.
    fn heading(&self, field: Field) -> String {
        match field {
            Field::Mark => String::new(),
            Field::Task => "TASK".to_string(),
            Field::State => "STATE".to_string(),
            Field::Stage => "STAGE".to_string(),
            Field::Model => "MODEL".to_string(),
            Field::Context => "CONTEXT".to_string(),
            Field::Tokens => "TOKENS".to_string(),
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
pub fn left(text: &str, width: u16) -> String {
    let cell = cut(text, width, "…");
    let padding = usize::from(width).saturating_sub(usize::from(wide(&cell)));
    format!("{cell}{}", " ".repeat(padding))
}

/// The same, against the column's right edge — which is how a number is
/// read beside another number.
#[must_use]
pub fn right(text: &str, width: u16) -> String {
    let cell = cut(text, width, "…");
    let padding = usize::from(width).saturating_sub(usize::from(wide(&cell)));
    format!("{}{cell}", " ".repeat(padding))
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
                (Field::State, 7, 17),
                (Field::Stage, 25, 7),
                (Field::Model, 33, 9),
                (Field::Context, 43, 14),
                (Field::Tokens, 58, 6),
                (Field::Commit, 65, 13),
                (Field::Title, 79, 39),
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
                start: 7,
                width: 47
            })
        );
        for (field, start) in [
            (Field::Stage, 55),
            (Field::Model, 63),
            (Field::Context, 73),
            (Field::Tokens, 88),
            (Field::Commit, 95),
            (Field::Title, 109),
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
                (Field::State, 7, 6),
                (Field::Title, 14, 104),
            ],
        );
        assert!(columns.finished());
        // And the live columns are not there to be asked for.
        assert_eq!(columns.place(Field::Commit), None);
    }

    #[test]
    fn the_header_names_the_columns_that_are_drawn() {
        // Given a finished board
        // Then the tasks header reads "  TASK STATE  LANDED"
        assert_eq!(Columns::landed(118).header(), "  TASK STATE  LANDED");
        // And a live one names every column, the tokens right-aligned over
        // the numbers under them and the commit column saying which two
        // counts it carries.
        let header = Columns::live(118, 17).header();
        assert_eq!(
            header,
            "  TASK STATE             STAGE   MODEL     CONTEXT        TOKENS COMMIT +~     TITLE",
        );
        for (field, heading) in [
            (Field::Task, "TASK"),
            (Field::State, "STATE"),
            (Field::Stage, "STAGE"),
            (Field::Model, "MODEL"),
            (Field::Context, "CONTEXT"),
            (Field::Tokens, "TOKENS"),
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
    fn a_row_narrower_than_its_columns_has_no_title_left_rather_than_a_negative_one() {
        // The bands are T7's; what this says is that the arithmetic does
        // not wrap round on the way there.
        let columns = Columns::live(20, 17);

        assert_eq!(
            columns.place(Field::Title),
            Some(Place {
                start: 79,
                width: 0
            })
        );
        assert_eq!(
            Columns::live(0, u16::MAX)
                .place(Field::Title)
                .map(|place| place.width),
            Some(0)
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

    #[test]
    fn a_cell_is_padded_to_its_column_from_whichever_side_it_is_read() {
        let place = Place { start: 0, width: 6 };

        assert_eq!(super::left("12.1M", place.width), "12.1M ");
        assert_eq!(super::right("12.1M", place.width), " 12.1M");
        assert_eq!(super::left("", place.width), "      ");
    }

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig {
            failure_persistence: Some(Box::new(
                proptest::test_runner::FileFailurePersistence::WithSource("proptest-regressions"),
            )),
            ..proptest::prelude::ProptestConfig::default()
        })]

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
