# Bug Report — `vacac`

## B1
- **File and line:** `src/aggregate_validation.rs:23-28`
- **Severity:** critical
- **What's wrong:** `named_type_id` uses `.find_map(...)` on `Type::Tuple(elements)` and `Type::Applied(_, arguments)`, so it returns only the first element or type argument that resolves to a named struct/enum and ignores all remaining elements. When an earlier element is an acyclic named type (e.g., `Good`) and a later element is a recursive by-value type (e.g., `(Good, Bad)`), cycle detection misses the cycle and `Codegen::llvm_type` (`src/codegen/declarations.rs:200-205`) recurses infinitely, crashing `vacac` with a stack overflow (`SIGABRT`).
- **How to trigger it:** Run `vacac test.vca --check` on:
  ```rust
  struct Good { x: i64 }
  struct Bad { pair: (Good, Bad) }
  fn use_bad(b: Bad) -> i64 { 0 }
  fn main() -> i64 { 0 }
  ```

## B2
- **File and line:** `src/codegen/places.rs:198-219`, `src/codegen/places.rs:338-361`
- **Severity:** high
- **What's wrong:** During pointer-dereference assignment (`Stmt::AssignPlace`, e.g., `*expr = rhs;`), `emit_assignment_statement` calls `self.place_is_mutable(target)`, which invokes `self.emit_expression(value)` into the active LLVM IR builder just to inspect `pointer.ty`, and then `self.emit_place_pointer(target, ...)` at line 361 calls `self.emit_expression(value)` a second time. Every pointer expression on the left-hand side of `*expr = rhs;` is evaluated twice at runtime, duplicating side effects, allocations, and bounds checks.
- **How to trigger it:** Compile and run:
  ```rust
  fn side_effect(v: &i64) -> i64 {
      print_i64(777);
      0
  }
  fn main() -> i64 {
      let x = 10;
      let arr: [*mut i64; 1] = [&raw mut x];
      let dummy = 0;
      *arr[side_effect(&dummy)] = 42;
      x - 42
  }
  ```
  Running the compiled binary prints `777` twice instead of once.

## B3
- **File and line:** `src/sema/expressions.rs:580-609`, `src/sema/expressions.rs:898-913`
- **Severity:** high
- **What's wrong:** When a shared borrow `&place` is passed directly as a call argument (e.g., `foo(&a, other_arg)`), `sema` sets `self.allow_shared_borrow = true` while checking `&a` so mutable locals can be borrowed, but never records `place` in `self.shared_borrows` while checking the remaining arguments of the call. Later arguments in the same call can therefore mutate `a` directly or via `&mut a` after `&a` has already been evaluated, violating shared-reference immutability in safe Vaca code.
- **How to trigger it:** Compile and run:
  ```rust
  fn observe(r: &i64, computed: i64) -> i64 {
      print_i64(*r);
      0
  }
  fn mutate(m: &mut i64) -> i64 {
      *m = 999;
      1
  }
  fn main() -> i64 {
      let a = 10;
      observe(&a, loop {
          const m: &mut i64 = &mut a;
          break mutate(m);
      })
  }
  ```
  This compiles without error and mutates `a` through `&mut a` while `&a` is live for `observe`.

## B4
- **File and line:** `src/sema/places.rs:261-289`, `src/codegen/places.rs:177-195`
- **Severity:** high
- **What's wrong:** `Checker::check_raw_pointer_place` resolves `root` via `self.lookup_local(root)` without checking `self.lambda_scope_boundaries` or recording a closure capture. Inside a lambda, `&raw const x` or `&raw mut x` on an enclosing local `x` passes `sema`, and in `codegen` (where lambda functions clear `self.scopes` and populate only captured bindings and parameters) `lookup_local("x")` returns `None` and falls back to `self.resolve_global_name("x")`, silently taking the address of a global named `x` instead of the local `x` (or failing in codegen if no global `x` exists).
- **How to trigger it:** Compile and run:
  ```rust
  let x: i64 = 999;
  fn main() -> i64 {
      let x = 42;
      let f: fn() -> *mut i64 = fn() -> *mut i64 {
          &raw mut x
      };
      let p = f();
      *p = 777;
      print_i64(x);
      0
  }
  ```
  `*p = 777` silently overwrites the global `x` while local `x` remains `42`.

