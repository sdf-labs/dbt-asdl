//! Build script for the `mini-sql-dialect` test shared library.

use std::env;
use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let asdl_path = manifest.join("../fixtures/mini_sql.asdl");
    println!("cargo:rerun-if-changed={}", asdl_path.display());

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("cargo sets OUT_DIR"));
    dbt_asdl::codegen::rust::generate_trees(&asdl_path, out_dir.join("gen.rs"))
        .expect("failed to generate Rust AST trees from mini_sql.asdl");
    dbt_asdl::codegen::rust::generate_flat(&asdl_path, out_dir.join("gen_flat.rs"))
        .expect("failed to generate flat Rust AST from mini_sql.asdl");
}
