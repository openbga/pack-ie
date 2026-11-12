#!/bin/sh
# Prints RUSTFLAGS and CFLAGS (as NAME=value lines, for $GITHUB_ENV or `export`) that map the
# checkout and cargo-home paths to fixed prefixes, so a build of the same commit gives the same
# engine binary wherever it is checked out. Run from the repository root.
set -eu
root="$(pwd -P)"
cargo_home="${CARGO_HOME:-$HOME/.cargo}"
rust="--remap-path-prefix=$root=/packie --remap-path-prefix=$cargo_home=/cargo"
# Through a symlink (macOS /tmp is /private/tmp) cargo may pass the logical path.
if [ "$PWD" != "$root" ]; then
    rust="$rust --remap-path-prefix=$PWD=/packie"
fi
if [ "$(uname -s)" = Darwin ]; then
    # ld64 hashes the object file paths into the Mach-O UUID.
    rust="$rust -C link-arg=-Wl,-oso_prefix,$root/"
fi
echo "RUSTFLAGS=$rust"
echo "CFLAGS=-ffile-prefix-map=$root=/packie -ffile-prefix-map=$cargo_home=/cargo"