## B5
- **File and line:** `src/modules/resolve.rs:233-262`
- **Severity:** high
- **What's wrong:** `resolve_reference` checks `symbols.contains_key(name)` (which matches root-module declarations, since root names are unprefixed) *before* checking `qualify(module, name)` (symbols declared in the current child module). Inside any child module, an unqualified reference to that module's own function, global, struct, or enum is shadowed by any root-module declaration of the same name — silently reading/writing/calling the root declaration if it is `pub`, or failing with a bogus `"is private"` error if the root declaration is private.
- **How to trigger it:** Compile `main.vca` and `child.vca` in the same directory and run `vacac main.vca --run`:
  ```rust
  // main.vca
  mod child;
  pub let x: i64 = 1;
  fn main() -> i64 {
      let c = child::update();
      print_i64(x);
      print_i64(c);
      0
  }

  // child.vca
  let x: i64 = 100;
  pub fn update() -> i64 {
      x = 42;
      x
  }
  ```
  `child::update()` overwrites `main.vca`'s `x` (printing `42` and `42` instead of `1` and `42`).

## B6
- **File and line:** `src/modules/resolve.rs:464-471`
- **Severity:** high
- **What's wrong:** In `resolve_expr` for `Expr::Call`, `resolve_reference` runs unconditionally on `name` without tracking local bindings or parameters in scope. If a function in a child module calls a parameter or `let` binding `cb` of function type (`fn(...) -> ...`) whose name matches a private function `fn cb(...)` in the root module, compilation fails with `function 'cb' is private` because `resolve_reference` resolves `cb` against the root module's symbol table.
- **How to trigger it:** Compile `main.vca` and `child.vca`:
  ```rust
  // main.vca
  mod child;
  fn cb() -> i64 { 99 }
  fn main() -> i64 {
      child::run(fn() -> i64 { 42 }) - 42
  }

  // child.vca
  pub fn run(cb: fn() -> i64) -> i64 {
      cb()
  }
  ```

## B7
- **File and line:** `src/main.rs:185-189`, `src/main.rs:243-250`
- **Severity:** high
- **What's wrong:** `Executable` mode does not check whether the output executable (`-o`) or derived object path (`output.with_extension("o")`) collides with the input source file or any `--link-object` file. Running `vacac main.vca -o main.vca` permanently overwrites the user's source file with the ELF executable, and running `vacac main.vca --link-object helper.o -o helper` clobbers `helper.o` with `main.vca`'s object file before invoking `cc`, destroying the input object file and causing linking to fail with duplicate/undefined symbol errors.
- **How to trigger it:**
  1. `vacac main.vca -o main.vca` — `main.vca` is overwritten on disk by an ELF binary.
  2. `cc -c helper.c -o helper.o && vacac main.vca --link-object helper.o -o helper` — `object_path("helper")` evaluates to `helper.o` and overwrites `helper.o` before `link_object`.

## B8
- **File and line:** `src/sema/matching.rs:104-123`, `src/codegen/matching.rs:82-90`, `src/codegen/matching.rs:443-448`
- **Severity:** medium
- **What's wrong:** In `Checker::check_scalar_match`, `MatchPattern::Wildcard` (lines 104–112) does not check `if wildcard_seen`, allowing `match 5 { _ => { 1 }, _ => { 2 } }` and `match 5 { x => { x }, _ => { 2 } }` to pass `sema` completely. In addition, both `emit_match` and `emit_scalar_match` in `codegen` break out of the dispatch-chain loop on a wildcard/binding arm while still emitting all `arm_blocks`, leaving any subsequent `match.next` blocks unterminated if reached.
- **How to trigger it:** Compile:
  ```rust
  fn main() -> i64 {
      match 5 {
          x => { x },
          _ => { 2 },
      }
  }
  ```
  `sema` accepts the unreachable `_` arm and compilation only fails later in `codegen` with a misleading `"duplicate wildcard match arm"` error.

