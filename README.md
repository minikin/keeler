# Keeler

> Named after [Leonarde Keeler](https://en.wikipedia.org/wiki/Leonarde_Keeler), builder of the first practical polygraph.

Keeler is a quality-assurance workflow for AI-assisted development: every
change starts as a human-approved spec, is built test-first, and must clear
independent gates: tests, coverage, CRAP score, review, and mutation
testing. A defect may slip past one gate, but rarely past all of them.

## How it works

Every feature begins as a Gherkin specification that a human reviews and
approves before any code exists — that approval is the only moment
requirements get decided. Implementation is strictly test-driven: the
failing test is shown first, then the minimal code, then the refactor.

The result must clear a series of independent verification gates: the test
suite, mechanical line-coverage and complexity-vs-coverage (CRAP) thresholds,
a spec-conformance review, and finally mutation testing — deliberately
planted bugs that the tests are required to catch. A surviving mutant means
the tests are too weak, and the rule is absolute: strengthen the test, never
bend the code to satisfy the tool. A per-function baseline recorded before
each feature guarantees the codebase never gets quietly worse.

Bug fixes and trivial edits take lighter roads. See
[WORKFLOW.md](WORKFLOW.md) for the full picture with diagrams, and
[CLAUDE.md](CLAUDE.md) for the rules the AI operates under.

## Quick start

- **Feature:** `/spec → approve → /tasks → /tdd → /qa → /review → /mutants` —
  or `/feature <problem>` to run the whole examination end to end.
- **Bugfix:** `/fix` — reproduce with a failing regression test first, then
  fix minimally.
- **Trivial** (docs, comments, config): fast path — `just lint`, no spec,
  no ceremony.

See *Change classes* in CLAUDE.md for how to pick the right road.

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
   or `/spec` if you prefer to drive stage by stage.

## Adopting Keeler in an existing project

You don't need to start from the template — one command installs the
workflow into any Rust project:

```bash
curl -fsSL https://raw.githubusercontent.com/minikin/keeler/main/install.sh \
    | bash -s /path/to/your/project
```

(Prefer to read before you run? `git clone https://github.com/minikin/keeler
&& ./keeler/install.sh /path/to/your/project` does the same from a local
checkout.)

It copies the commands, skills, spec template, Justfile, gate configs, and a
`keeler.yml` CI workflow. It never overwrites your files — conflicts land
next to the original as `<name>.keeler` for manual merge — and it prints the
few manual steps (dev-dependencies, tool install, `.gitignore` entries).

**Legacy code will not pass the gates on day one — that's expected.** Adopt
them incrementally:

1. `just crap-baseline` — freeze today's scores. From now on `just
   crap-delta` fails only on *regressions*: the legacy debt is grandfathered,
   new debt is not.
2. Use `just mutants-diff` (changed lines only) — never run full mutation
   testing on a legacy codebase.
3. Set the coverage threshold in the `cov` recipe to today's number, and
   ratchet it up as tested code grows. Same for the CRAP `--threshold`.
4. Write specs for new features only; legacy behavior earns specs
   opportunistically, one `/fix` at a time.

The principle: **the gates guard the delta, not the past.** Every change
must leave the codebase better; nobody is asked to repay ten years of debt
up front.

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
