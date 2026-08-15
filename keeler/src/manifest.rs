//! Editing a project's `Cargo.toml` — the sections Keeler's gates need,
//! added only where they are missing.
//!
//! The manifest is the project's own file, so this is an edit and not a
//! rewrite: `toml_edit` keeps their formatting, their comments and their
//! key order, and every decision is made against the parsed document
//! rather than against the text. Appending sections as text was how this
//! produced manifests cargo refuses to read — a second
//! `[dev-dependencies]` table, a `[lints.clippy]` beside inherited lints.

use toml_edit::{DocumentMut, Item, Table, value};

/// What configuring a manifest did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Changes {
    /// Sections and dependencies added, by name.
    pub added: Vec<String>,
    /// What the project's owner has to do themselves, and why.
    pub notes: Vec<String>,
}

/// True when the manifest is a workspace root with no package of its own.
///
/// There is nothing to configure in such a manifest: a dev-dependency
/// belongs to a package, and the members are the project's to manage.
#[must_use]
pub fn is_workspace_root(document: &DocumentMut) -> bool {
    document.contains_key("workspace") && !document.contains_key("package")
}

/// True when proptest is already a dependency, however it is declared.
///
/// All of `proptest = "1"`, `proptest.workspace = true` and a
/// `[dev-dependencies.proptest]` table are real declarations, and adding a
/// second breaks the build. Only `[dev-dependencies]` is consulted: a
/// feature or a patch entry of the same name is not the crate, and
/// `proptest-derive` is a different key entirely.
#[must_use]
pub fn declares_proptest(document: &DocumentMut) -> bool {
    document
        .get("dev-dependencies")
        .and_then(Item::as_table_like)
        .is_some_and(|table| table.contains_key("proptest"))
}

/// True when the manifest already decides its own clippy lints, or takes
/// them from the workspace.
///
/// Adding `[lints.clippy]` beside `lints.workspace = true` is not a merge:
/// cargo refuses to let a member override inherited lints at all.
#[must_use]
pub fn decides_its_own_lints(document: &DocumentMut) -> bool {
    document
        .get("lints")
        .and_then(Item::as_table_like)
        .is_some_and(|lints| lints.contains_key("clippy") || lints.contains_key("workspace"))
}

/// The manifest with whatever it lacks added, and a record of what that
/// was.
///
/// # Errors
///
/// Returns the parse failure if the manifest is not valid TOML — Keeler
/// will not guess at a file it cannot read.
pub fn configured(manifest: &str) -> Result<(String, Changes), toml_edit::TomlError> {
    let mut document: DocumentMut = manifest.parse()?;
    let mut changes = Changes::default();

    if is_workspace_root(&document) {
        changes.notes.push(
            "workspace root — the mutants profile belongs here, and proptest in each member \
             crate; add them yourself"
                .to_string(),
        );
        return Ok((document.to_string(), changes));
    }

    if !declares_proptest(&document) {
        let table = document["dev-dependencies"].or_insert(Item::Table(Table::new()));
        if let Some(table) = table.as_table_mut() {
            table.set_implicit(false);
        }
        document["dev-dependencies"]["proptest"] = value("1");
        changes.added.push("proptest (dev-dependency)".to_string());
    }

    // cargo-mutants builds every mutant, and dropping debug info is most
    // of what keeps that bearable. Cargo ignores a profile declared in a
    // member crate, so this goes only where it will actually be read.
    if !declares_mutants_profile(&document) {
        let mut mutants = Table::new();
        mutants["inherits"] = value("dev");
        mutants["debug"] = value(0);
        sub_table(&mut document, "profile", "mutants", mutants);
        changes.added.push("[profile.mutants]".to_string());
    }

    if !decides_its_own_lints(&document) {
        let mut clippy = Table::new();
        clippy["pedantic"] = pedantic();
        clippy["allow_attributes"] = value("warn");
        clippy["allow_attributes_without_reason"] = value("warn");
        sub_table(&mut document, "lints", "clippy", clippy);
        changes.added.push("[lints.clippy]".to_string());
    }

    Ok((document.to_string(), changes))
}

/// True when a mutants profile is already declared, or would be ignored
/// here because this manifest is a member rather than a workspace root.
fn declares_mutants_profile(document: &DocumentMut) -> bool {
    document.contains_key("workspace")
        || document
            .get("profile")
            .and_then(Item::as_table_like)
            .is_some_and(|profile| profile.contains_key("mutants"))
}

