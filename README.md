# dbt-asdl

`dbt-asdl` parses Abstract Syntax Description Language (ASDL) files and generates Abstract Syntax Tree (AST) source code.

## Overview

The parser reads an ASDL specification and validates all type and constructor definitions. This dialect extends the Zephyr ASDL grammar with fixed-width integer types (`byte`, `int`, `long`), the `bool` type, and `@stable` type annotations.

The Rust tree backend (`dbt_asdl::codegen::rust::generate_trees`) writes five source files from one `.asdl` input file:

- `<stem>.rs`: shared types (fieldless enums and plain copyable structs).
- `<stem>_arena.rs`: the arena-allocated tree, which uses a `bumpalo` arena lifetime.
- `<stem>_owned.rs`: the self-owned tree, which uses `Box` links and standard `Vec` containers.
- `<stem>_views.rs`: the read-only view API over both tree representations.
- `<stem>_sexpr.rs`: the [`dbt_asdl::Sexpr`] implementations for the generated AST types.

The flat Rust backend (`dbt_asdl::codegen::rust::generate_flat`) writes two source files for a stable `#[repr(C)]` ABI across shared library and language boundaries:

- `<dst>`: `#[repr(C)]` record structs, typed node reference newtypes, constructor tag enums, `TreeBuilder`, `@stable` typed projections, layout freeze dump, and schema descriptor tables.
- `gen_convert.rs`: functions that convert an arena or owned AST (through the read-only view API) into the flat boundary representation.

The flat C++ backend (`dbt_asdl::codegen::cpp::generate_flat`) writes one C++17 header file for flat tree consumers:

- `<dst>`: C-compatible record structs, typed `<Type>Ref` handles, tag enums, `<Type>View` classes, and `as_<type>` projection functions for all ASDL types.

The C++ runtime header (`include/dbt_asdl/flat_runtime.hpp`) wraps `*mut FlatTree` in `dbt_asdl::Tree` and `dbt_asdl::TreeView` for untyped and typed tree traversal. `dbt_asdl::codegen::cpp::write_runtime_header` writes `<dbt_asdl/flat_runtime.hpp>` into an include directory for build scripts and SDK bundles.

## Rust Build (`build.rs`)

1. Add `dbt-asdl` to `[dependencies]` and `[build-dependencies]` in `Cargo.toml`:

```toml
[dependencies]
dbt-asdl = "1.0"

[build-dependencies]
dbt-asdl = "1.0"
```

2. Call `dbt_asdl::codegen::rust::generate_trees` from `build.rs`:

```rust
use std::env;
use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("cargo sets OUT_DIR"));
    let asdl_path = manifest.join("sql.asdl");

    println!("cargo:rerun-if-changed={}", asdl_path.display());

    dbt_asdl::codegen::rust::generate_trees(&asdl_path, out_dir.join("gen.rs"))
        .expect("failed to generate Rust AST from sql.asdl");
}
```

## Rust API Usage

### Arena, Owned, and View Trees

The generated source files map to sibling modules in the consumer crate:

```rust
pub mod shared {
    include!(concat!(env!("OUT_DIR"), "/gen.rs"));
}
pub mod arena {
    include!(concat!(env!("OUT_DIR"), "/gen_arena.rs"));
}
pub mod owned {
    include!(concat!(env!("OUT_DIR"), "/gen_owned.rs"));
}
pub mod views {
    include!(concat!(env!("OUT_DIR"), "/gen_views.rs"));
}
mod sexpr {
    include!(concat!(env!("OUT_DIR"), "/gen_sexpr.rs"));
}
```

Both `arena` and `owned` nodes convert into the read-only `views` types (`<Type>Ref<'a>`). A single function over `views` inspects either representation and formats canonical S-expressions through `dbt_asdl::Sexpr`:

