# Spec 09 — Keeler as a Claude Code plugin

**Status:** Approved
**Effort:** Large
**Module:** `.claude-plugin/`, `commands/`, `skills/`, `hooks/`, `bin/`, `keeler.md`, `graph-mode.md`, `gates.md`, `Justfile`, `scripts/keeler-graph.sh`, `install.sh`, `templates/keeler.yml`, `xtask/`, `tests/`, `README.md`, `CONTRIBUTING.md`

## Context

Adopting Keeler today means letting `install.sh` put fifteen files into
your repository: nine slash commands, two skills, the rules file, a
1,400-line `Justfile`, `scripts/keeler-graph.sh`, three tool configs,
`KEELER.md`, `specs/TEMPLATE.md` and a CI workflow — plus edits to
`Cargo.toml`, `.gitignore` and `CLAUDE.md`. The `Justfile` is the sore
point. A project that already has one gets Keeler's copy beside it under
a name `just` will accept only if the installer's case-insensitive dance
succeeds; a project that later wants its own recipes has to grow them
inside Keeler's file, and an upgrade then lands as `Justfile.keeler` to
merge by hand. The workflow files are Keeler's; they are living in
someone else's repository.

Claude Code plugins are the mechanism this was waiting for. A plugin is a
directory Claude Code installs into its own cache and reads from there:
`commands/*.md` become `/keeler:<name>`, skills load from the plugin, a
`SessionStart` hook's stdout is added to the session, every file the
plugin ships is reachable from its commands as `${CLAUDE_PLUGIN_ROOT}`,
and whatever the plugin puts in `bin/` is on the PATH of the agent's
Bash tool while the plugin is enabled. `just` can run a justfile that is
not in the working directory (`--justfile <path> --working-directory
<dir>`). Together those facts let every Keeler file stay Keeler's: the
plugin carries the commands, the skills, the rules, the `Justfile` and
`keeler-graph.sh`; the adopter's repository holds nothing of Keeler's
but the CI workflow, which GitHub reads from the repository and nowhere
else, two tool configs (`clippy.toml`, `rustfmt.toml`) that configure
their own toolchain rather than Keeler, and the data the workflow
produces: `specs/`, `reviews/`, `crap-baseline.json`.

