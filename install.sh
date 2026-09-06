#!/usr/bin/env bash
# Keeler installer: prepares a Rust project for the workflow — tooling,
# Cargo.toml sections, .gitignore entries, the two tool configs and the CI
# workflow. Nothing else lands: the commands, the skills, the rules, the
# recipes and the graph parser are Keeler's and stay in the plugin, which
# Claude Code reads from its own cache. What is installed here is what
# something other than the plugin reads from the project — GitHub Actions
# reads the workflow from the repository and nowhere else, and clippy and
# rustfmt read their configs from the directory they run in.
#
# Safe to re-run: existing files are never overwritten (conflicts are
# written alongside as <name>.keeler), already-installed tools are skipped,
# and Cargo.toml / .gitignore entries are added only when missing.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/minikin/keeler/main/install.sh | bash -s .
#   ./install.sh /path/to/project        # from a clone
#   ./install.sh . --no-tools            # skip installing CLI tools
#   ./install.sh . --no-ci               # skip the GitHub Actions workflow
#
# KEELER_REF pins the version to install — a tag or a branch:
#   KEELER_REF=v0.4.1 curl -fsSL .../install.sh | bash -s .
set -euo pipefail

# codeload takes tags and branches in the same short form.
REPO_TARBALL="${KEELER_TARBALL:-https://codeload.github.com/minikin/keeler/tar.gz/${KEELER_REF:-main}}"
DEST=""
WITH_TOOLS=1
WITH_CI=1
usage() {
    cat <<'USAGE'
Keeler installer: prepares a Rust project for the workflow.

Usage:
  curl -fsSL https://raw.githubusercontent.com/minikin/keeler/main/install.sh | bash -s .
  ./install.sh /path/to/project        # from a clone
  ./install.sh . --no-tools            # skip installing CLI tools
  ./install.sh . --no-ci               # skip the GitHub Actions workflow

KEELER_REF pins the version to install — a tag or a branch:
  KEELER_REF=v0.4.1 curl -fsSL .../install.sh | bash -s .
USAGE
}
for arg in "$@"; do
    case "$arg" in
        --no-tools) WITH_TOOLS=0 ;;
        --no-ci) WITH_CI=0 ;;
        # A here-doc, not `sed` over BASH_SOURCE[0]: piped into bash there
        # is no file to read, and `set -u` aborted on the documented path.
        -h|--help) usage; exit 0 ;;
        # An unrecognised flag is a typo, not a directory. Taking it as one
        # silently replaces the destination the caller actually gave.
        -*) echo "error: unknown option $arg" >&2; usage >&2; exit 1 ;;
        *)
            if [ -n "$DEST" ]; then
                echo "error: two destinations given ($DEST and $arg)" >&2
                exit 1
            fi
            DEST="$arg"
            ;;
    esac
done
DEST="${DEST:-.}"

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
# The sentinel must be repo-only: probing for a file the install *lands*
# would mistake an already-Keelered project for the source and turn every
# piped upgrade into a silent no-op. `templates/keeler.yml` is the workflow
# before it is installed — under a name no project receives.
# An explicit pin means fetch, always. Piped, the script has no file of
# its own, so SRC becomes the working directory — and a Keeler clone looks
# exactly like an unpacked tarball. Using it would install whatever is
# checked out while the caller believes they pinned a version.
if [ -n "${KEELER_REF:-}${KEELER_TARBALL:-}" ] \
    || [ ! -f "$SRC/VERSION" ] || [ ! -f "$SRC/templates/keeler.yml" ]; then
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

    # tmux is graph mode's requirement, not a cargo tool: every agent
    # `just keeler-spawn` starts lives in a detached session, and binstall
    # has nothing to install it with — so this one is reported, not fixed.
    if command -v tmux >/dev/null 2>&1; then
        ok "tmux"
    else
        note "tmux not found — graph mode (just keeler-spawn) needs it: brew install tmux, or sudo apt-get install tmux"
    fi

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
        if rustup component add llvm-tools-preview >/dev/null 2>&1; then
            ok "llvm-tools-preview"
        else
            note "could not add llvm-tools-preview — coverage will fail until it is installed"
        fi
    fi
