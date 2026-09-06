//! Holding the plugin's manifests and its rules to VERSION.
//!
//! Both failures this refuses are silent ones. `/plugin update` compares
//! `plugin.json`'s `version`: a release that forgets to bump it ships
//! nothing to everyone who asks for the update, and nothing anywhere says
//! so. The `SessionStart` hook is the same shape — Claude Code files away
//! output over its cap and hands the agent a preview, so rules that outgrew
//! it arrive partially and without a word. Neither shows up in a test run,
//! a lint or a tag; they show up in someone else's session, weeks later.
//!
//! Every disagreement is reported, never just the first, for the reason
//! `guard::disagreements` reports all of its: one trip round the release
//! loop should fix all of them.

/// The name the plugin is installed by — `keeler@keeler`, whose second half
/// is the marketplace and whose first is this.
pub const NAME: &str = "keeler";

/// The manifest Claude Code reads to install the plugin, and the one
/// `/plugin update` compares versions with.
pub const MANIFEST: &str = ".claude-plugin/plugin.json";

/// The marketplace: this repository lists its own plugin.
pub const MARKETPLACE: &str = ".claude-plugin/marketplace.json";

/// The rules the `SessionStart` hook prints, at the plugin root — not
/// `.claude/keeler.md`, which is where they lived when they were installed
/// into someone else's repository.
pub const RULES: &str = "keeler.md";

/// The ceiling the rules must fit under, in bytes.
///
/// The hook's output is capped at 10,000 characters. Bytes are never fewer
/// than characters, so a file under this many bytes is under the cap
/// whichever unit the cap turns out to count — and the 500 left over is
/// room for the rule someone adds tomorrow, which fails here loudly
/// instead of truncating there quietly.
pub const CEILING: usize = 9_500;

/// The version the plugin manifest declares.
///
/// The first `"version"` in the file: a plugin manifest declares one, at
/// the top level, and the format nests none — unlike a `Cargo.toml`, where
/// the first `version =` is usually a dependency's.
#[must_use]
pub fn plugin_version(json: &str) -> Option<&str> {
    field(json, "version")
}

/// The version the marketplace declares for one plugin: the `version` of
/// the entry in `plugins` whose `name` is the one asked for.
///
/// `None` when the file lists no such entry, or lists it without a version
/// — both leave `/plugin update` with nothing to compare, which is the
/// failure this gate is here for.
#[must_use]
pub fn marketplace_version<'a>(json: &'a str, name: &str) -> Option<&'a str> {
    entries(json, "plugins")
        .into_iter()
        .find(|entry| field(entry, "name") == Some(name))
        .and_then(|entry| field(entry, "version"))
}

/// The marker's quarrel with VERSION, if it has one.
///
/// Its own function because two gates report it — the release guard and
/// this check, which the guard also runs — and a reader who sees it twice
/// goes looking for a second file to fix. There is one.
#[must_use]
pub fn marker_disagreement(marker: &str, version: &str) -> Option<String> {
    (marker != version)
        .then(|| format!("{RULES}'s marker '{marker}' disagrees with VERSION {version}"))
}

/// Everything the plugin's own files disagree with VERSION about: the two
/// manifests, and the size of the rules the hook has to carry.
///
/// The rules marker is not among them — `marker_disagreement` is its own
/// call, so the release guard, which already reports it, does not say it
/// twice.
#[must_use]
pub fn disagreements(version: &str, manifest: &str, marketplace: &str, rules: &str) -> Vec<String> {
    let mut found = Vec::new();
    found.extend(claim(MANIFEST, plugin_version(manifest), version));
    found.extend(claim(
        MARKETPLACE,
        marketplace_version(marketplace, NAME),
        version,
    ));
    if rules.len() > CEILING {
        found.push(format!(
            "{RULES} is {} bytes, over the {CEILING}-byte ceiling the SessionStart hook \
             truncates above",
            rules.len(),
        ));
    }
    found
}

