# Runtime and entry point

## Entry point

The required Vaca entry point is `fn main() -> i64`. The entry point must be defined in the root source file; modules cannot define `main`.

## Process exit status

The compiler emits a small C-ABI `main` wrapper that returns the low 8 bits of the Vaca result as the process exit status, following the host OS convention.

## Standard input

`read_i64()` accepts optional leading ASCII whitespace and an optional `+` or `-`, followed by at least one decimal digit and then whitespace or EOF. The first whitespace after a number is consumed as its delimiter; any following token remains available to the next call.

## Fatal runtime errors

A malformed input token, end-of-file before a number, a value outside the `i64` range, division or remainder by zero, signed division or remainder overflow, invalid shift count, or signed arithmetic overflow terminates the program with status 1.
