//! The command line, and the two things it must get right before anything
//! else: refusing what is not a Rust project, and saying what it did.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn binary() -> PathBuf {
    let mut path = std::env::current_exe().expect("the test binary has a path");
    path.pop();
    if path.ends_with("deps") {
        path.pop();
    }
    let binary = path.join("keeler");
    assert!(
        binary.is_file(),
        "{} is not built — the CLI is what this suite drives",
        binary.display(),
    );
    binary
}

struct Dir(PathBuf);

impl Dir {
    fn rust_project(name: &str) -> Self {
        let dir = Self::empty(name);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"adopter\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        std::fs::write(dir.join("src/lib.rs"), "pub fn adopter() {}\n").unwrap();
        dir
    }

    fn workspace_root(name: &str) -> Self {
        let dir = Self::empty(name);
        std::fs::write(
            dir.join("Cargo.toml"),
            "[workspace]\nmembers = [\"member\"]\nresolver = \"2\"\n",
        )
        .unwrap();
        dir
    }

    fn empty(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("keeler-cli-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn init(&self, args: &[&str]) -> Output {
        Command::new(binary())
            .arg("init")
            .arg(&self.0)
            .args(args)
            .output()
            .expect("failed to run keeler")
    }
}

impl std::ops::Deref for Dir {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn combined(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    )
}

#[test]
fn the_installer_refuses_a_directory_that_is_not_a_rust_project() {
    // Given a directory with no Cargo.toml
    let dir = Dir::empty("not-rust");
    std::fs::write(dir.join("notes.txt"), "just files\n").unwrap();

    // When Keeler is pointed at it
    let output = dir.init(&["--no-tools"]);

    // Then it refuses, says why, and leaves nothing behind — a project
    // half-Keelered because someone mistyped a path is worse than an error
    assert!(
        !output.status.success(),
        "a non-Rust directory was accepted"
    );
    assert!(
        combined(&output).contains("Cargo.toml"),
        "the refusal does not say what is missing:\n{}",
        combined(&output),
    );
    assert!(!dir.join("KEELER.md").exists(), "files were written anyway");
    assert!(!dir.join(".claude").exists(), "files were written anyway");
}

#[test]
fn a_successful_run_says_what_it_did() {
    // Given a Rust project
    let dir = Dir::rust_project("reports");

    // When Keeler is installed into it without tools
    let output = dir.init(&["--no-tools"]);
    let said = combined(&output);

    // Then it exits zero and accounts for the work: how many files, and
    // what it changed in the manifest
    assert!(output.status.success(), "{said}");
    assert!(
        said.contains("18") || said.contains("file"),
        "no file count:\n{said}"
    );
    assert!(
        said.contains("proptest"),
        "the manifest change is unreported:\n{said}"
    );
}

#[test]
fn a_workspace_root_is_told_so_on_the_command_line() {
    // Given a workspace root, whose manifest Keeler cannot configure
    let dir = Dir::workspace_root("workspace-note");

    // When Keeler is installed into it
    let output = dir.init(&["--no-tools"]);

    // Then the note reaches the terminal. Spec 03's contract checker greps
    // this exact phrase from a real clone of serde; a binary that keeps it
    // internal fails that job with nothing to explain it.
    assert!(
        combined(&output).contains("workspace root"),
        "the workspace-root note never reached stdout:\n{}",
        combined(&output),
    );
}

#[test]
fn no_tools_installs_no_tools() {
    // Given a project and a cargo that would record any call made to it
    let dir = Dir::rust_project("no-tools");
    let log = dir.join("cargo-calls.log");
    std::fs::create_dir_all(dir.join("bin")).unwrap();
    let stub = dir.join("bin/cargo");
    std::fs::write(
        &stub,
        format!("#!/usr/bin/env bash\necho \"$@\" >> {}\n", log.display()),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    // When Keeler is asked not to install tools
    let output = Command::new(binary())
        .arg("init")
        .arg(&*dir)
        .arg("--no-tools")
        .env(
            "PATH",
            format!(
                "{}:{}",
                dir.join("bin").display(),
                std::env::var("PATH").unwrap()
            ),
        )
        .output()
        .unwrap();

    // Then cargo is never called at all
    assert!(output.status.success(), "{}", combined(&output));
    assert!(
        !log.exists(),
        "cargo was called despite --no-tools:\n{}",
        std::fs::read_to_string(&log).unwrap_or_default(),
    );
}

/// Puts a `cargo` and a `just` on PATH that answer as told and record every
/// call. Installing tools is the one place Keeler runs other programs, so
/// it is the one place a stub still earns its keep.
fn stub_tools(dir: &Path, probe_exit: u8, install_exit: u8) -> String {
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let log = dir.join("calls.log");
    for program in ["cargo", "just"] {
        let path = bin.join(program);
        std::fs::write(
            &path,
            format!(
                "#!/usr/bin/env bash\n\
                 echo \"{program} $*\" >> {log}\n\
                 if [ \"$1\" = binstall ]; then exit {install_exit}; fi\n\
                 exit {probe_exit}\n",
                log = log.display(),
            ),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }
    format!("{}:{}", bin.display(), std::env::var("PATH").unwrap())
}

#[test]
fn missing_tools_are_installed_and_present_ones_are_not() {
    // Given a project and a toolchain where nothing Keeler needs is present
    let dir = Dir::rust_project("tools-missing");
    let path = stub_tools(&dir, 1, 0);

    // When Keeler is installed with tools
    let output = Command::new(binary())
        .arg("init")
        .arg(&*dir)
        .env("PATH", path)
        .output()
        .unwrap();

    // Then every one of them is asked for in a single binstall, with
    // --locked passed through — nextest refuses to build from source
    // without it
    assert!(output.status.success(), "{}", combined(&output));
    let calls = std::fs::read_to_string(dir.join("calls.log")).unwrap();
    let binstall = calls
        .lines()
        .find(|line| line.contains("binstall"))
        .unwrap_or_else(|| panic!("nothing was installed:\n{calls}"));
    assert!(binstall.contains("--locked"), "{binstall}");
    for tool in [
        "cargo-nextest",
        "cargo-llvm-cov",
        "cargo-mutants",
        "cargo-crap",
        "just",
    ] {
        assert!(
            binstall.contains(tool),
            "`{tool}` was not installed: {binstall}"
        );
    }
}

#[test]
fn a_toolchain_that_already_has_everything_installs_nothing() {
    // Given a toolchain where every probe answers yes
    let dir = Dir::rust_project("tools-present");
    let path = stub_tools(&dir, 0, 0);

    let output = Command::new(binary())
        .arg("init")
        .arg(&*dir)
        .env("PATH", path)
        .output()
        .unwrap();

    // Then nothing is installed — reinstalling what is there costs minutes
    // and changes nothing
    assert!(output.status.success(), "{}", combined(&output));
    let calls = std::fs::read_to_string(dir.join("calls.log")).unwrap_or_default();
    assert!(
        !calls.contains("binstall"),
        "something was installed:\n{calls}"
    );
}

#[test]
fn a_failed_tool_install_stops_the_run() {
    // Given an install that fails
    let dir = Dir::rust_project("tools-fail");
    let path = stub_tools(&dir, 1, 1);

    let output = Command::new(binary())
        .arg("init")
        .arg(&*dir)
        .env("PATH", path)
        .output()
        .unwrap();

    // Then Keeler says so and exits non-zero. A gate whose tool is missing
    // fails later, further from the cause.
    assert!(
        !output.status.success(),
        "a failed install reported success"
    );
    assert!(
        combined(&output).contains("cargo-nextest"),
        "the failure does not name what it could not install:\n{}",
        combined(&output),
    );
}

#[test]
fn an_unknown_option_is_refused_rather_than_read_as_a_path() {
    // Given a mistyped flag
    let dir = Dir::rust_project("bad-option");

    let output = dir.init(&["--no-tool"]);

    // Then it is refused *as an option*. Asserting only that the run
    // failed would pass for the wrong reason: read as a path, `--no-tool`
    // is a directory with no Cargo.toml, which also fails — and would
    // silently install into a directory of that name if one existed.
    assert!(!output.status.success(), "a mistyped flag was accepted");
    let said = combined(&output);
    assert!(
        said.contains("unknown option"),
        "refused for the wrong reason: {said}",
    );
    assert!(said.contains("--no-tool"), "{said}");
}

#[test]
fn the_usage_names_the_command_and_its_options() {
    let output = Command::new(binary()).arg("--help").output().unwrap();
    let said = combined(&output);
    assert!(output.status.success(), "{said}");
    assert!(said.contains("keeler"), "{said}");
    assert!(said.contains("init"), "{said}");
    assert!(said.contains("--no-tools"), "{said}");
}
