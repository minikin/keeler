//! Every colour and every glyph the board draws, in one value.
//!
//! A value and not a global: the environment is read once, in `main`, and
//! the answer is carried to the renderer — so a test draws one board
//! through two themes without touching the process it runs in, which under
//! nextest it shares with nothing and under `cargo test` with every other
//! test in the binary. Setting a variable is an unsafe, process-wide act in
//! this edition, and a theme read from the environment at the point of use
//! would make that the only way to test a colour.
//!
//! The two questions it answers are independent. `NO_COLOR` leaves every
//! style the terminal's default, so the glyphs alone carry the state —
//! which is why no two states share one. `KEELER_TOP_ASCII`, or a locale
//! that is not UTF-8, swaps every glyph **the theme owns** for one below
//! U+007F: the states, the bar, the marker, the connector, the borders and
//! the four punctuation marks.
//!
//! A glyph the theme does not own is not the theme's to swap, and there
//! are two. `board.rs` composes `blocked ← T1` as one string, so the `←`
//! reaches the frame inside the state's own text rather than through
//! [`Theme::waits_on`] — a frame that wants the ASCII set has to compose
//! that text itself. And `run.rs`'s `DASH` belongs to `--once`, which is a
//! frozen plain-text surface and has no theme at all.
//!
//! **The state table is the whole of what the board believes about
//! `keeler-status`'s vocabulary.** It classifies by the leading word and
//! passes the rest through untouched: `failed (exit 2)` is themed as
//! `failed` and shown whole, and a word the table has never heard of is a
//! row with a `?` rather than a row that is missing.

use std::ffi::OsStr;

use ratatui::style::{Color, Style};
use ratatui::symbols::border;

/// What text is drawn in, and every colour below it: all of them at least
/// 4.5:1 against the board's ground, the dim one included.
pub const TEXT: Color = Color::Rgb(0xc0, 0xca, 0xf5);

/// What is on the board because it has to be somewhere — a done row, a
/// task nobody has spawned, the connector under a live row.
pub const DIM: Color = Color::Rgb(0x78, 0x7f, 0x9e);

/// Rules and the empty half of a bar: the board's furniture, and nothing
/// that carries a reading.
pub const CHROME: Color = Color::Rgb(0x56, 0x5f, 0x89);

/// Work happening now.
pub const ORANGE: Color = Color::Rgb(0xff, 0x9e, 0x64);

/// Work that came out green.
pub const GREEN: Color = Color::Rgb(0x9e, 0xce, 0x6a);

/// Work that needs somebody.
pub const RED: Color = Color::Rgb(0xf7, 0x76, 0x8e);

/// Work that stopped short of an answer.
pub const YELLOW: Color = Color::Rgb(0xe0, 0xaf, 0x68);

/// Work somebody stopped on purpose.
pub const VIOLET: Color = Color::Rgb(0xbb, 0x9a, 0xf7);

/// Work that could start.
pub const BLUE: Color = Color::Rgb(0x7a, 0xa2, 0xf7);

/// The one accent that names a thing rather than a state: the tool a run
/// is in.
pub const CYAN: Color = Color::Rgb(0x7d, 0xcf, 0xff);

/// The selected row's ground. A lighter background and a marker, never a
/// reverse-video bar: reversing a row throws away the colours the row was
/// drawn in, which are the reading it exists to give.
pub const SELECTED_BG: Color = Color::Rgb(0x2a, 0x2f, 0x45);

/// The panels' borders. Every border cell is this, whatever the panel —
/// the title on the frame is what carries the panel's own colour.
pub const BORDER: Color = Color::Rgb(0x3b, 0x42, 0x61);

/// How many cells the context bar has.
pub const BAR_CELLS: u8 = 8;

/// The share at which the bar turns yellow.
const WARN: u8 = 60;

/// The share at which it turns red and the flag slot fills.
///
/// The same share `--once`'s context column marks at, and deliberately not
/// the same constant: that column is a frozen surface a script parses, and
/// a board that shared the number with it would move both at once.
const ALARM: u8 = 80;

/// One row of the state table: the word that names the state, its glyph in
/// each set, the colour it is drawn in, and the group it is ordered by.
struct State {
    word: &'static str,
    glyph: &'static str,
    ascii: &'static str,
    colour: Color,
    group: u8,
}