**Who this is for.** A Rust developer who wants the workflow in Claude
Code without Keeler's files in their repository. They run `/plugin
marketplace add minikin/keeler`, `/plugin install keeler@keeler`, then
`/keeler:init` once per project for the tools and the CI workflow.
`/keeler:feature` works the same day.

**The wrapper is the front door.** `bin/keeler` is a shell script that
resolves its own real location (through a symlink too), and runs
`just --justfile <its dir>/../Justfile --working-directory <root> "$@"`,
where `<root>` is `git rev-parse --show-toplevel` and, outside a
repository, the current directory. Every `just <recipe>` the commands
issue today becomes `keeler <recipe>`; the agent has it on PATH because
it lives in `bin/`, a human has it when they put `bin/` on their PATH,
and the CI workflow — which has no plugin — calls `just --justfile`
against a fetched checkout. The `Justfile` stops assuming it lives in
the project: its recipe-to-recipe calls are `just --justfile
"{{justfile()}}" --working-directory . <recipe>` — `just` from PATH, not
`just_executable()`, because the tests' stub `just` on PATH is how the
harness observes those calls — and `keeler-graph.sh` is
`{{justfile_directory()}}/scripts/keeler-graph.sh`. `$root` stays the
project: it is `git rev-parse --show-toplevel` and was always right.

**The rules split, because the hook has a size.** The rules reach the
agent through a `SessionStart` hook that prints `keeler.md` — so nothing
is appended to the adopter's `CLAUDE.md`. Hook output is capped at
10,000 characters; beyond that Claude Code files the text away and hands
the agent a preview, which is the rules arriving partially and
silently. Every size in this spec is in bytes as `wc -c` counts them —
bytes are never fewer than characters, so a bytes ceiling under the
characters cap is safe whichever the cap turns out to count.
`keeler.md` is 20,977 bytes (20,735 characters) today; by section, in
characters: graph mode 7,899; quality gates 2,651; workflow 2,208;
commands 1,421; skills 1,345; change classes 1,225; commits 1,004;
specs 884; testing conventions 868; reporting 779; preamble 451. Four
things leave. The graph-mode chapter becomes `graph-mode.md`, read on
instruction by `/keeler:graph`, `/keeler:spec` at the road question,
`/keeler:tasks` when it writes `Needs:`, and the prompt `keeler-spawn`
hands its agent. The quality-gates section becomes `gates.md` — the
gate table, baseline discipline and the review-enforcement paragraph —
read by `/keeler:qa`, `/keeler:mutants` and `/keeler:review`; the rules
keep one line saying that every gate must be green and where the table
is. The commands list and the recommended-skills list move to README,
replaced by one line (`keeler --list`). What remains is the law —
pipeline, change classes, commits, reporting, specs, testing conventions
— at about 6,450 characters before the new pointer lines, under a
9,500-byte ceiling the release guard enforces: roughly 3,000 bytes of
headroom, so a rule added tomorrow fits, and the tenth one fails loudly
instead of truncating.

**Where the hook speaks.** Once the plugin is enabled, the hook runs at
every session start on the machine — including a colleague's crate, a
`cargo new` to try something, a dependency checkout. It prints the rules
only where a `specs/` directory holding at least one `.md` file exists:
the one thing every Keeler project has once its first stage has run,
where the CI workflow is optional. A project with `specs/` and no Keeler
gets ~6,500 characters it did not ask for; that is the accepted cost of
never writing into the adopter's `CLAUDE.md`. The gap this leaves is the
first session: `/keeler:init` creates no `specs/`, so the session that
runs the first `/keeler:spec` starts without the hook's text, and even
after that spec is written the hook speaks only at the next start. So
the hook is not the only delivery. Every command opens by telling the
agent to read `${CLAUDE_PLUGIN_ROOT}/keeler.md` before anything else —
a command is never ruleless, whether or not the hook spoke — and the
hook is what covers the turns between commands: a plain "fix this"
that never invokes `/keeler:fix`.

**`/keeler:init` is the installer without the copying.** It runs
`install.sh` from the plugin. The installer loses the file loop, the
rules block, the `CLAUDE.md` block and the justfile-collision check, and
keeps: tools (`--no-tools` still skips them), `Cargo.toml` and
`.gitignore` edits exactly as spec 01 defined them, `clippy.toml` and
`rustfmt.toml` (tool configs, not workflow files; whether they should be
Keeler's to install at all is a question for another spec), and the CI
workflow (`--no-ci` skips it). `.cargo-mutants.toml` is no longer
installed: every setting it carried (`test_tool`, `jobs`, `profile`,
`timeout`, `cap_lints`, `skip_calls_defaults`, `skip_calls`,
`exclude_globs`) becomes a flag on the `cargo mutants` invocations in the
`mutants`, `mutants-all` and `mutants-diff` recipes, which is what keeps
`[profile.mutants]` in the adopter's `Cargo.toml` meaningful. The CI
`mutants` job, which called `cargo mutants` directly, goes through
`mutants-diff` with a base ref, so the flags reach CI too. The workflow's `KEELER_REF:`
line is written from `$SRC/VERSION`, the version of the Keeler doing the
installing — not from the `KEELER_REF` environment variable, which is
what selects the tarball to fetch and happens to share the name. An
existing workflow whose only difference is that line is left alone and
nothing lands beside it: the adopter repinned on purpose, and a second
init must not answer with a `.keeler` copy. Files an earlier install
left behind are named — with the `git rm` line that removes them — and
never touched.

**CI cannot see the plugin cache.** The shipped workflow fetches Keeler
at a pinned tag — the codeload tarball `install.sh` fetches today —
into `$RUNNER_TEMP/keeler`, cached by `actions/cache` keyed on the tag
so six jobs and every push do not each hit codeload, and runs every gate
as `just --justfile "$RUNNER_TEMP/keeler/Justfile" --working-directory .`.
The tag is `KEELER_REF`, one `env:` line the installer writes from the
version that installed it; an adopter's CI moves when they change that
line. A ref that does not resolve — an init run from an unreleased
checkout via `--plugin-dir` — fails the fetch step naming the ref and
the line to fix. Integrity rests on the tag, as `install.sh`'s does
today; a checksum is not added, because codeload does not promise
byte-stable tarballs. The `branch-baseline` job keeps guarding
`crap-baseline.json`, which still lives in the adopter's repository, and
stops reading the project's justfile for a `cov:` recipe — a check that
today fails outright when no justfile exists, which after this spec is
every adopter. The `review-record` job is unchanged. Its messages, and
`branch-baseline`'s, spell the next step `keeler keeler-land`, the same
as the recipes do — the workflow has no wrapper, but the human reading
the message does.

**The thresholds stay the adopter's.** With the recipes in the plugin,
`cov`'s `--fail-under-lines 90` and `crap`'s `--threshold 15` are no
longer lines the adopter can edit, and the workflow's comment sends
them to a README section on ratcheting the thresholds that would then
describe nothing. So the recipes read two environment variables,
`KEELER_COV_MIN` (default 90) and `KEELER_CRAP_MAX` (default 15), and
the workflow carries both as commented-out `env:` lines beside
`KEELER_REF`. An adopter ratchets by setting them — in the workflow for
CI, in their shell for `keeler dev`. The mutants flags stay fixed: they
are how the tool runs, not where the bar sits.

**Graph mode's runtime.** `keeler-spawn` writes a runner that starts
`claude -p` in the task's worktree. That session needs the plugin: the
runner passes `--plugin-dir <plugin root>`, derived from
`justfile_directory()` when the runner is written, so enablement does
not depend on the human's settings — and that flag is verified, not
assumed: a probe plugin run under `claude -p --plugin-dir` on 2026-09-05
had its `SessionStart` hook text in the agent's context and its `bin/`
on the Bash tool's PATH. The runner itself is a bash script the tmux
session runs, outside any agent, so the plugin's `bin/` is not on its
PATH by itself; the runner exports `PATH="$plugin_root/bin:$PATH"`
first, and every `keeler` it or its agent's prompt names resolves. Its
prompt names `${CLAUDE_PLUGIN_ROOT}/keeler.md` and `graph-mode.md`, its
allowed tools include `Bash(keeler:*)`, and its death-check calls the
plugin's `keeler-graph.sh`. The plugin's cache path changes on every version, so
a runner outlives the plugin that wrote it; `keeler-resume` already
writes the runner afresh rather than executing what is on disk, which is
what makes a stale path harmless. The worktree has `specs/`, so the hook
speaks there too.

**What this repository does to itself.** The repository root becomes the
plugin: `commands/`, `skills/`, `hooks/`, `bin/`, `keeler.md`,
`graph-mode.md` and `.claude-plugin/` at the top level; `.claude/commands/`
and `.claude/skills/` go, so the repository's own sessions do not
register every command twice. `CLAUDE.md` here imports `@keeler.md`.
Keeler develops Keeler with `claude --plugin-dir .`; the released one
installs from the marketplace like anyone's. `KEELER.md` stays where it
is — the reasoning document, in the repository and therefore in the
plugin, no longer installed. A new `cargo xtask plugin-check` holds the
manifests, the rules marker, the rules size and the marketplace entry in
agreement with `VERSION`; `just lint` runs it here (behind the same
repository-only marker as shellcheck) and `release-guard` runs it before
a tag, reading the rules from `keeler.md`. Since `/plugin update`
compares `plugin.json`'s `version`, a release that forgets the bump
ships nothing — which is what the guard refuses.

**Existing assertions that move.** `tests/graph.rs` and
`tests/fan_out.rs` require `.claude/keeler.md` and `KEELER.md` to walk a
graph-mode day — the rules half of that now reads `graph-mode.md`.
`tests/installer.rs` asserts the installed `KEELER.md` describes the
parallel road — retired with the install. `.github/workflows/ci.yml`
asserts `@.claude/keeler.md` appears once in an adopter's `CLAUDE.md`
and that the installed rules `cmp` equal to the repository's — retired.
`tests/release.rs` checks the README install section — which now leads
with the plugin. `tests/branch.rs` and `tests/spawn.rs` stub `just` and
match the recipe on `$1` — now the last argument. `xtask/src/lib.rs`
reads the rules at `.claude/keeler.md` — now `keeler.md`.

**Superseded scenarios.** Spec 01's installer scenarios that assert a
file landed (`Justfile`, `.claude/commands/keeler/*`, `.claude/skills/*`,
`.claude/keeler.md`, `scripts/keeler-graph.sh`, `specs/TEMPLATE.md`,
`KEELER.md`, `.cargo-mutants.toml`) and its `CLAUDE.md` scenarios
describe the retired contract. Spec 03's checker derives its tracked set
from a reference install and keeps working. Spec 06's `branch-baseline`
scenarios about the coverage bar are replaced below. Those files are not
edited; this paragraph is the record.

**Rejected alternatives.** *Rewriting the `Justfile` as a Rust CLI*:
spec 05 tried a fifth of that and was retired for cause; 1,400 lines of
bash would be the same lesson at six times the size. *Shipping the
`Justfile` into the project and only the commands in the plugin*: keeps
the file this spec exists to remove. *Appending `@` to the adopter's
`CLAUDE.md`*: a write into their file for a property the hook provides
without one. *Two hooks printing the whole 21,000 characters*: works,
but pays for graph mode in every session of every project, and the
ordering of two hooks' output is undocumented. *A hook that prints a
digest and a pointer*: the rules would then reach the agent at its
discretion. *Removing an earlier install's files automatically*: an
edited project copy is indistinguishable from an unedited one without
every released version at hand, and a deleted edit is the one loss the
installer has always promised not to cause. *Firing the hook on
`Cargo.toml`*: every Rust project on the machine gets the law.

**Assumptions approval confirms or overturns.** (1) `install.sh` stays
and `/keeler:init` runs it; the change ships as 0.5.0. (2) `Cargo.toml`
and `.gitignore` edits are unchanged from spec 01. (3) `clippy.toml`
and `rustfmt.toml` are still installed; `.cargo-mutants.toml` is not.
(4) The hook predicate is `specs/*.md`. (5) The rules ceiling is 9,500
bytes, enforced by `plugin-check`; graph mode and the gates leave the
rules. (6) A project's own
`specs/TEMPLATE.md` wins over the plugin's when present. (7)
`keeler-upgrade` is removed; `/plugin update` is the upgrade. (8) The
plugin is enabled in spawned sessions by `--plugin-dir`. (9) Thresholds
are overridden by `KEELER_COV_MIN` and `KEELER_CRAP_MAX`.

---

## Acceptance Tests

Offline, in the harness's style: the plugin's files are read from the
repository, `install.sh` and the recipes run as subprocesses against
generated crates with `cargo`, `just`, `tmux`, `claude` and `curl`
stubbed on PATH where a scenario says so, and the workflow's shell is
lifted out of the YAML as `tests/branch.rs` does today. `<plugin>` is
the repository root.

### The plugin

### Scenario: The plugin manifest names the plugin and its version

```
Given `.claude-plugin/plugin.json`
Then  its `name` is "keeler"
And   its `version` equals the contents of `VERSION`
```

### Scenario: The marketplace lists the plugin at the repository root

```
Given `.claude-plugin/marketplace.json`
Then  its marketplace `name` is "keeler", so the install line is `keeler@keeler`
And   its `plugins` list holds one entry, named "keeler", with `source` "./"
And   that entry's `version` equals the contents of `VERSION`
```

### Scenario: Every stage is a plugin command

```
Given the plugin's `commands/` directory
Then  it holds exactly spec.md, tasks.md, tdd.md, qa.md, review.md, mutants.md,
      feature.md, fix.md, graph.md and init.md
And   each opens with frontmatter carrying a `description:` line
```

### Scenario: The repository registers its commands once

```
Given the repository
Then  no `.claude/commands/` directory exists
And   no `.claude/skills/` directory exists
```

### Scenario: A command that runs a recipe runs it through the wrapper

```
Given every file under the plugin's `commands/`
Then  no line invokes `just`
And   every recipe invocation reads `keeler <recipe>`
```

### Scenario: No command names a file Keeler no longer installs

```
Given every file under the plugin's `commands/`
Then  none contains ".claude/keeler.md", ".claude/commands/", "scripts/keeler-graph.sh" or "KEELER.md"
And   every reference to the rules reads "${CLAUDE_PLUGIN_ROOT}/keeler.md"
```

### Scenario: A command is never ruleless

```
Given every file under the plugin's `commands/`
Then  its first instruction after the frontmatter tells the agent to read
      "${CLAUDE_PLUGIN_ROOT}/keeler.md" before anything else
```

### Scenario: The commands that run gates are told where the gate table is

```
Given the plugin's commands/qa.md, commands/mutants.md and commands/review.md
Then  each tells the agent to read "${CLAUDE_PLUGIN_ROOT}/gates.md"
```

### Scenario: The commands that need graph mode are told where it is

```
Given the plugin's commands/graph.md, commands/spec.md and commands/tasks.md
Then  each tells the agent to read "${CLAUDE_PLUGIN_ROOT}/graph-mode.md" before acting on the graph
```

### Scenario: The spec command prefers the project's template

```
Given the plugin's commands/spec.md
Then  it says to copy the project's `specs/TEMPLATE.md` when that file exists
And   `${CLAUDE_PLUGIN_ROOT}/templates/spec.md` otherwise
And   `templates/spec.md` exists in the plugin
```

### Scenario: The skills ship in the plugin

```
Given the plugin root
Then  skills/gherkin-specs/SKILL.md and skills/property-testing/SKILL.md exist
And   each opens with frontmatter carrying `name:` and `description:`
```

### Scenario: The rules carry the version

```
Given the plugin's keeler.md
Then  its `<!-- keeler-version: X -->` marker equals `VERSION`
```

### Scenario: The rules fit the hook

```
Given the plugin's keeler.md
Then  it is at most 9,500 bytes
And   its first byte is not "{"
```

### Scenario: The rules say where the rest is

```
Given the plugin's keeler.md
Then  it names graph-mode.md as where graph mode is documented
And   it names gates.md as where the gate table is
And   it names `keeler --list` as where the recipes are
And   it has no `## Graph mode`, `## Quality gates`, `## Commands` or `## Skills` section
```

### Scenario: The chapters that left are whole

```
Given the plugin's graph-mode.md and gates.md
Then  graph-mode.md describes feature-branch, graph, fan-out, spawn, status, resume, branch and land
And   gates.md holds the gate table with every row of today's, and the baseline-discipline paragraph
```

### Scenario: plugin-check refuses a plugin manifest that disagrees

```
Given VERSION reads "0.5.0"
And   `.claude-plugin/plugin.json` declares version "0.4.1"
When  `cargo xtask plugin-check` runs
Then  it exits non-zero
And   its output names `.claude-plugin/plugin.json`, "0.4.1" and "0.5.0"
```

### Scenario: plugin-check refuses a marketplace entry that disagrees

```
Given VERSION reads "0.5.0"
And   the "keeler" entry in `.claude-plugin/marketplace.json` declares version "0.4.1"
When  `cargo xtask plugin-check` runs
Then  it exits non-zero
And   its output names `.claude-plugin/marketplace.json`, "0.4.1" and "0.5.0"
```

### Scenario: plugin-check refuses rules the hook would truncate

```
Given a keeler.md of 9,501 bytes
When  `cargo xtask plugin-check` runs
Then  it exits non-zero
And   its output names keeler.md, "9501" and the ceiling "9500"
```

### Scenario: plugin-check reads the rules marker from the plugin root

```
Given VERSION reads "0.5.0"
And   `keeler.md` at the repository root carries `<!-- keeler-version: 0.4.1 -->`
And   no `.claude/keeler.md` exists
When  `cargo xtask plugin-check` runs
Then  it exits non-zero
And   its output names `keeler.md`, "0.4.1" and "0.5.0"
```

### Scenario: The release guard runs the plugin check

```
Given VERSION reads "0.5.0" and CHANGELOG has a 0.5.0 section
And   `.claude-plugin/plugin.json` declares version "0.4.1"
When  `cargo xtask release-guard v0.5.0` runs
Then  it exits non-zero
And   its output names `.claude-plugin/plugin.json`
```

### Scenario: Lint in this repository runs the plugin check

```
Given a checkout holding `templates/keeler.yml`, and stub `cargo` and `shellcheck` recording their arguments
When  `lint` runs from the plugin's Justfile
Then  the cargo stub was called with `xtask plugin-check`
```

### Scenario: Lint in an adopter's project does not

```
Given a crate with no `templates/keeler.yml`, and a stub `cargo` recording its arguments
When  `lint` runs from the plugin's Justfile
Then  the cargo stub was never called with `xtask`
```

### Scenario: The upgrade recipe is gone

```
Given the plugin's Justfile
When  `just --justfile <plugin>/Justfile --list` runs
Then  the output has no `keeler-upgrade` recipe
```

### The rules

### Scenario: A Keeler project's session opens with the rules

```
Given a directory holding `specs/01-foo.md`
And   CLAUDE_PLUGIN_ROOT set to the plugin root
When  hooks/session-start.sh runs with that directory as its working directory
Then  it exits zero
And   its stdout is byte-identical to the plugin's keeler.md
```

### Scenario: A Rust project that never adopted Keeler gets no rules

```
Given a directory holding `Cargo.toml` and no `specs/` directory
When  hooks/session-start.sh runs there
Then  it exits zero
And   its stdout is empty
```

### Scenario: A freshly initialised project is silent until its first spec

```
Given a fresh crate where `install.sh .` has just run, and `cargo` stubbed
When  hooks/session-start.sh runs there
Then  it exits zero
And   its stdout is empty
```

### Scenario: An empty specs directory is not adoption

```
Given a directory holding an empty `specs/` directory
When  hooks/session-start.sh runs there
Then  it exits zero
And   its stdout is empty
```

### Scenario: The hook refuses to guess where the rules are

```
Given a directory holding `specs/01-foo.md`
And   CLAUDE_PLUGIN_ROOT unset
When  hooks/session-start.sh runs there
Then  it exits non-zero
And   its stdout is empty
And   its stderr names CLAUDE_PLUGIN_ROOT
```

### Scenario: The hook is registered for every way a session begins

```
Given the plugin's `hooks/hooks.json`
Then  it registers hooks/session-start.sh under SessionStart
And   the matcher is "startup|resume|clear|compact"
```

### The Justfile leaves the project

### Scenario: A recipe runs against a project that has no justfile

```
Given a git repository with no justfile of any spelling, `specs/01-foo.md` with task T1 committed on HEAD
When  `just --justfile <plugin>/Justfile --working-directory <crate> keeler-graph specs/01-foo.md` runs
Then  it exits zero and reports T1 ready
And   no justfile of any spelling has appeared in the crate
```

### Scenario: The project's own justfile is neither read nor written

```
Given a git repository whose own `justfile` defines `keeler-graph` as `exit 1`
And   `specs/01-foo.md` with task T1 committed on HEAD
When  `just --justfile <plugin>/Justfile --working-directory <crate> keeler-graph specs/01-foo.md` runs
Then  it exits zero and reports T1 ready
And   the crate's `justfile` is byte-identical to before
```

### Scenario: A recipe that calls another recipe calls its own Justfile

```
Given a crate on branch keeler/99-fixture/t4 whose own `justfile` defines `dev` as `exit 1`
And   a stub `just` first on PATH that records every argument list it receives
When  `keeler-branch` runs from the plugin's Justfile by the real just's absolute path
Then  each recorded list starts with `--justfile <plugin>/Justfile --working-directory .`
And   the recorded recipes are dev, crap-delta, mutants-diff in that order
```

### Scenario: Every self-call in the Justfile names its own file

```
Given any recipe body in the plugin's Justfile
When  a token `just` starts a command in it
Then  the next argument is `--justfile "{{justfile()}}"`
```

### Scenario: The graph is read with the script beside the Justfile

```
Given a git repository with `specs/01-foo.md` (tasks T1 and T2, T2 Needs: T1) committed on HEAD
And   no `scripts/` directory in it
When  `keeler-graph specs/01-foo.md` runs from the plugin's Justfile
Then  it exits zero
And   the board reports T1 ready and T2 blocked on T1
```

### Scenario: Recipe outputs name the wrapper, not a project recipe

```
Given the plugin's Justfile
Then  no echo, printf or here-doc line in a recipe body contains `just ` followed by a recipe name
      (comments and the recipes' own self-calls are out of scope)
And   keeler-spawn's board line reads `  board:    keeler keeler-status <spec>`
And   keeler-fan-out's prompt hint reads `KEELER_FAN_OUT_YES=1 keeler keeler-fan-out <spec>`
```

### Scenario: Workflow messages name the wrapper too

```
Given the shipped `templates/keeler.yml`
Then  branch-baseline's refusal reads `run keeler keeler-land there`
And   no message in it reads `just keeler-`
```

### Scenario: The mutation recipes carry the settings the config file held

```
Given the plugin's Justfile
Then  the `cargo mutants` invocations in mutants, mutants-all and mutants-diff each pass
      `--test-tool nextest --jobs 4 --profile mutants --timeout 60 --cap-lints true
      --skip-calls-defaults true --skip-calls eprintln!,write!,writeln! --exclude 'tests/**/*.rs'`
