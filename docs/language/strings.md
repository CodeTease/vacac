# Strings

Strings are immutable; they can be bound, assigned, passed, returned, and compared by content with `==` and `!=`.

## Concatenation

The `+` operator concatenates two strings. Concatenation copies both inputs into a newly allocated NUL-terminated string; these allocations remain alive until the process exits. Concatenation uses UTF-8 bytes without changing their encoding.

Other arithmetic and ordering comparisons are not supported for `str`. `\0` is rejected because strings are NUL-terminated at runtime, and raw `0x00` bytes inside a string literal are rejected for the same reason, so an embedded NUL can never make compile-time constant comparison disagree with runtime comparison.

## C strings

A `str` passed to or returned from `extern "C"` is a non-null, NUL-terminated `const char *`. Vaca does not copy the bytes or transfer ownership. String literals and concatenation results live until process exit; a C-returned buffer must stay readable and unchanged while Vaca uses it. A `NULL` C string result traps with status 1 instead of becoming a `str`. See [C FFI](../c-ffi.md).

## Printing

`print(str)` writes a string to stdout as-is, and `println(str)` writes it followed by a newline. `print("x = "); print_i64(x);` prints a labelled integer, while `print_f32(x);` and `print_f64(y);` print floating-point values.
