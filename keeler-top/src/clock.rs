//! When a tool call was made, and how long ago that was.
//!
//! The stream stamps every record with an ISO-8601 instant, and the board
//! subtracts it from the clock. That is the whole requirement, and it is
//! why there is no date library here: a dependency that ships to every
//! adopter who runs the board would be earning its keep by parsing twenty
//! characters and taking one difference.
//!
//! The elapsed column is read against a running tool, so the answer that
//! matters is a few seconds to a few minutes old. Nothing here is a
//! calendar: no zones, no local time, no formatting of a date.

/// An instant, as seconds since the Unix epoch.
///
/// Signed, because the arithmetic that produces one runs through dates
/// before 1970 for no better reason than that the formula is written that
/// way, and clamping it would be a lie about what was parsed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Timestamp(i64);

impl Timestamp {
    /// The instant `seconds` after the epoch.
    #[must_use]
    pub fn from_epoch_seconds(seconds: i64) -> Self {
        Self(seconds)
    }

    /// The clock the elapsed column is read against.
    ///
    /// A clock before the epoch is not a case worth a branch — it is a
    /// machine whose time is wrong by fifty years, and the column it
    /// produces would be the least of that.
    #[must_use]
    pub fn now() -> Self {
        let since_epoch = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        Self(i64::try_from(since_epoch.as_secs()).unwrap_or(i64::MAX))
    }

    /// One record's `timestamp` field, or nothing for anything this cannot
    /// read.
    ///
    /// The shape is `2026-09-07T12:34:56.789Z`, and what is read of it is
    /// the date and the whole seconds: everything after them is a fraction
    /// the column cannot show. A stamp carrying a zone offset rather than
    /// `Z` would be read as if it were UTC — the CLI writes `Z`, and
    /// guessing at an offset nobody has seen is worse than reading the one
    /// that is there.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let (date, time) = text.split_once('T')?;
        let (year, month, day) = parse_date(date)?;
        let seconds_of_day = parse_time(time)?;
        Some(Self(
            days_from_civil(year, month, day) * 86_400 + seconds_of_day,
        ))
    }

    /// How long ago `self` was, from `now`. An instant in the future is no
    /// time at all rather than a negative one: a stream written by a
    /// machine whose clock runs ahead should show a tool that has just
    /// started, not one that will start.
    #[must_use]
    pub fn seconds_since(self, earlier: Self) -> u64 {
        u64::try_from(self.0.saturating_sub(earlier.0)).unwrap_or(0)
    }
}

/// `YYYY-MM-DD` into its three numbers.
fn parse_date(date: &str) -> Option<(i64, i64, i64)> {
    let mut parts = date.split('-');
    let year = parts.next()?.parse().ok()?;
    let month = parts.next()?.parse().ok()?;
    let day = parts.next()?.parse().ok()?;
    Some((year, month, day))
}

/// `HH:MM:SS`, and whatever follows the seconds, into seconds of the day.
fn parse_time(time: &str) -> Option<i64> {
    let mut parts = time.split(':');
    let hour: i64 = parts.next()?.parse().ok()?;
    let minute: i64 = parts.next()?.parse().ok()?;
    let whole_seconds = parts
        .next()?
        .split(|character: char| !character.is_ascii_digit())
        .next()?;
    let second: i64 = whole_seconds.parse().ok()?;
    Some(hour * 3_600 + minute * 60 + second)
}

/// Days from 1970-01-01 to `year-month-day`, by Howard Hinnant's
/// `days_from_civil`.
///
/// The trick is that the year starts in March, which puts the leap day at
/// the end of it: every other month length then falls out of one linear
/// formula, and the four-hundred-year era arithmetic below handles the
/// century rules without a table or a branch.
///
/// Euclidean division rather than Hinnant's `y >= 0 ? y : y - 399`, which
/// is what that expression is for — flooring instead of truncating. January
/// of the year 0 is enough to take the year below zero, and a branch there
/// would be one no stamp in a stream can reach through `/` alone.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = year - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let year_of_era = year.rem_euclid(400);
    let day_of_year = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// `MM:SS` under an hour, `H:MM:SS` at or above it.
