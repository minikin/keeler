//! The two things the board asks git itself.
//!
//! Everything else it knows comes from a recipe or a script. These two do
//! not: the spec as a ref holds it, which is what the graph is read from,
//! and the facts about a task's branch that make up the commit column.
//! Both are plain queries — nothing here writes a ref, a file in the
//! repository, or an index.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The spec as one ref holds it, in a file of its own.
///
/// A file rather than a string because the reader is a shell script that
/// takes a path, and the same detour `keeler-status` and `keeler-graph`
/// make — `git show <ref>:<rel>` into a temporary file — is what keeps the
/// board's graph and theirs the same graph.
#[derive(Debug)]
pub struct SpecCopy {
    dir: PathBuf,
    path: PathBuf,
}

impl SpecCopy {
    /// Where the copy is, for the length of this value's life.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for SpecCopy {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Writes `rel` as `git_ref` holds it into a temporary file.
///
/// The copy keeps the spec's own file name: the script it is handed to
/// names the file in every refusal it makes, and a refusal about
/// `tmp.md` would name nothing the reader could act on.
///
/// # Errors
///
/// The ref not holding that path — which is the ordinary state of a spec
/// written but not yet committed on the feature branch — or a temporary
/// directory that could not be made.
pub fn spec_from_ref(root: &Path, git_ref: &str, rel: &str) -> Result<SpecCopy, String> {
    let name = Path::new(rel)
        .file_name()
        .ok_or_else(|| format!("{rel} does not name a file"))?;
    let dir = std::env::temp_dir().join(format!(
        "keeler-top-{}-{}",
        std::process::id(),
        next_serial()
    ));
    // `create_dir`, not `create_dir_all`: the second adopts whatever is
    // already at that path, and in a shared temporary directory that is
    // somebody else's — the shell this mirrors reaches for `mktemp -d` for
    // the same reason. A tick that refuses is a tick; the next one has the
    // next serial.
    std::fs::create_dir(&dir).map_err(|err| format!("{}: {err}", dir.display()))?;
    // Built before the read, so a failure below still takes the directory
    // with it — the board makes this copy every second it is up.
    let copy = SpecCopy {
        path: dir.join(name),
        dir,
    };
    let shown = Command::new("git")
        .arg("-C")
        .arg(root)
        .arg("show")
        .arg(format!("{git_ref}:{rel}"))
        .output()
        .map_err(|err| format!("git: {err}"))?;
    if !shown.status.success() {
        return Err(format!(
            "{rel} is not committed on {git_ref} — there is no graph to read."
        ));
    }
    std::fs::write(&copy.path, &shown.stdout)
        .map_err(|err| format!("{}: {err}", copy.path.display()))?;
    Ok(copy)
}

/// Two copies alive at once must not share a directory, and the board
/// makes one per tick while the tests make several at once.
fn next_serial() -> u64 {
    static SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// One commit of a task's branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    /// The short hash, as git abbreviates it here.
    pub hash: String,
    /// The subject line.
    pub subject: String,
}

/// What a task's worktree and branch say about how far the work got.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchFacts {
    /// The short hash of the branch's head.
    pub head: String,
    /// How many commits it is ahead of the feature branch.
    pub ahead: usize,
    /// How many paths in the worktree are modified, staged or untracked.
    pub dirty: usize,
    /// The commits since the feature branch, newest first.
    pub commits: Vec<Commit>,
}

/// Reads a task worktree's branch facts, or nothing.
///
/// Nothing covers every way there is no answer to give rather than a wrong
/// one: a done task whose worktree `keeler-land` has removed, a feature
/// branch that a merge to main has deleted — the base of the comparison,
/// gone — and a directory that is not a checkout at all. The board shows
/// a dash for each of them, which is the honest column.
#[must_use]
pub fn branch_facts(worktree: &Path, base: &str) -> Option<BranchFacts> {
    let head = git(worktree, &["rev-parse", "--short", "HEAD"])?;
    let range = format!("{base}..HEAD");
    let ahead = git(worktree, &["rev-list", "--count", &range])?
        .trim()
        .parse()
        .ok()?;
    // `--untracked-files=normal` is git's default and is asked for anyway:
    // it is `status.showUntrackedFiles`'s default too, and a machine that
    // set that to `no` would have the board report a worktree holding
    // nothing but new files as clean — wrong in the direction that gets
    // work thrown away.
    let dirty = git(
        worktree,
        &["status", "--porcelain", "--untracked-files=normal"],
    )?
    .lines()
    .count();
    let commits = git(worktree, &["log", "--format=%h %s", &range])?
        .lines()
        .map(|line| {
            let (hash, subject) = line.split_once(' ').unwrap_or((line, ""));
            Commit {
                hash: hash.to_string(),
                subject: subject.to_string(),
            }
        })
        .collect();
    Some(BranchFacts {
        head: head.trim().to_string(),
        ahead,
        dirty,
        commits,
    })
}

