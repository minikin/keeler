//! Spec 09 — the repository is the plugin.
//!
//! Claude Code installs a plugin by reading its manifests and the files
//! they point at, so these tests read the repository the way Claude Code
//! will: the two manifests, the commands and skills where the plugin
//! format expects them, and the rules the `SessionStart` hook prints.
//!
//! There is no JSON crate in this tree — the harness drives shell — so the
//! manifests are read by field scan. They are ours, written by hand and
//! held to `VERSION` by `cargo xtask plugin-check`; a parser here would be
//! a second implementation of what that gate already refuses.

mod common;

use common::repo_root;

/// The version every manifest and the rules marker must agree with.
fn version() -> String {
    read("VERSION").trim().to_string()
}

fn read(rel: &str) -> String {
    let path = repo_root().join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|why| panic!("cannot read {rel}: {why}"))
}

/// The value of `"key": "value"` at any depth, first occurrence.
fn field(json: &str, key: &str) -> Option<String> {
    let (_, after) = json.split_once(&format!("\"{key}\""))?;
    let (_, after) = after.split_once(':')?;
    let (_, value) = after.trim_start().split_once('"')?;
    let (value, _) = value.split_once('"')?;
    Some(value.to_string())
}

/// The objects of a JSON array field, by brace matching — enough to say how
/// many entries a list has and what each one declares.
fn entries(json: &str, key: &str) -> Vec<String> {
    let Some((_, after)) = json.split_once(&format!("\"{key}\"")) else {
        return Vec::new();
    };
    let mut objects = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (at, byte) in after.char_indices() {
        match byte {
            '{' => {
                if depth == 0 {
                    start = at;
                }
                depth += 1;
            }
            '}' if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    objects.push(after[start..=at].to_string());
                }
            }
            ']' if depth == 0 => break,
            _ => {}
        }
    }
    objects
}

#[test]
fn the_plugin_manifest_names_the_plugin_and_its_version() {
    // Given .claude-plugin/plugin.json
    let manifest = read(".claude-plugin/plugin.json");

    // Then its name is "keeler" — the half of `keeler@keeler` that selects
    // the plugin, and what `${CLAUDE_PLUGIN_ROOT}` is rooted at
    assert_eq!(
        field(&manifest, "name").as_deref(),
        Some("keeler"),
        "plugin.json does not name the plugin `keeler`:\n{manifest}",
    );

    // And its version equals VERSION: `/plugin update` compares this field,
    // so a release that ships a stale one ships nothing
    assert_eq!(
        field(&manifest, "version"),
        Some(version()),
        "plugin.json's version is not VERSION ({}):\n{manifest}",
        version(),
    );
}

#[test]
fn the_marketplace_lists_the_plugin_at_the_repository_root() {
    // Given .claude-plugin/marketplace.json
    let marketplace = read(".claude-plugin/marketplace.json");

    // Then the marketplace is named "keeler", so the install line reads
    // `/plugin install keeler@keeler`
    assert_eq!(
        field(&marketplace, "name").as_deref(),
        Some("keeler"),
        "the marketplace is not named `keeler`:\n{marketplace}",
    );

    // And it lists one plugin, this repository itself
    let listed = entries(&marketplace, "plugins");
    assert_eq!(
        listed.len(),
        1,
        "the marketplace lists {} plugins, not one:\n{marketplace}",
        listed.len(),
    );
    let entry = &listed[0];
    assert_eq!(field(entry, "name").as_deref(), Some("keeler"), "{entry}");
    assert_eq!(
        field(entry, "source").as_deref(),
        Some("./"),
        "the entry's source is not the repository root:\n{entry}",
    );
    assert_eq!(
        field(entry, "version"),
        Some(version()),
        "the marketplace entry's version is not VERSION ({}):\n{entry}",
        version(),
    );
}

#[test]
fn the_repository_registers_its_commands_once() {
    // Given the repository
    // Then the commands and skills live in the plugin and nowhere else. A
    // copy under .claude/ would register every command a second time in
    // this repository's own sessions, and the two copies would drift.
    for stale in [".claude/commands", ".claude/skills"] {
        assert!(
            !repo_root().join(stale).exists(),
            "{stale} still exists — its commands register twice here",
        );
    }
}

