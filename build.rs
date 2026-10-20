//! Build script that generates the miniature SQL AST for integration tests.

use std::env;
use std::path::PathBuf;

#[allow(dead_code)]
#[macro_use]
#[path = "src/asdl.rs"]
mod asdl;
#[allow(dead_code)]
#[path = "src/rust.rs"]
mod rust;

fn main() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let asdl_path = manifest.join("tests/fixtures/mini_sql.asdl");
    println!("cargo:rerun-if-changed={}", asdl_path.display());
    if !asdl_path.exists() {
        return;
    }

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("cargo sets OUT_DIR"));
    rust::generate_trees(&asdl_path, out_dir.join("gen.rs"))
        .expect("failed to generate Rust AST from mini_sql.asdl");
}
