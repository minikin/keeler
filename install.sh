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
if ! SRC="$(cd "$(dirname "${BASH_SOURCE[0]:-.}")" 2>/dev/null && pwd)"; then
    SRC="$(pwd)"
fi
# The sentinel must be repo-only: Keeler ships specs/TEMPLATE.md into every
# project, so probing for a shipped file would mistake an already-Keelered
# project for the source and turn every piped upgrade into a silent no-op.
if [ ! -f "$SRC/VERSION" ] || [ ! -f "$SRC/templates/keeler.yml" ]; then
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
    # -L as well as -e: `-e` is false for a symlink whose target does not
    # exist, and `cp` would then write *through* the link — outside the
    # project, which is the one boundary this script promises to keep. A
    # symlink is the project's own content whatever it points at.
    if [ -e "$to" ] || [ -L "$to" ]; then
        cmp -s "$from" "$to" 2>/dev/null || { cp "$from" "$to.keeler"; merges+=("$rel"); }
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
if [ ! -e "$rules" ] && [ ! -L "$rules" ]; then
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
# Kept so a broken edit can be undone rather than shipped.
manifest_backup="$(mktemp)"
cp "$manifest" "$manifest_backup"
trap 'rm -f "$manifest_backup"' EXIT
# Whether cargo could read it *before* we touched it. A project with no
# sources yet cannot be parsed either, and refusing to install into one
# would punish a state we did not create — so the check below only fires
# when the edits are what broke it.
manifest_read_before=1
if command -v cargo >/dev/null 2>&1; then
    cargo metadata --manifest-path "$manifest" --no-deps --format-version 1 \
        >/dev/null 2>&1 || manifest_read_before=0
fi
if grep -q '^\[workspace\]' "$manifest" && ! grep -q '^\[package\]' "$manifest"; then
    note "workspace root — add proptest and the profile to member crates yourself"
else
    # Matches both declaration forms — `proptest = …` and the
    # `[dev-dependencies.proptest]` table — without catching proptest-derive.
    if grep -qE '^proptest[[:space:]]*=|^\[dev-dependencies\.proptest\]' "$manifest"; then
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

    # A [lints] table of any shape means the project already decides its
    # lints — often by inheriting them (`workspace = true`), which cargo
    # refuses to let a manifest override. Appending [lints.clippy] beside
    # one makes the file unparseable and every cargo command in the project
    # fail, and a second run does not repair it: the grep matches and skips.
    if grep -q '^\[lints\]' "$manifest"; then
        note "[lints] already set here — add Keeler's clippy lints yourself if you want them"
    elif grep -q '^\[lints\.clippy\]' "$manifest"; then
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

# The manifest is the project's build. Nothing above is worth leaving a
# file cargo cannot read, so if the edits broke it, put it back and say so
# rather than reporting success over a project that no longer builds.
if [ "$manifest_read_before" = 1 ] && command -v cargo >/dev/null 2>&1; then
    if ! cargo metadata --manifest-path "$DEST/Cargo.toml" --no-deps \
        --format-version 1 >/dev/null 2>&1; then
        if [ -f "$manifest_backup" ]; then
            cp "$manifest_backup" "$DEST/Cargo.toml"
        fi
        echo "error: the Cargo.toml edits left a manifest cargo cannot read" >&2
        echo "       the file has been restored; nothing else was undone" >&2
        exit 1
    fi
fi

# --- 4. .gitignore --------------------------------------------------------
say "Updating .gitignore"
gitignore="$DEST/.gitignore"
touch "$gitignore"
# A final line with no newline would glue the first appended entry onto it —
# breaking both the entry and the next run's already-present check.
if [ -s "$gitignore" ] && [ -n "$(tail -c1 "$gitignore")" ]; then
    echo >> "$gitignore"
fi
added=0
# crap-baseline.json is deliberately NOT ignored: it is the shared
# reference the delta gate measures against, so it belongs in git.
# Slash placement doesn't change what these patterns cover here, so a
# project's `target/` already covers our `/target` — equivalent forms must
# not pile up as duplicates.
for entry in '/target' 'lcov.info' 'crap-report.json' 'mutants.out*/'; do
    core="${entry#/}"; core="${core%/}"
    grep -qxF -e "$core" -e "/$core" -e "$core/" -e "/$core/" "$gitignore" \
        || { printf '%s\n' "$entry" >> "$gitignore"; added=$((added + 1)); }
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
