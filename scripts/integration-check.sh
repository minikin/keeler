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
# changed, bar the three documented append targets and a Cargo.lock the
# manifest edit explains; every conflict is named
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
    (cd "$1" && find . \( -type f -o -type l \) \
        -not -path './.git/*' -not -path './.git' | sed 's|^\./||' | sort)
}

# The package and every dependency a manifest declares, by name. Asking
# cargo rather than reading the text: the declaration forms are many and
# the point is what the manifest *means*.
manifest_names() {
    # `|| true`: a manifest cargo cannot read yields no names, and with
    # pipefail a grep that matches nothing would otherwise kill the script
    # silently — losing the very refusal this is here to produce.
    { cargo metadata --manifest-path "$1" --no-deps --format-version 1 2>/dev/null \
        | tr ',' '\n' | grep -oE '"name":"[^"]*"' || true; } | sort -u
}

# Whether the first file begins with the whole of the second — what
# "appended to" means, as opposed to "replaced".
starts_with() {
    local size
    size="$(wc -c < "$2" | tr -d ' ')"
    [ "$size" -eq 0 ] || head -c "$size" "$1" 2>/dev/null | cmp -s - "$2"
}

# A file's permission bits, in whichever dialect of stat is present.
file_mode() {
    stat -f '%Lp' "$1" 2>/dev/null || stat -c '%a' "$1" 2>/dev/null || echo unknown
}

# Compares two paths, symlinks included. `cmp` follows links, so a link
# replaced by a regular file of the same content would look unchanged —
# and a link written *through* would look untouched while a file outside
# the project changed.
same_entry() {
    if [ -L "$1" ] || [ -L "$2" ]; then
        [ -L "$1" ] && [ -L "$2" ] && [ "$(readlink "$1")" = "$(readlink "$2")" ]
    else
        cmp -s "$1" "$2"
    fi
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
# Whether cargo could read the manifest before we touched it: a project
# with no sources cannot be parsed either, and holding the installer to
# a state it did not create would be a false failure.
manifest_read_before=0
if command -v cargo >/dev/null 2>&1 \
    && cargo metadata --manifest-path "$project/Cargo.toml" --no-deps \
        --format-version 1 >/dev/null 2>&1; then
    manifest_read_before=1
fi
(cd "$project" && tar -cf - --exclude=./.git .) | (cd "$before" && tar -xf -)
list_files "$project" > "$work/project-before"

# --- 3. Install into the project under test -------------------------------
if ! bash "$install_sh" "$project" --no-tools > "$work/install.log" 2>&1; then
    fail_with_log "$work/install.log" "the installer exited non-zero on $project"
fi

# --- 4. Completeness ------------------------------------------------------
# The reference tree is the oracle and is kept, not thrown away after the
# `comm`: it holds Keeler's exact bytes for every tracked file, which is
# what makes it possible to ask whether a file *is what it should be*
# rather than merely whether something of that name exists.
missing=()
wrong=()
tracked_count=0
while IFS= read -r rel; do
    [ -n "$rel" ] || continue
    tracked_count=$((tracked_count + 1))
    if [ ! -e "$project/$rel" ] && [ ! -L "$project/$rel" ]; then
        missing+=("$rel")
        continue
    fi
    # CLAUDE.md and .gitignore legitimately differ: the installer appends
    # to whatever the project already had. Everything else must be ours,
    # unless the project owned it first, in which case theirs is kept and
    # the conflict check below accounts for it.
    case "$rel" in
        CLAUDE.md | .gitignore) continue ;;
    esac
    if [ ! -e "$before/$rel" ] && ! same_entry "$reference/$rel" "$project/$rel"; then
        wrong+=("$rel")
    fi
done < "$work/tracked"

if [ "${#missing[@]}" -gt 0 ]; then
    echo "integration-check: the install is incomplete — these never landed:" >&2
    printf '  %s\n' "${missing[@]}" >&2
    exit 1
fi

