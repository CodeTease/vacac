#!/usr/bin/env bash

set -u

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
APEXC="${APEXC:-$ROOT_DIR/target/release/apexc}"
EXAMPLES_DIR="$ROOT_DIR/examples"

if [[ ! -x "$APEXC" ]]; then
    printf 'error: apexc executable not found or not executable: %s\n' "$APEXC" >&2
    printf 'Build it first with: cargo build --release\n' >&2
    exit 2
fi

shopt -s nullglob
examples=("$EXAMPLES_DIR"/*.apx)
if (( ${#examples[@]} == 0 )); then
    printf 'error: no .apx examples found in %s\n' "$EXAMPLES_DIR" >&2
    exit 2
fi

failures=0
for example in "${examples[@]}"; do
    name="${example##*/}"

    printf '[CHECK] %s\n' "$name"
    if ! "$APEXC" "$example" --check; then
        printf '[FAIL]  check: %s\n' "$name"
        failures=$((failures + 1))
        continue
    fi

    printf '[RUN]   %s\n' "$name"
    if [[ "$name" == "read_number.apx" ]]; then
        if ! printf '42\n' | "$APEXC" "$example" --run; then
            printf '[FAIL]  run: %s\n' "$name"
            failures=$((failures + 1))
            continue
        fi
    elif ! "$APEXC" "$example" --run; then
        printf '[FAIL]  run: %s\n' "$name"
        failures=$((failures + 1))
        continue
    fi

    printf '[PASS]  %s\n' "$name"
done

if (( failures > 0 )); then
    printf '\n%d example check/run failure(s) out of %d.\n' "$failures" "${#examples[@]}"
    exit 1
fi

printf '\nAll %d example(s) passed check and run.\n' "${#examples[@]}"