#[test]
fn the_skills_ship_in_the_plugin() {
    // Given the plugin root
    for skill in ["gherkin-specs", "property-testing"] {
        let rel = format!("skills/{skill}/SKILL.md");
        assert!(
            repo_root().join(&rel).is_file(),
            "the plugin does not carry {rel}",
        );

        // Then each opens with frontmatter carrying name: and description:
        // — the description is what makes a skill load at the right moment,
        // so a skill without one is a skill that never fires
        let text = read(&rel);
        let Some((frontmatter, _)) = text
            .strip_prefix("---\n")
            .and_then(|rest| rest.split_once("\n---"))
        else {
            panic!("{rel} does not open with frontmatter");
        };
        for key in ["name:", "description:"] {
            assert!(
                frontmatter.lines().any(|line| line.starts_with(key)),
                "{rel}'s frontmatter has no `{key}` line:\n{frontmatter}",
            );
        }
    }
}

#[test]
fn the_rules_carry_the_version() {
    // Given the plugin's keeler.md
    let rules = read("keeler.md");

    // Then its marker equals VERSION — the line a bug report is asked for,
    // and what says which Keeler an agent is obeying
    let marker = rules
        .lines()
        .find_map(|line| {
            line.strip_prefix("<!-- keeler-version: ")
                .and_then(|rest| rest.strip_suffix(" -->"))
        })
        .expect("keeler.md carries no <!-- keeler-version: --> marker");
    assert_eq!(
        marker,
        version(),
        "keeler.md's marker is not VERSION ({})",
        version(),
    );
}

#[test]
fn the_rules_fit_the_hook() {
    // Given the plugin's keeler.md
    let rules = std::fs::read(repo_root().join("keeler.md")).unwrap();

    // Then it fits under the ceiling the SessionStart hook imposes.
    // Claude Code files hook output over 10,000 characters away and hands
    // the agent a preview — the rules arriving partially and silently — so
    // the bar is 500 below it, in bytes, which are never fewer than
    // characters.
    assert!(
        rules.len() <= 9500,
        "keeler.md is {} bytes; the hook truncates above 9500",
        rules.len(),
    );

    // And it does not open with `{`: stdout starting with a brace is
    // parsed as JSON instead of added to the session.
    assert_ne!(
        rules.first(),
        Some(&b'{'),
        "keeler.md opens with `{{`, so the hook's output is read as JSON",
    );
}

#[test]
fn the_rules_say_where_the_rest_is() {
    // Given the plugin's keeler.md
    let rules = read("keeler.md");

    // Then it names the two chapters that left and where the recipes are.
    // Rules that drop a chapter without a pointer do not shrink the law —
    // they hide it.
    for pointer in ["graph-mode.md", "gates.md", "keeler --list"] {
        assert!(rules.contains(pointer), "keeler.md never names `{pointer}`");
    }

    // And it no longer carries those chapters itself
    for heading in [
        "## Graph mode",
        "## Quality gates",
        "## Commands",
        "## Skills",
    ] {
        assert!(
            !rules.contains(heading),
            "keeler.md still carries a `{heading}` section — the rules do not fit the hook twice",
        );
    }
}

/// Every row of the gate table as it stands, as `(gate, command)`. The
/// chapters moved out of the rules whole; a row lost on the way out is a
/// gate an adopter stops running.
const GATES: [(&str, &str); 7] = [
    ("Format", "cargo fmt --all -- --check"),
    ("Lints", "cargo clippy --all-targets -- -D warnings"),
    (
        "Tests",
        "cargo nextest run --all-targets && cargo test --doc",
    ),
    ("Coverage", "just cov"),
    ("CRAP", "just crap"),
    ("Mutation", "just mutants-diff"),
    ("CRAP delta", "just crap-delta"),
];

#[test]
fn the_chapters_that_left_are_whole() {
    // Given the plugin's graph-mode.md and gates.md
    let graph_mode = read("graph-mode.md");
    let gates = read("gates.md");

    // Then graph mode describes every recipe it described in the rules
    for recipe in [
        "keeler-feature-branch",
        "keeler-graph",
        "keeler-fan-out",
        "keeler-spawn",
        "keeler-status",
        "keeler-resume",
        "keeler-branch",
        "keeler-land",
    ] {
        assert!(
            graph_mode.contains(recipe),
            "graph-mode.md does not describe `{recipe}`",
        );
    }

    // And gates.md holds the table, row for row ...
    for (gate, command) in GATES {
        let row = gates
            .lines()
            .find(|line| line.starts_with(&format!("| {gate} ")))
            .unwrap_or_else(|| panic!("gates.md has no `{gate}` row:\n{gates}"));
        assert!(
            row.contains(command),
            "gates.md's `{gate}` row no longer runs `{command}`: {row}",
        );
    }

    // ... and the paragraph that says the baseline is the shared reference
    let discipline = gates
        .split("\n\n")
        .find(|paragraph| paragraph.contains("Baseline discipline"))
        .expect("gates.md dropped the baseline-discipline paragraph");
    for token in ["crap-baseline.json", "crap-delta", "crap-baseline"] {
        assert!(
            discipline.contains(token),
            "the baseline-discipline paragraph no longer names `{token}`:\n{discipline}",
        );
    }
}

