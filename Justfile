# List available recipes
default:
    @just --list

# Run all tests (doc tests only when a package has a library target).
# --workspace, not the default: in a project whose root manifest is itself a
# package, cargo tests only that package and a member crate's tests never
# run. A test that is never run is not a test.
#
# Run every test in the workspace, plus doc tests.
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
#
# Check formatting and lints, the way CI does.
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
#
# Mutation tests for one file — `just mutants src/lib.rs`.
mutants FILE:
    cargo mutants --workspace --file {{FILE}}

# Mutation tests on every member (slow)
mutants-all:
    cargo mutants --workspace

# "Main" is one thing, decided once: the first of the four names below
# that exists in this repository. `mutants-diff` diffs against it and
# `keeler-land` refuses to run anywhere else — through this one helper, so
# no two recipes can disagree about where main is and be silently wrong in
# different directions. Private (the leading `_`): it prints a ref for
# other recipes, not a line for a human reading `just --list`.
_main-ref:
    #!/usr/bin/env bash
    set -euo pipefail
    for ref in origin/main origin/master main master; do
        if git rev-parse --verify --quiet "$ref" >/dev/null 2>&1; then
            echo "$ref"
            exit 0
        fi
    done
    echo "no main branch here: none of origin/main, origin/master, main, master exists" >&2
    exit 1

# Mutation tests on changed lines only (--in-diff vs HEAD, else the branch
# base, else the last commit)
#
# Mutation tests on the lines this branch changed.
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
        # Where main is, is `_main-ref`'s answer and no one else's.
        base=""
        if main_ref=$(just _main-ref 2>/dev/null); then
            base=$(git merge-base HEAD "$main_ref" 2>/dev/null || true)
        fi
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

# Diff-based by construction: all three of these measure this branch's
# own changes, and `crap-delta` reads the baseline without ever writing
# it. Moving the baseline is `just keeler-land`'s job, on main, at fan-in;
# CI refuses a keeler/* pull request whose diff touched it.
#
# `just dev` is untouched — an adopter on the linear road runs exactly the
# recipe they always did, and a spawned agent runs this one instead. The
# three go through `just` on PATH rather than recipe dependencies, because
# the order is the contract: dependencies all run before the body, and the
# sequence would stop being observable.
#
# The line below is the one `just --list` shows — `just` carries the last
# comment line above a recipe and no other, which is why each graph-mode
# recipe here ends its documentation with the one an adopter should read
# there. The rationale above it is for whoever opens this file.
#
# Graph mode: the gate a task branch runs — dev, then crap-delta, then mutants-diff.
keeler-branch:
    #!/usr/bin/env bash
    set -euo pipefail
    just dev
    # The delta gate needs a committed baseline to measure against — the
    # same condition /keeler:qa and the shipped workflow already check.
    # Without one, `dev`'s absolute CRAP threshold is the whole gate.
    # `git cat-file` and not `-f`: a baseline generated locally and left
    # uncommitted would have crap-delta measure this branch against itself,
    # a zero delta by construction and a gate that checked nothing.
    if git cat-file -e HEAD:crap-baseline.json 2>/dev/null; then
        just crap-delta
    else
        echo "no crap-baseline.json committed — threshold only, no delta gate"
    fi
    just mutants-diff

# The script's report is the machine's, one `<id> <state> [needs...]` line
# per task, and the spawn recipe reads it as such; this is the human's view
# of the same lines, where a blocked task shows only what it is still
# waiting on — the needs whose own line is not done — rather than every
# edge it declares. A refusal (a cycle, a need naming no task) is the
# script's: it exits non-zero naming the line, and nothing here is printed.
#
# Graph mode: what a spec's Tasks section says is ready, blocked or done — `just keeler-graph specs/01-foo.md`.
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

