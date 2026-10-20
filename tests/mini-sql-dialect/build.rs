//! Build script for the `mini-sql-dialect` test shared library.

use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let fixtures_dir = manifest.join("../fixtures");
    let asdl_path = fixtures_dir.join("mini_sql.asdl");
    let legacy_asdl = fixtures_dir.join("mini_sql_legacy.asdl");
    let v2_asdl = fixtures_dir.join("mini_sql_v2.asdl");

    for path in [&asdl_path, &legacy_asdl, &v2_asdl] {
        println!("cargo:rerun-if-changed={}", path.display());
    }

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("cargo sets OUT_DIR"));
    dbt_asdl::codegen::rust::generate_trees(&asdl_path, out_dir.join("gen.rs"))
        .expect("failed to generate Rust AST trees from mini_sql.asdl");
    dbt_asdl::codegen::rust::generate_flat(&asdl_path, out_dir.join("gen_flat.rs"))
        .expect("failed to generate flat Rust AST from mini_sql.asdl");

    let legacy_dir = out_dir.join("legacy");
    fs::create_dir_all(&legacy_dir).expect("create legacy out_dir");
    dbt_asdl::codegen::rust::generate_flat(&legacy_asdl, legacy_dir.join("gen_flat.rs"))
        .expect("failed to generate legacy flat Rust AST");

    let v2_dir = out_dir.join("v2");
    fs::create_dir_all(&v2_dir).expect("create v2 out_dir");
    dbt_asdl::codegen::rust::generate_flat(&v2_asdl, v2_dir.join("gen_flat.rs"))
        .expect("failed to generate v2 flat Rust AST");
}
