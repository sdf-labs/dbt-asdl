//! Command-line interface for the `dbt-asdl` code generator.

use std::env;
use std::process;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() == 3 && args[1] == "--emit-cpp-runtime" {
        if let Err(err) = dbt_asdl::codegen::cpp::write_runtime_header(&args[2]) {
            eprintln!("{err}");
            process::exit(1);
        }
        return;
    }
    if args.len() != 3 || args[1].starts_with('-') {
        eprintln!("usage: dbt-asdl INPUT.asdl OUTPUT.rs");
        eprintln!("       dbt-asdl --emit-cpp-runtime INCLUDE_DIR");
        process::exit(2);
    }
    if let Err(err) = dbt_asdl::codegen::rust::generate_trees(&args[1], &args[2]) {
        eprintln!("{err}");
        process::exit(1);
    }
}
