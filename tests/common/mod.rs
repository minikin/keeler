//! Shared fixtures for the tests that drive shipped shell.
//!
//! Two of Keeler's gates live as shell inside `templates/keeler.yml`. The
//! tests lift that shell out of the YAML and run it against throwaway git
//! repositories, the way `tests/installer.rs` drives `install.sh` as a
//! subprocess — so the thing under test is the shipped bytes and not a
//! Rust re-implementation of them.
//!
//! Included by more than one test binary, and each uses its own subset;
//! `dead_code` is about a single binary's view and would fire on the rest.
#![allow(
    dead_code,
    reason = "each including test binary uses its own subset of these fixtures"
)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

pub fn shipped_workflow() -> String {
    std::fs::read_to_string(repo_root().join("templates/keeler.yml")).unwrap()
}

pub fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

/// The lines of one top-level job under `jobs:` — from its key to the next
/// job's key. Enough YAML for a file we also own.
pub fn job_block(workflow: &str, job: &str) -> String {
    let key = format!("  {job}:");
    let mut lines = workflow.lines().skip_while(|line| line.trim_end() != key);
    let Some(first) = lines.next() else {
        panic!("the shipped workflow has no `{job}` job");
    };
    let mut block = vec![first];
    // A job ends where the next one begins — and the next one begins at
    // its documentation, not at its header line. Comments at job
    // indentation are held back until we know whether a header follows
    // them; if the block ends first, they were never ours.
    let mut pending: Vec<&str> = Vec::new();
    for line in lines {
        if !line.trim().is_empty() && !line.starts_with(' ') {
            break;
        }
        let at_job_indent = line.starts_with("  ") && !line.starts_with("   ");
        if at_job_indent && line.trim_start().starts_with('#') {
            pending.push(line);
            continue;
        }
        if at_job_indent && !line.trim().is_empty() {
            break;
        }
        block.append(&mut pending);
        block.push(line);
    }
    block.join("\n")
}

/// The shell of the job's `run: |` step, dedented so bash can run it as
/// the workflow would.
pub fn run_script(job: &str) -> String {
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

/// Whether the job's checkout asks for the whole history — the option
/// line itself, and not the words `fetch-depth: 0` occurring inside a
/// message *about* it. A `job.contains("fetch-depth: 0")` passes on a job
/// checked out shallow the moment its own error text names the fix, which
/// is an assertion that cannot fail.
pub fn checks_out_full_history(job: &str) -> bool {
    job.lines().any(|line| line.trim() == "fetch-depth: 0")
}

pub fn said(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

/// A git repository built for one test, removed on drop.
pub struct Repo(PathBuf);

impl Repo {
    pub fn new(prefix: &str, name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("keeler-{prefix}-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let repo = Self(dir);
        repo.git(&["init", "-qb", "main"]);
        repo
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    /// Runs git with a fixed identity and no user config, so a global
    /// `commit.gpgsign` cannot hang the suite waiting for a key.
    pub fn git_output(&self, args: &[&str]) -> Output {
        Command::new("git")
            .args(["-c", "user.email=probe@keeler", "-c", "user.name=probe"])
            .args(args)
            .current_dir(&self.0)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .output()
            .expect("failed to run git")
    }

    /// `git_output`, asserting success and returning stdout trimmed.
    pub fn git(&self, args: &[&str]) -> String {
        let output = self.git_output(args);
        assert!(
            output.status.success(),
            "git {args:?} failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    /// Writes a file, creating its parents — uncommitted, which is what
    /// the working tree of a checkout holds either way.
    pub fn write(&self, file: &str, body: &str) {
        let path = self.0.join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }

    /// Writes a file and commits it; returns the new commit's SHA.
    pub fn commit(&self, file: &str, body: &str, message: &str) -> String {
        self.write(file, body);
        self.git(&["add", file]);
        self.git(&["commit", "-qm", message]);
        self.git(&["rev-parse", "HEAD"])
    }

    /// Runs a shipped check in this repository with the environment the
    /// workflow would give it.
    pub fn run(&self, script: &str, env: &[(&str, &str)]) -> Output {
        let mut command = Command::new("bash");
        command
            .args(["-c", script])
            .current_dir(&self.0)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null");
        for (key, value) in env {
            command.env(key, value);
        }
        command.output().expect("failed to run the shipped check")
    }
}

impl Drop for Repo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
