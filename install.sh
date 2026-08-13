#!/usr/bin/env bash
# Keeler installer: sets the workflow up in a Rust project, end to end —
# tooling, workflow files, Cargo.toml sections, .gitignore entries.
#
# Safe to re-run: existing files are never overwritten (conflicts are
# written alongside as <name>.keeler), already-installed tools are skipped,
# and Cargo.toml / .gitignore entries are added only when missing.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/minikin/keeler/main/install.sh | bash -s .
#   ./install.sh /path/to/project        # from a clone
#   ./install.sh . --no-tools            # skip installing CLI tools
#
# KEELER_REF pins the version to install — a tag or a branch:
#   KEELER_REF=v0.1.0 curl -fsSL .../install.sh | bash -s .
set -euo pipefail

# codeload takes tags and branches in the same short form.
REPO_TARBALL="${KEELER_TARBALL:-https://codeload.github.com/minikin/keeler/tar.gz/${KEELER_REF:-main}}"
DEST="."
WITH_TOOLS=1
for arg in "$@"; do
    case "$arg" in
        --no-tools) WITH_TOOLS=0 ;;
        -h|--help) sed -n '2,12p' "${BASH_SOURCE[0]}"; exit 0 ;;
        *) DEST="$arg" ;;
    esac
done

say() { printf '\n\033[1m%s\033[0m\n' "$*"; }
ok()  { printf '  ✓ %s\n' "$*"; }
note(){ printf '  · %s\n' "$*"; }

[ -f "$DEST/Cargo.toml" ] || {
    echo "error: $DEST is not a Rust project (no Cargo.toml)" >&2
    exit 1
}
DEST="$(cd "$DEST" && pwd)"

# --- Source: local checkout when present, otherwise fetch a tarball --------
SRC="$(cd "$(dirname "${BASH_SOURCE[0]:-.}")" 2>/dev/null && pwd || pwd)"
if [ ! -f "$SRC/specs/TEMPLATE.md" ]; then
    tmp="$(mktemp -d)"
    trap 'rm -rf "$tmp"' EXIT
    say "Fetching Keeler"
    curl -fsSL "$REPO_TARBALL" | tar -xz -C "$tmp" --strip-components=1
    SRC="$tmp"
    ok "downloaded"
fi

KEELER_VERSION="$(cat "$SRC/VERSION" 2>/dev/null || echo unknown)"
say "Keeler $KEELER_VERSION"

# --- 1. Tools -------------------------------------------------------------
if [ "$WITH_TOOLS" = 1 ]; then
    say "Checking tools"
    missing=()
    for t in nextest llvm-cov mutants crap; do
        cargo "$t" --version >/dev/null 2>&1 && ok "cargo-$t" || missing+=("cargo-$t")
    done
    command -v just >/dev/null 2>&1 && ok "just" || missing+=("just")

    if [ "${#missing[@]}" -gt 0 ]; then
        note "installing: ${missing[*]}"
        if ! command -v cargo-binstall >/dev/null 2>&1; then
            note "installing cargo-binstall first"
            cargo install --locked cargo-binstall
        fi
        # --locked is passed through to cargo-install when binstall falls back
        # to compiling from source; nextest refuses to build without it.
        cargo binstall --no-confirm --locked "${missing[@]}"
    fi

    # cargo-llvm-cov needs this rustup component; without it coverage fails
    # with an unhelpful error.
    if command -v rustup >/dev/null 2>&1; then
        rustup component add llvm-tools-preview >/dev/null 2>&1 && ok "llvm-tools-preview"
    fi
fi

# --- 2. Workflow files ----------------------------------------------------
say "Installing workflow files"
copied=0
merges=()
# install_file <source path> [destination path, when it differs from the source]
install_file() {
    local from="$SRC/$1" rel="${2:-$1}" to="$DEST/${2:-$1}"
    mkdir -p "$(dirname "$to")"
    if [ -e "$to" ]; then
        cmp -s "$from" "$to" || { cp "$from" "$to.keeler"; merges+=("$rel"); }
    else
        cp "$from" "$to"
        copied=$((copied + 1))
    fi
}

# Commands live under .claude/commands/keeler/ so they are invoked as
# /keeler:spec, /keeler:fix, … and never collide with a project's own
# commands of the same name.
for f in .claude/commands/keeler/spec.md .claude/commands/keeler/tasks.md \
         .claude/commands/keeler/tdd.md .claude/commands/keeler/qa.md \
         .claude/commands/keeler/review.md .claude/commands/keeler/mutants.md \
         .claude/commands/keeler/feature.md .claude/commands/keeler/fix.md \
         .claude/skills/property-testing/SKILL.md \
         .claude/skills/gherkin-specs/SKILL.md \
         specs/TEMPLATE.md KEELER.md Justfile \
         .cargo-mutants.toml clippy.toml rustfmt.toml; do
    install_file "$f"
done