```

### Scenario: mutants-diff accepts the base CI diffs against

```
Given a git repository on a branch one commit ahead of main, the commit changing src/lib.rs
And   a stub `cargo` recording its arguments
When  `mutants-diff main` runs from the plugin's Justfile
Then  the stub was called with `mutants --in-diff` and a diff holding the src/lib.rs change
```

### Scenario: mutants-diff without a base behaves as before

```
Given a git repository with an uncommitted change to src/lib.rs
And   a stub `cargo` recording its arguments
When  `mutants-diff` runs from the plugin's Justfile with no argument
Then  the stub was called with `mutants --in-diff` and a diff holding that change
```

### Scenario: The coverage bar is the adopter's to move

```
Given a crate, and a stub `cargo` recording its arguments
When  `cov` runs from the plugin's Justfile with KEELER_COV_MIN=50
Then  the stub was called with `--fail-under-lines 50`
And   with the variable unset it is called with `--fail-under-lines 90`
```

### Scenario: The CRAP bar is the adopter's to move

```
Given a crate, and a stub `cargo` recording its arguments
When  `crap` runs from the plugin's Justfile with KEELER_CRAP_MAX=20
Then  the stub was called with `--threshold 20`
And   with the variable unset it is called with `--threshold 15`
```

### The wrapper

### Scenario: The wrapper runs the plugin's Justfile in the repository root

```
Given a git repository with `specs/01-foo.md` (task T1) committed on HEAD, and a subdirectory `src/`
When  `<plugin>/bin/keeler keeler-graph specs/01-foo.md` runs with `<crate>/src` as working directory
Then  it exits zero and reports T1 ready
And   no justfile of any spelling has appeared in the crate
```

### Scenario: The wrapper passes every argument through

```
Given a stub `just` first on PATH that records its argument list
And   a crate
When  `<plugin>/bin/keeler keeler-spawn specs/01-foo.md T1` runs from the crate root
Then  the recorded list is `--justfile <plugin>/Justfile --working-directory <crate> keeler-spawn specs/01-foo.md T1`
```

### Scenario: The wrapper passes any argument list through unchanged

```
Given any list of arguments, including ones with spaces and ones starting with `--`
And   a stub `just` first on PATH that records its argument list
When  `<plugin>/bin/keeler` runs with that list
Then  the recorded list is `--justfile <plugin>/Justfile --working-directory <root>` followed by the list verbatim
```

### Scenario: The wrapper finds its Justfile through a symlink

```
Given a symlink `<elsewhere>/keeler` pointing at `<plugin>/bin/keeler`
And   a stub `just` first on PATH that records its argument list
When  `<elsewhere>/keeler --list` runs
Then  the recorded list starts with `--justfile <plugin>/Justfile`
```

### Scenario: Outside a repository the wrapper uses the current directory

```
Given a directory that is not inside a git repository
And   a stub `just` first on PATH that records its argument list
When  `<plugin>/bin/keeler --list` runs there
Then  the recorded list holds `--working-directory <that directory>`
```

### Scenario: In a linked worktree the wrapper stays in the worktree

```
Given a repository with a linked worktree at `<worktree>`
And   a stub `just` first on PATH that records its argument list
When  `<plugin>/bin/keeler --list` runs from `<worktree>/src`
Then  the recorded list holds `--working-directory <worktree>`
```

### Scenario: The wrapper is executable as committed

```
Given the repository's git index
Then  `bin/keeler`, `hooks/session-start.sh` and `scripts/keeler-graph.sh` have mode 100755
```

### Graph mode's runtime

### Scenario: A spawned agent is given the plugin

```
Given a feature branch with an Approved spec whose T1 is ready
And   stub `tmux` and `claude` on PATH
When  `keeler-spawn specs/01-foo.md T1` runs from the plugin's Justfile
Then  the runner under `.keeler/runs/01-foo/t1.sh` starts claude with `--plugin-dir <plugin>`
And   its `--allowedTools` includes `Bash(keeler:*)`
And   it exports PATH with `<plugin>/bin` first, before anything runs
```

### Scenario: A spawned agent is told where the rules are

```
Given the runner `keeler-spawn` wrote for T1
Then  its prompt names `${CLAUDE_PLUGIN_ROOT}/keeler.md` and `${CLAUDE_PLUGIN_ROOT}/graph-mode.md`
And   the gate it names is `keeler keeler-branch`
And   it does not name `.claude/keeler.md`
```

### Scenario: The runner reads the graph with the plugin's parser

```
Given the runner `keeler-spawn` wrote for T1
Then  every `keeler-graph.sh` it invokes is `<plugin>/scripts/keeler-graph.sh`
And   none is under the worktree or the main checkout
```

### Scenario: A resume rewrites a runner that names a plugin that moved

```
Given a run of T1 whose runner passes `--plugin-dir /old/cache/keeler` and calls
      `/old/cache/keeler/scripts/keeler-graph.sh`, no exit file, and a tmux stub reporting no session