## B9
- **File and line:** `src/parser/expressions.rs:257-301`, `src/parser/expressions.rs:373-398`
- **Severity:** medium
- **What's wrong:** The parser treats any `path::Name { ... }` (when `path.len() >= 2` and `!self.parsing_match_value`) and any single identifier matching a declared struct name (`self.struct_names.contains(&name)`) followed by `{` as a struct literal. Because `if`, `while`, and `for` headers call `parse_expression(0)` without suppressing struct-literal parsing before the block's opening `{` (and line 373 ignores `self.parsing_match_value` altogether), valid module-qualified globals/variants in `if`/`while`/`for` headers and local variables shadowing struct names in `if`/`while`/`for`/`match` headers fail to parse.
- **How to trigger it:**
  1. Module-qualified constant in an `if` condition (`m.vca` contains `pub const ENABLED: bool = true;`):
     ```rust
     mod m;
     fn main() -> i64 {
         if m::ENABLED { 0 } else { 1 }
     }
     ```
     Fails with `expected field name, found integer '0'`.
  2. Variable shadowing a struct name in `if` or `match`:
     ```rust
     struct Point { x: i64 }
     fn main() -> i64 {
         let Point = 1;
         match Point { 1 => { 0 }, _ => { 1 } }
     }
     ```
     Fails with `expected field name, found integer '1'`.

## B10
- **File and line:** `src/parser/matching.rs:175`
- **Severity:** medium
- **What's wrong:** `parse_match` parses the match scrutinee with `self.parse_expression(7)` instead of `self.parse_expression(0)`. Because precedence `7` excludes `==`/`!=` (`6`), `&` (`5`), `^` (`4`), `|` (`3`), `&&` (`2`), and `||` (`1`), any unparenthesized equality, bitwise, or logical expression in a `match` scrutinee is rejected with a syntax error.
- **How to trigger it:** Compile:
  ```rust
  fn main() -> i64 {
      match 1 == 1 {
          true => { 0 },
          false => { 1 },
      }
  }
  ```
  Fails with `expected '{' before match arms, found '=='`.


## B11
- **File and line:** `src/parser/expressions.rs:302-308`, `src/parser/expressions.rs:455-489`
- **Severity:** medium
- **What's wrong:** `generic_struct_literal_ahead` scans tokens all the way to EOF counting `<`, `>`, and `>>` without stopping at statement or expression boundaries (`;`, `)`, `,`, `=`). Because a `match` arm arrow `=> {` is tokenized as `TokenKind::Equal`, `TokenKind::Greater`, `TokenKind::LBrace`, any `<` comparison on a module-qualified value (or variable sharing a struct's name) earlier in the file whose `depth` reaches `0` at a subsequent `=> {` is misclassified as generic struct type arguments.
- **How to trigger it:** Compile `main.vca` with `m.vca` (`pub const X: i64 = 1;`):
  ```rust
  mod m;
  fn main() -> i64 {
      let ok = m::X < 5;
      match ok {
          true => { 0 },
          false => { 1 },
      }
  }
  ```
  Fails on `let ok = m::X < 5;` with `expected type ..., found integer '5'`.

## B12
- **File and line:** `src/modules/generics/expected.rs:207-216`, `src/modules/generics/traversal.rs:222-230`, `src/modules/generics/inference.rs:250-255`
- **Severity:** medium
- **What's wrong:** During generic monomorphization, `Expr::Lambda` replaces `variables` with only `params` (discarding outer local variables in scope) and calls `self.process_block` instead of `self.process_block_with_expected(..., &expected_return, ...)`, while `Stmt::Let` / `Expr::Match` also fails to specialize `MatchPattern::Variant` on values returned from local `fn(...) -> Result<...>` calls. Consequently, lambdas cannot infer generic function calls using captured locals or specialize tail generic enum constructors (`Result::Ok(...)`), and matching on a `Result` returned from a local function value fails.
- **How to trigger it:** Compile either:
  ```rust
  fn identity<T>(v: T) -> T { v }
  fn main() -> i64 {
      let outer = 42;
      let f: fn() -> i64 = fn() -> i64 { identity(outer) };
      f() - 42
  }
  ```
  (fails with `cannot infer generic type parameter 'T' for 'identity'`) or:
  ```rust
  fn main() -> i64 {
      let f: fn() -> Result<i64, str> = fn() -> Result<i64, str> { Result::Ok(42) };
      match f() { Result::Ok(v) => { v - 42 }, Result::Err(_) => { 1 } }
  }
  ```
  (fails with `cannot infer generic type parameter 'E' for 'Result'`).

