# Keeler

> Named after [Leonarde Keeler](https://en.wikipedia.org/wiki/Leonarde_Keeler), builder of the first practical polygraph.

A polygraph for AI-written code: spec-first, test-driven development where
every claim the AI makes is verified by an independent channel — tests,
coverage, complexity scoring, review, and mutation testing. You can fool one
channel; you can't fool them all.

See [WORKFLOW.md](WORKFLOW.md) for how it works in plain words, and
[CLAUDE.md](CLAUDE.md) for the rules the AI operates under.

## Quick start

`/spec → approve → /tasks → /tdd → /qa → /review → /mutants` — or
`/feature <problem>` to run the whole examination end to end. Bugs take
`/fix` (reproduce with a failing test first); trivia takes the fast path
(see *Change classes* in CLAUDE.md).

## Using this template

1. Click **Use this template** on GitHub (enable *Template repository* in
   the repo settings after publishing).
2. Rename the crate in `Cargo.toml` (`name = "demo"`) to your project.
3. Install the toolchain:

   ```bash
   cargo install cargo-binstall
   cargo binstall cargo-nextest cargo-llvm-cov cargo-mutants cargo-crap
   brew install just   # or your platform's package manager
   ```

4. Verify the instrument is calibrated: `just dev` must pass on the clean
   template (it ships with a tiny placeholder function so every gate has
   something to measure).
5. Start your first examination: `/feature <problem>` in Claude Code —
   or `/spec` if you prefer to drive stage by stage. The placeholder
   `checked_total` and spec `00` conventions can be deleted with your first
   real feature.

## Skills

The template ships two project skills in `.claude/skills/` that load
themselves when relevant: **property-testing** (an invariant catalog for
proptest — round-trips, idempotence, shape checks) and **gherkin-specs**
(how to write scenarios that actually gate code).

Recommended companions, installed globally on your machine (not part of the
repo): **[rust-best-practices](https://github.com/apollographql/rust-best-practices)**
for idiomatic Rust (built on Apollo's handbook), **[clean-code](https://github.com/jackjin1997/ClawForge)**
for the refactor step of TDD (from the ClawForge collection), and
**[rust-async-patterns](https://github.com/search?q=rust-async-patterns+SKILL.md&type=code)**
once the project grows async code.

## Toolchain

- [just](https://github.com/casey/just) — task runner (`just` lists recipes)
- [cargo-nextest](https://nexte.st) — test runner
- [cargo-llvm-cov](https://github.com/taiki-e/cargo-llvm-cov) — coverage
- [cargo-crap](https://github.com/minikin/cargo-crap) — CRAP score gate (complexity × uncoverage)
- [cargo-mutants](https://mutants.rs) — mutation testing (the control questions)
- [proptest](https://proptest-rs.github.io/proptest/) — property-based tests

## Layout

- `specs/` — Gherkin specs (Given/When/Then), one per feature; `TEMPLATE.md` is the starting point
- `src/` — implementation with unit + property tests inline
- `tests/acceptance.rs` — one acceptance test per spec scenario
- `.claude/commands/` — the workflow slash commands
- `.claude/skills/` — self-triggering knowledge: property-testing, gherkin-specs
- `.github/workflows/ci.yml` — the same gates as physics, not discipline
