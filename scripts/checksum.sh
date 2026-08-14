#!/usr/bin/env bash
# Emit "<sha256>  <basename>" for a file — the standard checksum format
# that `sha256sum -c` (or `shasum -a 256 -c`) verifies.
#
# Usage: checksum.sh <file>
#
# The name in the output is the bare basename, so verification works in
# whatever directory the adopter downloaded the release assets into.
set -euo pipefail

file="${1:?usage: checksum.sh <file>}"
cd "$(dirname "$file")"
name="$(basename "$file")"

if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$name"
else
    shasum -a 256 "$name"
fi