/// The state vocabulary, all of it, ordered by what a watcher asks first:
/// is anything stuck, what is running, what is left.
///
/// The word is the state's first, which is all the board matches on —
/// `not spawned` is `not`, and no other state begins with it.
const STATES: [State; 10] = [
    State {
        word: "failed",
        glyph: "✗",
        ascii: "x",
        colour: RED,
        group: 0,
    },
    State {
        word: "died",
        glyph: "⊘",
        ascii: "X",
        colour: RED,
        group: 0,
    },
    State {
        word: "incomplete",
        glyph: "◔",
        ascii: "o",
        colour: YELLOW,
        group: 0,
    },
    State {
        word: "paused",
        glyph: "‖",
        ascii: "=",
        colour: VIOLET,
        group: 1,
    },
    State {
        word: "running",
        glyph: "●",
        ascii: "*",
        colour: ORANGE,
        group: 2,
    },
    State {
        word: "passed",
        glyph: "◐",
        ascii: "+",
        colour: GREEN,
        group: 3,
    },
    State {
        word: "ready",
        glyph: "◇",
        ascii: "<",
        colour: BLUE,
        group: 4,
    },
    State {
        word: "blocked",
        glyph: "○",
        ascii: "-",
        colour: DIM,
        group: 5,
    },
    State {
        word: "not",
        glyph: "·",
        ascii: ".",
        colour: DIM,
        group: 6,
    },
    State {
        word: "done",
        glyph: "✓",
        ascii: "v",
        colour: DIM,
        group: 7,
    },
];

/// A word the table has no row for.
///
/// It is grouped with the running tasks and not with the exceptions: the
/// board does not know what it means, and a state it cannot read must not
/// be promoted to the half of the header that says a human is needed.
const UNKNOWN: State = State {
    word: "",
    glyph: "?",
    ascii: "?",
    colour: TEXT,
    group: 2,
};

/// The group a done row is in, and the one whose colour depends on the
/// board around it.
const DONE: u8 = 7;

/// The borders a terminal that cannot draw a box gets.
const ASCII_BORDER: border::Set<'static> = border::Set {
    top_left: "+",
    top_right: "+",
    bottom_left: "+",
    bottom_right: "+",
    vertical_left: "|",
    vertical_right: "|",
    horizontal_top: "-",
    horizontal_bottom: "-",
};

/// A state's row of the table, or the fallback for a word it has none for.
fn state_of(state: &str) -> &'static State {
    let word = state.split_whitespace().next().unwrap_or_default();
    STATES
        .iter()
        .find(|entry| entry.word == word)
        .unwrap_or(&UNKNOWN)
}

/// The context bar, in the two pieces it is drawn in.
///
/// Two and not one string with one style: the empty cells are the track,
/// and a bar handed over whole would be drawn in the fill's colour — so a
/// run at 84% would show eight red cells, which is the one reading the bar
/// exists to give, said twice and in the wrong half.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bar {
    /// The cells the share has filled.
    pub filled: String,
    /// What those are drawn in: blue, yellow or red, by the share.
    pub fill: Style,
    /// The cells it has not.
    pub empty: String,
    /// What those are drawn in — [`CHROME`], whatever the share.
    pub track: Style,
}

/// What a state looks like: the glyph that stands for it, and the style
/// that glyph and its word are drawn in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Look {
    /// The one or two characters that stand for the state.
    pub glyph: &'static str,
    /// What the glyph and the state's word are drawn in — the default
    /// under `NO_COLOR`, where the glyph carries the whole reading.
    pub style: Style,
}

/// The colours and glyphs one terminal gets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    colour: bool,
    ascii: bool,
}

impl Theme {
    /// A theme said outright, which is how every test builds one.
    #[must_use]
    pub const fn new(colour: bool, ascii: bool) -> Self {
        Self { colour, ascii }
    }

    /// The theme this process's environment asks for.
    ///
    /// Called once, in `main`, and outside the mutation gate for the reason
    /// `Shell::in_tmux` gives: setting a variable is a process-wide, unsafe
    /// act in this edition and the suite runs its tests in threads, so a
    /// test cannot arrange either answer. Nothing is decided here — every
    /// decision is [`Self::chosen`]'s, which is a function of four values
    /// and is tested against them; what is left is the four names.
    #[cfg_attr(test, mutants::skip)]
    #[must_use]
    pub fn from_env() -> Self {
        Self::chosen(
            std::env::var_os("NO_COLOR").as_deref(),
            std::env::var_os("KEELER_TOP_ASCII").as_deref(),
            std::env::var_os("LC_ALL").as_deref(),
            std::env::var_os("LANG").as_deref(),
        )
    }