When  `keeler-resume specs/01-foo.md T1` runs from the plugin's Justfile
Then  the rewritten runner names `<plugin>` in both places and `/old/cache/keeler` nowhere
```

### /keeler:init

### Scenario: Init leaves the workflow and two tool configs in a fresh crate

```
Given a fresh crate with a `.gitignore`, and `cargo` stubbed
When  `install.sh .` runs
Then  the files created are `.github/workflows/keeler.yml`, `clippy.toml` and `rustfmt.toml`
And   the only files modified are `Cargo.toml` and `.gitignore`
And   no `.claude/`, `Justfile`, `KEELER.md`, `specs/`, `scripts/` or `.cargo-mutants.toml` exists
```

### Scenario: A crate without a gitignore gets one

```
Given a fresh crate with no `.gitignore`, and `cargo` stubbed
When  `install.sh .` runs
Then  the files created are `.github/workflows/keeler.yml`, `clippy.toml`, `rustfmt.toml` and `.gitignore`
```

### Scenario: Init without CI leaves no workflow

```
Given a fresh crate with a `.gitignore`, and `cargo` stubbed
When  `install.sh . --no-ci` runs
Then  the files created are `clippy.toml` and `rustfmt.toml`
And   no `.github/` directory exists
And   the only files modified are `Cargo.toml` and `.gitignore`
```

### Scenario: The tool configs still land

```
Given a fresh crate
When  `install.sh .` runs
Then  `clippy.toml` and `rustfmt.toml` exist, byte-identical to the plugin's
```

### Scenario: Tools are skipped on request

```
Given a fresh crate and a PATH with no cargo-nextest
When  `install.sh . --no-tools` runs
Then  it exits zero
And   no `cargo binstall` or `cargo install` was attempted
```

### Scenario: The run directory is ignored

```
Given a fresh crate with an empty `.gitignore`
When  `install.sh .` runs
Then  `.gitignore` gains `/target`, `lcov.info`, `crap-report.json`, `mutants.out*/` and `.keeler/`
```

### Scenario: The adopter's CLAUDE.md is not touched

```
Given a crate whose `CLAUDE.md` reads "# Mine\n"
When  `install.sh .` runs
Then  `CLAUDE.md` is byte-identical afterwards
```

### Scenario: A crate without CLAUDE.md does not get one

```
Given a crate with no `CLAUDE.md`
When  `install.sh .` runs
Then  no `CLAUDE.md` exists afterwards
```

### Scenario: Init is idempotent

```
Given any crate shape the installer accepts, where `install.sh .` has run once
When  `install.sh .` runs again
Then  every file is byte-identical to before
And   the run reports nothing added
```

### Scenario: The init command runs the installer from the plugin

```
Given the plugin's `commands/init.md`
Then  it runs `bash "${CLAUDE_PLUGIN_ROOT}/install.sh" .`
And   it forwards `--no-tools` and `--no-ci` when the user asks for them
And   it tells the user to put `${CLAUDE_PLUGIN_ROOT}/bin` on their PATH for `keeler` in a terminal
```

### Scenario: Files an earlier install left behind are named and kept

```
Given a crate holding `.claude/commands/keeler/spec.md`,
      `.claude/skills/gherkin-specs/SKILL.md`, `.claude/keeler.md`,
      `scripts/keeler-graph.sh`, `KEELER.md`, `.cargo-mutants.toml`,
      a `Justfile` containing a `keeler-spawn:` recipe,
      and a `CLAUDE.md` importing `@.claude/keeler.md`