# It refuses before creating anything when tmux is missing, when the spec
# differs from HEAD or is not Approved (the worktree is cut from HEAD, so an
# uncommitted graph is one the agent would never see), when the graph script
# says the task is blocked, and when the task already has a worktree.
# Otherwise it creates a worktree beside the repository on
# keeler/<spec-slug>/<task-id>, writes a runner script under .keeler/runs/,
# starts a detached tmux session on it and returns at once.
# `just keeler-status <spec>` is the board afterwards.
#
# Graph mode: hand one ready task to a headless agent on its own branch — `just keeler-spawn specs/01-foo.md T3`.
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
    # And it is read from the spec on the feature's own branch. A tick on a
    # *task* branch unblocks nothing — that is what keeps parallel branches
    # from racing each other's dependencies — while a tick on the feature
    # branch does, because arriving there is the landing. Which branch that
    # is, is a name a machine checks rather than one someone remembers.
    feature="feat/$(basename "$spec_abs" .md)"
    if ! git check-ref-format --branch "$feature" >/dev/null 2>&1; then
        echo "keeler-spawn: $rel would need the branch $feature, which git will not accept — rename the spec file." >&2
        exit 1
    fi
    here="$(git symbolic-ref --quiet --short HEAD || true)"
    if [ "$here" != "$feature" ]; then
        echo "keeler-spawn: on ${here:-a detached HEAD}, but this spec's tasks fan out from $feature — check it out, or create it, and spawn from there." >&2
        exit 1
    fi
    feature_copy="$(mktemp -d)"
    trap 'rm -rf "$feature_copy"' EXIT
    if ! git show "$feature:$rel" > "$feature_copy/$(basename "$spec_abs")" 2>/dev/null; then
        echo "keeler-spawn: $rel is not committed on $feature — the worktree is cut from it, so an uncommitted graph is one the agent would never see." >&2
        exit 1
    fi
    report="$(bash "$root/scripts/keeler-graph.sh" "$feature_copy/$(basename "$spec_abs")")"
    entry="$(printf '%s\n' "$report" | awk -v id="$task" '$1 == id')"
    if [ -z "$entry" ]; then
        echo "keeler-spawn: $rel defines no task $task" >&2
        exit 1
    fi
    state="$(printf '%s\n' "$entry" | awk '{ print $2 }')"
    # A done task has landed. Spawning it cuts a branch to redo work the
    # graph already counts — and the guard that refused only `blocked`
    # let exactly that through.
    if [ "$state" = done ]; then
        echo "keeler-spawn: $task is already done — nothing to spawn" >&2
        exit 1
    fi
    if [ "$state" = blocked ]; then
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
    # A branch whose worktree is gone is the same task, half cleaned up:
    # the commits are still on it. Refuse here, naming it, rather than
    # let `git worktree add` fail somewhere deeper.
    if git show-ref --verify --quiet "refs/heads/$branch"; then
        echo "keeler-spawn: branch $branch already exists — $task is already spawned; land it, or delete the branch to start over." >&2
        exit 1
    fi
    # Nothing on disk until the branch and the worktree are real: a path
    # that refuses must create nothing — and destroy nothing either.
    git worktree add -b "$branch" "$worktree" HEAD
    runs="$root/.keeler/runs/$slug"
    mkdir -p "$runs"
    exit_file="$runs/$tid.exit"
    log_file="$runs/$tid.log"
    runner="$runs/$tid.sh"
    # A verdict left by an earlier run of this task is not this run's.
    rm -f "$exit_file"
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
    # Bash(git:*) grants push. "Nothing is pushed" has to be a permission,
    # not a sentence in a prompt: take it back explicitly. The match is on
    # the command prefix, so this stops the ordinary `git push` and not a
    # determined `git -C … push` or a recipe that pushes — it is a guard
    # against the accident, and the guarantee that nothing reaches the
    # remote remains the human owning the push.
    blocked='Bash(git push:*)'
    cat > "$runner" <<RUNNER
    #!/usr/bin/env bash
    # Written by 'just keeler-spawn' for $branch. Re-runnable by hand:
    #     bash "$runner"
    cd "$worktree" || exit 1
    prompt=\$(cat <<'KEELER_PROMPT'
    $prompt
    KEELER_PROMPT
    )
    # Everything the session prints is teed to the log, so the run can be
    # read after the tmux window is gone.
    {
        claude -p "\$prompt" --permission-mode acceptEdits --allowedTools '$tools' --disallowedTools '$blocked'
        # The verdict is the gate's, not the agent's: claude -p exits zero
        # for any finished turn, including one that ended in FAIL.
        just keeler-branch
        echo \$? > "$exit_file"
    } 2>&1 | tee "$log_file"
    RUNNER
    printf -v run_cmd 'bash %q' "$runner"
    # If the session will not start, the branch and worktree must not
    # survive it: every retry would then refuse with "already spawned" for
    # a run that never began, and the task would be wedged.
    if ! tmux new-session -d -s "$session" -c "$worktree" "$run_cmd"; then
        echo "keeler-spawn: tmux would not start $session — undoing" >&2
        git worktree remove --force "$worktree" 2>/dev/null || true
        git branch -D "$branch" >/dev/null 2>&1 || true
        rm -f "$runner"
        exit 1
    fi
    echo "spawned $task on $branch"
    echo "  worktree: $worktree"
    echo "  session:  tmux attach -t $session   (a view, not a seat: claude -p is not interactive)"
    echo "  log:      $log_file"
    echo "  verdict:  $exit_file   (the exit code of just keeler-branch, once the run ends)"
    echo "  board:    just keeler-status $rel"