    /// The theme four environment variables come to.
    fn chosen(
        no_color: Option<&OsStr>,
        ascii: Option<&OsStr>,
        lc_all: Option<&OsStr>,
        lang: Option<&OsStr>,
    ) -> Self {
        // LC_ALL wins when it is set to something, as it does for every
        // other program that reads a locale — and an exported but empty
        // one is a shell standing aside rather than a locale, which
        // `or_else` on the variable alone would have read as `C`.
        let locale = lc_all.filter(|value| !value.is_empty()).or(lang);
        Self::new(!set(no_color), asked(ascii) || !utf8(locale))
    }

    /// How a state is drawn. `finished` is a board whose every row is
    /// done, where done is the answer rather than the background.
    #[must_use]
    pub fn look(&self, state: &str, finished: bool) -> Look {
        let entry = state_of(state);
        let colour = if finished && entry.group == DONE {
            GREEN
        } else {
            entry.colour
        };
        Look {
            glyph: self.pick(entry.glyph, entry.ascii),
            style: self.style(colour),
        }
    }

    /// Which of the state table's groups a state is in, which is the order
    /// the board puts its rows in.
    #[must_use]
    pub fn group(state: &str) -> u8 {
        state_of(state).group
    }

    /// How many of the bar's eight cells a share fills: an eighth of the
    /// window each, rounded half up, and never a ninth.
    ///
    /// Half up rather than to nearest-even for the reason the percentage
    /// itself rounds that way — the bar is a warning, and the quiet side is
    /// the wrong one to round to.
    #[must_use]
    pub fn cells(percent: u8) -> u8 {
        let cells = u16::from(BAR_CELLS);
        let rounded = (u16::from(percent) * cells * 2 + 100) / 200;
        u8::try_from(rounded).unwrap_or(BAR_CELLS).min(BAR_CELLS)
    }

    /// The bar's eight cells, in the two halves they are drawn in.
    #[must_use]
    pub fn bar(&self, percent: u8) -> Bar {
        let filled = usize::from(Self::cells(percent));
        Bar {
            filled: self.pick("█", "#").repeat(filled),
            fill: self.style(fill(percent)),
            empty: self
                .pick("░", "-")
                .repeat(usize::from(BAR_CELLS).saturating_sub(filled)),
            track: self.style(CHROME),
        }
    }

    /// The percentage and its flag slot: five cells, whatever the number.
    ///
    /// Right-aligned in four with the `%`, and the flag in a cell of its
    /// own — so 100% and 9% end in the same column and the `!` at 80 moves
    /// nothing beside it.
    #[must_use]
    pub fn percentage(percent: u8) -> String {
        let flag = if percent >= ALARM { "!" } else { " " };
        format!("{percent:>3}%{flag}")
    }

    /// The context column's characters whole: the bar, a space, and the
    /// percentage. What each of them is drawn in is [`Self::bar`]'s.
    #[must_use]
    pub fn context(&self, percent: u8) -> String {
        let bar = self.bar(percent);
        format!("{}{} {}", bar.filled, bar.empty, Self::percentage(percent))
    }

    /// One colour as a style — or the terminal's default, which is the
    /// whole of what `NO_COLOR` means and the one place it is decided.
    #[must_use]
    pub fn style(&self, colour: Color) -> Style {
        if self.colour {
            Style::new().fg(colour)
        } else {
            Style::new()
        }
    }

    /// The selected row's ground.
    #[must_use]
    pub fn selection(&self) -> Style {
        if self.colour {
            Style::new().bg(SELECTED_BG)
        } else {
            Style::new()
        }
    }

