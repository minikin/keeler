//! The Keeler installer.
//!
//! Everything it installs is carried inside the binary, so laying the files
//! down needs no network, no tarball and no clone to copy from. The binary
//! is the source.

use include_dir::{Dir, include_dir};

/// The command definitions, embedded whole: a command added to the
/// repository is carried without anyone remembering to list it.
static COMMANDS: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../.claude/commands/keeler");

/// The skills, likewise.
static SKILLS: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../.claude/skills");

/// Files carried one by one, as `(path in this repository, path in the
/// project)`. The two differ only where a name would collide: the workflow
/// template is `templates/keeler.yml` here and `keeler.yml` there, so a
/// project's own workflows are never shadowed.
const SINGLES: [(&str, &str); 7] = [
    (".claude/keeler.md", ".claude/keeler.md"),
    ("KEELER.md", "KEELER.md"),
    ("Justfile", "Justfile"),
    ("specs/TEMPLATE.md", "specs/TEMPLATE.md"),
    ("clippy.toml", "clippy.toml"),
    ("rustfmt.toml", "rustfmt.toml"),
    (".cargo-mutants.toml", ".cargo-mutants.toml"),
];

/// Where each embedded directory's contents come from and land.
const TREES: [(&str, &Dir<'_>); 2] = [
    (".claude/commands/keeler", &COMMANDS),
    (".claude/skills", &SKILLS),
];

/// Every file the binary carries, as `(path in this repository, path in the
/// project)`.
///
/// This is the whole shipped set: what is not here does not reach an
/// adopting project, and what is here must exist in the repository.
#[must_use]
pub fn shipped_files() -> Vec<(&'static str, String)> {
    let mut files: Vec<(&'static str, String)> = SINGLES
        .iter()
        .map(|(source, destination)| (*source, (*destination).to_string()))
        .collect();
    files.push((
        "templates/keeler.yml",
        ".github/workflows/keeler.yml".into(),
    ));
    for (prefix, dir) in TREES {
        collect(dir, prefix, &mut files);
    }
    files.sort_by(|a, b| a.0.cmp(b.0));
    files
}

fn collect(dir: &'static Dir<'static>, prefix: &str, into: &mut Vec<(&'static str, String)>) {
    for file in dir.files() {
        let relative = file.path().to_string_lossy();
        let source: &'static str = Box::leak(format!("{prefix}/{relative}").into_boxed_str());
        into.push((source, format!("{prefix}/{relative}")));
    }
    for sub in dir.dirs() {
        collect(sub, prefix, into);
    }
}

/// The bytes carried for a repository path, if it is carried at all.
#[must_use]
pub fn carried_bytes(source: &str) -> Option<&'static [u8]> {
    for (single, _) in SINGLES {
        if single == source {
            return embedded_single(single);
        }
    }
    if source == "templates/keeler.yml" {
        return Some(include_bytes!("../../templates/keeler.yml"));
    }
    for (prefix, dir) in TREES {
        if let Some(relative) = source
            .strip_prefix(prefix)
            .and_then(|r| r.strip_prefix('/'))
        {
            return dir.get_file(relative).map(include_dir::File::contents);
        }
    }
    None
}

fn embedded_single(source: &str) -> Option<&'static [u8]> {
    Some(match source {
        ".claude/keeler.md" => include_bytes!("../../.claude/keeler.md"),
        "KEELER.md" => include_bytes!("../../KEELER.md"),
        "Justfile" => include_bytes!("../../Justfile"),
        "specs/TEMPLATE.md" => include_bytes!("../../specs/TEMPLATE.md"),
        "clippy.toml" => include_bytes!("../../clippy.toml"),
        "rustfmt.toml" => include_bytes!("../../rustfmt.toml"),
        ".cargo-mutants.toml" => include_bytes!("../../.cargo-mutants.toml"),
        _ => return None,
    })
}

/// What a run did, so a silent no-op cannot pass for an install.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// Files written because nothing was there before.
    pub written: usize,
}

/// Anything a command can fail with, phrased for someone reading its output.
pub type Failure = Box<dyn std::error::Error>;

/// Writes every carried file into `project`.
///
/// Only the carrying, for now: what happens when a file is already there is
/// the next task's business.
///
/// # Errors
///
/// Returns the first write that fails, naming the path — a half-installed
/// project with no explanation is worse than a loud stop.
pub fn lay_down(project: &std::path::Path) -> Result<Report, Failure> {
    let mut report = Report::default();
    for (source, destination) in shipped_files() {
        let target = project.join(&destination);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|why| format!("cannot create {}: {why}", parent.display()))?;
        }
        let bytes =
            carried_bytes(source).ok_or_else(|| format!("nothing is carried for {source}"))?;
        std::fs::write(&target, bytes)
            .map_err(|why| format!("cannot write {}: {why}", target.display()))?;
        report.written += 1;
    }
    Ok(report)
}
