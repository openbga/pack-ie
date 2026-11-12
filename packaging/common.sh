# shellcheck shell=bash disable=SC2034
# Shared by the macOS and Linux build scripts (sourced, not executed).
# Provides: ROOT, VERSION, OUT_DIR, resolve_packie_bin.

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
VERSION=$("$ROOT/packaging/version.sh")
OUT_DIR="$ROOT/target/packaging"

die() {
    echo "error: $*" >&2
    exit 1
}

# resolve_packie_bin [explicit-path]
# Sets PACKIE_BIN_PATH to the binary to package: the argument, else $PACKIE_BIN, else
# target/release/packie, else the only target/<triple>/release/packie.
resolve_packie_bin() {
    local candidate="${1:-${PACKIE_BIN:-}}"
    if [ -z "$candidate" ]; then
        if [ -f "$ROOT/target/release/packie" ]; then
            candidate="$ROOT/target/release/packie"
        else
            local found=()
            local f
            for f in "$ROOT"/target/*/release/packie; do
                [ -f "$f" ] && found+=("$f")
            done
            if [ "${#found[@]}" -eq 1 ]; then
                candidate="${found[0]}"
            elif [ "${#found[@]}" -gt 1 ]; then
                die "several release binaries found (${found[*]}); choose one with PACKIE_BIN=<path>"
            else
                die "no packie binary found at target/release/packie or target/<triple>/release/packie; build it with 'cargo build --release -p packie' or set PACKIE_BIN=<path>"
            fi
        fi
    fi
    [ -f "$candidate" ] || die "packie binary not found: $candidate (set PACKIE_BIN=<path> or pass it as the first argument)"
    [ -x "$candidate" ] || die "packie binary is not executable: $candidate"
    PACKIE_BIN_PATH=$(cd "$(dirname "$candidate")" && pwd)/$(basename "$candidate")
}
