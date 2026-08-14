#!/usr/bin/env bash
# Refuse a release whose tag lies about what it ships.
#
# Usage: release-guard.sh <tag>            (run from the repository root)
#
# The tag must agree with VERSION, VERSION with the rules-file marker, and
# CHANGELOG.md must carry a section for the version — the same consistency
# the version CI job enforces on every push, re-checked at the moment it
# matters most.
set -euo pipefail

tag="${1:?usage: release-guard.sh <tag>}"

version="$(tr -d '[:space:]' < VERSION)"
if [ "$tag" != "v$version" ]; then
    echo "release-guard: tag $tag disagrees with VERSION $version — refusing to release" >&2
    exit 1
fi

marker="$(sed -n 's/^<!-- keeler-version: \(.*\) -->$/\1/p' .claude/keeler.md)"
if [ "$marker" != "$version" ]; then
    echo "release-guard: rules-file marker '$marker' disagrees with VERSION $version" >&2
    exit 1
fi

if ! grep -q "^## \[$version\]" CHANGELOG.md; then
    echo "release-guard: CHANGELOG.md has no section for $version" >&2
    exit 1
fi

echo "release-guard: v$version is consistent — tag, VERSION, marker, CHANGELOG agree"
