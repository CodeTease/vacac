# C FFI

Vaca can call native C functions through declarations such as:

```vaca
extern "C" fn labs(value: i64) -> i64;
extern "C" fn native_label() -> str;
extern "C" fn native_fill(label: str, out: *mut i64) -> i64;
```

The C symbol is the declared function name. The following types cross the boundary:

| Vaca type | C representation |
|---|---|
| `i64` | `int64_t` |
| `f32` | `float` |
| `f64` | `double` |
| `str` | `const char *` to non-null, NUL-terminated UTF-8 bytes |
| `*const T`, `*mut T` | Native object pointer; Vaca still checks the pointee type and mutability |
| `unit` | `void`, return only |

Raw pointers may target nested pointers and other legal raw-pointer pointees. Safe references, booleans, tuples, fixed arrays, vectors, structs, enums, function values, and variadic functions are not extern types.

## Strings

A `str` argument is passed as a pointer to its existing bytes. Vaca does not copy the string or transfer ownership; C must not modify or free it. String literals and concatenation results live until process exit.

A `str` result is treated as a borrowed, NUL-terminated C string. The returned buffer must remain readable and unchanged for as long as Vaca uses the string; static immutable storage is the simplest choice. Vaca copies no bytes and cannot validate a foreign pointer beyond a `NULL` check. A `NULL` direct `str` result is a fatal runtime error (process status 1) so a null pointer is never observed as a Vaca `str`. Raw-pointer results may be null and should be compared with typed null.

`*mut str` is a pointer to a writable **string slot** (`const char **`), not a pointer to writable string bytes. C may replace the slot with a valid borrowed string pointer. For nullable C-string APIs, prefer a C shim that returns a status and writes a valid string into a `*mut str` out slot only on success.

## Raw pointers

Raw pointers can be passed and returned directly. Type and mutability must match exactly; there is no implicit const/mut conversion, and a C pointer cannot become a safe reference. Use typed null in Vaca:

```vaca
const missing = native_lookup();
if missing == null::<*const Item>() {
    return 1;
}
```

Form pointers to local or global storage with `&raw const place` or `&raw mut place`, including struct fields, fixed-array elements, and vector elements. Vector element pointers point into the vector's current backing storage. A later `vec_push` through any alias may relocate that storage, so reacquire the pointer afterward and never let C retain it beyond the storage's lifetime.

Dereferencing a raw pointer, including one returned by C, has **no runtime validity, lifetime, or alignment check**. Vaca's struct, enum, vector, and closure layouts are not a promised C ABI. Pass scalar subplaces when C needs typed access, and use pointers to other Vaca values only as opaque handles.

## Linking native code

For native object files or libraries, pass repeatable linker inputs while building or running:

```sh
vacac app.vca --link-object support.o --link-search ./lib --link-lib support -o app
vacac app.vca --run --link-lib m
```

`--link-object <path>`, `--link-search <directory>`, and `--link-lib <name>` are available for executable and `--run` modes only; they are not used by `--check` or `--emit-llvm`. See [Using the compiler](usage.md#linking-native-code) for the full CLI description.

[`examples/c_ffi.vca`](../examples/c_ffi.vca) calls the C standard library's `labs()` and `atol()` functions.