# "Running" is tmux's answer, never the absence of a file; a task with no
# verdict at all died before its gate ever ran, which is a different thing
# from a gate that failed, and its log and worktree are what a resume reads.
#
# Graph mode: what every task of a spec is doing right now — running, passed, failed, died mid-pipeline, or never spawned.
keeler-status SPEC:
    #!/usr/bin/env bash
    set -euo pipefail
    spec="{{SPEC}}"
    [ -f "$spec" ] || { echo "keeler-status: $spec is not a file" >&2; exit 1; }
    root="$(git rev-parse --show-toplevel)"
    spec_abs="$(cd "$(dirname "$spec")" && pwd)/$(basename "$spec")"
    slug="$(basename "$spec_abs" .md)"
    runs="$root/.keeler/runs/$slug"
    # The graph comes from the feature's branch, the same place
    # keeler-spawn reads it: a board that answered from the working tree
    # would be the one place in graph mode where an uncommitted tick
    # counts, and it would report a task done that spawn does not believe.
    rel="${spec_abs#"$root/"}"
    feature="feat/$slug"
    # After a feature lands, its branch is gone and the board must still
    # answer — so HEAD stands in, and the run says which ref it read
    # rather than leaving the reader to assume.
    graph_ref="$feature"
    git rev-parse --verify -q "$feature^{commit}" >/dev/null || graph_ref=HEAD
    echo "graph: $rel on $graph_ref"
    graph_copy="$(mktemp -d)"
    trap 'rm -rf "$graph_copy"' EXIT
    if ! git show "$graph_ref:$rel" > "$graph_copy/$(basename "$spec_abs")" 2>/dev/null; then
        echo "keeler-status: $rel is not committed on $graph_ref — there is no graph to report against." >&2
        exit 1
    fi
    report="$(bash "$root/scripts/keeler-graph.sh" "$graph_copy/$(basename "$spec_abs")")"
    while read -r id graph_state _rest; do
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
        elif [ "$graph_state" = done ]; then
            # The graph answers before any leftover does. A landed task
            # whose worktree was never removed is done and not dead, and a
            # task that failed a run, was fixed and landed is done and not
            # failed — a verdict on disk is the record of a run that has
            # since been superseded, and the board does not invent a second
            # answer for work the graph already counts.
            printf '%-6s %s\n' "$id" "done"
            continue
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