if [ "${#wrong[@]}" -gt 0 ]; then
    echo "integration-check: these landed, but not with Keeler's content:" >&2
    printf '  %s\n' "${wrong[@]}" >&2
    exit 1
fi

echo "integration-check: $tracked_count tracked files present in $project"

# --- 5. Nothing of theirs changed -----------------------------------------
# Three files may be edited, and only by appending: CLAUDE.md gains the
# rules import, .gitignore gains entries, Cargo.toml gains sections. Every
# other file the project already had must come out byte-identical — a
# project's own content is not Keeler's to rewrite.
clobbered=()

# A refreshed Cargo.lock is cargo's record of the manifest edit, not a
# change of Keeler's own: `cargo add --dev proptest` cannot leave the lock
# alone. It is excused only when the manifest actually moved — a lockfile
# that changes with nothing behind it is still the project's loss.
manifest_edited=0
if [ -e "$before/Cargo.toml" ] && ! cmp -s "$before/Cargo.toml" "$project/Cargo.toml"; then
    manifest_edited=1
fi

while IFS= read -r rel; do
    [ -n "$rel" ] || continue
    # Deletion is never permitted, whatever the file. The exemptions below
    # are for *edits*, and skipping this check for them meant a deleted
    # lockfile counted as a refresh.
    if [ ! -e "$project/$rel" ] && [ ! -L "$project/$rel" ]; then
        clobbered+=("$rel (deleted)")
        continue
    fi
    case "$rel" in
        # Append-only, not anything-goes: the contract says these three
        # gain lines, so what they held must still be there, at the front.
        # Exempting them from every comparison let an installer empty a
        # project's .gitignore and pass.
        # The manifest is edited by a TOML-aware tool, which may insert a
        # section rather than append one — so a byte prefix is the wrong
        # law for it. What must hold is that nothing the project declared
        # disappeared; that it still parses is checked further down.
        Cargo.toml)
            if [ "$manifest_read_before" = 1 ]; then
                manifest_names "$before/$rel" > "$work/names-before"
                manifest_names "$project/$rel" > "$work/names-after"
                lost="$(comm -23 "$work/names-before" "$work/names-after" | tr '\n' ' ')"
                if [ -n "${lost// /}" ]; then
                    clobbered+=("$rel (lost:$lost)")
                fi
            fi
            continue
            ;;
        CLAUDE.md | .gitignore)
            # `head -c`, not `cmp -n`: BSD cmp reads -n as "compare at most
            # n bytes and still report EOF on the shorter file", so the
            # prefix test passed on GNU and failed here.
            if ! starts_with "$project/$rel" "$before/$rel"; then
                # A file whose last line had no newline gains one before
                # the first appended entry. That is still an append.
                { cat "$before/$rel"; printf '\n'; } > "$work/with-newline"
                if ! starts_with "$project/$rel" "$work/with-newline"; then
                    clobbered+=("$rel (rewritten, not appended to)")
                fi
            fi
            continue
            ;;
        # Keeler's to own: replaced wholesale, with the replaced text kept.
        .claude/keeler.md)
            if ! same_entry "$before/$rel" "$project/$rel"; then
                if ! cmp -s "$before/$rel" "$project/$rel.bak"; then
                    clobbered+=("$rel (replaced without keeping the old text)")
                fi
            fi
            continue
            ;;
        Cargo.lock)
            if [ "$manifest_edited" -eq 1 ]; then continue; fi
            ;;
    esac
    if ! same_entry "$before/$rel" "$project/$rel"; then
        clobbered+=("$rel")
    elif [ "$(file_mode "$before/$rel")" != "$(file_mode "$project/$rel")" ]; then
        # A repository's executable script stops working when its mode
        # changes, and content comparison sees nothing.
        clobbered+=("$rel (permissions changed)")
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
if [ ! -s "$work/reported" ] && grep -q 'differs' "$work/install.log"; then
    fail "the install log mentions conflicts but none could be parsed — the report format moved"
fi