    /// The panels' box.
    #[must_use]
    pub fn border(&self) -> border::Set<'static> {
        if self.ascii {
            ASCII_BORDER
        } else {
            border::PLAIN
        }
    }

    /// What marks the selected row. U+25B8 and never U+25B6, which some
    /// platforms draw as an emoji — two cells wide, and every column after
    /// it out by one.
    #[must_use]
    pub fn marker(&self) -> &'static str {
        self.pick("▸", ">")
    }

    /// What hangs the second line under a live row.
    #[must_use]
    pub fn connector(&self) -> &'static str {
        self.pick("└─", "`-")
    }

    /// What stands between a blocked task and what it waits on.
    #[must_use]
    pub fn waits_on(&self) -> &'static str {
        self.pick("←", "<-")
    }

    /// What stands between two things on one line.
    #[must_use]
    pub fn separator(&self) -> &'static str {
        self.pick("·", "-")
    }

    /// What says something was cut.
    #[must_use]
    pub fn ellipsis(&self) -> &'static str {
        self.pick("…", "...")
    }

    /// What stands between a step and what it leaves behind.
    #[must_use]
    pub fn leads_to(&self) -> &'static str {
        self.pick("→", "->")
    }

    /// The glyph set this terminal can draw.
    const fn pick(self, unicode: &'static str, ascii: &'static str) -> &'static str {
        if self.ascii { ascii } else { unicode }
    }
}

/// What the filled half of a bar says about the run behind it.
const fn fill(percent: u8) -> Color {
    if percent >= ALARM {
        RED
    } else if percent >= WARN {
        YELLOW
    } else {
        BLUE
    }
}

/// `NO_COLOR`, by the convention it is named for: present and not the
/// empty string, whatever the value — <https://no-color.org>. The empty
/// string is how a shell says no to a variable it has already exported.
fn set(value: Option<&OsStr>) -> bool {
    value.is_some_and(|value| !value.is_empty())
}

/// `KEELER_TOP_ASCII`, which is the board's own rather than a convention:
/// `=1` is how it is meant to be written, and `=0` is how somebody who has
/// already exported it turns it back off.
fn asked(value: Option<&OsStr>) -> bool {
    value.is_some_and(|value| !value.is_empty() && value != "0")
}

