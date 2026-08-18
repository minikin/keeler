//! Spec 06 — review leaves evidence, and CI wants it.
//!
//! Two shipped files carry the contract: `/keeler:review` writes
//! `reviews/<spec-slug>/<task-id>.md`, and the shipped workflow refuses a
//! keeler/* pull request whose record is missing or names a commit that
//! is not this branch's own. The check is shell inside the workflow step;
//! the tests here lift that shell out of the YAML and drive it against
//! fixture repositories, the way `install.sh` is driven as a subprocess.

use std::path::{Path, PathBuf};
use std::process::Output;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn shipped_workflow() -> String {
    std::fs::read_to_string(repo_root().join("templates/keeler.yml")).unwrap()
}

/// The job that guards the review record, by the name the workflow gives it.
const REVIEW_JOB: &str = "review-record";

/// The lines of one top-level job under `jobs:` — from its key to the next
/// job's key. Enough YAML for a file we also own.
fn job_block(workflow: &str, job: &str) -> String {
    let key = format!("  {job}:");
    let mut lines = workflow.lines().skip_while(|line| line.trim_end() != key);
    let Some(first) = lines.next() else {
        panic!("the shipped workflow has no `{job}` job");
    };
    let mut block = vec![first];
    for line in lines {
        let next_job = line.starts_with("  ")
            && !line.starts_with("   ")
            && !line.trim().is_empty()
            && !line.trim_start().starts_with('#');
        if next_job || (!line.trim().is_empty() && !line.starts_with(' ')) {
            break;
        }
        block.push(line);
    }
    block.join("\n")
}

fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

/// The shell of the job's `run: |` step, dedented so bash can run it as
/// the workflow would.
fn run_script(job: &str) -> String {
    let mut lines = job.lines();
    let run = lines
        .by_ref()
        .find(|line| line.trim() == "run: |")
        .expect("the job has no `run: |` block");
    let run_indent = indent_of(run);
    let body: Vec<&str> = lines
        .take_while(|line| line.trim().is_empty() || indent_of(line) > run_indent)
        .collect();
    let indent = body
        .iter()
        .filter(|line| !line.trim().is_empty())
        .map(|line| indent_of(line))
        .min()
        .expect("the run block is empty");
    let mut script = String::new();
    for line in body {
        if line.len() >= indent {
            script.push_str(&line[indent..]);
        }
        script.push('\n');
    }
    script
}

/// A git repository built for one test, removed on drop.
struct Repo(PathBuf);

