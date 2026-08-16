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
    /// The section is there but says nothing. A release must not ship with
    /// silent, blank notes, and `create` cannot be repaired afterwards.
    EmptySection(String),
}

impl fmt::Display for NotesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSuchVersion(version) => {
                write!(f, "no CHANGELOG section for version {version}")
            }
            Self::EmptySection(version) => write!(
                f,
                "the CHANGELOG section for {version} is empty — \
                 the entries are probably still under [Unreleased]"
            ),
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

/// True when the CHANGELOG carries a section for `version`, and that
/// section says something.
///
/// A heading is only a heading outside a fenced code block: a CHANGELOG
/// for a tool that documents markdown will quote one, and treating the
/// quote as a boundary invents sections and truncates notes.
#[must_use]
pub fn has_section(changelog: &str, version: &str) -> bool {
    release_notes(changelog, version).is_ok()
}

/// Whether a line opens or closes a fenced code block.
fn is_fence(line: &str) -> bool {
    let line = line.trim_start();
    line.starts_with("```") || line.starts_with("~~~")
}

/// Every line between `version`'s heading and the next one, or `None`
/// when there is no such heading.
///
/// A heading is only a heading outside a fenced code block. Boundary
/// finding is separated from the trimming below so that neither has to be
/// read while thinking about the other.
fn section_lines<'a>(changelog: &'a str, version: &str) -> Option<Vec<&'a str>> {
    let mut body = Vec::new();
    let mut found = false;
    let mut inside = false;
    let mut fenced = false;
    for line in changelog.lines() {
        if is_fence(line) {
            fenced = !fenced;
        } else if !fenced && line.starts_with("## ") {
            inside = opens_section(line, version);
            found = found || inside;
            continue;
        }
        if inside {
            body.push(line);
        }
    }
    found.then_some(body)
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
    let Some(body) = section_lines(changelog, version) else {
        return Err(NotesError::NoSuchVersion(version.to_string()));
    };

    let mut body = body;
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

    if start >= body.len() {
        return Err(NotesError::EmptySection(version.to_string()));
    }
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
    fn a_present_but_empty_section_is_an_error_too() {
        // The heading gets added in the release-prep commit while the
        // entries stay under [Unreleased] — a half-finished edit, and the
        // exact shape the checklist invites. `has_section` is satisfied,
        // so the guard passes, the tag goes up, and `gh release create`
        // publishes a release with no notes. The workflow is create-only
        // by policy, so nothing can repair it afterwards.
        let changelog = "# Changelog\n\n## [1.0.0] — 2026-01-01\n\n## [0.9.0]\n\n- old\n";
        assert!(release_notes(changelog, "1.0.0").is_err());
    }

    #[test]
    fn an_empty_section_says_where_the_entries_probably_are() {
        // This message is read at release time by someone who is about to
        // tag. Naming the likely cause turns a refusal into an
        // instruction.
        let changelog = "## [1.0.0]\n\n## [0.9.0]\n\n- old\n";
        let said = release_notes(changelog, "1.0.0").unwrap_err().to_string();
        assert!(said.contains("1.0.0"), "{said}");
        assert!(said.contains("Unreleased"), "{said}");
    }

    #[test]
    fn a_section_holding_only_links_is_empty_too() {
        let changelog = "## [1.0.0]\n\n[1.0.0]: https://example.com/1.0.0\n";
        assert!(release_notes(changelog, "1.0.0").is_err());
    }

    #[test]
    fn a_heading_inside_a_code_fence_does_not_end_the_section() {
        // A CHANGELOG for a tool that documents markdown will quote
        // headings. Treating one as a boundary truncates the notes
        // silently — short notes look like short notes.
        let changelog = "\
## [1.0.0]

- the first thing

```markdown
## [0.0.0]
- an example
```

- the second thing, after the fence

## [0.9.0]

- old
";
        let notes = release_notes(changelog, "1.0.0").unwrap();
        assert!(
            notes.contains("the second thing"),
            "the notes were truncated:\n{notes}"
        );
        assert!(
            release_notes(changelog, "0.0.0").is_err(),
            "a quoted heading became a section"
        );
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
