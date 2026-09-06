//! Spec 09 — Keeler as a Claude Code plugin. The gate recipes: what they
//! hand `cargo`, and the two bars an adopter can move now that the
//! `Justfile` is the plugin's and no longer a file in their repository.
//!
//! Every test here drives a real recipe out of the shipped `Justfile` with
//! a stub `cargo` first on PATH. The stub records the argument list it was
//! given and returns, which makes the flags observable without compiling a
//! crate or mutating a line — and it is the invocation, not a substring of
//! the `Justfile`, that these scenarios are about.

mod common;

use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;
use std::process::{Command, Output};
use std::sync::OnceLock;

use common::{Repo, repo_root, said};

/// The absolute path of the real `just`, resolved once against the
/// harness's own PATH — the fixture's PATH carries a stub `cargo`, and
/// `just` itself must still be the real one.
fn real_just() -> &'static str {
    static JUST: OnceLock<String> = OnceLock::new();
    JUST.get_or_init(|| {
        let out = Command::new("sh")
            .args(["-c", "command -v just"])
            .output()
            .expect("failed to look for just");
        assert!(out.status.success(), "just is not on PATH");
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    })
}

/// Stands in for every tool the gate recipes reach for. `metadata` is
/// answered rather than recorded: it is the probe `cov` and `crap` use to
/// decide whether there is anything to measure, and a fixture that failed
/// it would have the recipes skip before they ever called the tool the
/// scenario is about.
const CARGO_STUB: &str = r#"#!/usr/bin/env bash
set -euo pipefail
if [ "${1:-}" = "metadata" ]; then
    printf '%s\n' '{"packages":[{"targets":[{"kind":["lib"],"name":"fixture"}]}]}'
    exit 0
fi
printf '%s\n' "$*" >> "$KEELER_STUB_CARGO_LOG"
# --in-diff names a temp file the recipe deletes on the way out, so the
# only place its contents can be read from afterwards is here.
prev=""
for arg in "$@"; do
    if [ "$prev" = "--in-diff" ]; then cp "$arg" "$KEELER_STUB_CARGO_DIFF"; fi
    prev="$arg"
done
"#;

/// The settings `.cargo-mutants.toml` used to carry, as flags on the
/// command line — one per setting, in the spelling `cargo mutants`
/// accepts.
const MUTANTS_FLAGS: [&str; 8] = [
    "--test-tool nextest",
    "--jobs 4",
    "--profile mutants",
    "--timeout 60",
    "--cap-lints true",
    "--skip-calls-defaults true",
    "--skip-calls eprintln!,write!,writeln!",
    "--exclude tests/**/*.rs",
];

/// A throwaway project a gate recipe runs against, with a stub `cargo`
/// first on PATH and no justfile of its own — the shape every adopter has
/// once the `Justfile` lives in the plugin.
struct Fixture(Repo);

impl Fixture {
    fn new(name: &str) -> Self {
        let repo = Repo::new("gate-recipes", name);
        std::fs::create_dir_all(repo.path().join("bin")).unwrap();
        let stub = repo.path().join("bin/cargo");
        std::fs::write(&stub, CARGO_STUB).unwrap();
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
        Self(repo)
    }

    fn path(&self) -> &Path {
        self.0.path()
    }

    /// Runs one recipe out of the shipped `Justfile` against this project,
    /// with the environment the scenario names and nothing else — the two
    /// threshold variables are cleared, so a developer who exports them
    /// does not change what the suite measures.
    fn run(&self, args: &[&str], env: &[(&str, &str)]) -> Output {
        let dir = self.path();
        let path = std::env::var("PATH").unwrap();
        let mut command = Command::new(real_just());
        command
            .args(["--justfile", repo_root().join("Justfile").to_str().unwrap()])
            .args(["--working-directory", dir.to_str().unwrap()])
            .args(args)
            .current_dir(dir)
            .env("PATH", format!("{}:{path}", dir.join("bin").display()))
            .env("KEELER_STUB_CARGO_LOG", dir.join("cargo-calls"))
            .env("KEELER_STUB_CARGO_DIFF", dir.join("cargo-diff"))
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env_remove("KEELER_COV_MIN")
            .env_remove("KEELER_CRAP_MAX");
        for (key, value) in env {
            command.env(key, value);
        }
        command.output().expect("failed to run just")
    }

    /// The argument lists the stub was handed, in order.
    fn calls(&self) -> Vec<String> {
        std::fs::read_to_string(self.path().join("cargo-calls"))
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }

