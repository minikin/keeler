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

# Check formatting and lints (mirrors CI)
lint:
    cargo fmt --all -- --check
    cargo clippy --all-targets -- -D warnings

# Fast compile check without building test binaries
check:
    cargo check --all-targets

# All static checks + tests
ci: lint test

# Line coverage summary; fails mechanically below 90% lines
cov:
    cargo llvm-cov nextest --all-targets --no-tests=pass --summary-only --fail-under-lines 90

# Coverage + CRAP score gate: fails if any function scores above the threshold
crap:
    cargo llvm-cov nextest --all-targets --no-tests=pass --lcov --output-path lcov.info
    cargo crap --lcov lcov.info --path src --threshold 15 --fail-above

# Record a CRAP baseline (run before starting a feature)
crap-baseline:
    cargo llvm-cov nextest --all-targets --no-tests=pass --lcov --output-path lcov.info
    cargo crap --lcov lcov.info --path src --format json --sort file --output crap-baseline.json
    @echo "CRAP baseline saved to crap-baseline.json"

# CRAP delta vs the recorded baseline: fails on threshold breach OR any regression
crap-delta:
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

# Mutation tests only on lines changed vs HEAD (uncommitted), falling back
# to the last commit. Line-level via --in-diff: far cheaper than mutating
# whole changed files.
mutants-diff:
    #!/usr/bin/env bash
    set -euo pipefail
    paths=('src/*.rs' 'src/**/*.rs')
    # New files aren't in `git diff HEAD` — intent-to-add makes their full
    # content show up in the diff; reset afterwards to leave the index as-is.
    untracked=$(git ls-files --others --exclude-standard -- "${paths[@]}" 2>/dev/null || true)
    if [ -n "$untracked" ]; then git add -N $untracked; fi
    diff_file=$(mktemp)
    trap 'rm -f "$diff_file"' EXIT
    git diff HEAD -- "${paths[@]}" > "$diff_file" || true
    if [ -n "$untracked" ]; then git reset -q -- $untracked; fi
    if [ ! -s "$diff_file" ]; then
        git diff HEAD~1 HEAD -- "${paths[@]}" > "$diff_file" 2>/dev/null || true
    fi
    if [ ! -s "$diff_file" ]; then
        echo "No changed src lines — running all mutants"
        cargo mutants
    else
        echo "Running mutants on changed lines (--in-diff)"
        cargo mutants --in-diff "$diff_file"
    fi

# Full validation including mutation tests (slow)
dev-full: dev mutants-all