/// One manifest's claim about the version, compared with VERSION.
///
/// A manifest declaring none is a disagreement too, and not silence: the
/// field is what `/plugin update` resolves a version from, so a file it
/// cannot find one in updates nobody.
fn claim(path: &str, declared: Option<&str>, version: &str) -> Option<String> {
    match declared {
        Some(declared) if declared == version => None,
        Some(declared) => Some(format!(
            "{path} declares version {declared}, VERSION says {version}"
        )),
        None => Some(format!(
            "{path} declares no version, VERSION says {version}"
        )),
    }
}

/// The value of `"key": "value"`, first occurrence — enough JSON for two
/// manifests Keeler writes by hand, where a name and a version each appear
/// once per object. A value that is not a string is no answer: this reads
/// versions and names, and both are strings.
fn field<'a>(json: &'a str, key: &str) -> Option<&'a str> {
    let (_, after) = json.split_once(&format!("\"{key}\""))?;
    let (_, after) = after.split_once(':')?;
    let value = after.trim_start().strip_prefix('"')?;
    let (value, _) = value.split_once('"')?;
    Some(value)
}

/// The objects of a JSON array field, by brace matching: one string per
/// entry, so a field read inside one cannot come from its neighbour.
fn entries<'a>(json: &'a str, key: &str) -> Vec<&'a str> {
    let Some((_, after)) = json.split_once(&format!("\"{key}\"")) else {
        return Vec::new();
    };
    let mut objects = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (at, character) in after.char_indices() {
        match character {
            '{' => {
                if depth == 0 {
                    start = at;
                }
                depth += 1;
            }
            '}' if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    objects.push(&after[start..=at]);
                }
            }
            // The array ends where the entries stop being ours; anything
            // after it belongs to another field.
            ']' if depth == 0 => break,
            _ => {}
        }
    }
    objects
}

#[cfg(test)]
mod tests {
    use super::{
        CEILING, MANIFEST, MARKETPLACE, NAME, RULES, disagreements, marker_disagreement,
        marketplace_version, plugin_version,
    };

    fn manifest(version: &str) -> String {
        format!("{{\n  \"name\": \"keeler\",\n  \"version\": \"{version}\"\n}}\n")
    }

    fn marketplace(version: &str) -> String {
        format!(
            "{{\n  \"name\": \"keeler\",\n  \"owner\": {{ \"name\": \"minikin\" }},\n  \
             \"plugins\": [\n    {{ \"name\": \"keeler\", \"source\": \"./\", \
             \"version\": \"{version}\" }}\n  ]\n}}\n"
        )
    }

    /// Rules of exactly `bytes` bytes, marker included.
    fn rules(bytes: usize) -> String {
        let head = "<!-- keeler-version: 1.2.3 -->\n";
        format!("{head}{}", "x".repeat(bytes - head.len()))
    }

    #[test]
    fn the_plugin_manifests_own_version_is_read() {
        assert_eq!(plugin_version(&manifest("1.2.3")), Some("1.2.3"));
        assert_eq!(plugin_version("{ \"name\": \"keeler\" }"), None);
    }

    #[test]
    fn a_version_that_is_not_a_string_is_not_read_as_one() {
        // `"version": 5` is not a version this gate can compare, and the
        // next quoted thing in the file is some other field's value —
        // comparing that would refuse a release over a number nobody wrote.
        assert_eq!(
            plugin_version("{ \"version\": 5, \"name\": \"keeler\" }"),
            None,
        );
    }

    #[test]
    fn the_marketplace_entry_is_found_by_the_name_it_installs_under() {
        assert_eq!(
            marketplace_version(&marketplace("1.2.3"), NAME),
            Some("1.2.3"),
        );
        // And the owner block's `name` is not a plugin's: read as one, a
        // marketplace whose owner happens to share the plugin's name would
        // answer with whatever field followed it.
        assert_eq!(marketplace_version(&marketplace("1.2.3"), "minikin"), None);
    }

    #[test]
    fn a_marketplace_that_lists_the_plugin_without_a_version_declares_none() {
        let json = "{ \"plugins\": [ { \"name\": \"keeler\", \"source\": \"./\" } ] }";
        assert_eq!(marketplace_version(json, NAME), None);
        assert_eq!(marketplace_version("{ \"name\": \"keeler\" }", NAME), None);
    }