    /// The one recorded call whose first word is `subcommand`.
    fn call(&self, subcommand: &str, output: &Output) -> String {
        let calls = self.calls();
        let mut matching = calls
            .iter()
            .filter(|call| call.split_whitespace().next() == Some(subcommand));
        let found = matching.next().unwrap_or_else(|| {
            panic!(
                "no `cargo {subcommand}` call was recorded; the recipe said:\n{}",
                said(output)
            )
        });
        assert!(
            matching.next().is_none(),
            "the recipe ran `cargo {subcommand}` more than once: {calls:?}"
        );
        found.clone()
    }

    /// The diff `--in-diff` named, captured before the recipe deleted it.
    fn measured_diff(&self) -> String {
        std::fs::read_to_string(self.path().join("cargo-diff"))
            .expect("the recipe never handed cargo a diff to measure")
    }
}

/// Asserts one recorded `cargo mutants` call carries every setting the
/// config file used to hold.
fn carries_every_setting(recipe: &str, call: &str) {
    for flag in MUTANTS_FLAGS {
        assert!(
            call.contains(flag),
            "`{recipe}` runs mutants without `{flag}`: cargo {call}"
        );
    }
}

#[test]
fn the_mutation_recipes_carry_the_settings_the_config_file_held() {
    // Given a project with a source file and a change to it, so each of the
    // three recipes reaches the tool rather than reporting nothing to do
    for (name, args) in [
        ("one-file", &["mutants", "src/lib.rs"][..]),
        ("every-file", &["mutants-all"]),
        ("changed-lines", &["mutants-diff"]),
    ] {
        let fixture = Fixture::new(name);
        fixture
            .0
            .commit("src/lib.rs", "pub fn a() -> u32 { 1 }\n", "init");
        fixture.0.write("src/lib.rs", "pub fn a() -> u32 { 2 }\n");

        // When the recipe runs
        let output = fixture.run(args, &[]);
        assert!(output.status.success(), "{}", said(&output));

        // Then the settings `.cargo-mutants.toml` carried reach the tool on
        // its command line — the file is not installed any more, and a
        // project that never had one must still get the run Keeler means
        let call = fixture.call("mutants", &output);
        carries_every_setting(args[0], &call);

        // And the flags did not crowd out what the recipe is for: `mutants
        // FILE` read `$1`, which `just` never sets, so the recipe failed on
        // an unbound variable for every caller while a flags-only assertion
        // would have watched it do so and reported nothing.
        if args.len() > 1 {
            assert!(
                call.contains(&format!("--file {}", args[1])),
                "`{}` did not hand cargo the file it was given: cargo {call}",
                args[0]
            );
        }
    }
}

#[test]
fn mutants_diff_accepts_the_base_ci_diffs_against() {
    // Given a branch one commit ahead of main, the commit changing src/lib.rs
    // — and a clean working tree, as a CI checkout has
    let fixture = Fixture::new("explicit-base");
    fixture
        .0
        .commit("src/lib.rs", "pub fn a() -> u32 { 1 }\n", "init");
    fixture.0.git(&["checkout", "-qb", "feature"]);
    fixture
        .0
        .commit("src/lib.rs", "pub fn a() -> u32 { 2 }\n", "change");

    // When the gate runs against the base CI diffs against
    let output = fixture.run(&["mutants-diff", "main"], &[]);
    assert!(output.status.success(), "{}", said(&output));

    // Then the tool is asked to mutate that diff, and the diff holds the
    // change the branch made — the commit is behind HEAD, where the
    // working-tree diff CI's checkout produces would find nothing
    let call = fixture.call("mutants", &output);
    assert!(
        call.contains("--in-diff"),
        "the gate did not measure a diff: cargo {call}"
    );
    let diff = fixture.measured_diff();
    assert!(
        diff.contains("src/lib.rs") && diff.contains("+pub fn a() -> u32 { 2 }"),
        "the base's diff does not hold the branch's change:\n{diff}"
    );
}

