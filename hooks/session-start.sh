#!/usr/bin/env bash
# Keeler's SessionStart hook: prints the rules into a Keeler project's
# session, and nothing anywhere else.
#
# Claude Code adds a zero-exit hook's stdout to the session verbatim, so
# what this prints is what the agent reads as law — hence `cat` and no
# framing. Output starting with `{` would be parsed as JSON instead, and
# output over 10,000 characters is filed away and replaced by a preview:
# the rules arriving partially and silently. `cargo xtask plugin-check`
# holds keeler.md under 9,500 bytes so neither can happen here.
set -euo pipefail

# The plugin root is Claude Code's to tell us, and it moves with every
# version. Guessing it — a relative path, a cache directory — would print
# some other Keeler's rules, or a stranger's, without saying so.
[ -n "${CLAUDE_PLUGIN_ROOT:-}" ] || {
    echo "session-start: CLAUDE_PLUGIN_ROOT is unset" >&2
    exit 1
}

# Once the plugin is enabled the hook runs at every session start on the
# machine — a colleague's crate, a dependency checkout, a `cargo new`. A
# spec is the one thing every Keeler project has and nothing else does;
# `specs/` alone is a directory somebody made.
ls specs/*.md >/dev/null 2>&1 || exit 0

cat "$CLAUDE_PLUGIN_ROOT/keeler.md"
