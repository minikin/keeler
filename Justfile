# List available recipes
default:
    @just --list

# Run all tests (doc tests only when a package has a library target).
# --workspace, not the default: in a project whose root manifest is itself a
# package, cargo tests only that package and a member crate's tests never
# run. A test that is never run is not a test.
test:
    #!/usr/bin/env bash
    set -euo pipefail
    cargo nextest run --workspace --all-targets --no-tests=pass
    if cargo metadata --no-deps --format-version 1 | grep -q '"doctest":true'; then
        cargo test --workspace --doc
    else
        echo "no library target — skipping doc tests"
    fi

# Apply formatting
fmt:
    cargo fmt --all

# Check formatting and lints (mirrors CI): your formatting, your clippy.
# The shellcheck branch below is inert in your project — it is keyed on a
# marker file only Keeler's own repository has. Your shell scripts are
# yours to gate, not Keeler's.
lint:
    #!/usr/bin/env bash
    set -euo pipefail
    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets -- -D warnings
    if [ -e templates/keeler.yml ]; then
        # Globbed, not named: whatever shell lives here gets gated,
        # including a script added tomorrow. No nullglob either — if the
        # glob stops matching, shellcheck fails on the literal pattern
        # rather than the gate vanishing in silence.
        shellcheck install.sh scripts/*.sh
    fi

# Fast compile check without building test binaries
check:
    cargo check --workspace --all-targets

# All static checks + tests
ci: lint test

# Coverage and CRAP need compilable Rust targets somewhere in the project.
# They ask cargo where the sources are (--workspace) rather than assuming a
# src/ at the root: a workspace root has none, and hard-coding the path made
# the gate fail with "path does not exist" instead of measuring the members.
# Ask cargo, not the filesystem: a workspace root has no src/ of its own yet
# its members must be measured, while a crate that is only a test harness
# has nothing to measure — and the honest report of that fact must not read
# as a failure.
_no_src_msg := "no Rust sources to measure — skipping (no library or binary targets)"
_has_rust_targets := "cargo metadata --no-deps --format-version 1 2>/dev/null | grep -qE '\"kind\":\\[\"(bin|proc-macro|lib|rlib|dylib|cdylib|staticlib)'"

# Line coverage summary; fails mechanically below 90% lines
cov:
    #!/usr/bin/env bash
    set -euo pipefail
    {{_has_rust_targets}} || { echo "{{_no_src_msg}}"; exit 0; }
    cargo llvm-cov nextest --workspace --all-targets --no-tests=pass --summary-only --fail-under-lines 90

# Coverage + CRAP score gate: fails if any function scores above the threshold
crap:
    #!/usr/bin/env bash
    set -euo pipefail
    {{_has_rust_targets}} || { echo "{{_no_src_msg}}"; exit 0; }
    cargo llvm-cov nextest --workspace --all-targets --no-tests=pass --lcov --output-path lcov.info
    cargo crap --lcov lcov.info --workspace --threshold 15 --fail-above

# Record a CRAP baseline (run before starting a feature)
crap-baseline:
    #!/usr/bin/env bash
    set -euo pipefail
    {{_has_rust_targets}} || { echo "{{_no_src_msg}}"; exit 0; }
    cargo llvm-cov nextest --workspace --all-targets --no-tests=pass --lcov --output-path lcov.info
    cargo crap --lcov lcov.info --workspace --format json --sort file --output crap-baseline.json
    echo "CRAP baseline saved to crap-baseline.json"

# CRAP delta vs the recorded baseline: fails on threshold breach OR any regression
crap-delta:
    #!/usr/bin/env bash
    set -euo pipefail
    {{_has_rust_targets}} || { echo "{{_no_src_msg}}"; exit 0; }
    cargo llvm-cov nextest --workspace --all-targets --no-tests=pass --lcov --output-path lcov.info
    cargo crap --lcov lcov.info --workspace --threshold 15 --fail-above \
        --baseline crap-baseline.json --fail-regression

# Full local gate: format, lint, tests, coverage, CRAP
dev: fmt lint test crap

# Mutation tests for a specific file: just mutants src/lib.rs
# --workspace, or a member crate's file yields "Found 0 mutants" and the
# gate passes having tested nothing.
mutants FILE:
    cargo mutants --workspace --file {{FILE}}

# Mutation tests on every member (slow)
mutants-all:
    cargo mutants --workspace

# Mutation tests on changed lines only (--in-diff vs HEAD, else the branch
# base, else the last commit)
mutants-diff:
    #!/usr/bin/env bash
    set -euo pipefail
    # Both shapes: sources beside the root manifest, and sources in a
    # workspace member. `**/src/*.rs` does not match the root's own src/,
    # and `src/*.rs` does not match a member's — a gate that watches only
    # one of them is blind to half the projects it ships to.
    paths=('src/*.rs' 'src/**/*.rs' '**/src/*.rs' '**/src/**/*.rs')
    # New files aren't in `git diff HEAD` — intent-to-add makes their full
    # content show up in the diff; reset afterwards to leave the index as-is.
    # NUL-delimited into an array: paths with spaces stay whole.
    untracked=()
    while IFS= read -r -d '' f; do untracked+=("$f"); done \
        < <(git ls-files -z --others --exclude-standard -- "${paths[@]}" 2>/dev/null || true)
    if [ "${#untracked[@]}" -gt 0 ]; then git add -N -- "${untracked[@]}"; fi
    diff_file=$(mktemp)
    trap 'rm -f "$diff_file"' EXIT
    git diff HEAD -- "${paths[@]}" > "$diff_file" || true
    if [ "${#untracked[@]}" -gt 0 ]; then git reset -q -- "${untracked[@]}"; fi
    if [ ! -s "$diff_file" ]; then
        # A clean tree is not a measured tree: src changes committed earlier
        # on this branch still need mutating, so diff against the branch base.
        base=""
        for ref in origin/main origin/master main master; do
            if candidate=$(git merge-base HEAD "$ref" 2>/dev/null); then base="$candidate"; break; fi
        done
        if [ -n "$base" ] && [ "$base" != "$(git rev-parse HEAD)" ]; then
            git diff "$base" HEAD -- "${paths[@]}" > "$diff_file" || true
        fi
    fi
    if [ ! -s "$diff_file" ]; then
        git diff HEAD~1 HEAD -- "${paths[@]}" > "$diff_file" 2>/dev/null || true
    fi
    if [ ! -s "$diff_file" ]; then
        # An honest gate says what it did not measure — it never reports the
        # absence of survivors as evidence about a change it cannot see.
        # For everything mutants can measure, use `just mutants-all`.
        echo "No src/ changes — outside the mutation gate's reach; nothing was measured"
        exit 0
    fi
    echo "Running mutants on changed lines (--in-diff)"
    cargo mutants --workspace --in-diff "$diff_file"