When  `install.sh .` runs
Then  it exits zero
And   the output names each of those paths as left by an earlier Keeler
And   the output gives one `git rm -r --` line listing the files
And   the output names the `@.claude/keeler.md` line in CLAUDE.md as one to delete by hand
And   every one of those files is byte-identical afterwards
```

### Scenario: A project's own justfile is not mistaken for Keeler's

```
Given a crate whose `justfile` defines only its own recipes
When  `install.sh .` runs
Then  the output does not name the justfile
```

### Scenario: Any subset of stale files is reported exactly

```
Given any subset of the eight stale markers the previous two scenarios name
When  `install.sh .` runs
Then  the output names every path in the subset and none outside it
And   no file other than Cargo.toml, .gitignore, clippy.toml, rustfmt.toml
      and .github/workflows/keeler.yml differs afterwards
```

### The CI workflow

### Scenario: The installed workflow pins the version that installed it

```
Given VERSION reads "0.5.0"
When  `install.sh .` runs in a fresh crate
Then  `.github/workflows/keeler.yml` contains the line `  KEELER_REF: v0.5.0`
```

### Scenario: A repinned workflow is left alone

```
Given a crate whose `.github/workflows/keeler.yml` is the shipped one with `KEELER_REF: v0.4.1`
And   VERSION reads "0.5.0"
When  `install.sh .` runs
Then  the workflow is byte-identical afterwards
And   no `.github/workflows/keeler.yml.keeler` exists
```

### Scenario: A workflow that differs beyond its pin gets the new one alongside

```
Given a crate whose `.github/workflows/keeler.yml` is the shipped one with `KEELER_REF: v0.4.1`
      and one extra job
And   VERSION reads "0.5.0"
When  `install.sh .` runs
Then  the workflow is byte-identical afterwards
And   `.github/workflows/keeler.yml.keeler` exists, pinning v0.5.0
```

### Scenario: The pin comes from the installing Keeler, not the fetch variable

```
Given a Keeler checkout whose VERSION reads "0.5.0"
When  `KEELER_REF=v0.4.1 install.sh .` runs from that checkout in a fresh crate
Then  `.github/workflows/keeler.yml` contains `KEELER_REF: v0.5.0`
```

### Scenario: The workflow runs every gate from the fetched Justfile

```
Given the shipped `templates/keeler.yml`
Then  every `just` invocation in it reads `just --justfile "$RUNNER_TEMP/keeler/Justfile" --working-directory .`
```

### Scenario: The mutants job goes through the recipe

```
Given the shipped `templates/keeler.yml`
Then  its mutants job invokes `mutants-diff "origin/${BASE_REF}"` and no bare `cargo mutants`
And   that step keeps `if: github.event_name == 'pull_request'`
```

### Scenario: The workflow fetches the pinned tag once per ref

```
Given the shipped `templates/keeler.yml`
Then  every job with a `just` step has an `actions/cache` step with an `id:`, path `${{ runner.temp }}/keeler`
      and a key containing `${{ env.KEELER_REF }}`
