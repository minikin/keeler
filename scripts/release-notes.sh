#!/usr/bin/env bash
# Print one version's CHANGELOG section body — the release notes.
#
# Usage: release-notes.sh <version> <changelog-file>
#
# The output is everything between `## [<version>]` and the next `## [`
# heading, minus link-reference lines and edge blank lines. A version with
# no section is an error, never empty output — a release must not ship
# with silent, blank notes.
set -euo pipefail

version="${1:?usage: release-notes.sh <version> <changelog-file>}"
changelog="${2:?usage: release-notes.sh <version> <changelog-file>}"

awk -v ver="$version" '
    BEGIN { pfx = "## [" ver "]" }
    /^## / { on = (substr($0, 1, length(pfx)) == pfx); found = found || on; next }
    on     { lines[++n] = $0 }
    END {
        if (!found) {
            print "release-notes: no CHANGELOG section for version " ver > "/dev/stderr"
            exit 1
        }
        # Only the trailing link-reference block is scenery, and only the
        # last section can have collected it — a link-style line inside the
        # body is content and stays. Strip that block, then edge blanks.
        end = n
        while (end >= 1 && (lines[end] ~ /^[[:space:]]*$/ || lines[end] ~ /^\[[^]]*\]: /)) end--
        start = 1; while (start <= end && lines[start] ~ /^[[:space:]]*$/) start++
        for (i = start; i <= end; i++) print lines[i]
    }
' "$changelog"