fi

# --- 2. The files the project itself is read for ---------------------------
say "Installing project files"
copied=0
merges=()
# install_file <source path> [destination path, when it differs from the source]
# A name that is free, so a copy kept alongside never destroys one kept
# earlier. The first upgrade's .keeler or .bak holds text nobody has merged
# yet; overwriting it loses exactly what it was saved for.
free_name() {
    local candidate="$1" n=1
    while [ -e "$candidate" ] || [ -L "$candidate" ]; do
        candidate="$1.$n"
        n=$((n + 1))
    done
    printf '%s' "$candidate"
}

install_file() {
    local from="$SRC/$1" rel="${2:-$1}" to="$DEST/${2:-$1}"
    mkdir -p "$(dirname "$to")"
    # -L as well as -e: `-e` is false for a symlink whose target does not
    # exist, and `cp` would then write *through* the link — outside the
    # project, which is the one boundary this script promises to keep. A
    # symlink is the project's own content whatever it points at.
    if [ -e "$to" ] || [ -L "$to" ]; then
        cmp -s "$from" "$to" 2>/dev/null || {
            # An existing copy that already holds our version is the record
            # of this same conflict, from an earlier run. Leaving it is what
            # makes a second install change nothing; only a copy holding
            # something else needs a name of its own.
            if cmp -s "$from" "$to.keeler" 2>/dev/null; then
                kept="$to.keeler"
            else
                kept="$(free_name "$to.keeler")"
                cp "$from" "$kept"
            fi
            # The whole line, because the name written is not always
            # <file>.keeler — an earlier upgrade may hold that name.
            merges+=("$rel differs — wrote ${kept#"$DEST/"}, merge by hand")
        }
    else
        cp "$from" "$to"
        copied=$((copied + 1))
    fi
}

# Tool configuration, not workflow: clippy and rustfmt read these from the
# directory they run in, so the plugin cannot hold them on the project's
# behalf. (Whether they are Keeler's to install at all is another spec's
# question; they land as they always have.)
for f in clippy.toml rustfmt.toml; do
    install_file "$f"
done

# CI goes in under its own name so it never clashes with existing workflows.
# GitHub reads workflows from the repository and nowhere else, so this one
# cannot live in the plugin either. Like every other installed file, a
# project's own copy is never rewritten: when the gates change, the new
# workflow lands alongside as .keeler so the project can merge it. Leaving
# it untouched instead would strand every existing project on the workflow
# it first installed.
if [ "$WITH_CI" = 1 ]; then
    install_file templates/keeler.yml .github/workflows/keeler.yml
