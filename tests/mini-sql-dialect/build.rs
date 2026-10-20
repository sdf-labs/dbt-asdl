//! Build script for the `mini-sql-dialect` test shared library.

use std::env;
use std::fs;
use std::path::PathBuf;

const LEGACY_MINI_SQL_ASDL: &str = r#"-- Older compatible version of mini_sql.asdl (same ABI version 1, untouched @stable region)
module MiniSql version 1 {
  code_location = (int? lineno, int? col_offset, long? index)
  span = (code_location start, code_location end)

  bin_op = Add | Sub | Eq

  @stable
  data_type = Int(bool? unsigned, byte? width)
            | Bool
            attributes (span span, bool nullable)

  literal = Boolean(bool val)
          | Number(string text)
          | String(string value)
          attributes (span span)

  @stable
  when_then = (expr when, expr then, bin_op? op)

  expr = Paren(expr inner)
       | Identifier(identifier* parts)
       | Literal(literal value)
       | BinaryOp(expr left, bin_op op, expr right)
       | Case(expr? operand, when_then* cases, expr? else)
       | Cast(expr e, data_type to, string? legacy_hint)
       attributes (span span)

  file = (expr* exprs, span* spans)
}
"#;

const V2_MINI_SQL_ASDL: &str = r#"-- Incompatible version 2 of mini_sql.asdl
module MiniSql version 2 {
  code_location = (int? lineno, int? col_offset, long? index)
  span = (code_location start, code_location end)

  bin_op = Add | Sub | Eq

  @stable
  data_type = Int(bool? unsigned, byte? width, bool? zerofill)
            | Bool
            attributes (span span, bool nullable)

  literal = Number(string text)
          | String(string value)
          | Null
          attributes (span span)

  @stable
  when_then = (expr when, expr then, bin_op? op)

  expr = Identifier(identifier* parts)
       | Literal(literal value)
       | BinaryOp(expr left, bin_op op, expr right)
       | Case(expr? operand, when_then* cases, expr? else)
       | Cast(expr e, data_type to, string* notes)
       attributes (span span)

  file = (expr* exprs, span* spans)
}
"#;

fn main() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let asdl_path = manifest.join("../fixtures/mini_sql.asdl");
    println!("cargo:rerun-if-changed={}", asdl_path.display());

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("cargo sets OUT_DIR"));
    dbt_asdl::codegen::rust::generate_trees(&asdl_path, out_dir.join("gen.rs"))
        .expect("failed to generate Rust AST trees from mini_sql.asdl");
    dbt_asdl::codegen::rust::generate_flat(&asdl_path, out_dir.join("gen_flat.rs"))
        .expect("failed to generate flat Rust AST from mini_sql.asdl");

    let legacy_dir = out_dir.join("legacy");
    fs::create_dir_all(&legacy_dir).expect("create legacy out_dir");
    let legacy_asdl = legacy_dir.join("mini_sql_legacy.asdl");
    fs::write(&legacy_asdl, LEGACY_MINI_SQL_ASDL).expect("write legacy asdl");
    dbt_asdl::codegen::rust::generate_flat(&legacy_asdl, legacy_dir.join("gen_flat.rs"))
        .expect("failed to generate legacy flat Rust AST");

    let v2_dir = out_dir.join("v2");
    fs::create_dir_all(&v2_dir).expect("create v2 out_dir");
    let v2_asdl = v2_dir.join("mini_sql_v2.asdl");
    fs::write(&v2_asdl, V2_MINI_SQL_ASDL).expect("write v2 asdl");
    dbt_asdl::codegen::rust::generate_flat(&v2_asdl, v2_dir.join("gen_flat.rs"))
        .expect("failed to generate v2 flat Rust AST");
}
