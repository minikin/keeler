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

/// The version a manifest declares for its own package.
///
/// `None` when there is no `[package]` section (a virtual workspace root)
/// or when the version is inherited from the workspace — an inherited
/// version is not an independent claim and cannot disagree with anything.
/// Scoped to the `[package]` table on purpose: the first `version =` in a
/// manifest often belongs to a dependency.
#[must_use]
pub fn package_version(manifest: &str) -> Option<&str> {
    let mut in_package = false;
    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_package = line == "[package]";
            continue;
        }
        if !in_package {
            continue;
        }
        if let Some(rest) = line.strip_prefix("version") {
            let rest = rest.trim_start();
            let Some(value) = rest.strip_prefix('=') else {
                continue; // `version.workspace = true` and friends
            };
            return Some(unquote(value));
        }
    }
    None
}

/// The workspace members a root manifest lists, in order.
///
/// A deliberately small parse: the `members = [...]` array inside
/// `[workspace]`, which is all this repository has and all the guard needs.
/// Anything it cannot read it reports as no members rather than guessing.
#[must_use]
pub fn workspace_members(manifest: &str) -> Vec<String> {
    let mut in_workspace = false;
    let mut collecting = false;
    let mut members = Vec::new();
    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_workspace = line == "[workspace]";
            continue;
        }
        if !in_workspace {
            continue;
        }
        let rest = if collecting {
            line
        } else if let Some(rest) = line.strip_prefix("members") {
            let Some(rest) = rest.trim_start().strip_prefix('=') else {
                continue;
            };
            collecting = true;
            rest.trim_start().trim_start_matches('[')
        } else {
            continue;
        };
        for entry in rest.split(',') {
            let entry = entry.trim().trim_end_matches(']').trim();
            let entry = entry.trim_matches('"');
            if !entry.is_empty() {
                members.push(entry.to_string());
            }
        }
        if rest.contains(']') {
            break;
        }
    }
    members
}

/// A TOML string value: the quotes off, and any trailing comment with
/// them. Either quote character is valid TOML.
fn unquote(value: &str) -> &str {
    let value = value.trim();
    let quote = value.chars().next().filter(|c| *c == '"' || *c == '\'');
    match quote {
        Some(quote) => value[1..].split(quote).next().unwrap_or(value),
        None => value.split('#').next().unwrap_or(value).trim(),
    }
}

/// The version a workspace declares for the members that inherit it.
///
/// `version.workspace = true` is not an independent claim, but this is —
/// and reading neither meant the guard could check nothing while
/// reporting that everything agreed.
#[must_use]
pub fn workspace_version(manifest: &str) -> Option<&str> {
    let mut in_workspace_package = false;
    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_workspace_package = line == "[workspace.package]";
            continue;
        }
        if let Some(value) = line
            .strip_prefix("version")
            .filter(|_| in_workspace_package)
            .and_then(|rest| rest.trim_start().strip_prefix('='))
        {
            return Some(unquote(value));
        }
    }
    None
}