#[test]
fn keelers_own_claude_md_imports_the_rules_from_the_plugin_root() {
    // Given the repository's CLAUDE.md
    let claude_md = read("CLAUDE.md");

    // Then it imports the rules where they now live ...
    assert!(
        claude_md.lines().any(|line| line.trim() == "@keeler.md"),
        "CLAUDE.md does not import @keeler.md:\n{claude_md}",
    );

    // ... and nowhere names the path they left. Keeler develops Keeler
    // with `claude --plugin-dir .`, so an import of a file that is gone is
    // the repository failing to eat what it ships.
    let stale: Vec<&str> = claude_md
        .lines()
        .filter(|line| line.contains(".claude/keeler.md"))
        .collect();
    assert!(
        stale.is_empty(),
        "CLAUDE.md still names .claude/keeler.md:\n{}",
        stale.join("\n"),
    );
}

// ---------------------------------------------------------------------------
// T6 — the commands speak the plugin's language
// ---------------------------------------------------------------------------

/// Every stage, one file each. `commands/<name>.md` is what makes
/// `/keeler:<name>` exist, so this list is the pipeline as Claude Code
/// sees it.
const COMMANDS: [&str; 10] = [
    "feature.md",
    "fix.md",
    "graph.md",
    "init.md",
    "mutants.md",
    "qa.md",
    "review.md",
    "spec.md",
    "tasks.md",
    "tdd.md",
];

fn command(name: &str) -> String {
    read(&format!("commands/{name}"))
}

/// A command's frontmatter and the body under it.
fn split_frontmatter(name: &str, text: &str) -> (String, String) {
    let Some((frontmatter, body)) = text
        .strip_prefix("---\n")
        .and_then(|rest| rest.split_once("\n---\n"))
    else {
        panic!("commands/{name} does not open with frontmatter");
    };
    (frontmatter.to_string(), body.to_string())
}

/// Every recipe the plugin's Justfile defines. A command that names one of
/// these is invoking a recipe and has to spell it the wrapper's way; a
/// command that names anything else is running something else entirely.
/// `tests/justfile.rs` reads recipe headers too, in another binary and for
/// a harder question — here only the names at the left margin matter.
fn recipe_names() -> Vec<String> {
    read("Justfile")
        .lines()
        .filter_map(|line| {
            if line.starts_with(char::is_whitespace) || line.trim().is_empty() {
                return None;
            }
            let (head, tail) = line.split_once(':')?;
            if tail.starts_with('=') || head.contains('#') {
                return None;
            }
            let name = head.split_whitespace().next()?;
            name.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
                .then(|| name.to_string())
        })
        .collect()
}

/// Every command a command file puts in front of the agent: the content of
/// each inline code span, and each line of each fenced block. Prose is not
/// scanned — what a command means to be run, it writes as code.
fn shown_commands(text: &str) -> Vec<String> {
    let mut shown = Vec::new();
    let mut fenced = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            shown.push(line.trim().to_string());
            continue;
        }
        for (index, span) in line.split('`').enumerate() {
            if index % 2 == 1 {
                shown.push(span.trim().to_string());
            }
        }
    }
    shown.retain(|span| !span.is_empty());
    shown
}

#[test]
fn every_stage_is_a_plugin_command() {
    // Given the plugin's commands/ directory
    let mut found: Vec<String> = std::fs::read_dir(repo_root().join("commands"))
        .expect("the plugin has no commands/ directory")
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    found.sort();

    // Then it holds exactly the ten stages — a file more is a command the
    // pipeline never names, a file fewer is a stage that cannot be run
    assert_eq!(found, COMMANDS, "commands/ is not the ten stages");

    // And each opens with frontmatter carrying a description: the line
    // Claude Code shows in its command list, and all a user has to go on
    for name in COMMANDS {
        let (frontmatter, _) = split_frontmatter(name, &command(name));
        assert!(
            frontmatter
                .lines()
                .any(|line| line.starts_with("description:")),
            "commands/{name}'s frontmatter has no `description:` line:\n{frontmatter}",
        );
    }
}

