# Diagnostics

## Parser recovery

When parsing fails, the compiler attempts to recover at top-level declaration boundaries and within function bodies and match arms, reporting further syntax errors from later declarations, statements, and arms where possible. Recovery is best-effort: malformed declarations or nested constructs may be skipped, and any parser or module-loading diagnostics stop compilation before semantic checking and code generation.

Recovery reports multiple diagnostics where possible in recovery order, not necessarily source order.

## Source locations

Diagnostics use one-based source line and column numbers. Columns count Unicode scalar values (a tab is one source column); human-readable excerpts expand tabs to four-column stops and align carets using the same scalar-value convention. Messages name a whole character rather than a single UTF-8 byte, so an unexpected `λ` or `€` is reported verbatim and control characters are escaped as `\u{7}`.

## JSON diagnostics

JSON diagnostics always contain `severity`, `file`, `line`, `column`, `end_column`, and `message`; unavailable file or location values are `null`. Pass `--json` to render errors as JSON.
