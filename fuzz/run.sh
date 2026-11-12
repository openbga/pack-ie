#!/bin/sh
# Runs fuzz targets for a fixed time: fuzz/run.sh <seconds> [target ...]  (default: all targets)
# JOBS=n runs n targets at the same time (default 1).
#
# The working corpus grows in work/<target>; corpus/<target> holds the committed seeds and
# minimized regression inputs and is only read. A crash leaves its input in
# artifacts/<target>/ and fails the script after the remaining targets have run. The log of
# each run (including libFuzzer's final statistics) is work/log/<target>.log.
set -u
cd "$(dirname "$0")" || exit 2

# Longest input worth generating for each target (seeds are all well below these).
max_len() {
    case "$1" in
        transform_path) echo 256 ;;
        astm_frames) echo 1024 ;;
        hl7_ack) echo 2048 ;;
        files_read | config_parse) echo 8192 ;;
        *) echo 4096 ;;
    esac
}

# run.sh --one <seconds> <target>
if [ "${1:-}" = "--one" ]; then
    seconds=$2
    target=$3
    mkdir -p "work/$target" work/log
    echo "== $target (${seconds}s)"
    cargo fuzz run "$target" "work/$target" "corpus/$target" -- \
        -max_total_time="$seconds" -max_len="$(max_len "$target")" \
        -timeout=10 -rss_limit_mb=2048 -print_final_stats=1 >"work/log/$target.log" 2>&1
    code=$?
    grep -E "^(stat::(number_of_executed_units|average_exec_per_sec)|#[0-9]+\s+(DONE|INITED))" "work/log/$target.log" | tail -4
    if [ "$code" -ne 0 ]; then
        echo "FAILED: $target exited with $code (see work/log/$target.log, artifacts/$target/)"
    fi
    exit "$code"
fi

seconds=${1:?usage: run.sh <seconds> [target ...]}
shift
if [ "$#" -eq 0 ]; then
    set -- $(cargo fuzz list)
fi
cargo fuzz build || exit 2
printf '%s\n' "$@" | xargs -P "${JOBS:-1}" -I{} "$0" --one "$seconds" {}