#[test]
fn a_base_the_checkout_cannot_reach_is_refused() {
    // Given a repository that has no `origin/main` — a shallow checkout, a
    // fork's pull request, or a KEELER_REF-style typo in the workflow
    let fixture = Fixture::new("unreachable-base");
    fixture
        .0
        .commit("src/lib.rs", "pub fn a() -> u32 { 1 }\n", "init");

    // When the gate is asked to measure against that base
    let output = fixture.run(&["mutants-diff", "origin/main"], &[]);

    // Then it refuses and names the ref. The recipe's own rule is that an
    // honest gate never reports the absence of survivors as evidence about
    // a change it cannot see — and a base it cannot resolve is exactly
    // that: falling back to the working tree would leave CI green having
    // compared the pull request against nothing.
    assert!(
        !output.status.success(),
        "the gate passed on a base it could not resolve: {}",
        said(&output)
    );
    assert!(
        said(&output).contains("origin/main"),
        "the refusal does not name the base: {}",
        said(&output)
    );
    assert!(
        fixture.calls().is_empty(),
        "the gate ran the tool anyway: {:?}",
        fixture.calls()
    );
}

#[test]
fn mutants_diff_without_a_base_behaves_as_before() {
    // Given an uncommitted change to src/lib.rs — the working tree a
    // developer runs the gate from
    let fixture = Fixture::new("no-base");
    fixture
        .0
        .commit("src/lib.rs", "pub fn a() -> u32 { 1 }\n", "init");
    fixture.0.write("src/lib.rs", "pub fn a() -> u32 { 2 }\n");

    // When the gate runs with no argument
    let output = fixture.run(&["mutants-diff"], &[]);
    assert!(output.status.success(), "{}", said(&output));

    // Then it measures that change, as it always did: the parameter is
    // optional, and the road an adopter is on did not move
    let call = fixture.call("mutants", &output);
    assert!(
        call.contains("--in-diff"),
        "the gate did not measure a diff: cargo {call}"
    );
    let diff = fixture.measured_diff();
    assert!(
        diff.contains("src/lib.rs") && diff.contains("+pub fn a() -> u32 { 2 }"),
        "the working tree's change was not what got measured:\n{diff}"
    );
}

#[test]
fn the_coverage_bar_is_the_adopters_to_move() {
    // Given a project whose adopter has raised — or lowered — the bar,
    // which used to be a line they could edit in their own Justfile
    let fixture = Fixture::new("cov-bar");

    // When `cov` runs with KEELER_COV_MIN set
    let output = fixture.run(&["cov"], &[("KEELER_COV_MIN", "50")]);
    assert!(output.status.success(), "{}", said(&output));

    // Then that is the bar coverage is measured against
    assert!(
        fixture
            .call("llvm-cov", &output)
            .contains("--fail-under-lines 50"),
        "KEELER_COV_MIN did not reach the coverage gate: {:?}",
        fixture.calls()
    );

    // And with the variable unset the default stands, so a project that
    // sets nothing runs the gate Keeler has always run
    let fixture = Fixture::new("cov-default");
    let output = fixture.run(&["cov"], &[]);
    assert!(output.status.success(), "{}", said(&output));
    assert!(
        fixture
            .call("llvm-cov", &output)
            .contains("--fail-under-lines 90"),
        "the default coverage bar is no longer 90: {:?}",
        fixture.calls()
    );
}

#[test]
fn the_crap_bar_is_the_adopters_to_move() {
    // Given a project whose adopter has moved the CRAP threshold
    let fixture = Fixture::new("crap-bar");

    // When `crap` runs with KEELER_CRAP_MAX set
    let output = fixture.run(&["crap"], &[("KEELER_CRAP_MAX", "20")]);
    assert!(output.status.success(), "{}", said(&output));

    // Then that is the threshold the gate fails above
    assert!(
        fixture.call("crap", &output).contains("--threshold 20"),
        "KEELER_CRAP_MAX did not reach the CRAP gate: {:?}",
        fixture.calls()
    );

    // And the delta gate reads the same variable — one bar, not two: a
    // project that moved the threshold and found `crap-delta` still
    // failing at 15 would have moved nothing at all
    let fixture = Fixture::new("crap-delta-bar");
    fixture
        .0
        .write("crap-baseline.json", "{\"functions\":[]}\n");
    let output = fixture.run(&["crap-delta"], &[("KEELER_CRAP_MAX", "20")]);
    assert!(output.status.success(), "{}", said(&output));
    assert!(
        fixture.call("crap", &output).contains("--threshold 20"),
        "KEELER_CRAP_MAX did not reach the delta gate: {:?}",
        fixture.calls()
    );

    // And with the variable unset the default stands
    let fixture = Fixture::new("crap-default");
    let output = fixture.run(&["crap"], &[]);
    assert!(output.status.success(), "{}", said(&output));
    assert!(
        fixture.call("crap", &output).contains("--threshold 15"),
        "the default CRAP threshold is no longer 15: {:?}",
        fixture.calls()
    );
}
