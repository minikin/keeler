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
# What it asserts, so far: the installer exits zero, and every file a clean
# install produces exists in the project afterwards.
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

if ! bash "$repo/install.sh" "$reference" --no-tools > "$work/reference.log" 2>&1; then
    echo "integration-check: the reference install failed — the checker cannot judge anything" >&2
    cat "$work/reference.log" >&2
    exit 1
fi
list_files "$reference" > "$work/reference-after"

# Tracked = what the install added to an empty crate.
comm -13 "$work/reference-before" "$work/reference-after" > "$work/tracked"

if [ ! -s "$work/tracked" ]; then
    echo "integration-check: the reference install added no files — the checker is blind" >&2
    exit 1
fi

# --- 2. Install into the project under test -------------------------------
if ! bash "$install_sh" "$project" --no-tools > "$work/install.log" 2>&1; then
    echo "integration-check: the installer exited non-zero on $project" >&2
    cat "$work/install.log" >&2
    exit 1
fi

# --- 3. Completeness ------------------------------------------------------
missing=()
tracked_count=0
while IFS= read -r rel; do
    [ -n "$rel" ] || continue
    tracked_count=$((tracked_count + 1))
    [ -e "$project/$rel" ] || missing+=("$rel")
done < "$work/tracked"

if [ "${#missing[@]}" -gt 0 ]; then
    echo "integration-check: the install is incomplete — these tracked files never landed:" >&2
    printf '  %s\n' "${missing[@]}" >&2
    exit 1
fi

echo "integration-check: $tracked_count tracked files present in $project"
