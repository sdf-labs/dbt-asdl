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

The flat Rust backend (`dbt_asdl::codegen::rust::generate_flat`) writes two source files for a stable `#[repr(C)]` ABI across shared library boundaries:

- `<dst>`: the flat ABI boundary runtime (`FlatTree`, `Tree`, `Node`, `Value`, `export_sql_dialect!`, and `loader`), `#[repr(C)]` record structs, typed node reference newtypes, constructor tag enums, `TreeBuilder`, `@stable` typed projections, layout freeze dump, and schema descriptor tables.
- `gen_convert.rs`: functions that convert an arena or owned AST (through the read-only view API) into the flat boundary representation.

## Usage in `build.rs`

1. Add `dbt-asdl` to `[dependencies]` and `[build-dependencies]` in `Cargo.toml`:

```toml
[dependencies]
dbt-asdl = "0.2"

[build-dependencies]
dbt-asdl = "0.2"
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

## License

This crate uses the Apache-2.0 license.
