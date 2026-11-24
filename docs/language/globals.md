# Global constants and variables

Declare module-level values with `const` (immutable) or `let` (mutable). Types may be written explicitly or inferred from the initializer:

```vaca
const LIMIT: i64 = 40 + 2;
const NEXT = LIMIT + 1;
pub let total: i64 = NEXT;

fn main() -> i64 {
    total = total + 1;
    total - 44
}
```

## Initializers

Global initializers must be compile-time constant expressions. They may use scalar and aggregate literals, constant arithmetic, and references to immutable globals (including public globals in other modules); function calls, local variables, mutable-global dependencies, and initializer cycles are rejected.

Globals use static LLVM storage—there is no startup initializer execution.

## Mutability and visibility

Mutable globals can be assigned, while constants cannot be rebound or mutated through fields or array elements. As with other declarations, a global in a different module must be marked `pub` to be accessed through its qualified name.