/// Everything that disagrees about which version is being released. Empty
/// means tag, VERSION, marker, CHANGELOG and every manifest say the same
/// thing. `manifests` is `(path, declared version)` per package.
#[must_use]
pub fn disagreements(
    tag: &str,
    version: &str,
    marker: &str,
    changelog: &str,
    manifests: &[(String, String)],
) -> Vec<String> {
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
    for (path, declared) in manifests {
        if declared != version {
            found.push(format!(
                "{path} declares version {declared}, VERSION says {version}"
            ));
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::{disagreements, marker, package_version, workspace_version};

    const RULES: &str = "<!-- keeler-version: 1.2.3 -->\n\n# Keeler\n";
    const CHANGELOG: &str = "# Changelog\n\n## [1.2.3] — 2026-01-01\n\n- shipped\n";

    #[test]
    fn a_comment_beside_the_version_is_not_part_of_it() {
        // `version = "0.3.0"  # keep in sync with VERSION` is a comment
        // this repository's own style invites. Reading it as part of the
        // version turned a truthful release into a refusal whose message
        // read like a mismatch, sending the reader after drift that was
        // not there.
        assert_eq!(
            package_version("[package]\nversion = \"1.2.3\"  # keep in sync\n"),
            Some("1.2.3"),
        );
        assert_eq!(
            package_version("[package]\nversion = '1.2.3'\n"),
            Some("1.2.3")
        );
    }

    #[test]
    fn a_manifest_version_is_read_from_its_package_section() {
        let manifest = "[package]\nname = \"x\"\nversion = \"1.2.3\"\n\n\
                        [dependencies]\nserde = { version = \"1.0\" }\n";
        assert_eq!(package_version(manifest), Some("1.2.3"));
    }

    #[test]
    fn a_dependency_version_is_not_mistaken_for_the_packages() {
        // The first `version =` in the file belongs to a dependency table,
        // not to [package] — reading it would compare the wrong number.
        let manifest = "[dependencies]\nsha2 = \"0.10\"\n\n\
                        [package]\nname = \"x\"\nversion = \"9.9.9\"\n";
        assert_eq!(package_version(manifest), Some("9.9.9"));
    }

    #[test]
    fn an_inherited_version_is_not_a_disagreement() {
        // `version.workspace = true` defers to the workspace, so there is
        // no independent number here to disagree with anything.
        let manifest = "[package]\nname = \"x\"\nversion.workspace = true\n";
        assert_eq!(package_version(manifest), None);
    }

    #[test]
    fn a_virtual_manifest_has_no_package_version() {
        assert_eq!(package_version("[workspace]\nmembers = [\"a\"]\n"), None);
    }

    #[test]
    fn a_workspace_inherited_version_is_the_workspaces_to_declare() {
        // `version.workspace = true` is not an independent claim, but
        // [workspace.package] version is — and reading neither meant the
        // guard checked nothing while reporting success.
        let root = "[workspace]\nmembers = [\"m\"]\n\n\
                    [workspace.package]\nversion = \"1.2.3\"\n\n\
                    [package]\nname = \"root\"\nversion.workspace = true\n";
        assert_eq!(workspace_version(root), Some("1.2.3"));
        assert_eq!(workspace_version("[workspace]\nmembers = []\n"), None);
    }

    #[test]
    fn a_manifest_that_disagrees_with_version_is_named() {
        let manifests = [("Cargo.toml".to_string(), "0.1.0".to_string())];
        let found = disagreements("v1.2.3", "1.2.3", "1.2.3", CHANGELOG, &manifests);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(
            found[0].contains("Cargo.toml")
                && found[0].contains("0.1.0")
                && found[0].contains("1.2.3"),
            "the refusal does not name the manifest and both versions: {found:?}",
        );
    }

    #[test]
    fn every_manifest_is_checked_not_only_the_first() {
        let manifests = [
            ("Cargo.toml".to_string(), "1.2.3".to_string()),
            ("xtask/Cargo.toml".to_string(), "0.0.1".to_string()),
        ];
        let found = disagreements("v1.2.3", "1.2.3", "1.2.3", CHANGELOG, &manifests);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("xtask/Cargo.toml"), "{found:?}");
    }

    #[test]
    fn agreeing_manifests_say_nothing() {
        let manifests = [
            ("Cargo.toml".to_string(), "1.2.3".to_string()),
            ("xtask/Cargo.toml".to_string(), "1.2.3".to_string()),
        ];
        assert_eq!(
            disagreements("v1.2.3", "1.2.3", "1.2.3", CHANGELOG, &manifests),
            Vec::<String>::new(),
        );
    }

    #[test]
    fn a_consistent_release_has_nothing_to_say() {
        assert_eq!(
            disagreements("v1.2.3", "1.2.3", "1.2.3", CHANGELOG, &[]),
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
        let found = disagreements("v9.9.9", "1.2.3", "1.2.3", CHANGELOG, &[]);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(
            found[0].contains("v9.9.9") && found[0].contains("1.2.3"),
            "{found:?}"
        );
    }

    #[test]
    fn a_marker_that_disagrees_with_version_is_named() {
        let found = disagreements("v1.2.3", "1.2.3", "0.0.1", CHANGELOG, &[]);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("0.0.1"), "{found:?}");
    }

    #[test]
    fn a_missing_changelog_section_is_named() {
        let found = disagreements("v1.2.3", "1.2.3", "1.2.3", "# Changelog\n", &[]);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("CHANGELOG"), "{found:?}");
    }

    #[test]
    fn a_lookalike_heading_does_not_satisfy_the_guard() {
        // The dots in a version are not wildcards, and a heading for a
        // different version must not stand in for the missing one.
        let changelog = "# Changelog\n\n## [1.2.3-beta] — 2026-01-01\n\n- not it\n";
        let found = disagreements("v1.2.3", "1.2.3", "1.2.3", changelog, &[]);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("CHANGELOG"), "{found:?}");
    }

    #[test]
    fn every_disagreement_is_reported_not_just_the_first() {
        let found = disagreements("v9.9.9", "1.2.3", "0.0.1", "# Changelog\n", &[]);
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

            let found = disagreements(&tag, &version, &marker_version, &changelog, &[]);
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
