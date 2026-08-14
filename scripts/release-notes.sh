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
    /^## /               { on = (substr($0, 1, length(pfx)) == pfx); found = found || on; next }
    /^\[[^]]*\]: /       { next }   # link-reference block
    on                   { lines[++n] = $0 }
    END {
        if (!found) {
            print "release-notes: no CHANGELOG section for version " ver > "/dev/stderr"
            exit 1
        }
        start = 1; while (start <= n && lines[start] ~ /^[[:space:]]*$/) start++
        end = n;   while (end >= start && lines[end] ~ /^[[:space:]]*$/) end--
        for (i = start; i <= end; i++) print lines[i]
    }
' "$changelog"
