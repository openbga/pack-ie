#!/bin/sh
# Prints the PACK-IE version: `[workspace.package] version` of the workspace Cargo.toml.
# Every packaging script uses this; packaging/version.ps1 holds the same logic for Windows.
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
manifest="$root/Cargo.toml"

version=$(awk '
    /^\[/ { section = ($0 ~ /^\[workspace\.package\][ \t]*$/) }
    section && /^version[ \t]*=/ {
        sub(/^[^"]*"/, ""); sub(/".*$/, ""); print; exit
    }
' "$manifest")

if [ -z "$version" ]; then
    echo "version.sh: no [workspace.package] version found in $manifest" >&2
    exit 1
fi
printf '%s\n' "$version"