/// Whether a locale can draw the glyphs.
///
/// A locale nobody set is a terminal nobody has said anything about, and
/// every terminal shipped this decade is UTF-8 — so the glyphs are drawn,
/// and `KEELER_TOP_ASCII` is the way out for the one that is not. A value
/// that is not text is not a locale either.
fn utf8(locale: Option<&OsStr>) -> bool {
    let Some(locale) = locale.and_then(OsStr::to_str) else {
        return true;
    };
    let locale = locale.to_ascii_lowercase();
    locale.contains("utf-8") || locale.contains("utf8")
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    use ratatui::style::Style;

    use super::{BAR_CELLS, BLUE, GREEN, RED, Theme, YELLOW};

    /// The board as it is meant to look, the one a terminal that refuses
    /// colour gets, and the one a terminal that cannot draw the glyphs
    /// gets. Constructed here rather than read from the process: a test
    /// that set `NO_COLOR` would be setting it for every other test in the
    /// process, and this is the whole reason the theme is a value.
    const COLOURED: Theme = Theme::new(true, false);
    const PLAIN: Theme = Theme::new(false, false);
    const ASCII: Theme = Theme::new(true, true);

    /// Every state `keeler-status` prints and the two the graph adds, in
    /// the order the state table gives them, and last the word the theme
    /// has no entry for.
    const VOCABULARY: [&str; 11] = [
        "failed (exit 2)",
        "died",
        "incomplete (no review record)",
        "paused",
        "running",
        "passed",
        "ready",
        "blocked ← T6",
        "not spawned",
        "done",
        "sulking",
    ];

    #[test]
    fn the_state_table_is_the_one_the_spec_prints() {
        // Given the ten states and the fallback
        // When each is themed
        // Then the glyph, the colour and the group are the table's row
        let looks = VOCABULARY.map(|state| COLOURED.look(state, false));
        let glyphs = looks.map(|look| look.glyph);
        let styles = looks.map(|look| look.style);

        assert_eq!(
            glyphs,
            ["✗", "⊘", "◔", "‖", "●", "◐", "◇", "○", "·", "✓", "?"]
        );
        assert_eq!(
            styles,
            [
                super::RED,
                super::RED,
                super::YELLOW,
                super::VIOLET,
                super::ORANGE,
                super::GREEN,
                super::BLUE,
                super::DIM,
                super::DIM,
                super::DIM,
                super::TEXT,
            ]
            .map(|colour| Style::new().fg(colour)),
        );
        assert_eq!(
            VOCABULARY.map(Theme::group),
            [0, 0, 0, 1, 2, 3, 4, 5, 6, 7, 2]
        );
    }

    #[test]
    fn a_finished_view_paints_done_green_and_leaves_every_other_state_alone() {
        // The one state whose colour depends on the board around it: a
        // wave still running has done rows in the background, and a
        // feature that is finished has nothing else to say.
        assert_eq!(COLOURED.look("done", true).style, Style::new().fg(GREEN));
        assert_eq!(
            COLOURED.look("done", false).style,
            Style::new().fg(super::DIM)
        );
        assert_eq!(
            COLOURED.look("running", true).style,
            Style::new().fg(super::ORANGE),
        );
    }

    #[test]
    fn every_state_maps_to_a_distinct_glyph_in_both_glyph_sets() {
        // Given the ten states and the fallback
        // When each is themed in the Unicode set and in the ASCII set
        // Then no two share a glyph in either set
        for theme in [COLOURED, ASCII] {
            let mut glyphs: Vec<&str> = VOCABULARY
                .iter()
                .map(|state| theme.look(state, false).glyph)
                .collect();
            glyphs.sort_unstable();
            glyphs.dedup();

            assert_eq!(
                glyphs.len(),
                VOCABULARY.len(),
                "two states share a glyph in {theme:?}, which is the whole of what a \
                 NO_COLOR board has to tell them apart by",
            );
        }
    }

    #[test]
    fn the_thresholds_are_at_60_and_80() {
        // Given context at 59%, 60%, 79% and 80%
        // When the board renders
        // Then 59% is blue, 60% and 79% yellow, 80% red with "!"
        let fill = |percent| COLOURED.bar(percent).fill;

        assert_eq!(fill(59), Style::new().fg(BLUE));
        assert_eq!(fill(60), Style::new().fg(YELLOW));
        assert_eq!(fill(79), Style::new().fg(YELLOW));
        assert_eq!(fill(80), Style::new().fg(RED));
        assert_eq!(Theme::percentage(79), " 79% ");
        assert_eq!(Theme::percentage(80), " 80%!");
    }

    #[test]
    fn the_percentage_and_its_flag_never_move_a_neighbour() {
        // Given context at 9%, 41% and 100%
        // When the board renders
        // Then the CONTEXT column reads … and each is 14 cells
        for (percent, column) in [
            (9, "█░░░░░░░   9% "),
            (41, "███░░░░░  41% "),
            (100, "████████ 100%!"),
        ] {
            assert_eq!(COLOURED.context(percent), column);
            assert_eq!(
                unicode_width::UnicodeWidthStr::width(column),
                14,
                "{column:?} is not the column's width",
            );
        }
    }

    /// The bar's eight cells, whichever half they are in.
    fn bar_glyphs(theme: Theme, percent: u8) -> String {
        let bar = theme.bar(percent);
        format!("{}{}", bar.filled, bar.empty)
    }

    #[test]
    fn the_bar_is_eight_cells_however_full_the_window_is() {
        for (percent, bar) in [
            (0, "░░░░░░░░"),
            (9, "█░░░░░░░"),
            (41, "███░░░░░"),
            (65, "█████░░░"),
            (84, "███████░"),
            (100, "████████"),
        ] {
            assert_eq!(bar_glyphs(COLOURED, percent), bar, "at {percent}%");
        }
        // Half a cell is an eighth of the window either side of 6.25%,
        // and the rounding is up, as the percentage's own is.
        assert_eq!(Theme::cells(6), 0);
        assert_eq!(Theme::cells(7), 1);
        // A share above full is a model the board guessed the window for,
        // not a bar that runs into the column beside it.
        assert_eq!(bar_glyphs(COLOURED, u8::MAX), "████████");
    }

    #[test]
    fn the_track_is_chrome_at_every_share_and_the_fill_never_paints_it() {
        // The two halves are handed over separately because a bar drawn as
        // one span would paint the empty cells red at 80% — which is the
        // one reading the bar exists to give, said twice and in the wrong
        // place.
        for percent in [0, 41, 80, 100] {
            let bar = COLOURED.bar(percent);

            assert_eq!(bar.track, Style::new().fg(super::CHROME), "at {percent}%");
            assert_ne!(bar.fill, bar.track, "at {percent}%");
        }
    }

    #[test]
    fn the_ascii_theme_replaces_every_glyph_the_theme_owns() {
        let border = ASCII.border();
        let glyphs: Vec<String> = VOCABULARY
            .iter()
            .map(|state| ASCII.look(state, false).glyph.to_string())
            .chain([bar_glyphs(ASCII, 50)])
            .chain(
                [
                    ASCII.marker(),
                    ASCII.connector(),
                    ASCII.waits_on(),
                    ASCII.separator(),
                    ASCII.ellipsis(),
                    ASCII.leads_to(),
                    border.top_left,
                    border.top_right,
                    border.bottom_left,
                    border.bottom_right,
                    border.vertical_left,
                    border.vertical_right,
                    border.horizontal_top,
                    border.horizontal_bottom,
                ]
                .map(str::to_string),
            )
            .collect();

        for glyph in &glyphs {
            assert!(glyph.is_ascii(), "{glyph:?} is above U+007F");
        }
        assert_eq!(
            VOCABULARY.map(|state| ASCII.look(state, false).glyph),
            ["x", "X", "o", "=", "*", "+", "<", "-", ".", "v", "?"],
        );
        assert_eq!(bar_glyphs(ASCII, 50), "####----");
        assert_eq!(
            [
                ASCII.marker(),
                ASCII.connector(),
                ASCII.waits_on(),
                ASCII.separator(),
                ASCII.ellipsis(),
                ASCII.leads_to(),
            ],
            [">", "`-", "<-", "-", "...", "->"],
        );
        assert_eq!(
            [border.top_left, border.horizontal_top, border.vertical_left],
            ["+", "-", "|"],
        );
    }

    #[test]
    fn the_unicode_theme_draws_the_box_and_the_marker_that_is_not_an_emoji() {
        let border = COLOURED.border();

        assert_eq!(
            [border.top_left, border.horizontal_top, border.vertical_left],
            ["┌", "─", "│"],
        );
        // U+25B8, never U+25B6, which some platforms draw as an emoji —
        // two cells wide, and every column after it out by one.
        assert_eq!(COLOURED.marker(), "▸");
        assert_eq!(
            [
                COLOURED.connector(),
                COLOURED.waits_on(),
                COLOURED.separator(),
                COLOURED.ellipsis(),
                COLOURED.leads_to(),
            ],
            ["└─", "←", "·", "…", "→"],
        );
    }

    #[test]
    fn the_selection_is_a_background_and_the_borders_a_colour_of_their_own() {
        assert_eq!(COLOURED.selection(), Style::new().bg(super::SELECTED_BG));
        assert_eq!(
            COLOURED.style(super::BORDER),
            Style::new().fg(super::BORDER)
        );
    }

    /// The four variables, as the strings a shell would export them as.
    fn chosen(
        no_color: Option<&str>,
        ascii: Option<&str>,
        lc_all: Option<&str>,
        lang: Option<&str>,
    ) -> Theme {
        Theme::chosen(
            no_color.map(OsStr::new),
            ascii.map(OsStr::new),
            lc_all.map(OsStr::new),
            lang.map(OsStr::new),
        )
    }

    #[test]
    fn the_environment_answers_the_two_questions_the_theme_asks_it() {
        assert_eq!(chosen(None, None, None, None), Theme::new(true, false));
        // NO_COLOR by the convention it is named for: set to anything at
        // all but the empty string, whatever the value.
        assert_eq!(
            chosen(Some("1"), None, None, None),
            Theme::new(false, false)
        );
        assert_eq!(
            chosen(Some("0"), None, None, None),
            Theme::new(false, false)
        );
        assert_eq!(chosen(Some(""), None, None, None), Theme::new(true, false));
        // KEELER_TOP_ASCII is the board's own, and `=0` is how somebody
        // who has already exported it turns it back off.
        assert_eq!(chosen(None, Some("1"), None, None), Theme::new(true, true));
        assert_eq!(chosen(None, Some("0"), None, None), Theme::new(true, false));
        assert_eq!(chosen(None, Some(""), None, None), Theme::new(true, false));
        // The two are independent: a terminal with no colours still draws
        // the glyphs, and one with no glyphs still has its colours.
        assert_eq!(
            chosen(Some("1"), Some("1"), None, None),
            Theme::new(false, true),
        );
    }

    #[test]
    fn a_locale_that_cannot_draw_the_glyphs_asks_for_the_ascii_set() {
        for utf8 in ["en_US.UTF-8", "en_US.utf8", "C.UTF-8"] {
            assert_eq!(
                chosen(None, None, None, Some(utf8)),
                Theme::new(true, false),
                "{utf8}",
            );
        }
        for ascii in ["C", "POSIX", "en_US.ISO8859-1"] {
            assert_eq!(
                chosen(None, None, None, Some(ascii)),
                Theme::new(true, true),
                "{ascii}",
            );
        }
        // LC_ALL decides when it is set to something, as it does for
        // every other program that reads a locale — and an exported but
        // empty LC_ALL is a shell standing aside rather than a locale.
        // Reading it as one would drop a UTF-8 terminal into the ASCII
        // set for a variable nobody meant to set.
        assert_eq!(
            chosen(None, None, Some("C"), Some("en_US.UTF-8")),
            Theme::new(true, true),
        );
        assert_eq!(
            chosen(None, None, Some(""), Some("en_US.UTF-8")),
            Theme::new(true, false),
        );
        assert_eq!(
            chosen(None, None, Some("en_US.UTF-8"), Some("C")),
            Theme::new(true, false),
        );
    }

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig {
            failure_persistence: Some(Box::new(
                proptest::test_runner::FileFailurePersistence::WithSource("proptest-regressions"),
            )),
            ..proptest::prelude::ProptestConfig::default()
        })]

        /// Given any two percentages a ≤ b, a's filled cells ≤ b's, both
        /// in 0..=8, and 100% fills all eight. The bar is read at a
        /// glance and never against a scale, so a share that grew and a
        /// bar that shrank would be worse than no bar.
        #[test]
        fn any_percentage_fills_a_monotone_number_of_cells(
            first in proptest::prelude::any::<u8>(),
            second in proptest::prelude::any::<u8>(),
        ) {
            let (low, high) = if first <= second { (first, second) } else { (second, first) };

            proptest::prop_assert!(Theme::cells(low) <= Theme::cells(high));
            proptest::prop_assert!(Theme::cells(high) <= BAR_CELLS);
            proptest::prop_assert_eq!(Theme::cells(100), BAR_CELLS);
        }

        /// Given any state and any percentage, the NO_COLOR theme's every
        /// Style is the default — which is what makes the glyphs alone
        /// carry the state, and why every one of them is distinct.
        #[test]
        fn the_plain_theme_carries_no_colour(
            state in state(),
            percent in proptest::prelude::any::<u8>(),
        ) {
            proptest::prop_assert_eq!(PLAIN.look(&state, false).style, Style::default());
            proptest::prop_assert_eq!(PLAIN.look(&state, true).style, Style::default());
            proptest::prop_assert_eq!(PLAIN.bar(percent).fill, Style::default());
            proptest::prop_assert_eq!(PLAIN.bar(percent).track, Style::default());
            proptest::prop_assert_eq!(PLAIN.selection(), Style::default());
            proptest::prop_assert_eq!(PLAIN.style(RED), Style::default());
        }

        /// The theme classifies a state by its leading word and passes the
        /// rest through untouched: `failed (exit 2)` and `failed (exit
        /// 137)` are one row of the table, and so is a reason nobody has
        /// written yet.
        #[test]
        fn a_reason_after_the_state_changes_neither_its_glyph_nor_its_group(
            word in vocabulary(),
            reason in "[ a-zA-Z0-9(),←-]{0,30}",
        ) {
            let state = format!("{word} {reason}");

            proptest::prop_assert_eq!(
                COLOURED.look(&state, false).glyph,
                COLOURED.look(&word, false).glyph,
            );
            proptest::prop_assert_eq!(Theme::group(&state), Theme::group(&word));
        }
    }

    /// The leading word of every state the board is shown, and one the
    /// table has no row for.
    fn vocabulary() -> impl proptest::prelude::Strategy<Value = String> {
        proptest::prelude::Strategy::prop_map(
            proptest::sample::select(vec![
                "failed",
                "died",
                "incomplete",
                "paused",
                "running",
                "passed",
                "ready",
                "blocked",
                "not spawned",
                "done",
                "sulking",
                "",
            ]),
            str::to_string,
        )
    }

    /// A whole state as the board is shown one: the word and whatever the
    /// recipe wrote after it.
    fn state() -> impl proptest::prelude::Strategy<Value = String> {
        proptest::prelude::Strategy::prop_map(
            (vocabulary(), "[ a-zA-Z0-9(),←-]{0,30}"),
            |(word, reason)| format!("{word} {reason}"),
        )
    }
}
