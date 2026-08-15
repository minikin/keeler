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

/// The one file Keeler owns outright rather than merges.
const RULES_FILE: &str = ".claude/keeler.md";

/// Files carried one by one, as `(path in this repository, path in the
/// project)`. These two agree; the workflow template is the only shipped
/// file whose name changes on the way in — `templates/keeler.yml` here,
/// `.github/workflows/keeler.yml` there, so a project's own workflows are
/// never shadowed. It is added in `shipped_files` rather than listed here.
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
fn carries(path: &std::path::Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(is_workflow_file)
}

fn is_workflow_file(name: &str) -> bool {
    !name.starts_with('.') && !name.ends_with('~')
}

fn collect(dir: &'static Dir<'static>, prefix: &str, into: &mut Vec<(String, String)>) {
    for file in dir.files().filter(|file| carries(file.path())) {
        let relative = file.path().to_string_lossy();
        let path = format!("{prefix}/{relative}");
        into.push((path.clone(), path));
    }
    // A hidden directory is swept whole if it is entered at all, so the
    // filter applies here too — .cache/ beside a skill would otherwise
    // ship every file it holds.
    for sub in dir.dirs().filter(|sub| carries(sub.path())) {
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

/// What installing one carried file will do to the project.
#[derive(Debug, PartialEq, Eq)]
enum Plan {
    /// Nothing is there: write it.
    Fresh,
    /// Exactly ours already: leave it, and say nothing.
    Unchanged,
    /// Theirs, and different — or there but unreadable, which is the same
    /// answer for a smaller reason. Keep theirs, put ours beside it.
    Conflict,
    /// The rules file, differing: replace it, keeping what it replaced.
    ReplaceRules(Vec<u8>),
}

/// Decides what one file's install will do, without doing any of it.
///
/// The distinction that matters is between *absent* and *unreadable*.
/// `install.sh` asked whether the path existed; asking whether it can be
/// read instead answers "no" for a file that is merely locked down, and
/// then overwrites content it could not even see.
fn plan_for(target: &std::path::Path, bytes: &[u8], is_rules: bool) -> Result<Plan, Failure> {
    let existing = match std::fs::read(target) {
        Ok(existing) => Some(existing),
        Err(why) if why.kind() == std::io::ErrorKind::NotFound => None,
        Err(why) => {
            if is_rules {
                // The promise that makes owning this file acceptable is
                // that the text it replaces is kept. Text that cannot be
                // read cannot be kept, so this one is refused outright.
                return Err(format!(
                    "cannot read {}: {why} — refusing to replace a file whose text cannot be kept",
                    target.display()
                )
                .into());
            }
            return Ok(Plan::Conflict);
        }
    };

    Ok(match existing {
        None => Plan::Fresh,
        Some(existing) if existing == bytes => Plan::Unchanged,
        Some(existing) if is_rules => Plan::ReplaceRules(existing),
        Some(_) => Plan::Conflict,
    })
}

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
/// Returns before writing anything if a carried file is missing or the
/// rules file cannot be read. Every decision is made first and every write
/// second, so a project is never left half-installed by a refusal.
pub fn lay_down(project: &std::path::Path) -> Result<Report, Failure> {
    // Plan everything first. A refusal after twelve files are on disk
    // leaves a tree nobody asked for, so nothing is written until every
    // decision has been made.
    let planned = plan_all(project)?;

    let mut report = Report::default();
    for (destination, target, bytes, plan) in planned {
        apply(&target, bytes, plan, destination, &mut report)?;
    }
    Ok(report)
}

/// One carried file, decided but not yet acted on: where it goes, what
/// bytes it holds, and what installing it will do.
type Planned = (String, std::path::PathBuf, &'static [u8], Plan);

/// One decision per carried file, made before any of them is acted on.
fn plan_all(project: &std::path::Path) -> Result<Vec<Planned>, Failure> {
    let mut planned = Vec::new();
    for (source, destination) in shipped_files() {
        let bytes =
            carried_bytes(&source).ok_or_else(|| format!("nothing is carried for {source}"))?;
        let target = project.join(&destination);
        let plan = plan_for(&target, bytes, source == RULES_FILE)?;
        planned.push((destination, target, bytes, plan));
    }
    Ok(planned)
}

/// Carries out one decision.
fn apply(
    target: &std::path::Path,
    bytes: &[u8],
    plan: Plan,
    destination: String,
    report: &mut Report,
) -> Result<(), Failure> {
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|why| format!("cannot create {}: {why}", parent.display()))?;
    }
    match plan {
        Plan::Unchanged => {}
        Plan::Fresh => {
            write(target, bytes)?;
            report.written += 1;
        }
        Plan::Conflict => {
            write(&with_suffix(target, ".keeler"), bytes)?;
            report.conflicts.push(destination);
        }
        Plan::ReplaceRules(existing) => {
            write(&with_suffix(target, ".bak"), &existing)?;
            write(target, bytes)?;
            report.written += 1;
        }
    }
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
    fn a_hidden_directory_is_not_descended_into() {
        // The rule is applied to directories as well as files, and only a
        // direct check keeps it that way: a clean checkout has no hidden
        // directory in the swept trees, so no integration test exercises
        // this path.
        assert!(!super::carries(std::path::Path::new(".cache")));
        assert!(super::carries(std::path::Path::new("gherkin-specs")));
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