/// Puts `child` at `[parent.child]`, as a section rather than an inline
/// table, leaving whatever else `parent` holds alone.
fn sub_table(document: &mut DocumentMut, parent: &str, child: &str, table: Table) {
    let entry = document
        .entry(parent)
        .or_insert_with(|| Item::Table(Table::new()));
    if let Some(parent_table) = entry.as_table_mut() {
        // Implicit: `[profile]` is not written out on its own, only
        // `[profile.mutants]` — which is what a hand-written manifest looks
        // like, and this file is the project's to read.
        parent_table.set_implicit(true);
        parent_table.insert(child, Item::Table(table));
    }
}

/// `{ level = "warn", priority = -1 }` — pedantic as a group, at a
/// priority that lets individual lints override it.
fn pedantic() -> Item {
    let mut inline = toml_edit::InlineTable::new();
    inline.insert("level", "warn".into());
    inline.insert("priority", (-1_i64).into());
    value(inline)
}

#[cfg(test)]
mod tests {
    use super::{configured, decides_its_own_lints, declares_proptest, is_workspace_root};

    fn document(text: &str) -> toml_edit::DocumentMut {
        text.parse().unwrap()
    }

    #[test]
    fn a_root_that_is_also_a_package_is_not_a_workspace_root() {
        // This repository's own shape. Both conditions have to hold, or a
        // root package would be told to configure itself and never be.
        assert!(!is_workspace_root(&document(
            "[workspace]\nmembers = [\"x\"]\n\n[package]\nname = \"a\"\n"
        )));
        assert!(is_workspace_root(&document(
            "[workspace]\nmembers = [\"x\"]\n"
        )));
        assert!(!is_workspace_root(&document("[package]\nname = \"a\"\n")));
    }

    #[test]
    fn every_declaration_form_counts_and_lookalikes_do_not() {
        for declared in [
            "[dev-dependencies]\nproptest = \"1\"\n",
            "[dev-dependencies]\nproptest.workspace = true\n",
            "[dev-dependencies.proptest]\nversion = \"1\"\n",
        ] {
            assert!(declares_proptest(&document(declared)), "{declared}");
        }
        for not_declared in [
            "[dev-dependencies]\nproptest-derive = \"0.5\"\n",
            "[features]\nproptest = []\n",
            "[dependencies]\nproptest = \"1\"\n",
            "",
        ] {
            assert!(
                !declares_proptest(&document(not_declared)),
                "{not_declared}"
            );
        }
    }

    #[test]
    fn lints_are_left_alone_when_the_project_already_decides_them() {
        assert!(decides_its_own_lints(&document(
            "[lints]\nworkspace = true\n"
        )));
        assert!(decides_its_own_lints(&document(
            "[lints]\nclippy.pedantic = \"warn\"\n"
        )));
        assert!(!decides_its_own_lints(&document(
            "[lints]\nrust.unsafe_code = \"forbid\"\n"
        )));
        assert!(!decides_its_own_lints(&document("")));
    }

    #[test]
    fn a_manifest_that_is_not_toml_is_refused_rather_than_guessed_at() {
        let error = configured("[package\nname = ").unwrap_err().to_string();
        assert!(!error.is_empty());
    }

    #[test]
    fn pedantic_is_a_group_that_individual_lints_can_override() {
        // The priority is not decoration: at -1 the group sits below
        // single lints, so a project can allow one of them without
        // turning the whole group off. At the default it cannot.
        let (out, _) = configured("[package]\nname = \"a\"\n").unwrap();
        assert!(
            out.contains(r#"pedantic = { level = "warn", priority = -1 }"#),
            "the pedantic group is not what was intended:\n{out}"
        );
    }

    #[test]
    fn the_mutants_profile_is_written_as_a_section() {
        let (out, _) = configured("[package]\nname = \"a\"\n").unwrap();
        assert!(out.contains("[profile.mutants]"), "{out}");
        assert!(out.contains("inherits = \"dev\""), "{out}");
        assert!(out.contains("debug = 0"), "{out}");
    }

    #[test]
    fn their_comments_and_formatting_survive() {
        let theirs = "# our crate\n[package]\nname = \"a\"   # why\nversion = \"0.1.0\"\n";
        let (out, _) = configured(theirs).unwrap();
        assert!(out.starts_with("# our crate\n"), "{out}");
        assert!(out.contains("name = \"a\"   # why"), "{out}");
    }
}
