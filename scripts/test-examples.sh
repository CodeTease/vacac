#!/usr/bin/env bash

set -u

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
VACAC="${VACAC:-$ROOT_DIR/target/release/vacac}"
EXAMPLES_DIR="$ROOT_DIR/examples"

# Use colors only for interactive output, and honor the NO_COLOR convention.
if [[ -t 1 && "${TERM:-}" != "dumb" && -z "${NO_COLOR:-}" ]]; then
    RESET=$'\033[0m'
    BLUE=$'\033[34m'
    YELLOW=$'\033[33m'
    GREEN=$'\033[32m'
    RED=$'\033[31m'
else
    RESET=''
    BLUE=''
    YELLOW=''
    GREEN=''
    RED=''
fi

if [[ ! -x "$VACAC" ]]; then
    printf 'error: vacac executable not found or not executable: %s\n' "$VACAC" >&2
    printf 'Build it first with: cargo build --release\n' >&2
    exit 2
fi

mapfile -d '' examples < <(
    {
        find "$EXAMPLES_DIR" -maxdepth 1 -type f -name '*.vca' -print0
        find "$EXAMPLES_DIR" -mindepth 2 -type f -name main.vca -print0
    } | sort -z
)
if (( ${#examples[@]} == 0 )); then
    printf 'error: no .vca examples found in %s\n' "$EXAMPLES_DIR" >&2
    exit 2
fi

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT
printf '%s\0' "${examples[@]}" > "$tmp_dir/list.txt"

run_single_test() {
    local example="$1"
    local name="${example#"$EXAMPLES_DIR"/}"

    printf '%s[CHECK]%s %s\n' "$BLUE" "$RESET" "$name"
    if ! "$VACAC" "$example" --check; then
        printf '%s[FAIL]%s  check: %s\n' "$RED" "$RESET" "$name"
        return 1
    fi

    printf '%s[RUN]%s   %s\n' "$YELLOW" "$RESET" "$name"
    if [[ "${name##*/}" == "read_number.vca" ]]; then
        if ! printf '42\n' | "$VACAC" "$example" --run; then
            printf '%s[FAIL]%s  run: %s\n' "$RED" "$RESET" "$name"
            return 1
        fi
    elif ! "$VACAC" "$example" --run; then
        printf '%s[FAIL]%s  run: %s\n' "$RED" "$RESET" "$name"
        return 1
    fi

    printf '%s[PASS]%s  %s\n' "$GREEN" "$RESET" "$name"
    return 0
}
export -f run_single_test
export VACAC EXAMPLES_DIR BLUE YELLOW GREEN RED RESET

jobs_count=$(nproc 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo 4)

xargs -0 -P "$jobs_count" -I {} bash -c 'run_single_test "$@"' _ {} < "$tmp_dir/list.txt"
exit_code=$?

if (( exit_code != 0 )); then
    printf '\n%sSome example check/run failure(s).%s\n' "$RED" "$RESET"
    exit 1
fi

printf '\n%sAll %d example(s) passed check and run.%s\n' "$GREEN" "${#examples[@]}" "$RESET"
