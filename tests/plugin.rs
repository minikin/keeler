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
