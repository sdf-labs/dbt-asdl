# dbt-asdl

`dbt-asdl` parses Abstract Syntax Description Language (ASDL) files and generates Abstract Syntax Tree (AST) source code.

## Overview

The parser reads an ASDL specification and validates all type and constructor definitions. This dialect extends the Zephyr ASDL grammar with fixed-width integer types (`byte`, `int`, `long`), the `bool` type, and `@stable` type annotations.

The Rust backend (`dbt_asdl::rust::generate_trees`) writes five source files from one `.asdl` input file:

- `<stem>.rs`: shared types (fieldless enums and plain copyable structs).
- `<stem>_arena.rs`: the arena-allocated tree, which uses a `bumpalo` arena lifetime.
- `<stem>_owned.rs`: the self-owned tree, which uses `Box` links and standard `Vec` containers.
- `<stem>_views.rs`: the read-only view API over both tree representations.
- `<stem>_sexpr.rs`: the canonical S-expression writer implementations.

## Usage in `build.rs`

1. Add `dbt-asdl` to `[build-dependencies]` in `Cargo.toml`:

```toml
[build-dependencies]
dbt-asdl = "0.1"
```

2. Call `dbt_asdl::rust::generate_trees` from `build.rs`:

```rust
use std::env;
use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("cargo sets OUT_DIR"));
    let asdl_path = manifest.join("sql.asdl");

    println!("cargo:rerun-if-changed={}", asdl_path.display());

    dbt_asdl::rust::generate_trees(&asdl_path, out_dir.join("gen.rs"))
        .expect("failed to generate Rust AST from sql.asdl");
}
```

## License

This crate uses the Apache-2.0 license.
