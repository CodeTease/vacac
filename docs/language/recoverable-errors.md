# Recoverable errors

The compiler provides the generic `Result<T, E>` enum with `Ok(T)` and `Err(E)` variants. Use it as a function's return type to represent success or a recoverable error. Postfix `?` unwraps an `Ok` value; on `Err`, it immediately returns that error from the current function. The enclosing function must also return a `Result` with the same error type.

`Result<unit, E>` and `()` make `?` usable for operations that only produce side effects:

```vaca
enum LookupError { Missing }

fn lookup(id: i64) -> Result<i64, LookupError> {
    if id == 42 {
        Result::Ok(42)
    } else {
        Result::Err(LookupError::Missing)
    }
}

fn read_answer() -> Result<i64, LookupError> {
    let answer = lookup(42)?;
    Result::Ok(answer + 1)
}
```

`Result` constructors infer their type arguments from an expected type, such as the function's return type or a binding annotation. Match on the returned value at a boundary where you want to handle errors rather than propagate them. See [`result_propagation.vca`](../../examples/result_propagation.vca) for a complete, runnable example.
