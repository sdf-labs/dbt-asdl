//! Build script that generates the miniature SQL AST for integration tests.

use std::env;
use std::path::PathBuf;

#[allow(dead_code)]
#[macro_use]
#[path = "src/asdl.rs"]
mod asdl;
#[allow(dead_code)]
#[path = "src/flat.rs"]
mod flat;
#[allow(dead_code)]
#[path = "src/rust.rs"]
mod rust;

fn main() {
    println!("cargo:rustc-check-cfg=cfg(feature, values(\"builder\"))");
    println!("cargo:rustc-cfg=feature=\"builder\"");

    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let asdl_path = manifest.join("tests/fixtures/mini_sql.asdl");
    println!("cargo:rerun-if-changed={}", asdl_path.display());
    if !asdl_path.exists() {
        return;
    }

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("cargo sets OUT_DIR"));
    rust::generate_trees(&asdl_path, out_dir.join("gen.rs"))
        .expect("failed to generate Rust AST from mini_sql.asdl");
    flat::generate_flat(&asdl_path, out_dir.join("gen_flat.rs"))
        .expect("failed to generate flat Rust AST from mini_sql.asdl");

    let hpp_path = manifest.join("include/dbt_asdl/flat_runtime.hpp");
    let cpp_path = manifest.join("tests/flat_cpp.cpp");
    println!("cargo:rerun-if-changed={}", hpp_path.display());
    println!("cargo:rerun-if-changed={}", cpp_path.display());
    if cpp_path.exists() {
        let include_dir = out_dir.join("cpp_include");
        let staged_hpp = include_dir.join("dbt_asdl/flat_runtime.hpp");
        std::fs::create_dir_all(staged_hpp.parent().unwrap())
            .expect("failed to create staged C++ include directory");
        std::fs::copy(&hpp_path, &staged_hpp).expect("failed to stage flat_runtime.hpp");
        flat::generate_cpp_flat(&asdl_path, include_dir.join("gen_flat.hpp"))
            .expect("failed to generate flat C++ AST from mini_sql.asdl");

        cc::Build::new()
            .cpp(true)
            .std("c++17")
            .include(&include_dir)
            .file(&cpp_path)
            .compile("flat_cpp_shim");
    }
}