fi
ok "$copied file(s) installed"
[ "${#merges[@]}" -gt 0 ] && for m in "${merges[@]}"; do
    note "$m"
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
    # Scoped to [dev-dependencies], because that is the only table whose
    # entries the project's tests can use. A whole-file grep counted a
    # [workspace.dependencies] or [build-dependencies] entry as present and
    # left the package unable to compile its property tests. All three
    # declaration forms count — `proptest = …`, `proptest.workspace = …`
    # and the `[dev-dependencies.proptest]` table — and `proptest-derive`
    # does not, since what follows the name must be `=` or `.`.
    if awk '
        /^\[/ {
            in_dev = ($0 ~ /^\[dev-dependencies\][[:space:]]*$/)
            if ($0 ~ /^\[dev-dependencies\.proptest\][[:space:]]*$/) { found = 1 }
            next
        }
        in_dev && /^[[:space:]]*proptest[[:space:]]*[=.]/ { found = 1 }
        END { exit !found }
    ' "$manifest"; then
        ok "proptest already a dev-dependency"
    else
        # Not `&&`: under `set -e` a failure in a non-final element of an
        # && list does not abort, so the script used to report success over
        # a project whose first `just dev` cannot compile.
        if (cd "$DEST" && cargo add --dev --quiet proptest); then
            ok "proptest added (dev-dependency)"
        else
            echo "error: could not add proptest as a dev-dependency" >&2
            exit 1
        fi
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
for entry in '/target' 'lcov.info' 'crap-report.json' 'mutants.out*/' '.keeler/'; do
    core="${entry#/}"; core="${core%/}"
    grep -qxF -e "$core" -e "/$core" -e "$core/" -e "/$core/" "$gitignore" \
        || { printf '%s\n' "$entry" >> "$gitignore"; added=$((added + 1)); }
done
ok "$added entry(ies) added"

# --- 5. What an earlier install left behind -------------------------------
# Keeler used to copy its commands, skills, rules, recipes and graph parser
# into the project; the plugin carries them now. Removing them here is not
# this script's call: an edited copy is indistinguishable from an untouched
# one without every released version at hand, and a deleted edit is the one
# loss the installer has always promised not to cause. So they are named,
# with the command that removes them, and left exactly as they are.
stale=()
for path in .claude/commands/keeler .claude/skills/gherkin-specs \
    .claude/skills/property-testing .claude/keeler.md \
    scripts/keeler-graph.sh KEELER.md .cargo-mutants.toml; do
    if [ -e "$DEST/$path" ] || [ -L "$DEST/$path" ]; then
        stale+=("$path")
    fi
done
# A justfile is Keeler's by what is in it, never by its name: sharing the
# name is what every project with its own recipes does, and naming theirs
# would be telling them to delete their own work. The entry is matched the
# way `just` matches it — case-insensitively, either spelling — so the path
# reported is the one the project actually has.
shopt -s nullglob dotglob nocasematch
for entry in "$DEST"/*; do
    base="${entry##*/}"
    if [ -f "$entry" ] && [[ "$base" == justfile || "$base" == .justfile ]] \
        && grep -qE '^keeler-spawn[^:]*:' "$entry"; then
        stale+=("$base")
    fi
done
shopt -u nullglob dotglob nocasematch
# The file is the project's and only the line is ours, so the line is what
# the report names — and nothing offers to delete the file.
claude_import=""
if [ -f "$DEST/CLAUDE.md" ] && grep -qE '^@\.claude/keeler\.md' "$DEST/CLAUDE.md"; then
    claude_import="@.claude/keeler.md"
fi

if [ "${#stale[@]}" -gt 0 ] || [ -n "$claude_import" ]; then
    say "Left by an earlier Keeler"
    if [ "${#stale[@]}" -gt 0 ]; then
        for path in "${stale[@]}"; do
            note "$path"
        done
        printf '\n  These are no longer installed — the plugin carries them. Nothing\n'
        printf '  here was touched; remove them when you are ready:\n\n'
        printf '    git rm -r -- %s\n' "${stale[*]}"
    fi
    if [ -n "$claude_import" ]; then
        printf '\n  CLAUDE.md still imports the old rules: delete its %s line\n' "$claude_import"
        printf '  by hand. The rules now arrive at every session start, from the plugin.\n'
    fi
fi

# --- Done -----------------------------------------------------------------
say "Keeler installed in $DEST"
cat <<'NEXT'

The recipes live in the plugin, not in this project — run them as
`keeler <recipe>`, with the plugin's bin/ on your PATH:

  keeler crap-baseline  # freeze today's scores — gates then guard the delta
  keeler dev            # fmt, clippy, tests, coverage, CRAP

Expect `keeler dev` to flag things on an existing codebase — that is the
point. Legacy debt is grandfathered by the baseline; new debt is not.

Then open the project in Claude Code and run:
  /keeler:feature <what you want to build>
NEXT