impl Repo {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "keeler-review-record-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let repo = Self(dir);
        repo.git(&["init", "-qb", "main"]);
        repo
    }

    fn path(&self) -> &Path {
        &self.0
    }

    /// Runs git with a fixed identity and no user config, so a global
    /// `commit.gpgsign` cannot hang the suite waiting for a key.
    fn git_output(&self, args: &[&str]) -> Output {
        std::process::Command::new("git")
            .args(["-c", "user.email=probe@keeler", "-c", "user.name=probe"])
            .args(args)
            .current_dir(&self.0)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .output()
            .expect("failed to run git")
    }

    /// `git_output`, asserting success and returning stdout trimmed.
    fn git(&self, args: &[&str]) -> String {
        let output = self.git_output(args);
        assert!(
            output.status.success(),
            "git {args:?} failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    /// Writes a file and commits it; returns the new commit's SHA.
    fn commit(&self, file: &str, body: &str, message: &str) -> String {
        let path = self.0.join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
        self.git(&["add", file]);
        self.git(&["commit", "-qm", message]);
        self.git(&["rev-parse", "HEAD"])
    }

    /// Writes the review record for `slug`/`task` — uncommitted, which
    /// is what the working tree of a checkout holds either way.
    fn record(&self, slug: &str, task: &str, body: &str) {
        let path = self.0.join("reviews").join(slug).join(format!("{task}.md"));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }

    /// Runs the shipped check in this repository as the workflow would:
    /// `HEAD_REF` and `BASE_REF` set from the pull request.
    fn check(&self, script: &str, head_ref: &str, base_ref: &str) -> Output {
        std::process::Command::new("bash")
            .args(["-c", script])
            .current_dir(&self.0)
            .env("HEAD_REF", head_ref)
            .env("BASE_REF", base_ref)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .output()
            .expect("failed to run the review-record check")
    }
}

impl Drop for Repo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn said(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn record_body(slug: &str, task: &str, sha: &str) -> String {
    format!("Spec: {slug}\nTask: {task}\nCommit: {sha}\nVerdict: pass\n\nnone\n")
}

const SLUG: &str = "99-fixture";
const TASK: &str = "t3";
const BRANCH: &str = "keeler/99-fixture/t3";
const RECORD: &str = "reviews/99-fixture/t3.md";

/// A fixture with `main` at one commit and a task branch one commit
/// ahead of it, checked out. Returns `(repo, merge base, branch commit)`.
fn task_branch(name: &str) -> (Repo, String, String) {
    let repo = Repo::new(name);
    let base = repo.commit("README.md", "hello\n", "init");
    repo.git(&["checkout", "-qb", BRANCH]);
    let head = repo.commit("src/lib.rs", "pub fn t3() {}\n", "feat: t3");
    (repo, base, head)
}

#[test]
fn review_leaves_evidence() {
    // Given a task branch ready for fan-in
    // When the review stage's command is read
    let review =
        std::fs::read_to_string(repo_root().join(".claude/commands/keeler/review.md")).unwrap();

    // Then it instructs writing reviews/<spec-slug>/<task-id>.md ...
    assert!(
        review.contains("reviews/<spec-slug>/<task-id>.md"),
        "review.md does not say where the record goes"
    );
    // ... with a header of Spec:, Task:, Commit: and Verdict:, followed by
    // the findings — or an explicit none, because a review that found
    // nothing still happened
    for field in ["Spec:", "Task:", "Commit:", "Verdict:"] {
        assert!(
            review.contains(field),
            "review.md does not name the `{field}` header line"
        );
    }
    assert!(
        review.contains("none"),
        "review.md does not say what a review with no findings writes"
    );
    // And Commit: is the SHA the review examined, on the branch it examined
    let commit_line = review
        .lines()
        .find(|line| line.trim_start().starts_with("Commit:"))
        .expect("review.md shows no Commit: line");
    assert!(
        commit_line.contains("SHA"),
        "review.md does not say Commit: is the SHA the review examined: {commit_line}"
    );
    // And the names are derived the one way the paths agree on: the spec
    // file name without .md, and the task id lowercased
    assert!(
        review.contains("without `.md`") && review.contains("lowercase"),
        "review.md does not say how <spec-slug> and <task-id> are derived"
    );
}

#[test]
fn a_review_record_must_name_a_commit_on_its_own_branch() {
    // Given a keeler/* branch pull request, and the shipped workflow
    let workflow = shipped_workflow();
    let job = job_block(&workflow, REVIEW_JOB);

    // When the shipped CI workflow runs on it — a checkout with history,
    // triggered on keeler/* pull requests
    assert!(
        job.contains("fetch-depth: 0"),
        "the review-record job checks out shallow and cannot see ancestors:\n{job}"
    );
    assert!(
        job.contains("startsWith(github.head_ref, 'keeler/')")
            && job.contains("github.event_name == 'pull_request'"),
        "the review-record job does not key on keeler/* pull requests:\n{job}"
    );
    let script = run_script(&job);

    // Then it fails if reviews/<spec-slug>/<task-id>.md is missing
    let (repo, base, head) = task_branch("missing");
    let out = repo.check(&script, BRANCH, "main");
    assert!(!out.status.success(), "a missing record passed");
    assert!(
        said(&out).contains(RECORD) && said(&out).contains("missing"),
        "the refusal does not name the record as missing:\n{}",
        said(&out)
    );
    assert!(
        !said(&out).contains("No such file") && !said(&out).contains("integer expression"),
        "a missing record tripped the shell instead of the gate:\n{}",
        said(&out)
    );

    // And it fails if the record's Commit: is not an ancestor of the
    // branch head — a record copied from another task names a commit
    // that is not here
    let elsewhere = {
        repo.git(&["checkout", "-q", "main"]);
        repo.git(&["checkout", "-qb", "keeler/99-fixture/t4"]);
        let sha = repo.commit("src/t4.rs", "pub fn t4() {}\n", "feat: t4");
        repo.git(&["checkout", "-q", BRANCH]);
        sha
    };
    repo.record(SLUG, TASK, &record_body(SLUG, TASK, &elsewhere));
    let out = repo.check(&script, BRANCH, "main");
    assert!(
        !out.status.success(),
        "a record naming another branch's commit passed"
    );
    assert!(
        said(&out).contains(&elsewhere) && said(&out).contains("ancestor"),
        "the refusal does not say the commit is not an ancestor:\n{}",
        said(&out)
    );

    // And it fails if Commit: is an ancestor of main — the merge base is
    // an ancestor of every branch and names work the branch has not done
    repo.record(SLUG, TASK, &record_body(SLUG, TASK, &base));
    let out = repo.check(&script, BRANCH, "main");
    assert!(
        !out.status.success(),
        "a record naming the merge base passed"
    );
    assert!(
        said(&out).contains(&base) && said(&out).contains("main"),
        "the refusal does not say the commit is already on main:\n{}",
        said(&out)
    );

    // And a record whose Commit: is a commit the branch itself made passes
    repo.record(SLUG, TASK, &record_body(SLUG, TASK, &head));
    let out = repo.check(&script, BRANCH, "main");
    assert!(
        out.status.success(),
        "an honest record was refused:\n{}",
        said(&out)
    );

    // And it passes just the same in the checkout CI actually has: a clone
    // where main and the branch exist only as origin/ refs and HEAD is
    // detached — which is why the checkout must not be shallow
    let clone = Repo::new("clone");
    let _ = std::fs::remove_dir_all(clone.path());
    let cloned = std::process::Command::new("git")
        .args(["clone", "-q", "--no-single-branch"])
        .arg(repo.path())
        .arg(clone.path())
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .status()
        .expect("failed to run git clone");
    assert!(cloned.success(), "git clone of the fixture failed");
    clone.git(&["checkout", "-q", "--detach", &format!("origin/{BRANCH}")]);
    clone.record(SLUG, TASK, &record_body(SLUG, TASK, &head));
    let out = clone.check(&script, BRANCH, "main");
    assert!(
        out.status.success(),
        "an honest record was refused in a clone with only origin/ refs:\n{}",
        said(&out)
    );

    // And in the one CI has for a pull request from a fork: the branch is
    // known by no name at all, only as the HEAD that was checked out
    for ref_ in [
        format!("refs/remotes/origin/{BRANCH}"),
        format!("refs/heads/{BRANCH}"),
    ] {
        let _ = clone.git_output(&["update-ref", "-d", &ref_]);
    }
    let out = clone.check(&script, BRANCH, "main");
    assert!(
        out.status.success(),
        "an honest record was refused when the branch exists only as HEAD:\n{}",
        said(&out)
    );
}

#[test]
fn a_malformed_record_or_branch_is_refused_and_does_not_crash_the_gate() {
    // Given the shipped check and an honest branch
    let script = run_script(&job_block(&shipped_workflow(), REVIEW_JOB));
    let (repo, _base, head) = task_branch("malformed");

    // When the record is malformed in each of the ways a hand can make it
    let cases = [
        (
            "no-commit-line",
            "Spec: x\nTask: t3\nVerdict: pass\n",
            "Commit:",
        ),
        (
            "empty-commit",
            "Spec: x\nTask: t3\nCommit:\nVerdict: pass\n",
            "Commit:",
        ),
        (
            "not-a-sha",
            "Spec: x\nTask: t3\nCommit: deadbeef-not-here\nVerdict: pass\n",
            "deadbeef-not-here",
        ),
        (
            "two-commit-lines",
            &format!("Spec: x\nTask: t3\nCommit: {head}\nCommit: {head}\nVerdict: pass\n"),
            "found 2",
        ),
    ];
    for (name, body, expected) in cases {
        repo.record(SLUG, TASK, body);
        let out = repo.check(&script, BRANCH, "main");

        // Then it fails naming the record and the problem — a verdict,
        // not a shell error
        assert!(!out.status.success(), "{name}: a malformed record passed");
        let said = said(&out);
        assert!(
            said.contains(RECORD) && said.contains(expected),
            "{name}: the refusal names neither the record nor `{expected}`:\n{said}"
        );
        assert!(
            !said.contains("unbound variable") && !said.contains("fatal:"),
            "{name}: the gate crashed instead of judging:\n{said}"
        );
    }

    // And a branch that is not keeler/<spec-slug>/<task-id> is refused
    // naming the shape, not read as some slug and task it never had
    repo.record(SLUG, TASK, &record_body(SLUG, TASK, &head));
    for branch in [
        "keeler/99-fixture",
        "keeler/99-fixture/t3/extra",
        "keeler//t3",
    ] {
        let out = repo.check(&script, branch, "main");
        assert!(!out.status.success(), "branch `{branch}` was accepted");
        assert!(
            said(&out).contains(branch) && said(&out).contains("keeler/<spec-slug>/<task-id>"),
            "the refusal of `{branch}` does not name the branch and the shape:\n{}",
            said(&out)
        );
    }
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig {
        cases: 12,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::WithSource("proptest-regressions"),
        )),
        ..proptest::prelude::ProptestConfig::default()
    })]

    /// Over any history — some commits on main before the branch, some
    /// on the branch, some on main after it left — a record passes for
    /// exactly the commits the branch itself made: ancestors of its head
    /// that are not ancestors of main. Either half alone lets a wrong
    /// commit through; the invariant is the two together.
    #[test]
    fn a_record_passes_for_exactly_the_commits_the_branch_made(
        before in 1usize..3,
        on_branch in 1usize..3,
        after in 0usize..2,
        pick in 0usize..7,
    ) {
        let script = run_script(&job_block(&shipped_workflow(), REVIEW_JOB));
        let repo = Repo::new("property");
        let mut commits = Vec::new();
        for i in 0..before {
            commits.push((repo.commit(&format!("main-{i}.md"), "m\n", "main"), false));
        }
        repo.git(&["checkout", "-qb", BRANCH]);
        for i in 0..on_branch {
            commits.push((repo.commit(&format!("branch-{i}.md"), "b\n", "branch"), true));
        }
        repo.git(&["checkout", "-q", "main"]);
        for i in 0..after {
            commits.push((repo.commit(&format!("later-{i}.md"), "l\n", "later"), false));
        }
        repo.git(&["checkout", "-q", BRANCH]);

        let (sha, own) = &commits[pick % commits.len()];
        repo.record(SLUG, TASK, &record_body(SLUG, TASK, sha));
        let out = repo.check(&script, BRANCH, "main");
        proptest::prop_assert_eq!(
            out.status.success(),
            *own,
            "commit {} (own: {}) — the check said:\n{}",
            sha,
            own,
            said(&out)
        );
    }
}
