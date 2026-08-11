#!/usr/bin/env bash
# Keeler installer: copies the workflow into an existing Rust project.
# Non-destructive — never overwrites an existing file; conflicting files
# are written next to the original as <name>.keeler for manual merge.
#
# Usage:
#   ./install.sh /path/to/your/project          (from a clone)
#   curl -fsSL https://raw.githubusercontent.com/minikin/keeler/main/install.sh \
#       | bash -s /path/to/your/project         (no clone needed)
set -euo pipefail

REPO_TARBALL="${KEELER_TARBALL:-https://codeload.github.com/minikin/keeler/tar.gz/refs/heads/main}"
DEST="${1:?usage: ./install.sh /path/to/your/project}"

# When run next to the repo files, install from them; when piped from curl,
# fetch a fresh tarball into a temp dir instead.
SRC="$(cd "$(dirname "${BASH_SOURCE[0]:-.}")" 2>/dev/null && pwd || pwd)"
if [ ! -f "$SRC/specs/TEMPLATE.md" ]; then
    tmp="$(mktemp -d)"
    trap 'rm -rf "$tmp"' EXIT
    echo "Fetching Keeler..."
    curl -fsSL "$REPO_TARBALL" | tar -xz -C "$tmp" --strip-components=1
    SRC="$tmp"
fi

if [ ! -f "$DEST/Cargo.toml" ]; then
    echo "error: $DEST does not look like a Rust project (no Cargo.toml)" >&2
    exit 1
fi

copied=()
merges=()

# Copy a file; if the destination exists, place a .keeler copy for merging.
install_file() {
    local rel="$1"
    local from="$SRC/$rel" to="$DEST/$rel"
    mkdir -p "$(dirname "$to")"
    if [ -e "$to" ]; then
        if ! cmp -s "$from" "$to"; then
            cp "$from" "$to.keeler"
            merges+=("$rel  (wrote $rel.keeler — merge by hand)")
        fi
    else
        cp "$from" "$to"
        copied+=("$rel")
    fi
}

# The workflow itself
for f in .claude/commands/spec.md .claude/commands/tasks.md \
         .claude/commands/tdd.md .claude/commands/qa.md \
         .claude/commands/review.md .claude/commands/mutants.md \
         .claude/commands/feature.md .claude/commands/fix.md \
         .claude/skills/property-testing/SKILL.md \
         .claude/skills/gherkin-specs/SKILL.md \
         specs/TEMPLATE.md CLAUDE.md WORKFLOW.md Justfile \
         .cargo-mutants.toml clippy.toml rustfmt.toml; do
    install_file "$f"
done

# CI under a keeler-specific name so it never clashes with existing workflows
mkdir -p "$DEST/.github/workflows"
if [ -e "$DEST/.github/workflows/keeler.yml" ]; then
    merges+=(".github/workflows/keeler.yml  (already exists — left untouched)")
else
    cp "$SRC/.github/workflows/ci.yml" "$DEST/.github/workflows/keeler.yml"
    copied+=(".github/workflows/keeler.yml")
fi

echo "Keeler installed into $DEST"
echo
echo "Copied:"
printf '  %s\n' "${copied[@]:-none}"
if [ "${#merges[@]}" -gt 0 ]; then
    echo
    echo "Needs manual merge (existing files were not touched):"
    printf '  %s\n' "${merges[@]}"
fi

cat <<'NEXT'

Next steps (manual, one-time):

1. Cargo.toml — add, if missing:
       [dev-dependencies]
       proptest = "1"

       [profile.mutants]     # keeps cargo-mutants builds fast
       inherits = "dev"
       debug = 0

       [lints.clippy]        # optional but recommended
       pedantic = { level = "warn", priority = -1 }

2. Install the tools:
       cargo install cargo-binstall
       cargo binstall cargo-nextest cargo-llvm-cov cargo-mutants cargo-crap

3. .gitignore — add: lcov.info, crap-baseline.json, crap-report.json, mutants.out*/

4. Adopt the gates INCREMENTALLY — legacy code will not pass them day one.
   See "Adopting Keeler in an existing project" in README.md:
       just crap-baseline        # freeze today's scores as the baseline
   From then on, gate on *regressions and new code only* (just crap-delta,
   just mutants-diff) and ratchet absolute thresholds up over time.
NEXT
