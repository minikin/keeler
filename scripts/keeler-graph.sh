#!/usr/bin/env bash
# Read a spec's Tasks section as a dependency graph.
#
# Usage: keeler-graph.sh <spec.md>
#
# Prints one line per task, in the order the spec lists them:
#
#     <id> <state> [needs...]
#
# where state is `done` (its box is ticked), `ready` (every need is done)
# or `blocked` (some need is not). Exit 0 with that report, or exit 1
# naming the line and what is wrong with it — a Needs: naming no task, an
# id defined twice, two Needs: in one item — and print nothing as ready.
#
# The grammar is spec 06's, and it is deliberately small:
#   - the Tasks section runs from `## Tasks` to the next `## ` heading;
#     nothing outside it is a task;
#   - an item runs from a line beginning `- [ ]` or `- [x]` to the next
#     such line, however many physical lines it wraps across;
#   - the item opens with `**Tn — `; Tn is the id;
#   - `Needs: Ta, Tb.` may appear once, anywhere in the item; absent means
#     a root; the checkbox is the only completion signal.
#
# `/keeler:graph`, `just keeler-graph` and `just keeler-spawn` all call
# this and nothing else reads the format — so a human and the tools cannot
# disagree about what the spec's graph is.
#
# All of it is one awk program: macOS ships bash 3.2, which has no
# associative arrays, and awk has had them since 1977.
set -euo pipefail

spec="${1:?usage: keeler-graph.sh <spec.md>}"
[ -f "$spec" ] || { echo "keeler-graph: $spec is not a file" >&2; exit 1; }

awk -v spec="$spec" '
function fail(line, why) {
    printf "keeler-graph: %s line %d: %s\n", spec, line, why > "/dev/stderr"
    # `exit` inside a rule still runs END, which would print the report —
    # and a refusal must report nothing. The flag tells END to stay quiet.
    refused = 1
    exit 1
}

# Close the item being read: parse it, record it, refuse what is wrong.
function close_item(    id, rest, cnt, list, m, k, need) {
    if (!open) return
    open = 0
    if (match(text, /^- \[[ x]\] \*\*T[0-9]+ /) == 0)
        fail(open_line, "an item that does not open with **Tn — : " substr(text, 1, 60))
    id = substr(text, RSTART + 8, RLENGTH - 9)
    if (id in seen) fail(open_line, id " is defined twice (first at line " seen[id] ")")
    seen[id] = open_line

    cnt = gsub(/Needs:/, "Needs:", text)
    if (cnt > 1) fail(open_line, id " carries two Needs: lines — which one is the graph?")
    list = ""
    if (cnt == 1) {
        rest = text
        sub(/.*Needs:[ \t]*/, "", rest)
        sub(/\..*/, "", rest)
        gsub(/,/, " ", rest)
        list = rest
    }
    n++
    ids[n] = id; lines[n] = open_line; ticked[n] = tick; needs[n] = list
}

/^## Tasks[ \t]*$/ { in_tasks = 1; next }
in_tasks && /^## /  { close_item(); in_tasks = 0 }
!in_tasks           { next }
/^- \[[ x]\] /      {
    close_item()
    open = 1; open_line = NR
    tick = ($0 ~ /^- \[x\]/) ? 1 : 0
    text = $0
    next
}
open && /^[ \t]+[^ \t]/ { sub(/^[ \t]+/, " "); text = text $0; next }
open && /^[ \t]*$/      { next }
open                    { close_item() }

END {
    if (refused) exit 1
    close_item()
    # Every need must name a task in this spec.
    for (i = 1; i <= n; i++) {
        k = split(needs[i], m, " ")
        for (j = 1; j <= k; j++)
            if (!(m[j] in seen)) fail(lines[i], ids[i] " needs " m[j] ", which no task defines")
    }
    for (i = 1; i <= n; i++) if (ticked[i]) done_[ids[i]] = 1
    for (i = 1; i <= n; i++) {
        state = "ready"
        if (ids[i] in done_) state = "done"
        else {
            k = split(needs[i], m, " ")
            for (j = 1; j <= k; j++) if (!(m[j] in done_)) { state = "blocked"; break }
        }
        line = ids[i] " " state
        k = split(needs[i], m, " ")
        for (j = 1; j <= k; j++) line = line " " m[j]
        report = report line "\n"
    }
    # Only now, and only if nothing above refused: a malformed section
    # must report nothing as ready, so nothing is printed until it all is.
    if (!refused) printf "%s", report
}
' "$spec"