    #[test]
    fn the_entry_asked_for_is_the_one_read() {
        // A marketplace may list more than one plugin, and the version of
        // the entry before the right one is a different claim entirely.
        let json = "{ \"plugins\": [ { \"name\": \"other\", \"version\": \"0.0.1\" }, \
                    { \"name\": \"keeler\", \"version\": \"1.2.3\" } ] }";
        assert_eq!(marketplace_version(json, NAME), Some("1.2.3"));
    }

    #[test]
    fn rules_at_the_ceiling_fit_and_one_byte_over_does_not() {
        let version = "1.2.3";
        assert_eq!(
            disagreements(
                version,
                &manifest(version),
                &marketplace(version),
                &rules(CEILING)
            ),
            Vec::<String>::new(),
            "rules of exactly the ceiling were refused",
        );

        let found = disagreements(
            version,
            &manifest(version),
            &marketplace(version),
            &rules(CEILING + 1),
        );
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(
            found[0].contains(RULES) && found[0].contains("9501") && found[0].contains("9500"),
            "the refusal does not name the file, its size and the ceiling: {found:?}",
        );
    }

    #[test]
    fn a_manifest_left_behind_is_named_with_both_versions() {
        let found = disagreements("0.5.0", &manifest("0.4.1"), &marketplace("0.5.0"), "");
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(
            found[0].contains(MANIFEST) && found[0].contains("0.4.1") && found[0].contains("0.5.0"),
            "{found:?}",
        );
    }

    #[test]
    fn a_marketplace_entry_left_behind_is_named_with_both_versions() {
        let found = disagreements("0.5.0", &manifest("0.5.0"), &marketplace("0.4.1"), "");
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(
            found[0].contains(MARKETPLACE)
                && found[0].contains("0.4.1")
                && found[0].contains("0.5.0"),
            "{found:?}",
        );
    }

    #[test]
    fn a_manifest_declaring_no_version_at_all_is_refused() {
        // Not "it agrees": there is nothing there to agree, and
        // `/plugin update` reads that field or updates nobody.
        let found = disagreements("0.5.0", "{}", "{}", "");
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(found.iter().all(|f| f.contains("no version")), "{found:?}");
    }

    #[test]
    fn a_marker_that_agrees_says_nothing_and_one_that_does_not_names_its_file() {
        assert_eq!(marker_disagreement("1.2.3", "1.2.3"), None);
        let found = marker_disagreement("0.4.1", "0.5.0").expect("a stale marker passed");
        assert!(
            found.contains(RULES) && found.contains("0.4.1") && found.contains("0.5.0"),
            "the refusal does not name the file and both versions: {found}",
        );
    }

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig {
            cases: 256,
            failure_persistence: Some(Box::new(
                proptest::test_runner::FileFailurePersistence::WithSource("proptest-regressions"),
            )),
            ..proptest::prelude::ProptestConfig::default()
        })]

        /// For any pair of declared versions and either side of the
        /// ceiling, every source that disagrees is named — and named once.
        /// A report that stopped early sends someone round the release loop
        /// per mistake; one that named a file twice sends them looking for
        /// a second copy of it.
        #[test]
        fn every_disagreeing_source_is_named_exactly_once(
            version in "[0-9]\\.[0-9]",
            declared in "[0-9]\\.[0-9]",
            listed in "[0-9]\\.[0-9]",
            over in proptest::bool::ANY,
        ) {
            let rules = rules(if over { CEILING + 1 } else { CEILING });
            let found = disagreements(&version, &manifest(&declared), &marketplace(&listed), &rules);

            for (source, disagrees) in [
                (MANIFEST, declared != version),
                (MARKETPLACE, listed != version),
                (RULES, over),
            ] {
                let named = found.iter().filter(|f| f.contains(source)).count();
                proptest::prop_assert_eq!(
                    named, usize::from(disagrees),
                    "{} named {} time(s) in {:?}", source, named, found,
                );
            }
            proptest::prop_assert_eq!(
                found.len(),
                usize::from(declared != version)
                    + usize::from(listed != version)
                    + usize::from(over),
                "the report says something else as well: {:?}", found,
            );
        }
    }
}