## B13
- **File and line:** `src/codegen/globals/constants.rs:321-333`
- **Severity:** medium
- **What's wrong:** `Codegen::evaluate_constant_expr` evaluates both `left` and `right` of `Expr::Binary` eagerly before inspecting `op`, ignoring the short-circuiting semantics of `BinaryOp::And` (`&&`) and `BinaryOp::Or` (`||`). Global constant initializers such as `false && (1 / 0 == 0)` or `true || (arr[99] == 0)` fail compilation even though identical runtime expressions short-circuit cleanly in `Codegen::emit_logical`.
- **How to trigger it:** Compile:
  ```rust
  const SAFE: bool = false && (1 / 0 == 0);
  fn main() -> i64 {
      if SAFE { 1 } else { 0 }
  }
  ```
  Fails with `invalid integer division in constant expression`.

## B14
- **File and line:** `src/sema/statements.rs:18-21`, `src/codegen/statements.rs:150-164`, `src/codegen/loops.rs:37-42`
- **Severity:** medium
- **What's wrong:** `Checker::check_block` checks every statement in a block even after an unconditional `return`, allowing an unreachable `break <expr>;` to populate a value-producing `loop`'s `result_type`, whereas `Codegen::emit_block` stops emitting statements once `terminated` is `true`. A diverging `loop` expression such as `loop { return 0; break 1; }` therefore passes `sema` and then fails in `codegen` with `"value-producing loop must contain a 'break' with a value"`.
- **How to trigger it:** Compile:
  ```rust
  fn f() -> i64 {
      let x = loop {
          return 0;
          break 1;
      };
      x
  }
  fn main() -> i64 { f() }
  ```

## B15
- **File and line:** `src/lexer.rs:421-505`, `src/codegen/globals/constants.rs:672`, `src/codegen/runtime/strings.rs:196`
- **Severity:** medium
- **What's wrong:** Although the lexer rejects `\0` escape sequences (`src/lexer.rs:451-461`) because strings are lowered to NUL-terminated C strings (`strlen`/`strcmp`/`printf`), the `Some(&byte)` branch at line 493 accepts raw `0x00` bytes directly inside string literals. Embedded raw NUL bytes silently truncate strings on output and concatenation at runtime and cause `==`/`!=` on compile-time globals (which compare full Rust `String`s) to disagree with runtime `==`/`!=` (which call `strcmp`).
- **How to trigger it:** Write a `.vca` file containing raw `0x00` bytes inside string literals (`"a\x00b"` and `"a\x00c"`):
  ```rust
  const G1: str = "a\0b"; // raw 0x00 byte in source file
  const G2: str = "a\0c";
  const GEQ: bool = G1 == G2;
  fn main() -> i64 {
      let l1 = "a\0b";
      let l2 = "a\0c";
      if GEQ != (l1 == l2) { return 99; }
      0
  }
  ```
  Running the program exits with status `99` (`GEQ` is `false`, while `l1 == l2` is `true`).


## B16
- **File and line:** `src/sema/statements.rs:373-383`, `src/sema/places.rs:261-269`
- **Severity:** medium
- **What's wrong:** Two mutability checks in `sema` are skipped: (1) in `Stmt::AssignPlace` (`src/sema/statements.rs:373-383`), `if vector_place` returns `Ok(false)` *before* checking `if through_pointer { if !self.place_is_mutable(target)? ... }`, letting `(*const_ptr).vec_field[0] = 42;` bypass `sema` into `codegen`; and (2) `check_raw_pointer_place` (`src/sema/places.rs:261-269`) calls `self.reject_mutably_borrowed(root, span)` without checking `self.shared_borrows`, allowing `&raw mut a` to be created and mutated through while an immutable safe borrow `const r: &i64 = &a;` is active.
- **How to trigger it:**
  1. `struct H { v: Vec<i64> } fn main() -> i64 { let v: Vec<i64> = vec_new(); vec_push(v, 1); let h = H { v: v }; let p: *const H = &raw const h; (*p).v[0] = 2; 0 }` (`sema` accepts; `codegen` rejects).
  2. `fn main() -> i64 { let a = 10; const r: &i64 = &a; const p: *mut i64 = &raw mut a; *p = 99; print_i64(*r); 0 }` (compiles and prints `99` while `r: &i64` borrows `a`).

