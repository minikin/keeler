//! Reading one version's section out of a CHANGELOG.
//!
//! The release notes are that section's body and nothing else. A version
//! with no section is an error, never empty output — a release must not
//! ship with silent, blank notes.

use std::fmt;

/// The reason a version's notes could not be produced.
#[derive(Debug, PartialEq, Eq)]
pub enum NotesError {
    /// The CHANGELOG has no section for this version.
    NoSuchVersion(String),
}

impl fmt::Display for NotesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSuchVersion(version) => {
                write!(f, "no CHANGELOG section for version {version}")
            }
        }
    }
}

impl std::error::Error for NotesError {}

/// True for the heading that opens `version`'s section.
///
/// A literal prefix including the closing bracket, so `## [0.1.0] — date`
/// opens 0.1.0's section while `## [0.1.0-beta]` opens its own: the dots in
/// a version are not wildcards and a lookalike heading must not match.
fn opens_section(line: &str, version: &str) -> bool {
    line.starts_with(&format!("## [{version}]"))
}

/// A link-reference line, as Markdown collects at the foot of a file.
fn is_link_reference(line: &str) -> bool {
    line.starts_with('[')
        && line
            .split_once("]: ")
            .is_some_and(|(tag, _)| !tag.contains(']'))
}

/// True when the CHANGELOG carries a section for `version`.
#[must_use]
pub fn has_section(changelog: &str, version: &str) -> bool {
    changelog.lines().any(|line| opens_section(line, version))
}

/// The body of `version`'s section: everything between its heading and the
/// next `## ` heading, minus the trailing link-reference block and blank
/// lines at either edge.
///
/// # Errors
///
/// Returns [`NotesError::NoSuchVersion`] when the CHANGELOG has no section
/// for the version — an absent version is loud, never empty notes.
pub fn release_notes(changelog: &str, version: &str) -> Result<String, NotesError> {
    let mut body: Vec<&str> = Vec::new();
    let mut found = false;
    let mut inside = false;
    for line in changelog.lines() {
        if line.starts_with("## ") {
            inside = opens_section(line, version);
            found = found || inside;
            continue;
        }
        if inside {
            body.push(line);
        }
    }
    if !found {
        return Err(NotesError::NoSuchVersion(version.to_string()));
    }

    // Only the trailing link block is scenery, and only the last section can
    // have collected it — a link-style line inside the body is content.
    while body
        .last()
        .is_some_and(|line| line.trim().is_empty() || is_link_reference(line))
    {
        body.pop();
    }
    let start = body
        .iter()
        .position(|line| !line.trim().is_empty())
        .unwrap_or(body.len());

    Ok(body[start..].join("\n"))
}

#[cfg(test)]
mod tests {
    use super::{NotesError, release_notes};

    const FIXTURE: &str = "\
# Changelog

## [Unreleased]

- pending work

## [0.2.0] — 2026-02-01

### Added

- the second thing
- a line that looks like a link: [breaking]: the --foo flag was removed

## [0.1.0] — 2026-01-01

- the first thing

[Unreleased]: https://example.com/compare
[0.2.0]: https://example.com/0.2.0
[0.1.0]: https://example.com/0.1.0
";

    #[test]
    fn the_notes_are_exactly_the_versions_section() {
        let notes = release_notes(FIXTURE, "0.2.0").unwrap();
        assert_eq!(
            notes,
            "### Added\n\n- the second thing\n- a line that looks like a link: [breaking]: the --foo flag was removed",
        );
    }

    #[test]
    fn a_link_block_at_the_foot_is_not_part_of_the_last_section() {
        let notes = release_notes(FIXTURE, "0.1.0").unwrap();
        assert_eq!(notes, "- the first thing");
    }

    #[test]
    fn an_absent_version_is_an_error_not_empty_notes() {
        assert_eq!(
            release_notes(FIXTURE, "9.9.9"),
            Err(NotesError::NoSuchVersion("9.9.9".to_string())),
        );
    }

    #[test]
    fn a_lookalike_heading_opens_its_own_section() {
        let changelog = "## [0.1.0-beta]\n\n- the beta\n\n## [0.1.0]\n\n- the release\n";
        assert_eq!(release_notes(changelog, "0.1.0").unwrap(), "- the release");
        assert_eq!(
            release_notes(changelog, "0.1.0-beta").unwrap(),
            "- the beta",
        );
    }

    proptest::proptest! {
        // In-process, so the case count is no longer limited by the cost of a
        // subprocess per section the way it was when this ran against a script.
        #![proptest_config(proptest::prelude::ProptestConfig {
            cases: 256,
            failure_persistence: Some(Box::new(
                proptest::test_runner::FileFailurePersistence::WithSource("proptest-regressions"),
            )),
            ..proptest::prelude::ProptestConfig::default()
        })]

        /// Extraction totality: every entry line lands in exactly its own
        /// version's extraction — never another's, never dropped.
        #[test]
        fn every_changelog_entry_lands_in_exactly_one_extraction(
            entry_counts in proptest::collection::vec(1usize..4, 1..6),
        ) {
            use std::fmt::Write as _;

            // Given a CHANGELOG whose section Sᵢ holds entries unique to it
            let mut changelog = String::from("# Changelog\n\n## [Unreleased]\n\n- pending\n\n");
            let versions: Vec<String> =
                (0..entry_counts.len()).map(|i| format!("0.{i}.9")).collect();
            for (i, count) in entry_counts.iter().enumerate() {
                let _ = write!(changelog, "## [{}] — 2026-01-0{}\n\n", versions[i], i + 1);
                for j in 0..*count {
                    let _ = writeln!(changelog, "- s{i}-entry{j}");
                }
                changelog.push('\n');
            }
            for version in &versions {
                let _ = writeln!(changelog, "[{version}]: https://example.com/{version}");
            }

            // When each version is extracted, every entry appears in its own
            // extraction and in no other
            for (i, version) in versions.iter().enumerate() {
                let notes = release_notes(&changelog, version)
                    .expect("extraction failed for a version that is present");
                for (k, count) in entry_counts.iter().enumerate() {
                    for j in 0..*count {
                        let entry = format!("s{k}-entry{j}");
                        proptest::prop_assert_eq!(
                            notes.contains(&entry),
                            k == i,
                            "entry {} vs extraction of {}:\n{}", entry, version, notes,
                        );
                    }
                }
                proptest::prop_assert!(
                    !notes.contains("pending") && !notes.contains("https://example.com"),
                    "Unreleased or link lines leaked into {}:\n{}", version, notes,
                );
            }
        }
    }
}
