#!/usr/bin/env bash
# Assert the installer's contract on a project directory.
#
# Usage: integration-check.sh <project-dir>
#
# CI points this at shallow clones of pinned real-world repositories; the
# harness points it at generated shapes. The script itself never clones —
# it takes a directory that already exists, which is what keeps the local
# suite offline (spec 03).
#
# What it asserts: the installer exits zero; every file a clean install
# produces exists in the project afterwards; nothing the project already had
# changed, bar the three documented append targets; every conflict is named
# and matches the .keeler files on disk; a workspace root is told that its
# manifest is the project's own to manage; and a second run moves nothing.
#
# The tracked file set is *derived*, never listed: a reference install into
# an empty crate says what a correct install produces, so the set cannot
# drift from what install.sh actually does.
#
# KEELER_INSTALL_SH overrides the installer under test — the seam the
# harness uses to hand this script a deliberately defective installer. The
# reference always comes from this repository, so a defective installer
# cannot quietly redefine what "complete" means.
set -euo pipefail

# One collation for the whole script. `sort` and `comm` must agree on order
# or comm silently mis-pairs: under a UTF-8 locale "Cargo.toml" and
# "CLAUDE.md" sort in the opposite order to C, and a file present in both
# lists gets reported as newly added.
export LC_ALL=C

project="${1:?usage: integration-check.sh <project-dir>}"
if [ ! -d "$project" ]; then
    echo "integration-check: $project is not a directory" >&2
    exit 1
fi
project="$(cd "$project" && pwd)"

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
install_sh="${KEELER_INSTALL_SH:-$repo/install.sh}"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# Every refusal says what went wrong and shows the evidence behind it, so a
# CI failure is readable without rerunning anything locally.
fail() {
    printf 'integration-check: %s\n' "$@" >&2
    exit 1
}

fail_with_log() {
    local log="$1"
    shift
    printf 'integration-check: %s\n' "$@" >&2
    cat "$log" >&2
    exit 1
}

# Every file in a directory, relative and sorted. `.git` is the project's
# own business and never part of the install set.
list_files() {
    (cd "$1" && find . -type f -not -path './.git/*' | sed 's|^\./||' | sort)
}

# --- 1. What does a correct install produce? ------------------------------
reference="$work/reference"
mkdir -p "$reference/src"
printf '[package]\nname = "keeler-reference"\nversion = "0.0.0"\nedition = "2021"\n' \
    > "$reference/Cargo.toml"
printf 'pub fn reference() {}\n' > "$reference/src/lib.rs"
list_files "$reference" > "$work/reference-before"

# The reference must describe what the installer *copies*, not what its
# tools leave behind. A real `cargo add` writes Cargo.lock; a workspace
# root skips `cargo add` entirely, so it would then be failed for missing a
# file Keeler never installs. A no-op cargo keeps the reference to the file
# set alone — and makes it identical on every machine.
mkdir -p "$work/stub"
printf '#!/usr/bin/env bash\nexit 0\n' > "$work/stub/cargo"
chmod +x "$work/stub/cargo"

if ! PATH="$work/stub:$PATH" bash "$repo/install.sh" "$reference" --no-tools \
    > "$work/reference.log" 2>&1; then
    fail_with_log "$work/reference.log" \
        "the reference install failed — the checker cannot judge anything"
fi
list_files "$reference" > "$work/reference-after"

# Tracked = what the install added to an empty crate.
comm -13 "$work/reference-before" "$work/reference-after" > "$work/tracked"

if [ ! -s "$work/tracked" ]; then
    fail "the reference install added no files — the checker is blind"
fi

# --- 2. What did the project already have? --------------------------------
# A copy, not a list of hashes: comparing with `cmp` needs no digest tool and
# says nothing about how the bytes differ, only that they do.
before="$work/before"
mkdir -p "$before"
(cd "$project" && tar -cf - --exclude=./.git .) | (cd "$before" && tar -xf -)
list_files "$project" > "$work/project-before"

# --- 3. Install into the project under test -------------------------------
if ! bash "$install_sh" "$project" --no-tools > "$work/install.log" 2>&1; then
    fail_with_log "$work/install.log" "the installer exited non-zero on $project"
fi

