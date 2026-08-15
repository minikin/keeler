# List available recipes
default:
    @just --list

# Run all tests (doc tests only when the package has a library target)
test:
    #!/usr/bin/env bash
    set -euo pipefail
    cargo nextest run --all-targets --no-tests=pass
    if cargo metadata --no-deps --format-version 1 | grep -q '"doctest":true'; then
        cargo test --doc
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
    cargo clippy --all-targets -- -D warnings
    if [ -e templates/keeler.yml ]; then
        # No nullglob: if the glob stops matching, shellcheck fails on the
        # literal pattern — a gate that vanishes silently is no gate.
        shellcheck install.sh scripts/*.sh
    fi

# Fast compile check without building test binaries
check:
    cargo check --all-targets

# All static checks + tests
ci: lint test

# Coverage and CRAP need compilable Rust targets somewhere in the project.
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
    cargo llvm-cov nextest --all-targets --no-tests=pass --summary-only --fail-under-lines 90

# Coverage + CRAP score gate: fails if any function scores above the threshold
crap:
    #!/usr/bin/env bash
    set -euo pipefail
    {{_has_rust_targets}} || { echo "{{_no_src_msg}}"; exit 0; }
    cargo llvm-cov nextest --all-targets --no-tests=pass --lcov --output-path lcov.info
    cargo crap --lcov lcov.info --path src --threshold 15 --fail-above

# Record a CRAP baseline (run before starting a feature)
crap-baseline:
    #!/usr/bin/env bash
    set -euo pipefail
    {{_has_rust_targets}} || { echo "{{_no_src_msg}}"; exit 0; }
    cargo llvm-cov nextest --all-targets --no-tests=pass --lcov --output-path lcov.info
    cargo crap --lcov lcov.info --path src --format json --sort file --output crap-baseline.json
    echo "CRAP baseline saved to crap-baseline.json"

# CRAP delta vs the recorded baseline: fails on threshold breach OR any regression
crap-delta:
    #!/usr/bin/env bash
    set -euo pipefail
    {{_has_rust_targets}} || { echo "{{_no_src_msg}}"; exit 0; }
    cargo llvm-cov nextest --all-targets --no-tests=pass --lcov --output-path lcov.info
    cargo crap --lcov lcov.info --path src --threshold 15 --fail-above \
        --baseline crap-baseline.json --fail-regression

# Full local gate: format, lint, tests, coverage, CRAP
dev: fmt lint test crap

# Mutation tests for a specific file: just mutants src/lib.rs
mutants FILE:
    cargo mutants --file {{FILE}}

# Mutation tests on the whole crate (slow)
mutants-all:
    cargo mutants

# Mutation tests on changed lines only (--in-diff vs HEAD, else the branch
# base, else the last commit)
mutants-diff:
    #!/usr/bin/env bash
    set -euo pipefail
    paths=('src/*.rs' 'src/**/*.rs')
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
    cargo mutants --in-diff "$diff_file"

# Full validation including mutation tests (slow)
dev-full: dev mutants-all

# Upgrade Keeler itself (KEELER_REF=v0.1.0 just keeler-upgrade to pin a tag)
keeler-upgrade:
    curl -fsSL https://raw.githubusercontent.com/minikin/keeler/main/install.sh | bash -s .
