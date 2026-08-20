<!-- What changed and why. The diff already says what — say why. -->

**Road taken** (see [CONTRIBUTING.md](https://github.com/minikin/keeler/blob/main/CONTRIBUTING.md)):

- [ ] **Feature** — a spec in `specs/`, approved in this discussion before the
      implementation, then test-first
- [ ] **Bugfix** — a regression test that failed on the current build first,
      then the minimal fix
- [ ] **Trivial** — docs, comments, config; no behavior change

**Gates** — `just dev` locally, and:

- [ ] New or changed behavior is covered by a test named after its scenario
- [ ] `just mutants-diff` — no survivors in the changed files
- [ ] `just crap-delta` — no function's score regressed
- [ ] Docs that had to keep up were updated (`CHANGELOG.md` under
      `[Unreleased]`, and whatever else CONTRIBUTING lists)

<!-- If a box is unticked, say why here — an honest gap beats a ticked box. -->