# --- 4. Completeness ------------------------------------------------------
missing=()
tracked_count=0
while IFS= read -r rel; do
    [ -n "$rel" ] || continue
    tracked_count=$((tracked_count + 1))
    [ -e "$project/$rel" ] || missing+=("$rel")
done < "$work/tracked"

if [ "${#missing[@]}" -gt 0 ]; then
    echo "integration-check: the install is incomplete — these never landed:" >&2
    printf '  %s\n' "${missing[@]}" >&2
    exit 1
fi

echo "integration-check: $tracked_count tracked files present in $project"

# --- 5. Nothing of theirs changed -----------------------------------------
# Three files may be edited, and only by appending: CLAUDE.md gains the
# rules import, .gitignore gains entries, Cargo.toml gains sections. Every
# other file the project already had must come out byte-identical — a
# project's own content is not Keeler's to rewrite.
clobbered=()
while IFS= read -r rel; do
    [ -n "$rel" ] || continue
    case "$rel" in
        CLAUDE.md | .gitignore | Cargo.toml) continue ;;
    esac
    if [ ! -e "$project/$rel" ]; then
        clobbered+=("$rel (deleted)")
    elif ! cmp -s "$before/$rel" "$project/$rel"; then
        clobbered+=("$rel")
    fi
done < "$work/project-before"

if [ "${#clobbered[@]}" -gt 0 ]; then
    echo "integration-check: the installer changed content that was not its own:" >&2
    printf '  %s\n' "${clobbered[@]}" >&2
    exit 1
fi

# --- 6. Every conflict named, and named honestly --------------------------
# A file the project already had, whose content differs from Keeler's, is
# kept and the Keeler version lands beside it as <name>.keeler. The report
# and the disk must agree: an unnamed .keeler file is a surprise, and a
# named one that does not exist is a lie.
sed -n 's/^  · \(.*\) differs .*wrote .*/\1/p' "$work/install.log" | sort > "$work/reported"
(cd "$project" && find . -type f -name '*.keeler' -not -path './.git/*' | sed 's|^\./||') \
    | while IFS= read -r keeler; do
        [ -e "$before/$keeler" ] || printf '%s\n' "${keeler%.keeler}"
    done | sort > "$work/on-disk"

if ! diff -u "$work/reported" "$work/on-disk" > "$work/conflict-diff" 2>&1; then
    echo "integration-check: the conflicts reported and the .keeler files on disk disagree" >&2
    echo "  (-) reported but absent, (+) present but unreported:" >&2
    cat "$work/conflict-diff" >&2
    exit 1
fi

echo "integration-check: $(wc -l < "$work/on-disk" | tr -d ' ') conflicts named and kept alongside"

# --- 7. A workspace root must be told it is one ---------------------------
# Keeler cannot add proptest and the mutants profile to a root that has no
# [package] of its own — the member crates need them. The installer says so
# rather than silently doing nothing, and a silent installer leaves the
# project half-configured with no hint of it.
if grep -q '^\[workspace\]' "$project/Cargo.toml" && ! grep -q '^\[package\]' "$project/Cargo.toml"; then
    if ! grep -q 'workspace root' "$work/install.log"; then
        fail_with_log "$work/install.log" \
            "this is a workspace root, but the installer never reported the" \
            "root manifest as the project's own to manage"
    fi
    echo "integration-check: the workspace root was told to manage its own manifest"
fi

# --- 8. A second run must change nothing ----------------------------------
# Installing twice is the ordinary case — an upgrade, a re-run after a
# failure, a CI step that does not know the project already has Keeler. The
# second run has no exemptions: not one byte may move, including the three
# files the first run was allowed to append to.
after_first="$work/after-first"
mkdir -p "$after_first"
(cd "$project" && tar -cf - --exclude=./.git .) | (cd "$after_first" && tar -xf -)

if ! bash "$install_sh" "$project" --no-tools > "$work/install-2.log" 2>&1; then
    fail_with_log "$work/install-2.log" "the installer exited non-zero on its second run"
fi

if ! diff -rq -x .git "$after_first" "$project" > "$work/second-run-diff" 2>&1; then
    echo "integration-check: the second run changed the project:" >&2
    cat "$work/second-run-diff" >&2
    exit 1
fi

echo "integration-check: the second run changed nothing"