And   a fetch step guarded by `if: steps.<that id>.outputs.cache-hit != 'true'` downloading
      `https://codeload.github.com/minikin/keeler/tar.gz/${KEELER_REF}` into `$RUNNER_TEMP/keeler`
```

### Scenario: The thresholds are one uncomment away in CI

```
Given the shipped `templates/keeler.yml`
Then  its top-level `env:` holds `KEELER_REF`, and commented-out `KEELER_COV_MIN` and `KEELER_CRAP_MAX` lines
And   its header comment names those two variables, not a README section on ratcheting recipes
```

### Scenario: A ref that does not resolve is named

```
Given the fetch step's shell lifted from the workflow
And   a stub `curl` that exits 22 as on HTTP 404
When  the step runs with KEELER_REF=v9.9.9
Then  it exits non-zero
And   its output names v9.9.9 and the `KEELER_REF` line as the thing to fix
```

### Scenario: The branch-baseline job no longer reads the project's justfile

```
Given the shipped `templates/keeler.yml`
Then  its `branch-baseline` job names `crap-baseline.json`
And   no step in it reads a justfile or a `cov:` recipe
```

### Scenario: A task branch that moves crap-baseline.json is still refused

```
Given a repository whose branch keeler/99-fixture/t4 changes `crap-baseline.json`
When  the branch-baseline job's shell runs against it
Then  it exits non-zero
And   its output names crap-baseline.json
```

### Scenario: A task branch that leaves the baseline alone passes without a justfile

```
Given a repository with no justfile whose branch keeler/99-fixture/t4 changes only src/lib.rs
When  the branch-baseline job's shell runs against it
Then  it exits zero
```

### Scenario: The review-record job stays

```
Given a repository whose branch keeler/99-fixture/t4 ticks T4 with a record naming a commit the branch did not make
When  the review-record job's shell runs against it
Then  it exits non-zero, as tests/review_record.rs pins today
```

### Scenario: The installed workflow is valid YAML

```
Given a fresh crate where `install.sh .` has run
Then  `.github/workflows/keeler.yml` parses as YAML with jobs lints, test, quality, mutants, branch-baseline and review-record
```

### This repository

### Scenario: Keeler's own CLAUDE.md imports the rules from the plugin root

```
Given the repository's `CLAUDE.md`
Then  it contains the line `@keeler.md`
And   no line contains `.claude/keeler.md`
```

### Scenario: The README leads with the plugin

```
Given the repository's `README.md` Install section
Then  it names `/plugin marketplace add minikin/keeler` and `/plugin install keeler@keeler`
And   `/keeler:init`
And   it lists `keeler --list` as where the recipes are
And   its ratcheting section names `KEELER_COV_MIN` and `KEELER_CRAP_MAX`
```

---

## Tasks

Hot files, and who writes where. `install.sh`: T1 owns it, T2 and T3
follow it. `Justfile`: T9 owns the self-calls, the `keeler-graph.sh`
paths, the printed lines, `default` and `keeler-upgrade`; T10 owns the
`mutants*`, `cov`, `crap` and `crap-delta` bodies; T7 owns `lint`; T11
owns `_write-runner` after T9 has been through it. `templates/keeler.yml`:
T3 owns the top `env:`, T12 the rest, after T3. `tests/installer.rs`:
T1 rewrites the superseded scenarios, T2 and T3 add under their own
headings. `tests/plugin.rs`: T4 creates it, T5 and T6 add under their
own headings. New test files otherwise: `tests/plugin_check.rs` (T7),
`tests/wrapper.rs` (T8), `tests/justfile.rs` (T9), `tests/thresholds.rs`
(T10). `tests/spawn.rs` and `tests/branch.rs`: T9 changes the stubs to
match the last argument, T11 and T12 add under their own headings.

- [ ] **T1 — The installer stops copying Keeler's files.** `install.sh` loses the file loop, the rules block, the `CLAUDE.md` block and the justfile-collision scan; keeps tools, `clippy.toml`, `rustfmt.toml`, `Cargo.toml`, `.gitignore`, the workflow; gains `--no-ci`. The superseded scenarios in `tests/installer.rs` and the `CLAUDE.md`/rules assertions in `.github/workflows/ci.yml` go. Scenarios: _Init leaves the workflow and two tool configs in a fresh crate; A crate without a gitignore gets one; Init without CI leaves no workflow; The tool configs still land; Tools are skipped on request; The run directory is ignored; The adopter's CLAUDE.md is not touched; A crate without CLAUDE.md does not get one; Init is idempotent_. Tests: acceptance + property (idempotence over any accepted crate shape).
- [ ] **T2 — The installer names what an earlier install left behind.** Needs: T1. Stale detection by path, by justfile content, by `CLAUDE.md` line; one `git rm -r --` line; exit zero; nothing touched. Scenarios: _Files an earlier install left behind are named and kept; A project's own justfile is not mistaken for Keeler's; Any subset of stale files is reported exactly_. Tests: acceptance + property (report names exactly the subset, nothing else differs).
- [ ] **T3 — The workflow pins the version that installed it.** Needs: T1. `templates/keeler.yml` gets `env: KEELER_REF: v@VERSION@`; the installer writes the line from `$SRC/VERSION` and compares an existing workflow with that line ignored on both sides. Scenarios: _The installed workflow pins the version that installed it; A repinned workflow is left alone; A workflow that differs beyond its pin gets the new one alongside; The pin comes from the installing Keeler, not the fetch variable_. Tests: acceptance.
- [ ] **T4 — The repository becomes the plugin.** Needs: T1. `.claude-plugin/plugin.json` and `marketplace.json`; `git mv` of `.claude/commands/keeler/*` to `commands/`, `.claude/skills/*` to `skills/`, `specs/TEMPLATE.md` to `templates/spec.md`; `.claude/keeler.md` becomes `keeler.md`, `graph-mode.md` and `gates.md`; the repository's `CLAUDE.md` imports `@keeler.md`; `xtask/src/lib.rs` reads the marker from `keeler.md`; `tests/graph.rs` and `tests/fan_out.rs` read the graph-mode day from `graph-mode.md`. `tests/plugin.rs` is created here. Scenarios: _The plugin manifest names the plugin and its version; The marketplace lists the plugin at the repository root; The repository registers its commands once; The skills ship in the plugin; The rules carry the version; The rules fit the hook; The rules say where the rest is; The chapters that left are whole; Keeler's own CLAUDE.md imports the rules from the plugin root_. Tests: acceptance.
- [ ] **T5 — The hook speaks where a spec exists.** Needs: T4. `hooks/hooks.json` and `hooks/session-start.sh`. Scenarios: _A Keeler project's session opens with the rules; A Rust project that never adopted Keeler gets no rules; A freshly initialised project is silent until its first spec; An empty specs directory is not adoption; The hook refuses to guess where the rules are; The hook is registered for every way a session begins_. Tests: acceptance.
- [ ] **T6 — The commands speak the plugin's language.** Needs: T4. Every command opens with the rules read, invokes `keeler <recipe>`, names `${CLAUDE_PLUGIN_ROOT}` paths, points at `gates.md` and `graph-mode.md` where it needs them; `commands/init.md` is written. Scenarios: _Every stage is a plugin command; A command that runs a recipe runs it through the wrapper; No command names a file Keeler no longer installs; A command is never ruleless; The commands that run gates are told where the gate table is; The commands that need graph mode are told where it is; The spec command prefers the project's template; The init command runs the installer from the plugin_. Tests: acceptance.
- [ ] **T7 — plugin-check holds the plugin to VERSION.** Needs: T4. New `cargo xtask plugin-check`; `release-guard` calls it; `lint` runs it behind the repository-only marker. Scenarios: _plugin-check refuses a plugin manifest that disagrees; plugin-check refuses a marketplace entry that disagrees; plugin-check refuses rules the hook would truncate; plugin-check reads the rules marker from the plugin root; The release guard runs the plugin check; Lint in this repository runs the plugin check; Lint in an adopter's project does not_. Tests: unit (the two JSON field readers, the byte ceiling) + property (every disagreement is reported, none twice) + acceptance.
- [ ] **T8 — The wrapper.** Needs: T5, T9. `bin/keeler`, executable in the index with the hook script and the graph parser. Scenarios: _The wrapper runs the plugin's Justfile in the repository root; The wrapper passes every argument through; The wrapper passes any argument list through unchanged; The wrapper finds its Justfile through a symlink; Outside a repository the wrapper uses the current directory; In a linked worktree the wrapper stays in the worktree; The wrapper is executable as committed_. Tests: acceptance + property (argv passes through verbatim).
- [x] **T9 — The Justfile stops assuming it lives in the project.** Self-calls through `--justfile "{{justfile()}}"`, the parser at `{{justfile_directory()}}/scripts/keeler-graph.sh`, printed lines say `keeler`, `keeler-upgrade` deleted; the `just` stubs in `tests/branch.rs` and `tests/spawn.rs` match the last argument. Scenarios: _A recipe runs against a project that has no justfile; The project's own justfile is neither read nor written; A recipe that calls another recipe calls its own Justfile; Every self-call in the Justfile names its own file; The graph is read with the script beside the Justfile; Recipe outputs name the wrapper, not a project recipe; The upgrade recipe is gone_. Tests: acceptance + property (every `just` token that begins a command is followed by `--justfile`).
- [ ] **T10 — The gate recipes carry their settings and read the adopter's bars.** Mutants flags on the three recipes, `mutants-diff BASE`, `KEELER_COV_MIN`, `KEELER_CRAP_MAX`; the repository's own `.cargo-mutants.toml` is deleted once the flags carry it. Scenarios: _The mutation recipes carry the settings the config file held; mutants-diff accepts the base CI diffs against; mutants-diff without a base behaves as before; The coverage bar is the adopter's to move; The CRAP bar is the adopter's to move_. Tests: acceptance.
- [ ] **T11 — A spawned agent gets the plugin.** Needs: T8, T9. `_write-runner` exports PATH, passes `--plugin-dir`, allows `Bash(keeler:*)`, names the plugin's rules and parser; the resume scenario pins regeneration. Scenarios: _A spawned agent is given the plugin; A spawned agent is told where the rules are; The runner reads the graph with the plugin's parser; A resume rewrites a runner that names a plugin that moved_. Tests: acceptance.
- [ ] **T12 — The workflow runs the gates from a fetched Keeler.** Needs: T3, T10. Cache and fetch steps, `--justfile` everywhere, the mutants job through `mutants-diff`, threshold lines, `branch-baseline` without the justfile check, messages spelling `keeler`. Scenarios: _The workflow runs every gate from the fetched Justfile; The mutants job goes through the recipe; The workflow fetches the pinned tag once per ref; The thresholds are one uncomment away in CI; A ref that does not resolve is named; The branch-baseline job no longer reads the project's justfile; A task branch that moves crap-baseline.json is still refused; A task branch that leaves the baseline alone passes without a justfile; The review-record job stays; The installed workflow is valid YAML; Workflow messages name the wrapper too_. Tests: acceptance.
- [ ] **T13 — The documents follow.** Needs: T6, T8, T10. README install and ratcheting sections, migration note, macOS floor; CONTRIBUTING (`claude --plugin-dir .`, bump checklist, no `keeler-upgrade`); CHANGELOG `[Unreleased]`; `tests/release.rs`'s README check follows. Scenarios: _The README leads with the plugin_. Tests: acceptance.

---

## Implementation Notes

**Layout.** Repository root is the plugin root:

```
.claude-plugin/plugin.json      name "keeler", version = VERSION, description, author
.claude-plugin/marketplace.json name "keeler", one entry: name "keeler", source "./", version = VERSION
commands/*.md                   ten commands (nine moved from .claude/commands/keeler + init.md)
skills/*/SKILL.md               moved verbatim from .claude/skills
hooks/hooks.json                SessionStart, matcher "startup|resume|clear|compact" → hooks/session-start.sh
hooks/session-start.sh          see below
bin/keeler                      see below
keeler.md                       the law; ≤ 9,500 bytes; names graph-mode.md, gates.md and `keeler --list`
graph-mode.md                   the graph-mode chapter, verbatim
gates.md                        the quality-gates section, verbatim
templates/spec.md               moved from specs/TEMPLATE.md
templates/keeler.yml            same place, new body
Justfile, scripts/keeler-graph.sh, install.sh, KEELER.md   same place
```

**`hooks/session-start.sh`.**

```bash
#!/usr/bin/env bash
set -euo pipefail
[ -n "${CLAUDE_PLUGIN_ROOT:-}" ] || { echo "session-start: CLAUDE_PLUGIN_ROOT is unset" >&2; exit 1; }
ls specs/*.md >/dev/null 2>&1 || exit 0
cat "$CLAUDE_PLUGIN_ROOT/keeler.md"
```

Plain stdout from a zero-exit SessionStart hook is added to context;
stdout starting with `{` is parsed as JSON — hence the first-character
scenario. Output over 10,000 characters is filed away and replaced by a
preview — hence the ceiling, enforced by `plugin-check`, 500 below the
cap.

**`bin/keeler`.**

```bash
#!/usr/bin/env bash
set -euo pipefail
here="$(cd -P -- "$(dirname -- "$(readlink -f -- "${BASH_SOURCE[0]}")")" && pwd)"
root="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
exec just --justfile "$here/../Justfile" --working-directory "$root" "$@"
```

`readlink -f` for the symlink scenario: GNU coreutils and macOS since
12.3 (verified on this machine's 26.6); the floor is stated in README's
requirements. `--show-toplevel` in a linked worktree is
the worktree root, which is what the worktree scenario pins.

**Justfile.** Self-calls: `just --justfile "{{justfile()}}"
--working-directory . <recipe>` — `just` from PATH so the stub in
`tests/branch.rs` and `tests/spawn.rs` sees them; the stubs match the
last argument instead of `$1`. `keeler-graph.sh` is
`{{justfile_directory()}}/scripts/keeler-graph.sh`. `mutants-diff` gains
an optional `BASE="HEAD"` parameter; CI passes `origin/${BASE_REF}`. The
three `cargo mutants` invocations carry `--test-tool nextest --jobs 4
--profile mutants --timeout 60 --cap-lints true --skip-calls-defaults
true --skip-calls eprintln!,write!,writeln! --exclude 'tests/**/*.rs'`
— spellings verified against `cargo mutants --help` on 2026-09-05:
`--cap-lints` and `--skip-calls-defaults` both take a value, globs go
to `-e/--exclude`. An explicit `BASE` replaces `mutants-diff`'s
three-tier fallback (working diff, merge-base with `_main-ref`,
`HEAD~1`) with `git diff BASE...HEAD` plus the working tree; with no
argument the fallback is as today. `cov` reads `KEELER_COV_MIN`
(default 90), `crap` and `crap-delta` read `KEELER_CRAP_MAX` (default
15). `keeler-upgrade` is deleted. Human-facing "run this next" lines say `keeler <recipe>`.
The `_write-runner` recipe bakes `plugin_root="{{justfile_directory()}}"`,
writes `export PATH="$plugin_root/bin:$PATH"` as the runner's first
command, emits `claude -p … --plugin-dir "$plugin_root"`, adds
`Bash(keeler:*)` to `$tools`, names `${CLAUDE_PLUGIN_ROOT}/keeler.md`
and `graph-mode.md` in the prompt, and calls
`"$plugin_root/scripts/keeler-graph.sh"` in the death-check.
`keeler-resume` already regenerates the runner; the scenario pins that
the regenerated one names the current plugin.

