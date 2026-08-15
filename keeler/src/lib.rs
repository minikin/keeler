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
/// The one file Keeler owns outright rather than merges.
const RULES_FILE: &str = ".claude/keeler.md";

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
pub fn shipped_files() -> Vec<(String, String)> {
    let mut files: Vec<(String, String)> = SINGLES
        .iter()
        .map(|(source, destination)| ((*source).to_string(), (*destination).to_string()))
        .collect();
    files.push((
        "templates/keeler.yml".to_string(),
        ".github/workflows/keeler.yml".to_string(),
    ));
    for (prefix, dir) in TREES {
        collect(dir, prefix, &mut files);
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    files
}

/// Whether a swept file is part of the workflow rather than something the
/// filesystem left lying around.
///
/// `include_dir!` embeds whatever the directory holds, `.gitignore` and
/// all, so a `.DS_Store` beside a skill would be compiled in and laid into
/// every adopting project. The declared singles are exempt from this: one
/// of them is legitimately a dotfile.
fn is_workflow_file(name: &str) -> bool {
    !name.starts_with('.') && !name.ends_with('~')
}

fn collect(dir: &'static Dir<'static>, prefix: &str, into: &mut Vec<(String, String)>) {
    for file in dir.files() {
        if !file
            .path()
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(is_workflow_file)
        {
            continue;
        }
        let relative = file.path().to_string_lossy();
        let path = format!("{prefix}/{relative}");
        into.push((path.clone(), path));
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
    /// Files written because nothing was there before, or because they
    /// already held exactly what Keeler carries.
    pub written: usize,
    /// Files the project already had with content of its own. Theirs was
    /// kept; Keeler's landed beside it as `<name>.keeler`. Reported by
    /// name, because a conflict nobody is told about is a file nobody
    /// merges.
    pub conflicts: Vec<String>,
}

/// Anything a command can fail with, phrased for someone reading its output.
pub type Failure = Box<dyn std::error::Error>;

/// Writes every carried file into `project`, keeping whatever the project
/// already had.
///
/// A file with content of its own is never overwritten: Keeler's copy
/// lands beside it as `<name>.keeler` and the run reports it by name. The
/// rules file is the documented exception — it is Keeler's to own, so it is
/// replaced and the text it replaced is kept as `.bak` rather than lost.
///
/// # Errors
///
/// Returns the first write that fails, naming the path. Every byte is
/// resolved before the first write, so a missing carried file cannot leave
/// a half-installed project behind.
pub fn lay_down(project: &std::path::Path) -> Result<Report, Failure> {
    // Resolve everything first: a lookup that fails after twelve files are
    // on disk leaves a tree nobody asked for.
    let mut resolved = Vec::new();
    for (source, destination) in shipped_files() {
        let bytes =
            carried_bytes(&source).ok_or_else(|| format!("nothing is carried for {source}"))?;
        resolved.push((source, destination, bytes));
    }

    let mut report = Report::default();
    for (source, destination, bytes) in resolved {
        let target = project.join(&destination);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|why| format!("cannot create {}: {why}", parent.display()))?;
        }

        if source == RULES_FILE {
            replace_rules(&target, bytes, &mut report)?;
            continue;
        }

        match std::fs::read(&target) {
            Ok(existing) if existing == bytes => {}
            Ok(_) => {
                let beside = with_suffix(&target, ".keeler");
                write(&beside, bytes)?;
                report.conflicts.push(destination);
                continue;
            }
            Err(_) => {
                write(&target, bytes)?;
            }
        }
        report.written += 1;
    }
    Ok(report)
}

/// The rules file: Keeler's to own, so an upgrade replaces it and keeps
/// what it replaced. A project that edited it anyway must not lose the text
/// silently — project-specific instructions belong in CLAUDE.md.
fn replace_rules(
    target: &std::path::Path,
    bytes: &[u8],
    report: &mut Report,
) -> Result<(), Failure> {
    match std::fs::read(target) {
        Ok(existing) if existing == bytes => {}
        Ok(existing) => {
            write(&with_suffix(target, ".bak"), &existing)?;
            write(target, bytes)?;
            report.written += 1;
            return Ok(());
        }
        Err(_) => {
            write(target, bytes)?;
        }
    }
    report.written += 1;
    Ok(())
}

fn with_suffix(path: &std::path::Path, suffix: &str) -> std::path::PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(suffix);
    std::path::PathBuf::from(name)
}

fn write(target: &std::path::Path, bytes: &[u8]) -> Result<(), Failure> {
    std::fs::write(target, bytes)
        .map_err(|why| format!("cannot write {}: {why}", target.display()).into())
}

#[cfg(test)]
mod tests {
    use super::is_workflow_file;

    #[test]
    fn a_command_or_skill_file_is_carried() {
        assert!(is_workflow_file("spec.md"));
        assert!(is_workflow_file("SKILL.md"));
    }

    #[test]
    fn what_the_filesystem_leaves_lying_around_is_not() {
        // The integration gate cannot exercise this: a clean checkout has
        // no junk in it, and include_dir! sweeps at compile time, so a file
        // created during a test would never be embedded anyway. Checking
        // the rule itself is the only way it stays checked.
        assert!(!is_workflow_file(".DS_Store"));
        assert!(!is_workflow_file(".gitkeep"));
        assert!(!is_workflow_file("SKILL.md~"));
    }
}