```rust
use bumpalo::Bump;
use bumpalo::collections::Vec as BVec;
use dbt_asdl::{Identifier, Sexpr};
use shared::Span;
use views::{ExprKind, ExprRef};

fn count_parts(expr: ExprRef<'_>) -> usize {
    match expr.kind() {
        ExprKind::Identifier => expr.as_identifier().unwrap().parts().len(),
        ExprKind::BinaryOp => {
            let bin = expr.as_binary_op().unwrap();
            count_parts(bin.left()) + count_parts(bin.right())
        }
        _ => 0,
    }
}

fn example(span: Span) {
    let b = Bump::new();
    let arena_expr: arena::ExprRef<'_> = b.alloc(arena::Expr::Identifier(
        b.alloc(arena::expr::Identifier {
            span,
            parts: BVec::from_iter_in([Identifier::from("t"), Identifier::from("a")], &b),
        }),
    ));

    let owned_expr = owned::Expr::Identifier(Box::new(owned::expr::Identifier {
        span,
        parts: vec![Identifier::from("t"), Identifier::from("a")],
    }));

    assert_eq!(count_parts(ExprRef::from(arena_expr)), 2);
    assert_eq!(count_parts(ExprRef::from(&owned_expr)), 2);
    assert_eq!(arena_expr.to_sexpr(), "(Identifier (parts [\"t\" \"a\"]))");
}
```

### Flat Tree in Rust

`gen_convert.rs` lowers any `views::FileRef` into a `*mut dbt_asdl::FlatTree`. On the consumer side, `dbt_asdl::Tree::from_raw` wraps the pointer in an RAII handle with untyped `Node` cursors and `@stable` typed projections (`StableNodeExt`):

```rust
pub mod gen_flat {
    pub mod ast {
        pub use crate::shared::*;
        pub use crate::views::*;
    }
    include!(concat!(env!("OUT_DIR"), "/gen_flat.rs"));
    include!(concat!(env!("OUT_DIR"), "/gen_convert.rs"));
}

fn inspect_flat_in_rust(file: views::FileRef<'_>) {
    use gen_flat::{DataTypeTag, StableNodeExt};

    let raw: *mut dbt_asdl::FlatTree = gen_flat::convert_file(file);
    let tree = unsafe { dbt_asdl::Tree::from_raw(raw) }.expect("valid flat tree");

    let root: dbt_asdl::Node<'_> = tree.root();
    if let Some(dbt_asdl::Value::Seq(exprs)) = root.field("exprs") {
        for item in exprs.iter() {
            if let dbt_asdl::Value::Node(expr_node) = item
                && expr_node.kind() == Some("Cast")
                && let Some(dbt_asdl::Value::Node(to_node)) = expr_node.field("to")
                && let Some(dt) = to_node.as_data_type()
                && dt.tag() == Some(DataTypeTag::Int)
            {
                let int_rec = dt.as_int().unwrap();
                assert_eq!(int_rec.width.get(), Some(64));
            }
        }
    }
}
```

## C++ Build

### Rust-Centric Build (`build.rs` and `cc`)

When Cargo drives the build, use `build.rs` and the `cc` crate.

1. Add `dbt-asdl` and `cc` to `Cargo.toml`:

```toml
[dependencies]
dbt-asdl = "1.0"

[build-dependencies]
cc = "1"
dbt-asdl = "1.0"
```

2. Generate the flat Rust AST, the C++ runtime header, and the flat C++ AST header in `build.rs`:

```rust
use std::env;
use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("cargo sets OUT_DIR"));
    let asdl_path = manifest.join("mini_sql.asdl");
    let cpp_include = out_dir.join("cpp_include");

    println!("cargo:rerun-if-changed={}", asdl_path.display());
    println!("cargo:rerun-if-changed=src/consumer.cpp");

    dbt_asdl::codegen::rust::generate_trees(&asdl_path, out_dir.join("gen.rs")).unwrap();
    dbt_asdl::codegen::rust::generate_flat(&asdl_path, out_dir.join("gen_flat.rs")).unwrap();

    dbt_asdl::codegen::cpp::write_runtime_header(&cpp_include).unwrap();
    dbt_asdl::codegen::cpp::generate_flat(&asdl_path, cpp_include.join("gen_flat.hpp")).unwrap();

    cc::Build::new()
        .cpp(true)
        .std("c++17")
        .include(&cpp_include)
        .file("src/consumer.cpp")
        .compile("consumer");
}
```

### C++-Centric Build (CLI and CMake)

When CMake or another C++ build system drives the build, use the `dbt-asdl` CLI and the `dbt_asdl::dbt_asdl` CMake target.

1. Emit the C++ runtime header and the generated AST header with the `dbt-asdl` CLI:

```sh
dbt-asdl --emit-cpp-runtime build/generated/include
dbt-asdl --cpp-flat mini_sql.asdl build/generated/include/gen_flat.hpp
```

2. If the project uses CMake, link the `dbt_asdl::dbt_asdl` target from `add_subdirectory` or `FetchContent`:

```cmake
cmake_minimum_required(VERSION 3.15)
project(my_consumer LANGUAGES CXX)

set(DBT_ASDL_CPP_SHIM OFF CACHE BOOL "" FORCE)
add_subdirectory(path/to/dbt-asdl)

set(GEN_INCLUDE_DIR "${CMAKE_CURRENT_BINARY_DIR}/generated")
set(GEN_FLAT_HPP "${GEN_INCLUDE_DIR}/gen_flat.hpp")

add_custom_command(
    OUTPUT "${GEN_FLAT_HPP}"
    COMMAND dbt-asdl --cpp-flat
        "${CMAKE_CURRENT_SOURCE_DIR}/mini_sql.asdl"
        "${GEN_FLAT_HPP}"
    DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/mini_sql.asdl"
    VERBATIM
)

add_executable(my_consumer src/consumer.cpp "${GEN_FLAT_HPP}")
target_include_directories(my_consumer PRIVATE "${GEN_INCLUDE_DIR}")
target_link_libraries(my_consumer PRIVATE dbt_asdl::dbt_asdl)
```

## C++ API Usage

The Rust producer converts an AST into a `*mut dbt_asdl::FlatTree` with `convert_file` (or `TreeBuilder::finish`) and passes the pointer to C++:

```rust
unsafe extern "C" {
    fn inspect_tree(raw: *mut dbt_asdl::FlatTree) -> i64;
}

pub fn run_cpp_consumer(file: views::FileRef<'_>) -> i64 {
    let raw: *mut dbt_asdl::FlatTree = gen_flat::convert_file(file);
    unsafe { inspect_tree(raw) }
}
```

On the C++ side, `dbt_asdl::Tree::from_raw` validates the runtime ABI version and takes ownership of the `FlatTree*`. Its destructor calls `release` automatically. When the caller retains ownership, `dbt_asdl::TreeView::from_raw` creates a borrow view without a destructor.

```cpp
#include <dbt_asdl/flat_runtime.hpp>
#include "gen_flat.hpp"

extern "C" int64_t inspect_tree(dbt_asdl::FlatTree* raw) {
    auto tree_opt = dbt_asdl::Tree::from_raw(raw);
    if (!tree_opt) {
        return -1;
    }
    dbt_asdl::Tree tree = std::move(*tree_opt);

    if (!dbt_asdl::validate_schema(*tree.raw()->schema, mini_sql::ABI_VERSION) ||
        tree.schema_hash() != mini_sql::SCHEMA_HASH) {
        return -2;
    }

    dbt_asdl::Node root = tree.root();
    const mini_sql::File* file = mini_sql::as_file(root);
    if (!file) {
        return -3;
    }

    auto exprs_opt = tree.seq("expr", file->exprs);
    if (!exprs_opt) {
        return -4;
    }
    dbt_asdl::SeqView exprs = *exprs_opt;

    int64_t total = 0;
    for (dbt_asdl::Value item : exprs) {
        auto expr_node = item.as_node();
        if (!expr_node) {
            continue;
        }
        std::optional<mini_sql::ExprView> expr = mini_sql::as_expr(*expr_node);
        if (!expr || !expr->tag()) {
            continue;
        }

        switch (*expr->tag()) {
            case mini_sql::ExprTag::Literal: {
                const mini_sql::expr::Literal* lit_ctor = expr->as_literal();
                auto lit = mini_sql::as_literal(tree, lit_ctor->value);
                if (lit && lit->tag() == mini_sql::LiteralTag::Number) {
                    if (auto text = tree.str(lit->as_number()->text)) {
                        total += static_cast<int64_t>(text->size());
                    }
                }
                break;
            }
            case mini_sql::ExprTag::Identifier: {
                const mini_sql::expr::Identifier* id = expr->as_identifier();
                if (auto parts = tree.seq("identifier", id->parts)) {
                    total += static_cast<int64_t>(parts->len());
                }
                break;
            }
            default:
                break;
        }
    }

    std::string sexpr = tree.to_sexpr();
    (void)sexpr;

    return total;
}
```

## Development

The C++ runtime header is `include/dbt_asdl/flat_runtime.hpp`. The C++ FFI test shim is `tests/flat_cpp.cpp`. Cargo compiles the shim through `build.rs` and the `cc` crate.

To give `clangd` the exact flags for the shim, generate a compilation database:

```sh
cmake -B build -DCMAKE_EXPORT_COMPILE_COMMANDS=ON
```

`tests/.clangd` holds fallback flags for the case where the database is absent.

## License

This crate uses the Apache-2.0 license.
