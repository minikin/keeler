//! Editing a project's `Cargo.toml` — the sections Keeler's gates need,
//! appended only where they are missing.
//!
//! The manifest is the project's own file, so nothing here rewrites it: the
//! sections are appended, and a manifest that already declares them is left
//! untouched to the byte.

/// What configuring a manifest did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Changes {
    /// Sections appended, by name.
    pub added: Vec<String>,
    /// What the project's owner has to do themselves, and why.
    pub notes: Vec<String>,
}

/// The mutants profile: cargo-mutants builds every mutant, and dropping
/// debug info is most of what keeps that bearable.
const PROFILE: &str = "\n[profile.mutants]\ninherits = \"dev\"\ndebug = 0\n";

/// Pedantic clippy, and lints that stop an `allow` from going unexplained.
const LINTS: &str = "\n[lints.clippy]\npedantic = { level = \"warn\", priority = -1 }\n\
                     allow_attributes = \"warn\"\nallow_attributes_without_reason = \"warn\"\n";

/// A dev-dependency on proptest, which the property tests need.
const PROPTEST: &str = "\n[dev-dependencies]\nproptest = \"1\"\n";

/// True when the manifest is a workspace root with no package of its own.
///
/// There is nothing to configure in such a manifest: a dev-dependency
/// belongs to a package, and the members are the project's to manage.
#[must_use]
pub fn is_workspace_root(manifest: &str) -> bool {
    has_section(manifest, "[workspace]") && !has_section(manifest, "[package]")
}

/// True when a section heading stands at the start of a line.
fn has_section(manifest: &str, heading: &str) -> bool {
    manifest.lines().any(|line| line.trim_end() == heading)
}

/// True when proptest is already a dependency, in either declaration form.
///
/// Both `proptest = "1"` under `[dev-dependencies]` and a
/// `[dev-dependencies.proptest]` table are real declarations; adding a
/// second would break the build. `proptest-derive` is neither — a prefix
/// match would skip a project that has only the derive macro.
#[must_use]
pub fn declares_proptest(manifest: &str) -> bool {
    manifest.lines().any(|line| {
        let line = line.trim();
        line == "[dev-dependencies.proptest]"
            || line
                .strip_prefix("proptest")
                .is_some_and(|rest| rest.trim_start().starts_with('='))
    })
}

/// The manifest with whatever it lacks appended, and a record of what that
/// was.
#[must_use]
pub fn configured(manifest: &str) -> (String, Changes) {
    let mut changes = Changes::default();
    if is_workspace_root(manifest) {
        changes.notes.push(
            "workspace root — add proptest and the mutants profile to the member crates yourself"
                .to_string(),
        );
        return (manifest.to_string(), changes);
    }

    let mut out = manifest.to_string();
    if !declares_proptest(manifest) {
        out.push_str(PROPTEST);
        changes.added.push("proptest (dev-dependency)".to_string());
    }
    for (heading, body) in [("[profile.mutants]", PROFILE), ("[lints.clippy]", LINTS)] {
        if !has_section(manifest, heading) {
            out.push_str(body);
            changes.added.push(heading.to_string());
        }
    }
    (out, changes)
}

#[cfg(test)]
mod tests {
    use super::{declares_proptest, is_workspace_root};

    #[test]
    fn a_root_that_is_also_a_package_is_not_a_workspace_root() {
        // This repository's own shape: a [workspace] table beside a
        // [package]. Both conditions have to hold, or a root package would
        // be told to configure itself and then never configured.
        let both = "[workspace]\nmembers = [\"x\"]\n\n[package]\nname = \"a\"\n";
        assert!(!is_workspace_root(both));
        assert!(is_workspace_root("[workspace]\nmembers = [\"x\"]\n"));
        assert!(!is_workspace_root("[package]\nname = \"a\"\n"));
    }

    #[test]
    fn a_heading_inside_a_string_is_not_a_section() {
        // The match is on a whole line, so a heading mentioned in prose or
        // a value does not count as declaring the section.
        assert!(!is_workspace_root(
            "description = \"see [workspace] docs\"\n"
        ));
    }

    #[test]
    fn both_declaration_forms_count_and_the_derive_macro_does_not() {
        assert!(declares_proptest("[dev-dependencies]\nproptest = \"1\"\n"));
        assert!(declares_proptest(
            "[dev-dependencies.proptest]\nversion = \"1\"\n"
        ));
        assert!(!declares_proptest(
            "[dev-dependencies]\nproptest-derive = \"0.5\"\n"
        ));
        assert!(!declares_proptest("[dev-dependencies]\n"));
    }
}