///
/// The two shapes rather than one because a gate run is minutes and a
/// session is hours, and `00:14:32` in a column that is nearly always
/// `MM:SS` reads as a wider number rather than a longer wait.
#[must_use]
pub fn format_elapsed(seconds: u64) -> String {
    let (hours, minutes, seconds) = (seconds / 3_600, (seconds % 3_600) / 60, seconds % 60);
    if hours == 0 {
        format!("{minutes:02}:{seconds:02}")
    } else {
        format!("{hours}:{minutes:02}:{seconds:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::{Timestamp, format_elapsed};

    fn at(text: &str) -> Timestamp {
        Timestamp::parse(text).expect("the test's own timestamp did not parse")
    }

    #[test]
    fn the_epoch_is_where_the_epoch_is() {
        // The one absolute the board depends on: a parsed stamp is compared
        // with the machine's clock, so an offset that cancels between two
        // parsed stamps would still put every elapsed column decades out.
        assert_eq!(at("1970-01-01T00:00:00Z"), Timestamp::from_epoch_seconds(0));
        assert_eq!(
            at("1970-01-02T00:00:00Z"),
            Timestamp::from_epoch_seconds(86_400),
        );
        assert_eq!(
            at("2026-09-07T12:34:56.789Z"),
            Timestamp::from_epoch_seconds(1_788_784_496),
        );
    }

    #[test]
    fn the_leap_rules_are_the_calendars_and_not_a_multiple_of_four() {
        let day = 86_400;
        // A leap year has a 29th of February between the 28th and March.
        assert_eq!(
            at("2024-03-01T00:00:00Z").seconds_since(at("2024-02-28T00:00:00Z")),
            2 * day,
        );
        assert_eq!(
            at("2023-03-01T00:00:00Z").seconds_since(at("2023-02-28T00:00:00Z")),
            day,
        );
        // A century is not, unless it is a multiple of four hundred.
        assert_eq!(
            at("2000-03-01T00:00:00Z").seconds_since(at("2000-02-28T00:00:00Z")),
            2 * day,
        );
        assert_eq!(
            at("2100-03-01T00:00:00Z").seconds_since(at("2100-02-28T00:00:00Z")),
            day,
        );
        // And the formula's year-starts-in-March shift must not lose the
        // year boundary it steps over.
        assert_eq!(
            at("2026-01-01T00:00:00Z").seconds_since(at("2025-12-31T00:00:00Z")),
            day,
        );
        // January of the year 0 is the one date that takes the shifted year
        // below zero, where the era arithmetic has to floor rather than
        // truncate. No stream holds such a stamp; the formula is still
        // wrong if it cannot.
        assert_eq!(
            at("0000-02-01T00:00:00Z").seconds_since(at("0000-01-01T00:00:00Z")),
            31 * day,
        );
    }

    #[test]
    fn the_time_of_day_is_read_whatever_follows_the_seconds() {
        for stamp in [
            "2026-09-07T00:00:00Z",
            "2026-09-07T00:00:00.000Z",
            "2026-09-07T00:00:00",
        ] {
            assert_eq!(
                at("2026-09-07T01:02:03Z").seconds_since(at(stamp)),
                3_723,
                "{stamp}",
            );
        }
    }

    #[test]
    fn a_stamp_this_cannot_read_is_nothing_rather_than_a_guess() {
        for text in [
            "",
            "2026-09-07",
            "2026-09T00:00:00Z",
            "2026-09-07T00:00Z",
            "not-a-date T00:00:00Z",
            "2026-09-07Txx:00:00Z",
            "2026-09-07T00:xx:00Z",
            "2026-09-07T00:00:xxZ",
        ] {
            assert_eq!(
                Timestamp::parse(text),
                None,
                "{text} was read as an instant"
            );
        }
    }

    #[test]
    fn an_instant_in_the_future_is_no_time_at_all() {
        assert_eq!(
            at("2026-09-07T12:00:00Z").seconds_since(at("2026-09-07T12:00:05Z")),
            0,
        );
        assert_eq!(
            at("2026-09-07T12:00:05Z").seconds_since(at("2026-09-07T12:00:00Z")),
            5,
        );
    }

    #[test]
    fn now_is_the_machines_clock_and_not_the_epoch() {
        // Loose on purpose: what is being pinned is that the clock is read
        // at all, since a `now` stuck at the epoch would make every elapsed
        // column read the age of the Unix epoch instead.
        assert!(Timestamp::now() > at("2020-01-01T00:00:00Z"));
    }

    #[test]
    fn elapsed_shows_hours_only_once_there_are_hours() {
        assert_eq!(format_elapsed(0), "00:00");
        assert_eq!(format_elapsed(134), "02:14");
        assert_eq!(format_elapsed(3_599), "59:59");
        assert_eq!(format_elapsed(3_600), "1:00:00");
        assert_eq!(format_elapsed(3_725), "1:02:05");
        assert_eq!(format_elapsed(360_000), "100:00:00");
    }
}
