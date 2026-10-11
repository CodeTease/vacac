#!/usr/bin/env bash

set -u

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
APEXC="${APEXC:-$ROOT_DIR/target/release/apexc}"
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

if [[ ! -x "$APEXC" ]]; then
    printf 'error: apexc executable not found or not executable: %s\n' "$APEXC" >&2
    printf 'Build it first with: cargo build --release\n' >&2
    exit 2
fi

mapfile -d '' examples < <(
    {
        find "$EXAMPLES_DIR" -maxdepth 1 -type f -name '*.apx' -print0
        find "$EXAMPLES_DIR" -mindepth 2 -type f -name main.apx -print0
    } | sort -z
)
if (( ${#examples[@]} == 0 )); then
    printf 'error: no .apx examples found in %s\n' "$EXAMPLES_DIR" >&2
    exit 2
fi

failures=0
for example in "${examples[@]}"; do
    name="${example#"$EXAMPLES_DIR"/}"

    printf '%s[CHECK]%s %s\n' "$BLUE" "$RESET" "$name"
    if ! "$APEXC" "$example" --check; then
        printf '%s[FAIL]%s  check: %s\n' "$RED" "$RESET" "$name"
        failures=$((failures + 1))
        continue
    fi

    printf '%s[RUN]%s   %s\n' "$YELLOW" "$RESET" "$name"
    if [[ "${name##*/}" == "read_number.apx" ]]; then
        if ! printf '42\n' | "$APEXC" "$example" --run; then
            printf '%s[FAIL]%s  run: %s\n' "$RED" "$RESET" "$name"
            failures=$((failures + 1))
            continue
        fi
    elif ! "$APEXC" "$example" --run; then
        printf '%s[FAIL]%s  run: %s\n' "$RED" "$RESET" "$name"
        failures=$((failures + 1))
        continue
    fi

    printf '%s[PASS]%s  %s\n' "$GREEN" "$RESET" "$name"
done

if (( failures > 0 )); then
    printf '\n%s%d example check/run failure(s) out of %d.%s\n' "$RED" "$failures" "${#examples[@]}" "$RESET"
    exit 1
fi

printf '\n%sAll %d example(s) passed check and run.%s\n' "$GREEN" "${#examples[@]}" "$RESET"