**Installer.** Loses: the `install_file` loop for the workflow files, the
`Justfile` copy and its collision scan, the rules block, the `CLAUDE.md`
block. Keeps: tools, `clippy.toml`, `rustfmt.toml`, `Cargo.toml`,
`.gitignore`, the workflow via `install_file` (so an existing one lands
as `.keeler`). Gains: `--no-ci`; `KEELER_REF` substitution — the
template carries `KEELER_REF: v@VERSION@`, the installer writes the
resolved line (from `$SRC/VERSION`) through a temp file, and the
comparison `install_file` makes against an existing workflow ignores the
`KEELER_REF:` line on both sides, so a repin alone is "already there";
the stale report. Stale detection: by path for `.claude/commands/keeler/`,
`.claude/skills/gherkin-specs`, `.claude/skills/property-testing`,
`.claude/keeler.md`, `scripts/keeler-graph.sh`, `KEELER.md`,
`.cargo-mutants.toml`; by content for a justfile of any spelling holding
a line matching `^keeler-spawn.*:`; by line for `CLAUDE.md`
(`^@\.claude/keeler\.md`). Report is prose, one `git rm -r --` line with
every stale file, one line naming the CLAUDE.md import; exit zero.

**CI template.** Top: `env:` with `KEELER_REF: v@VERSION@` and, commented
out, `KEELER_COV_MIN` and `KEELER_CRAP_MAX`. Each job with a gate:
`- uses: actions/cache@v4` with `id: keeler`, `path: ${{ runner.temp
}}/keeler`, `key: keeler-${{ env.KEELER_REF }}` (the `env` context is
available in `steps.*.with` per GitHub's context-availability table),
then a step `if: steps.keeler.outputs.cache-hit != 'true'` whose shell
is `set -euo pipefail; mkdir -p "$RUNNER_TEMP/keeler"; if ! curl -fsSL
"https://codeload.github.com/minikin/keeler/tar.gz/${KEELER_REF}" | tar
-xz -C "$RUNNER_TEMP/keeler" --strip-components=1; then echo "cannot
fetch Keeler at ${KEELER_REF} — check the KEELER_REF line in
.github/workflows/keeler.yml" >&2; exit 1; fi` — `pipefail` set
explicitly, because a `run:` step's default is `bash -e` without it and
the pipeline's status would be tar's. `branch-baseline` keeps its
`crap-baseline.json` diff check, drops the `justfiles_at` /
`cov_recipe` block, and its `moved=` text says `keeler keeler-land`.
`review-record` untouched.

**xtask.** `plugin-check` (new subcommand, no CLI arguments; the
repository root comes from the dispatcher the way `release-guard`'s
does, so a fixture root can be handed in): reads `VERSION`,
`.claude-plugin/plugin.json` (`version`), `.claude-plugin/marketplace.json`
(`plugins[name=="keeler"].version` — a documented field: the docs list
it second in `/plugin update`'s version resolution, after
`plugin.json`'s), `keeler.md` (marker; byte length ≤ 9,500, a byte
because `wc -c` is what a contributor will reach for); reports every
disagreement,
in `guard.rs`'s `disagreements()` style. `release-guard <tag>` calls it
and reads the marker from `keeler.md`. JSON reading is a small
hand-rolled field scan like `package_version`, or `serde_json` if it is
already in the tree — a fact for /keeler:tasks. `just lint` here:
inside the existing `if [ -e templates/keeler.yml ]` branch, after
shellcheck, `cargo xtask plugin-check`.

**Invariants worth a property test.** Init idempotence over any crate
shape (extends spec 01's). Stale-report exactness over any subset of
the eight markers. Justfile self-containment: every `just` token that
begins a command in a recipe body is followed by `--justfile`. Wrapper
argv: for any argument list, the recorded list is the fixed prefix plus
the arguments verbatim.

**Docs.** README: Install section leads with the plugin, keeps
`curl | bash` as what `/keeler:init` runs, gains the recipe list and the
recommended-skills list that leave `keeler.md`, rewrites the ratcheting
section around `KEELER_COV_MIN` / `KEELER_CRAP_MAX`, states the macOS
12.3 floor, and adds a "Migrating from an install.sh install" note that
quotes the stale report. CONTRIBUTING:
`claude --plugin-dir .`; bump checklist adds `plugin.json` and
`marketplace.json`; the `keeler-upgrade` mention goes. CHANGELOG: an
`[Unreleased]` entry naming the breaking change. `KEELER.md` is unchanged; the review-enforcement paragraph travels
with the gate table into `gates.md`.

### Non-goals

- Rewriting any recipe in Rust; the `Justfile` stays a justfile (spec 05).
- Other agents: the plugin is Claude Code's format and nothing else's.
- Publishing to the official Anthropic marketplace; the repository is the marketplace.
- Removing an earlier install's files automatically, or editing the adopter's `CLAUDE.md`.
- Injecting the rules where no `specs/*.md` exists — including every Rust project that never adopted Keeler.
- `just <recipe>` from the adopter's project without the wrapper on PATH; `keeler <recipe>` is the spelling.
- Per-project amendment of the rules: there is no project copy to edit; project rules go in the project's own `CLAUDE.md`.
- Deciding whether `clippy.toml` and `rustfmt.toml` are Keeler's to install; they land as today, and that question is another spec's.
- Changing what any gate measures or its default thresholds; `KEELER_COV_MIN` and `KEELER_CRAP_MAX` move the bar, not the ruler.
- A checksum on the CI tarball; the tag is the pin, as it is for `install.sh`.
- Non-Rust projects; Windows without WSL.
- A `keeler-upgrade` recipe; `/plugin update` is the upgrade.
