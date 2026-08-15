//! Refusing a release whose tag lies about what it ships.
//!
//! The tag must agree with VERSION, VERSION with the rules-file marker, and
//! the CHANGELOG must carry a section for it. All three are checked, and
//! every disagreement is reported: stopping at the first one sends someone
//! round the loop again for the next.

use crate::changelog::has_section;

/// The version recorded in the rules file, if it carries the marker.
#[must_use]
pub fn marker(rules: &str) -> Option<&str> {
    rules.lines().find_map(|line| {
        line.strip_prefix("<!-- keeler-version: ")
            .and_then(|rest| rest.strip_suffix(" -->"))
            .map(str::trim)
    })
}

/// Everything that disagrees about which version is being released. Empty
/// means tag, VERSION, marker and CHANGELOG all say the same thing.
#[must_use]
pub fn disagreements(tag: &str, version: &str, marker: &str, changelog: &str) -> Vec<String> {
    let mut found = Vec::new();
    if tag != format!("v{version}") {
        found.push(format!("tag {tag} disagrees with VERSION {version}"));
    }
    if marker != version {
        found.push(format!(
            "rules-file marker '{marker}' disagrees with VERSION {version}"
        ));
    }
    if !has_section(changelog, version) {
        found.push(format!("CHANGELOG.md has no section for {version}"));
    }
    found
}

#[cfg(test)]
mod tests {
    use super::{disagreements, marker};

    const RULES: &str = "<!-- keeler-version: 1.2.3 -->\n\n# Keeler\n";
    const CHANGELOG: &str = "# Changelog\n\n## [1.2.3] — 2026-01-01\n\n- shipped\n";

    #[test]
    fn a_consistent_release_has_nothing_to_say() {
        assert_eq!(
            disagreements("v1.2.3", "1.2.3", "1.2.3", CHANGELOG),
            Vec::<String>::new(),
        );
    }

    #[test]
    fn the_marker_is_read_out_of_the_rules_file() {
        assert_eq!(marker(RULES), Some("1.2.3"));
        assert_eq!(marker("# Keeler\n"), None);
    }

    #[test]
    fn a_tag_that_disagrees_with_version_is_named() {
        let found = disagreements("v9.9.9", "1.2.3", "1.2.3", CHANGELOG);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(
            found[0].contains("v9.9.9") && found[0].contains("1.2.3"),
            "{found:?}"
        );
    }

    #[test]
    fn a_marker_that_disagrees_with_version_is_named() {
        let found = disagreements("v1.2.3", "1.2.3", "0.0.1", CHANGELOG);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("0.0.1"), "{found:?}");
    }

    #[test]
    fn a_missing_changelog_section_is_named() {
        let found = disagreements("v1.2.3", "1.2.3", "1.2.3", "# Changelog\n");
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("CHANGELOG"), "{found:?}");
    }

    #[test]
    fn a_lookalike_heading_does_not_satisfy_the_guard() {
        // The dots in a version are not wildcards, and a heading for a
        // different version must not stand in for the missing one.
        let changelog = "# Changelog\n\n## [1.2.3-beta] — 2026-01-01\n\n- not it\n";
        let found = disagreements("v1.2.3", "1.2.3", "1.2.3", changelog);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("CHANGELOG"), "{found:?}");
    }

    #[test]
    fn every_disagreement_is_reported_not_just_the_first() {
        let found = disagreements("v9.9.9", "1.2.3", "0.0.1", "# Changelog\n");
        assert_eq!(found.len(), 3, "the guard stopped early: {found:?}");
    }

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig {
            cases: 256,
            failure_persistence: Some(Box::new(
                proptest::test_runner::FileFailurePersistence::WithSource("proptest-regressions"),
            )),
            ..proptest::prelude::ProptestConfig::default()
        })]

        /// Guard diagnosis: for any triple of tag, VERSION and marker, the
        /// report names every pair that disagrees — never just the first.
        /// A guard that stops early sends someone round the release loop
        /// once per mistake instead of once.
        #[test]
        fn every_mismatched_pair_is_named(
            tag_version in "[0-9]\\.[0-9]",
            version in "[0-9]\\.[0-9]",
            marker_version in "[0-9]\\.[0-9]",
            has_notes in proptest::bool::ANY,
        ) {
            let tag = format!("v{tag_version}");
            let changelog = if has_notes {
                format!("# Changelog\n\n## [{version}] — 2026-01-01\n\n- shipped\n")
            } else {
                "# Changelog\n".to_string()
            };

            let found = disagreements(&tag, &version, &marker_version, &changelog);
            let expected = usize::from(tag_version != version)
                + usize::from(marker_version != version)
                + usize::from(!has_notes);
            proptest::prop_assert_eq!(
                found.len(), expected,
                "tag={} version={} marker={} notes={} gave {:?}",
                tag, version, marker_version, has_notes, found,
            );
            if tag_version != version {
                proptest::prop_assert!(
                    found.iter().any(|f| f.contains(&tag)),
                    "the disagreeing tag is not named: {:?}", found,
                );
            }
            if marker_version != version {
                proptest::prop_assert!(
                    found.iter().any(|f| f.contains(&marker_version)),
                    "the disagreeing marker is not named: {:?}", found,
                );
            }
        }
    }
}