## B17
- **File and line:** `src/lexer.rs:542-551`, `src/lexer.rs:473-487`
- **Severity:** low
- **What's wrong:** When an unexpected non-ASCII character appears outside a comment or string literal (line 550), `b as char` casts only the leading UTF-8 byte as Latin-1, producing mojibake in the error message (e.g., `λ` [`0xCE 0xBB`] is reported as ``unexpected character `Î` ``). In addition, unknown escape handling at lines 479 and 490–491 assumes a 1-byte escape character (`end_column: escape_column + 2`, `i += 2`).
- **How to trigger it:** Run `vacac test.vca --check` on `fn main() -> i64 { λ }` — outputs ``unexpected character `Î` `` instead of ``unexpected character `λ` ``.

## B18
- **File and line:** `src/modules.rs:12-13`
- **Severity:** low
- **What's wrong:** `modules::load` propagates errors from `resolve::resolve_names(&mut program)?` and `generics::monomorphize(&mut program)?` directly via `?` without calling `with_source_id` using `program.source_files` and `program.source_texts` (unlike `sema::check` in `src/main.rs:214-224` and `backend_diagnostic` in `src/main.rs:338-361`). As a result, all name-resolution and generic-specialization diagnostics emit `"file": null` in `--json` mode and omit the file path, source line excerpt, and caret underline in human-readable mode.
- **How to trigger it:** Run `vacac main.vca --check --json` on:
  ```rust
  fn broken<T>(x: Unknown) -> i64 { 0 }
  fn main() -> i64 { 0 }
  ```
  Emits `{"severity":"error","file":null,"line":1,"column":14,"end_column":21,"message":"unknown type `Unknown`"}`.

## B19
- **File and line:** `src/codegen/output.rs:5`
- **Severity:** low
- **What's wrong:** `Codegen::target_machine` calls `Target::initialize_x86(&InitializationConfig::default())` instead of `Target::initialize_native`, even though line 6 queries `TargetMachine::get_default_triple()`. On any non-x86 host architecture (such as `aarch64-unknown-linux-gnu` or `aarch64-apple-darwin`), `Target::from_triple(&triple)` fails because the host's LLVM target backend was never initialized.
- **How to trigger it:** Build and run `vacac examples/hello.vca --check` on an ARM64 (`aarch64`) Linux or macOS host: compilation fails in `Codegen::target_machine` with `No available targets are compatible with triple ...`.

## B20
- **File and line:** `src/main.rs:64-75`, `src/main.rs:289-302`
- **Severity:** low
- **What's wrong:** `parse_args()` unconditionally consumes the first CLI argument (unless `-h`/`--help` or `-V`/`--version`) as `<source.vca>`, and `link_object` passes `object`, `link_objects`, and `output` to `cc` without guarding leading `-` paths. Passing options before the source file (e.g., `vacac --quiet main.vca --check`) fails with `unknown argument: main.vca`, and passing a dash-prefixed path to `-o`, `--emit-obj`, or `--link-object` (e.g., `vacac main.vca -o -E`) causes `cc` to treat the path as a compiler flag and leaves `-E.o` behind on disk.
- **How to trigger it:**
  1. `vacac --quiet examples/hello.vca --check` -> `vacac: unknown argument: examples/hello.vca (try --help)`.
  2. `vacac examples/hello.vca -o -E` -> `cc: error: unrecognized command-line option '-E.o'`.


---

## Summary