# `just dev` runs first and the baseline moves only if it was green: a
# baseline recorded from a red main would write "broken" down as the new
# normal, and two branches that were each green alone can be wrong
# together. Not `dev-full` — hours of mutants at every fan-in is a cost
# nobody would pay, and `mutants-diff` on a freshly merged main has nothing
# to diff.
#
# Then it finishes what the fan-in finished: a spec whose every box is
# ticked — as committed, never as the working tree happens to read — gets
# `Status: Implemented`, staged beside the baseline for the same human
# commit, and each landed task's worktree and branch are removed. Only
# when there is nothing to lose, though: a worktree with uncommitted
# changes, one that has moved to another branch, and a branch holding
# commits main does not have are each named and left where they are.
# Nothing here is committed, and nothing a human has not seen is thrown
# away.
#
# Graph mode: fan-in, on main — gates first, baseline second, staged and never committed.
keeler-land:
    #!/usr/bin/env bash
    set -euo pipefail
    # The gates run as nested `just` calls and not as recipe dependencies:
    # a dependency runs before this body, and so before the refusal below —
    # a branch would have paid for the whole gate suite to be told it was
    # on the wrong branch.
    main_ref="$(just _main-ref)"
    main_branch="${main_ref##*/}"
    current="$(git symbolic-ref --quiet --short HEAD || echo 'a detached HEAD')"
    if [ "$current" != "$main_branch" ]; then
        echo "keeler-land: on $current, not $main_branch — the baseline moves at fan-in, on main, and nowhere else." >&2
        exit 1
    fi
    if ! just dev; then
        echo "keeler-land: main is red after fan-in — branches that were green alone are wrong together. The baseline is untouched and nothing is staged; fix or revert, then land again." >&2
        exit 1
    fi
    just crap-baseline
    # Staged, never committed: the rules say no commit without the human's
    # word, and a moved baseline is a decision worth a diff someone reads.
    if [ -e crap-baseline.json ]; then
        git add -- crap-baseline.json
        echo "keeler-land: crap-baseline.json is staged, not committed — review the diff and commit it yourself:"
        echo "    git diff --cached -- crap-baseline.json"
    else
        echo "keeler-land: no baseline was produced — nothing in this project to measure yet, so nothing is staged."
    fi
    # Fan-in, part two: a spec whose every box is ticked is finished, and a
    # landed task's worktree is litter. Readiness is the graph script's
    # answer here as it is everywhere else — one parser reads the format,
    # so this recipe and `just keeler-graph` cannot disagree about which
    # tasks are done.
    root="$(git rev-parse --show-toplevel)"
    for spec in "$root"/specs/*.md; do
        [ -f "$spec" ] || continue
        rel="${spec#"$root"/}"
        # Ticks that are not committed are not landed. `keeler-spawn`
        # refuses the mirror of this and for the same reason: readiness is
        # what the repository records, not what a file happens to say right
        # now. It is also what keeps the staged diff honest — a spec with
        # unrelated edits would have them swept into the commit the human
        # is asked to review as the baseline and the Status: line.
        if [ -n "$(git status --porcelain -- "$spec")" ]; then
            echo "keeler-land: $rel differs from HEAD — nothing here reads uncommitted ticks; commit it and land again." >&2
            continue
        fi
        # A spec the parser refuses is one nothing here may act on: not
        # marked, not cleaned up after. Say so and leave it alone.
        if ! report="$(bash "$root/scripts/keeler-graph.sh" "$spec" 2>&1)"; then
            echo "keeler-land: $rel does not parse as a graph, so it is left exactly as it is:" >&2
            printf '%s\n' "$report" >&2
            continue
        fi
        slug="$(basename "$spec" .md)"
        # Every box ticked, and only that, is a finished spec — a spec with
        # no tasks at all has finished nothing.
        all_done="$(printf '%s\n' "$report" | awk '
            NF { n++; if ($2 == "done") d++ }
            END { print (n > 0 && n == d) ? "yes" : "no" }')"
        # The Status: line's value, not the line: `TEMPLATE.md` carries the
        # menu `Draft | Approved | Implemented`, and a substring test reads
        # that as an approval and rewrites the menu.
        status="$(grep -m1 -E '^[*_[:space:]]*Status:' "$spec" \
            | sed -E 's/^[*_[:space:]]*Status:[*_[:space:]]*//; s/[[:space:]]*$//' || true)"
        # Implemented follows Approved: a Draft nobody approved is not a
        # contract that can have been fulfilled, however many boxes are
        # ticked — and a spec already Implemented needs no second write.
        if [ "$all_done" = yes ] && [ "$status" = Approved ]; then
            # Onto a copy and then a move, never a truncate-and-write: a
            # land stopped mid-write would otherwise leave the spec empty.
            # `cp -p` first, so the copy carries the spec's own permissions
            # rather than a temporary file's.
            marked="$spec.keeler-land"
            cp -p "$spec" "$marked"
            awk '!written && /^[*_[:space:]]*Status:/ { sub(/Approved/, "Implemented"); written = 1 } { print }' \
                "$spec" > "$marked"
            mv -f "$marked" "$spec"
            git add -- "$spec"
            echo "keeler-land: every task in $rel is ticked — its Status: is now Implemented, staged alongside the baseline for the same commit."
        fi
        # A landed task's worktree and branch have done their job — but
        # only when the worktree is clean. Uncommitted work is the human's
        # to look at before anything removes it.
        while read -r id state _rest; do
            [ "$state" = done ] || continue
            tid="$(printf '%s' "$id" | tr '[:upper:]' '[:lower:]')"
            branch="keeler/$slug/$tid"
            worktree="$(dirname "$root")/$(basename "$root")-$slug-$tid"
            # Only a worktree this repository registered: a directory that
            # merely has the name is not ours to delete. No `grep -q`: it
            # closes the pipe on its first match, and under `pipefail` a
            # producer killed by that SIGPIPE makes the whole pipeline
            # non-zero — which here would silently skip a worktree that is
            # registered. Draining grep's input costs nothing.
            registered="$(git worktree list --porcelain)"
            printf '%s\n' "$registered" | grep -xF "worktree $worktree" > /dev/null || continue
            # And it must still be the task's worktree: a human who
            # switched it to a branch of their own left the task's branch
            # standing somewhere else, and deleting it here would be
            # deleting something this recipe never looked at.
            on="$(git -C "$worktree" symbolic-ref --quiet --short HEAD || echo 'a detached HEAD')"
            if [ "$on" != "$branch" ]; then
                echo "keeler-land: $worktree is on $on, not $branch — left in place; it is not the task's worktree any more."
                continue
            fi
            if [ -n "$(git -C "$worktree" status --porcelain)" ]; then
                echo "keeler-land: $worktree has uncommitted changes — left in place, with $branch; look at it before removing them."
                continue
            fi
            # Committed is not merged. A branch holding commits main does
            # not have is work nobody has looked at, and deleting the
            # branch would leave it reachable only through the reflog. A
            # squash merge looks exactly like this from here, so the
            # refusal comes with the command that finishes the job.
            if ! git merge-base --is-ancestor "$branch" HEAD; then
                echo "keeler-land: $branch has commits that are not on $main_branch — left in place, with $worktree. If they landed as a squash merge, finish it yourself:"
                echo "    git worktree remove $worktree && git branch -D $branch"
                continue
            fi
            # Tidying up is the last thing this recipe does and the least
            # of it: the baseline and the spec are already staged, so a
            # worktree that will not go — locked, on an unmounted disk —
            # is named and stepped over, not a raw git error that ends the
            # run over work already done.
            if ! git worktree remove "$worktree"; then
                echo "keeler-land: $worktree could not be removed — left in place, with $branch." >&2
                continue
            fi
            # `-d`, not `-D`: the branch is contained in HEAD by the check
            # above, so the safe delete is the one that can succeed — and
            # nothing here can quietly destroy a commit.
            if git branch -d "$branch" >/dev/null 2>&1; then
                echo "keeler-land: removed the landed worktree $worktree and its branch $branch"
            else
                echo "keeler-land: removed the landed worktree $worktree; its branch $branch is still here, delete it yourself"
            fi
            # The verdict and the runner belong to the run that has now
            # landed: `keeler-status` reads the verdict before the graph,
            # so a stale one has the board reporting "passed" beside the
            # path of a worktree this very land removed. The log stays —
            # it is the only record of what the run said.
            rm -f "$root/.keeler/runs/$slug/$tid.exit" "$root/.keeler/runs/$slug/$tid.sh"
        done <<< "$report"
    done
