---
name: Bug report
about: Something Keeler does is wrong
title: ''
labels: bug
assignees: ''
---

**What happened, and what you expected instead**
One or two sentences. If a gate or a recipe reported something, paste what it
said rather than paraphrasing it.

**How to reproduce**
The shortest sequence that shows it — ideally starting from a throwaway
project, since that is how the test harness will pin it:

```bash
cargo new --lib /tmp/probe
./install.sh /tmp/probe --no-tools
# then …
```

**Environment**

- Keeler version: <!-- the keeler-version marker at the top of .claude/keeler.md, or the tag you installed -->
- OS: <!-- macOS 15, Ubuntu 24.04, WSL2, … -->
- Shell: <!-- bash 5.2, zsh 5.9, … -->
- `just --version`, `cargo --version`:
- Graph mode involved? <!-- yes/no; if yes, `tmux -V` and the output of `just keeler-status <spec>` -->

**Anything else**
Logs under `.keeler/runs/`, the branch you were on, a link to the failing CI
run — whatever you have.