| ID | File | Severity | One-line Description |
|---|---|---|---|
| B1 | `src/aggregate_validation.rs` | critical | `find_map` in `named_type_id` misses recursive by-value cycles after an acyclic named tuple/generic element, crashing `vacac` via stack overflow in codegen. |
| B2 | `src/codegen/places.rs` | high | `place_is_mutable` emits live LLVM IR for pointer dereference targets before `emit_place_pointer` emits them again, evaluating LHS pointer expressions twice at runtime. |
| B3 | `src/sema/expressions.rs` | high | Direct `&place` call arguments are not recorded in `shared_borrows` across sibling arguments, allowing the borrowed variable to be mutated in the same call. |
| B4 | `src/sema/places.rs` | high | Raw address-taking (`&raw const`/`&raw mut`) inside closures skips capture checks in `sema` and silently resolves to same-named globals in `codegen`. |
| B5 | `src/modules/resolve.rs` | high | `resolve_reference` checks root-module symbols before current-module symbols, causing root declarations to shadow or block child-module declarations of the same name. |
| B6 | `src/modules/resolve.rs` | high | `resolve_expr` resolves unqualified call names against module symbols without tracking local scopes, rejecting calls to local closures when a private root function shares the name. |
| B7 | `src/main.rs` | high | Unchecked `-o` and derived `.o` output paths can overwrite and destroy the input `.vca` source file or `--link-object` inputs before linking. |
| B8 | `src/sema/matching.rs` | medium | `check_scalar_match` does not check `wildcard_seen` on `_` arms, letting unreachable wildcard arms after binding/wildcard arms bypass `sema`. |
| B9 | `src/parser/expressions.rs` | medium | `if`/`while`/`for` headers and struct-named variables eagerly parse `{` as struct literals, rejecting valid module globals/variants and shadowed struct names before block `{`. |
| B10 | `src/parser/matching.rs` | medium | `parse_match` parses the match scrutinee with `min_precedence = 7`, rejecting valid unparenthesized `==`, `!=`, bitwise, and `&&`/`||` scrutinees. |
| B11 | `src/parser/expressions.rs` | medium | `generic_struct_literal_ahead` scans `<` and `>` across statements up to `=> {`, misparsing `<` comparisons on module paths or struct-named variables as generic type arguments. |
| B12 | `src/modules/generics/expected.rs` | medium | Generic specialization drops outer local types inside lambdas and fails to propagate expected/returned generic enum types through lambdas and indirect calls. |
| B13 | `src/codegen/globals/constants.rs` | medium | Compile-time constant evaluation eagerly evaluates the RHS of `&&` and `||`, failing on short-circuited expressions like `false && (1 / 0 == 0)` that succeed at runtime. |
| B14 | `src/sema/statements.rs` | medium | Diverging value `loop`s with an unreachable `break <expr>;` after `return` pass `sema` but fail in `codegen` because `emit_block` stops at `return`. |
| B15 | `src/lexer.rs` | medium | Raw `0x00` bytes inside string literals are accepted despite `\0` escapes being banned, truncating strings at runtime and causing compile-time vs. runtime `==` divergence. |
| B16 | `src/sema/statements.rs` | medium | `sema` skips pointer-mutability checks on vector element assignments through `*const` pointers and skips `shared_borrows` checks on `&raw mut`. |
| B17 | `src/lexer.rs` | low | Unexpected non-ASCII bytes are formatted via `b as char` (producing corrupted UTF-8 in diagnostics) and unknown multi-byte escapes assume a 1-byte character width. |
| B18 | `src/modules.rs` | low | Name-resolution and monomorphization errors are returned without `with_source_id`, causing `"file": null` in `--json` mode and missing file paths/underlines in human mode. |
| B19 | `src/codegen/output.rs` | low | `Codegen::target_machine` initializes only the x86 LLVM target instead of the native target, breaking compilation on non-x86 hosts such as `aarch64`. |
| B20 | `src/main.rs` | low | CLI flags before `<source.vca>` are misparsed as the source path, and dash-prefixed paths in `-o`/`--emit-obj`/`--link-object` are interpreted as flags by `cc`. |

**Total count:** 20