# The rules file is not installed like the others: it is ours to own, so an
# upgrade replaces it wholesale and the version marker inside always matches
# what we just installed. The copy being replaced is kept as .bak — the file
# is Keeler's, but a project that edited it anyway must not lose the text
# silently. Project-specific instructions belong in CLAUDE.md, never touched.
rules="$DEST/.claude/keeler.md"
mkdir -p "$(dirname "$rules")"
if [ ! -e "$rules" ]; then
    cp "$SRC/.claude/keeler.md" "$rules"
    copied=$((copied + 1))
elif ! cmp -s "$SRC/.claude/keeler.md" "$rules"; then
    cp "$rules" "$rules.bak"
    cp "$SRC/.claude/keeler.md" "$rules"
    note "workflow rules updated to $KEELER_VERSION (previous kept as .claude/keeler.md.bak)"
fi

# CLAUDE.md is never overwritten or duplicated: the rules live in
# .claude/keeler.md and CLAUDE.md merely imports them with one @-line, so a
# project that already has its own CLAUDE.md keeps every word of it.
claude_md="$DEST/CLAUDE.md"
if [ ! -e "$claude_md" ]; then
    cat > "$claude_md" <<'MD'
# CLAUDE.md

This file provides guidance to Claude Code when working with code in this repository.

The Keeler workflow rules live in their own file so they can be updated
without touching your project's instructions:

@.claude/keeler.md

<!-- Add project-specific instructions below this line. -->
MD
    copied=$((copied + 1))
    ok "CLAUDE.md created (imports .claude/keeler.md)"
elif grep -q '^@\.claude/keeler\.md' "$claude_md"; then
    ok "CLAUDE.md already imports the Keeler rules"
else
    cat >> "$claude_md" <<'MD'

## Keeler workflow

This project follows the Keeler spec-first, test-driven workflow:

@.claude/keeler.md
MD
    ok "CLAUDE.md left intact — appended a one-line import"
fi

# CI goes in under its own name so it never clashes with existing workflows.
# Like every other installed file, a project's own copy is never rewritten:
# when the gates change, the new workflow lands alongside as .keeler so the
# project can merge it. Leaving it untouched instead would strand every
# existing project on the workflow it first installed.
install_file templates/keeler.yml .github/workflows/keeler.yml
ok "$copied file(s) installed"
[ "${#merges[@]}" -gt 0 ] && for m in "${merges[@]}"; do
    note "$m differs — wrote $m.keeler, merge by hand"
done

# --- 3. Cargo.toml --------------------------------------------------------
say "Configuring Cargo.toml"
manifest="$DEST/Cargo.toml"
if grep -q '^\[workspace\]' "$manifest" && ! grep -q '^\[package\]' "$manifest"; then
    note "workspace root — add proptest and the profile to member crates yourself"
else
    if grep -q '^proptest' "$manifest" || grep -qA20 '^\[dev-dependencies\]' "$manifest" | grep -q proptest; then
        ok "proptest already a dev-dependency"
    else
        (cd "$DEST" && cargo add --dev --quiet proptest) && ok "proptest added (dev-dependency)"
    fi

    # cargo-mutants builds every mutant — dropping debug info keeps it fast.
    if grep -q '^\[profile\.mutants\]' "$manifest"; then
        ok "[profile.mutants] present"
    else
        printf '\n[profile.mutants]\ninherits = "dev"\ndebug = 0\n' >> "$manifest"
        ok "[profile.mutants] added"
    fi

    if grep -q '^\[lints\.clippy\]' "$manifest"; then
        ok "[lints.clippy] present"
    else
        cat >> "$manifest" <<'TOML'

[lints.clippy]
pedantic = { level = "warn", priority = -1 }
allow_attributes = "warn"
allow_attributes_without_reason = "warn"
TOML
        ok "[lints.clippy] added (pedantic)"
    fi
fi

# --- 4. .gitignore --------------------------------------------------------
say "Updating .gitignore"
gitignore="$DEST/.gitignore"
touch "$gitignore"
added=0
# crap-baseline.json is deliberately NOT ignored: it is the shared
# reference the delta gate measures against, so it belongs in git.
for entry in '/target' 'lcov.info' 'crap-report.json' 'mutants.out*/'; do
    grep -qxF "$entry" "$gitignore" || { printf '%s\n' "$entry" >> "$gitignore"; added=$((added + 1)); }
done
ok "$added entry(ies) added"

# --- Done -----------------------------------------------------------------
say "Keeler installed in $DEST"
cat <<'NEXT'

Next:
  just crap-baseline    # freeze today's scores — gates then guard the delta
  just dev              # fmt, clippy, tests, coverage, CRAP

Expect `just dev` to flag things on an existing codebase — that is the
point. Legacy debt is grandfathered by the baseline; new debt is not.
See the Install section in README.md, and KEELER.md for the full workflow.

Then open the project in Claude Code and run:
  /keeler:feature <what you want to build>
NEXT