/// One git query, answered or not. A git that could not be run and a git
/// that refused are the same answer: this repository will not say.
fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::{branch_facts, spec_from_ref};
    use std::path::PathBuf;

    /// A repository of its own, removed on drop. Small on purpose: the
    /// scenarios that need a feature branch, a task branch and a worktree
    /// beside it build one in the suite next door, and these want a
    /// repository only so the queries have somewhere to fail.
    struct Repo(PathBuf);

    impl Repo {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("keeler-top-unit-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let repo = Self(dir);
            repo.git(&["init", "-qb", "main"]);
            repo
        }

        /// git with a fixed identity and no user config, so a global
        /// `commit.gpgsign` cannot hang the suite waiting for a key.
        fn git(&self, args: &[&str]) -> String {
            let output = super::Command::new("git")
                .args(["-c", "user.email=probe@keeler", "-c", "user.name=probe"])
                .args(args)
                .current_dir(&self.0)
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_SYSTEM", "/dev/null")
                .output()
                .expect("failed to run git");
            assert!(
                output.status.success(),
                "git {args:?} failed:\n{}",
                String::from_utf8_lossy(&output.stderr),
            );
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        }

        fn commit(&self, rel: &str, body: &str) {
            let path = self.0.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, body).unwrap();
            self.git(&["add", rel]);
            self.git(&["commit", "-qm", rel]);
        }
    }

    impl Drop for Repo {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_worktree_that_is_not_there_has_no_facts() {
        let repo = Repo::new("no-worktree");
        repo.commit("specs/01-foo.md", "## Tasks\n");

        assert_eq!(branch_facts(&repo.0.join("gone"), "HEAD"), None);
    }

    #[test]
    fn a_base_that_resolves_to_nothing_has_no_facts() {
        // The feature branch once a merge to main has deleted it: HEAD is
        // still readable, and the distance from a ref that is gone is not
        // a number to guess at.
        let repo = Repo::new("no-base");
        repo.commit("specs/01-foo.md", "## Tasks\n");

        assert_eq!(branch_facts(&repo.0, "refs/heads/no-such-base"), None);
    }

    #[test]
    fn a_spec_the_ref_does_not_hold_is_a_refusal_naming_both() {
        let repo = Repo::new("not-committed");
        repo.commit("specs/01-foo.md", "## Tasks\n");
        std::fs::write(repo.0.join("specs/02-bar.md"), "## Tasks\n").unwrap();

        let refused = spec_from_ref(&repo.0, "HEAD", "specs/02-bar.md")
            .expect_err("the spec is written but not committed");

        assert!(
            refused.contains("specs/02-bar.md") && refused.contains("HEAD"),
            "the refusal names neither the spec nor the ref: {refused}",
        );
    }

    #[test]
    fn a_rel_that_names_no_file_is_refused_before_git_is_asked() {
        let repo = Repo::new("nameless");

        assert!(spec_from_ref(&repo.0, "HEAD", "..").is_err());
    }

    #[test]
    fn two_copies_alive_at_once_do_not_share_a_directory() {
        // What the serial is for. Sharing would be worse than untidy:
        // the directory is made rather than adopted, so the second read
        // would refuse outright, and the first copy's end would take the
        // second's file with it.
        let repo = Repo::new("two-copies");
        repo.commit("specs/01-foo.md", "## Tasks\n");

        let first = spec_from_ref(&repo.0, "HEAD", "specs/01-foo.md").expect("HEAD holds it");
        let second = spec_from_ref(&repo.0, "HEAD", "specs/01-foo.md").expect("HEAD holds it");

        assert_ne!(first.path(), second.path());
        drop(first);
        assert!(
            second.path().exists(),
            "one copy's end took the other's file",
        );
    }

    #[test]
    fn the_copy_keeps_the_specs_name_and_goes_when_it_does() {
        let repo = Repo::new("copy");
        repo.commit("specs/01-foo.md", "## Tasks\n\n- [ ] **T1 — one.**\n");

        let copy = spec_from_ref(&repo.0, "HEAD", "specs/01-foo.md").expect("HEAD holds it");
        let path = copy.path().to_path_buf();

        assert_eq!(path.file_name().unwrap(), "01-foo.md");
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "## Tasks\n\n- [ ] **T1 — one.**\n",
        );

        drop(copy);
        assert!(!path.exists(), "the copy outlived the value that owns it");
    }
}