# `.keeler.1` and friends: the installer picks a free name when an earlier
# upgrade already left a `.keeler`, so a glob for `*.keeler` alone misses
# them in both directions.
(cd "$project" && find . \( -type f -o -type l \) \
    \( -name '*.keeler' -o -name '*.keeler.[0-9]*' \) -not -path './.git/*' \
    | sed 's|^\./||') \
    | while IFS= read -r keeler; do
        [ -e "$before/$keeler" ] || printf '%s\n' "${keeler%%.keeler*}"
    done | sort -u > "$work/on-disk"

# What the conflicts *should* have been, from the reference rather than
# from the installer's own account of itself. Comparing the installer's
# report against the installer's own files is two of its outputs agreeing
# with each other: an installer that keeps nothing and says nothing gives
# ∅ == ∅ and passes.
: > "$work/expected"
while IFS= read -r rel; do
    [ -n "$rel" ] || continue
    case "$rel" in
        CLAUDE.md | .gitignore | Cargo.toml | .claude/keeler.md) continue ;;
    esac
    if [ -e "$before/$rel" ] || [ -L "$before/$rel" ]; then
        same_entry "$before/$rel" "$reference/$rel" || printf '%s\n' "$rel" >> "$work/expected"
    fi
done < "$work/tracked"
sort -o "$work/expected" "$work/expected"

for side in reported on-disk; do
    if ! diff -u "$work/expected" "$work/$side" > "$work/conflict-diff" 2>&1; then
        echo "integration-check: the conflicts $side do not match the ones that exist" >&2
        echo "  (-) should have been a conflict, (+) claimed but is not one:" >&2
        cat "$work/conflict-diff" >&2
        exit 1
    fi
done

echo "integration-check: $(wc -l < "$work/on-disk" | tr -d ' ') conflict(s) named and kept alongside"

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

# --- 8. Nothing was added that has no business being there ----------------
# Step 4 asks whether everything expected arrived; nothing asked whether
# anything else did. Scratch files, editor droppings and half-written
# temporaries are all things an installer should not leave behind.
{
    cat "$work/tracked"
    sed 's/$/.keeler/' "$work/expected"
    if [ "$manifest_edited" -eq 1 ]; then echo "Cargo.lock"; fi
    if [ -e "$project/.claude/keeler.md.bak" ]; then echo ".claude/keeler.md.bak"; fi
} | sort -u > "$work/allowed"
list_files "$project" > "$work/project-after"
comm -13 "$work/project-before" "$work/project-after" | sort > "$work/added"
if ! comm -23 "$work/added" "$work/allowed" > "$work/unexpected" 2>/dev/null; then
    : # comm needs sorted input; both are
fi
if [ -s "$work/unexpected" ]; then
    echo "integration-check: the installer left files nobody asked for:" >&2
    sed 's/^/  /' "$work/unexpected" >&2
    exit 1
fi

# --- 9. The manifest is still a manifest, and it was actually configured --
# The edit is the part real repositories exercise hardest — table-form
# dependencies, inherited lints, workspace roots — and it was the one thing
# nothing checked. A manifest cargo refuses to read shipped because of it.
if [ "$manifest_read_before" = 1 ]; then
    if ! cargo metadata --manifest-path "$project/Cargo.toml" --no-deps \
        --format-version 1 > /dev/null 2>&1; then
        fail "the installer left a Cargo.toml cargo cannot read"
    fi
fi
if ! grep -q '^\[workspace\]' "$project/Cargo.toml" \
    || grep -q '^\[package\]' "$project/Cargo.toml"; then
    # The profile and the lints the installer writes itself, so they must
    # be in the file. Adding the dependency is cargo's job, and cargo can
    # be offline or stubbed — so for proptest the installer is held to
    # having accounted for it, not to a tool's success.
    grep -q '^\[profile\.mutants\]' "$project/Cargo.toml" \
        || fail "the manifest gained no [profile.mutants] — the mutation gate loses its profile"
    grep -q 'proptest' "$work/install.log" \
        || fail "the run says nothing about proptest — the property tests cannot compile"
fi

# --- 10. A second run must change nothing ----------------------------------
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
