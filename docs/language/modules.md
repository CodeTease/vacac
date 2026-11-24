# Modules and imports

## Hierarchical modules

Declare a child module with `mod name;`. The compiler loads its source relative to the declaring file, recursively checks all module files as one program, and resolves names such as `geometry::distance_squared()` or `geometry::point::Point`. Module files can use either `name.vca` or `name/mod.vca`; nested declarations are resolved relative to their containing module directory. For example, `mod geometry;` in `main.vca` can load `geometry.vca`, and a `mod point;` declared in `geometry.vca` can load `geometry/point.vca`.

Declarations in another module are accessible only when marked `pub`; private functions, structs, and enums remain local to their declaring module. The root source file must define the required `fn main() -> i64` entry point. Modules can import the names they use; see [Module imports and aliases](#module-imports-and-aliases).

The [`modules/`](../../examples/modules/) example demonstrates nested modules, imports, qualified type and function names, and enum matching. From the repository root, check, compile, or run it with:

```sh
target/release/vacac examples/modules/main.vca --check
target/release/vacac examples/modules/main.vca -o modules-example
./modules-example
# Or compile and run without leaving artifacts:
target/release/vacac examples/modules/main.vca --run
```

## Module imports and aliases

Qualified names work across modules, but a module can also import the names it uses with `use`. The path is resolved like any other name: relative to the declaring module first, then from the root. Without `as`, the last path segment is the local name; with `as`, the alias is:

```vaca
mod geometry;

use geometry as geo;
use geometry::point::Point;
use geometry::point::Direction as Heading;
use geometry::distance_squared;

fn distance() -> i64 {
    let origin = Point { x: 0, y: 0 };
    let heading = Heading::North;
    match heading {
        Heading::North => { distance_squared() + origin.x },
        Heading::South => { geo::distance_squared() },
    }
}
```

An import may name a module, function, extern function, global, struct, enum, or trait. A single enum variant can also be imported and then used directly as a value, a call target, or a match pattern, including payload patterns:

```vaca
use shapes::Shape;
use shapes::Shape::Circle;

fn radius(shape: Shape) -> i64 {
    match shape {
        Circle(radius) => { radius },
        Shape::Square => { 0 },
    }
}
```

Imports are private to the module that declares them, so another module needs its own `use` declaration. Re-exports (`pub use`), grouped imports (`use a::{b, c}`), and glob imports are not supported. Each local name may be imported once, may not collide with a declaration or submodule of the same name in the module, and cannot be `Result` or a built-in. Importing a private declaration from another module is rejected, and imported names stay unusable outside the importing module. In a `match` arm an imported unit variant is a variant pattern rather than a new binding.

Internal names are still fully qualified, so imports are resolved before trait validation and generic specialization and add no runtime cost. See [Traits and static dispatch](traits.md) and [Generics](generics.md).