# Full validation including mutation tests (slow)
dev-full: dev mutants-all

# Graph mode: what a spec's Tasks section says is ready, blocked or done —
# `just keeler-graph specs/01-foo.md`. The script's report is the
# machine's, one `<id> <state> [needs...]` line per task, and the spawn
# recipe reads it as such; this is the human's view of the same lines,
# where a blocked task shows only what it is still waiting on — the needs
# whose own line is not done — rather than every edge it declares. A
# refusal (a cycle, a need naming no task) is the script's: it exits
# non-zero naming the line, and nothing here is printed.
keeler-graph SPEC:
    #!/usr/bin/env bash
    set -euo pipefail
    report=$(bash scripts/keeler-graph.sh "{{SPEC}}")
    printf '%s\n' "$report" | awk '
        NF { row[++n] = $0; if ($2 == "done") done_[$1] = 1 }
        END {
            for (r = 1; r <= n; r++) {
                k = split(row[r], f, " ")
                line = f[1] " " f[2]
                if (f[2] == "blocked") {
                    sep = " (waiting on "
                    for (j = 3; j <= k; j++) if (!(f[j] in done_)) { line = line sep f[j]; sep = ", " }
                    line = line ")"
                }
                print line
            }
        }'

# Graph mode: hand one ready task to a headless agent on its own branch —
# `just keeler-spawn specs/01-foo.md T3`. It refuses before creating
# anything when tmux is missing, when the spec differs from HEAD or is not
# Approved (the worktree is cut from HEAD, so an uncommitted graph is one
# the agent would never see), when the graph script says the task is
# blocked, and when the task already has a worktree. Otherwise it creates a
# worktree beside the repository on keeler/<spec-slug>/<task-id>, writes a
# runner script under .keeler/runs/, starts a detached tmux session on it
# and returns at once. `just keeler-status <spec>` is the board afterwards.
keeler-spawn SPEC TASK:
    #!/usr/bin/env bash
    set -euo pipefail
    if ! command -v tmux >/dev/null 2>&1; then
        echo "keeler-spawn: tmux is required — every spawned agent runs in a detached tmux session." >&2
        echo "  macOS:         brew install tmux" >&2
        echo "  Debian/Ubuntu: sudo apt-get install tmux" >&2
        exit 1
    fi
    spec="{{SPEC}}"
    task="{{TASK}}"
    [ -f "$spec" ] || { echo "keeler-spawn: $spec is not a file" >&2; exit 1; }
    root="$(git rev-parse --show-toplevel)"
    spec_abs="$(cd "$(dirname "$spec")" && pwd)/$(basename "$spec")"
    case "$spec_abs" in
        "$root"/*) ;;
        *)
            echo "keeler-spawn: $spec_abs is not inside $root — the worktree is cut from this repository's HEAD and would never see it." >&2
            exit 1
            ;;
    esac
    rel="${spec_abs#"$root"/}"
    # The spec as committed, because that is what the new worktree will see:
    # /keeler:tasks leaves the graph uncommitted, and a spawn straight after
    # would compute readiness from a file the worktree does not have.
    if [ -n "$(git status --porcelain -- "$spec_abs")" ]; then
        echo "keeler-spawn: $rel differs from HEAD — commit it first; the worktree is cut from HEAD and would not see it." >&2
        exit 1
    fi
    status_line="$(grep -m1 -E '^[*_[:space:]]*Status:' "$spec_abs" || true)"
    case "$status_line" in
        *Approved*) ;;
        *)
            echo "keeler-spawn: $rel is not Approved (${status_line:-no Status: line}) — an unapproved spec is not a contract to build from." >&2
            exit 1
            ;;
    esac
    # Readiness is the graph script's answer, never this recipe's: one
    # parser reads the format, so a human and the tools cannot disagree.
    report="$(bash "$root/scripts/keeler-graph.sh" "$spec_abs")"
    entry="$(printf '%s\n' "$report" | awk -v id="$task" '$1 == id')"
    if [ -z "$entry" ]; then
        echo "keeler-spawn: $rel defines no task $task" >&2
        exit 1
    fi
    if [ "$(printf '%s\n' "$entry" | awk '{ print $2 }')" = blocked ]; then
        unmet=""
        for need in $(printf '%s\n' "$entry" | cut -d' ' -f3-); do
            if [ "$(printf '%s\n' "$report" | awk -v n="$need" '$1 == n { print $2 }')" != done ]; then
                unmet="$unmet $need"
            fi
        done
        echo "keeler-spawn: $task is blocked, waiting on:$unmet" >&2
        exit 1
    fi
    # One name in four places: the spec's file name, and the id lowercased
    # once, on the way out of the parser.
    slug="$(basename "$spec_abs" .md)"
    tid="$(printf '%s' "$task" | tr '[:upper:]' '[:lower:]')"
    branch="keeler/$slug/$tid"
    session="keeler-$slug-$tid"
    worktree="$(dirname "$root")/$(basename "$root")-$slug-$tid"
    if [ -e "$worktree" ]; then
        echo "keeler-spawn: $worktree already exists — $task is already spawned; land it or remove it first." >&2
        exit 1
    fi
    runs="$root/.keeler/runs/$slug"
    mkdir -p "$runs"
    exit_file="$runs/$tid.exit"
    log_file="$runs/$tid.log"
    runner="$runs/$tid.sh"
    # A verdict left by an earlier run of this task is not this run's.
    rm -f "$exit_file"
    git worktree add -b "$branch" "$worktree" HEAD
    prompt="Implement task $task of $rel, and nothing else.

    Read $rel in full first, then .claude/keeler.md.

    Run the whole per-task pipeline for this one task, in order and without
    stopping between stages: /keeler:tdd, then /keeler:qa, then
    /keeler:review, then /keeler:mutants. The gate is 'just keeler-branch';
    it must be green before the task is done.

    You are on branch $branch, in the worktree $worktree. Commit there as
    each stage finishes: the human ran keeler-spawn for this task, and that
    was the consent for those commits. Commit nowhere else, never push, and
    never open a pull request."
    # Enough to edit, test and commit inside the worktree, and no more. Not
    # bypassPermissions: a headless agent with an unrestricted shell is not
    # a decision a recipe should make by default.
    tools='Bash(cargo:*),Bash(just:*),Bash(git:*)'
    cat > "$runner" <<RUNNER
    #!/usr/bin/env bash
    # Written by 'just keeler-spawn' for $branch. Re-runnable by hand:
    #     bash $runner
    cd "$worktree" || exit 1
    prompt=\$(cat <<'KEELER_PROMPT'
    $prompt
    KEELER_PROMPT
    )
    # Everything the session prints is teed to the log, so the run can be
    # read after the tmux window is gone.
    {
        claude -p "\$prompt" --permission-mode acceptEdits --allowedTools '$tools'
        # The verdict is the gate's, not the agent's: claude -p exits zero
        # for any finished turn, including one that ended in FAIL.
        just keeler-branch
        echo \$? > "$exit_file"
    } 2>&1 | tee "$log_file"
    RUNNER
    printf -v run_cmd 'bash %q' "$runner"
    tmux new-session -d -s "$session" -c "$worktree" "$run_cmd"
    echo "spawned $task on $branch"
    echo "  worktree: $worktree"
    echo "  session:  tmux attach -t $session   (a view, not a seat: claude -p is not interactive)"
    echo "  log:      $log_file"
    echo "  verdict:  $exit_file   (the exit code of just keeler-branch, once the run ends)"
    echo "  board:    just keeler-status $rel"

# Graph mode: what every task of a spec is doing right now — running,
# passed, failed, died mid-pipeline, or never spawned. "Running" is tmux's
# answer, never the absence of a file; a task with no verdict at all died
# before its gate ever ran, which is a different thing from a gate that
# failed, and its log and worktree are what a resume reads.
keeler-status SPEC:
    #!/usr/bin/env bash
    set -euo pipefail
    spec="{{SPEC}}"
    [ -f "$spec" ] || { echo "keeler-status: $spec is not a file" >&2; exit 1; }
    root="$(git rev-parse --show-toplevel)"
    spec_abs="$(cd "$(dirname "$spec")" && pwd)/$(basename "$spec")"
    slug="$(basename "$spec_abs" .md)"
    runs="$root/.keeler/runs/$slug"
    report="$(bash "$root/scripts/keeler-graph.sh" "$spec_abs")"
    while read -r id _rest; do
        [ -n "$id" ] || continue
        tid="$(printf '%s' "$id" | tr '[:upper:]' '[:lower:]')"
        session="keeler-$slug-$tid"
        worktree="$(dirname "$root")/$(basename "$root")-$slug-$tid"
        exit_file="$runs/$tid.exit"
        log_file="$runs/$tid.log"
        # The `=` is exact matching: without it tmux answers about t10 when
        # asked about t1.
        if command -v tmux >/dev/null 2>&1 && tmux has-session -t "=$session" 2>/dev/null; then
            state=running
        elif [ -f "$exit_file" ]; then
            code="$(tr -d '[:space:]' < "$exit_file")"
            if [ "$code" = 0 ]; then state=passed; else state="failed (exit $code)"; fi
        elif [ -e "$worktree" ] || [ -f "$log_file" ]; then
            state=died
        else
            printf '%-6s %s\n' "$id" "not spawned"
            continue
        fi
        printf '%-6s %-16s log %s  worktree %s\n' "$id" "$state" "$log_file" "$worktree"
    done <<< "$report"

# Upgrade Keeler itself (KEELER_REF=v0.3.0 just keeler-upgrade to pin a tag)
keeler-upgrade:
    curl -fsSL https://raw.githubusercontent.com/minikin/keeler/main/install.sh | bash -s .