#[test]
fn a_command_that_runs_a_recipe_runs_it_through_the_wrapper() {
    // Given every file under the plugin's commands/
    let recipes = recipe_names();
    let mut wrapped = 0;

    for name in COMMANDS {
        for shown in shown_commands(&command(name)) {
            // A command can be handed its environment first —
            // `KEELER_FAN_OUT_YES=1 keeler keeler-fan-out` is a line the
            // Justfile prints — so the invocation starts at the first word
            // that is not an assignment.
            let mut words = shown.split_whitespace().skip_while(|word| {
                word.split_once('=')
                    .is_some_and(|(name, _)| !name.is_empty())
            });
            let first = words.next().unwrap_or_default();

            // Then no line invokes `just`, wherever in the line it sits:
            // the recipes live in the plugin, and an adopter's project has
            // no justfile to reach them by
            assert!(
                !shown.split_whitespace().any(|word| word == "just"),
                "commands/{name} invokes `just`, which an adopter has no \
                 justfile for: `{shown}`",
            );

            // And a recipe is never named bare either — `dev` alone is
            // `just dev` with the word left off
            assert!(
                !recipes.iter().any(|recipe| recipe == first),
                "commands/{name} runs the recipe `{first}` without the \
                 wrapper: `{shown}`",
            );

            // And every recipe invocation reads `keeler <recipe>`, naming a
            // recipe that exists. The bare word is the wrapper itself —
            // what a command tells a human to put on their PATH — and
            // invokes nothing.
            if first == "keeler" {
                let Some(recipe) = words.next() else { continue };
                assert!(
                    recipe.starts_with('-') || recipes.iter().any(|known| known == recipe),
                    "commands/{name} runs `keeler {recipe}`, which the \
                     plugin's Justfile does not define: `{shown}`",
                );
                wrapped += 1;
            }
        }
    }

    // And the scan saw the invocations at all: a reader that matches
    // nothing passes a set of commands that has stopped running gates
    assert!(
        wrapped >= 12,
        "only {wrapped} wrapper invocations found; the scan is looking in \
         the wrong place",
    );
}

#[test]
fn no_command_names_a_file_keeler_no_longer_installs() {
    // Given every file under the plugin's commands/
    for name in COMMANDS {
        let text = command(name);

        // Then none names a path that only an install.sh install ever had.
        // A command that names one sends the agent to a file that is not
        // there — or, in a project that adopted Keeler before this version,
        // to a stale copy left behind, holding rules a release behind.
        for gone in [
            ".claude/keeler.md",
            ".claude/commands/",
            "scripts/keeler-graph.sh",
            "KEELER.md",
        ] {
            assert!(
                !text.contains(gone),
                "commands/{name} names `{gone}`, which no project has any more",
            );
        }

        // And every reference to the rules reads the plugin's copy
        for (at, _) in text.match_indices("keeler.md") {
            assert!(
                text[..at].ends_with("${CLAUDE_PLUGIN_ROOT}/"),
                "commands/{name} names the rules as something other than \
                 ${{CLAUDE_PLUGIN_ROOT}}/keeler.md: {}",
                text[..at].lines().next_back().unwrap_or_default(),
            );
        }
    }
}

#[test]
fn a_command_is_never_ruleless() {
    // Given every file under the plugin's commands/
    for name in COMMANDS {
        let (_, body) = split_frontmatter(name, &command(name));

        // Then its first instruction is to read the rules. The
        // SessionStart hook covers the turns between commands, but it
        // speaks only where specs/*.md already exists and only at a
        // session's start — a command that assumed it had spoken would be
        // ruleless in the very session that writes the first spec.
        let first = body
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .unwrap_or_else(|| panic!("commands/{name} has no body"));
        for token in [
            "Read",
            "${CLAUDE_PLUGIN_ROOT}/keeler.md",
            "before anything else",
        ] {
            assert!(
                first.contains(token),
                "commands/{name}'s first instruction does not say `{token}`: {first}",
            );
        }
    }
}

/// Whether some line of this command tells the agent to read that file.
fn tells_the_agent_to_read(text: &str, path: &str) -> bool {
    text.lines()
        .any(|line| line.contains(path) && line.to_lowercase().contains("read"))
}

#[test]
fn the_commands_that_run_gates_are_told_where_the_gate_table_is() {
    // Given the commands that run gates
    for name in ["qa.md", "mutants.md", "review.md"] {
        // Then each tells the agent to read the gate table. The rules keep
        // only the line saying every gate must be green; the bars, the
        // baseline discipline and what enforces the review stage left with
        // the table.
        assert!(
            tells_the_agent_to_read(&command(name), "${CLAUDE_PLUGIN_ROOT}/gates.md"),
            "commands/{name} never tells the agent to read \
             ${{CLAUDE_PLUGIN_ROOT}}/gates.md",
        );
    }
}

#[test]
fn the_commands_that_need_graph_mode_are_told_where_it_is() {
    // Given the commands that act on the graph
    for name in ["graph.md", "spec.md", "tasks.md"] {
        // Then each tells the agent to read the chapter that left the
        // rules — the road question, the `Needs:` lines and the board are
        // documented there and nowhere else now
        assert!(
            tells_the_agent_to_read(&command(name), "${CLAUDE_PLUGIN_ROOT}/graph-mode.md"),
            "commands/{name} never tells the agent to read \
             ${{CLAUDE_PLUGIN_ROOT}}/graph-mode.md",
        );
    }
}

#[test]
fn the_spec_command_prefers_the_projects_template() {
    // Given the plugin's commands/spec.md
    let text = command("spec.md");

    // Then one line names both templates, the project's first: a project
    // that keeps its own specs/TEMPLATE.md has a shape it chose, and the
    // plugin's is the fallback for the projects that hold nothing of
    // Keeler's at all
    let line = text
        .lines()
        .find(|line| line.contains("specs/TEMPLATE.md"))
        .unwrap_or_else(|| panic!("commands/spec.md never names specs/TEMPLATE.md"));
    let theirs = line
        .find("specs/TEMPLATE.md")
        .expect("the line names the project's template");
    let ours = line
        .find("${CLAUDE_PLUGIN_ROOT}/templates/spec.md")
        .unwrap_or_else(|| {
            panic!("commands/spec.md does not fall back to the plugin's template:\n{line}")
        });
    assert!(
        theirs < ours,
        "commands/spec.md reaches for the plugin's template first:\n{line}",
    );
    for token in ["exists", "otherwise"] {
        assert!(
            line.contains(token),
            "commands/spec.md does not say `{token}`, so the preference is \
             not a condition:\n{line}",
        );
    }

    // And the fallback is a file the plugin ships
    assert!(
        repo_root().join("templates/spec.md").is_file(),
        "the plugin carries no templates/spec.md",
    );
}

#[test]
fn the_init_command_runs_the_installer_from_the_plugin() {
    // Given the plugin's commands/init.md
    let text = command("init.md");

    // Then it runs the installer out of the plugin — there is no copy in
    // the project, and a `curl | bash` would fetch a released Keeler over
    // the one that is running
    assert!(
        text.contains("bash \"${CLAUDE_PLUGIN_ROOT}/install.sh\" ."),
        "commands/init.md does not run the plugin's installer:\n{text}",
    );

    // And it forwards the two flags the installer takes. The body is where
    // that instruction has to be: `argument-hint:` in the frontmatter is a
    // hint to the user typing the command, and holds no instruction the
    // agent running it ever reads.
    let (_, body) = split_frontmatter("init.md", &text);
    for flag in ["--no-tools", "--no-ci"] {
        assert!(
            body.contains(flag),
            "commands/init.md never forwards `{flag}`",
        );
    }

    // And it says how a human gets `keeler` in a terminal: the agent's
    // Bash tool has the plugin's bin/ on PATH while the plugin is enabled,
    // and nothing else does
    let path_line = text
        .lines()
        .find(|line| line.contains("${CLAUDE_PLUGIN_ROOT}/bin"))
        .unwrap_or_else(|| panic!("commands/init.md never names the plugin's bin/:\n{text}"));
    assert!(
        path_line.contains("PATH"),
        "commands/init.md names the plugin's bin/ without saying it goes on \
         PATH:\n{path_line}",
    );
}
