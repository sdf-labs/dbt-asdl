//! ASDL parser and code generator for SQL-like AST definitions.
//!
//! This crate parses Abstract Syntax Description Language (ASDL) files and
//! generates Abstract Syntax Tree (AST) source files for target languages.
//!
//! # Examples
//!
//! Call [`codegen::rust::generate_trees`] or [`codegen::rust::generate_flat`] from a Cargo build script (`build.rs`):
//!
//! ```no_run
//! use std::env;
//! use std::path::PathBuf;
//!
//! let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
//! let out_dir = PathBuf::from(env::var("OUT_DIR").expect("cargo sets OUT_DIR"));
//! let asdl_path = manifest.join("sql.asdl");
//!
//! dbt_asdl::codegen::rust::generate_trees(&asdl_path, out_dir.join("gen.rs"))
//!     .expect("failed to generate Rust AST from sql.asdl");
//!
//! dbt_asdl::codegen::rust::generate_flat(&asdl_path, out_dir.join("gen_flat.rs"))
//!     .expect("failed to generate flat Rust AST from sql.asdl");
//! ```

#[macro_use]
pub(crate) mod asdl;
pub(crate) mod flat;
mod flat_runtime;
pub(crate) mod rust;
mod sexpr;

pub use flat_runtime::*;
pub use sexpr::{IdentList, Sexpr, StringList, StringListIter};
/// Interned string type for ASDL `identifier` fields.
pub use ustr::Ustr as Identifier;

/// Target-language AST code generators.
pub mod codegen {
    /// C++ AST code generation and runtime header emission for flat AST consumers.
    pub mod cpp {
        pub use crate::flat::generate_cpp_flat as generate_flat;
        pub use crate::flat_runtime::write_runtime_header;
    }

    /// Rust AST code generation from ASDL specifications.
    pub mod rust {
        pub use crate::flat::generate_flat;
        pub use crate::rust::generate_trees;
    }
}

#[cfg(test)]
mod tests {
    use crate::asdl::*;
    use crate::codegen::rust::{generate_flat, generate_trees};
    use crate::flat::{fnv1a64, generate_flat_from_module};
    use crate::rust::{CodegenError, generate};

    const MINI_SQL_ASDL: &str = include_str!("../tests/fixtures/mini_sql.asdl");

    #[test]
    fn parses_and_checks_mini_asdl() {
        let mod_node = parse_str(MINI_SQL_ASDL).expect("mini.asdl parses");
        assert_eq!(mod_node.name, "MiniSql");
        assert_eq!(mod_node.version, 1);
        assert_eq!(mod_node.dfns.len(), 8);
        assert_eq!(mod_node.dfns[3].annotations, vec!["stable"]);
        assert_eq!(mod_node.dfns[5].annotations, vec!["stable"]);
        let builtins = [
            "identifier",
            "string",
            "int",
            "constant",
            "byte",
            "long",
            "bool",
        ];
        assert_eq!(check_with_builtins(&mod_node, &builtins), Ok(()));
    }

    #[test]
    fn parses_annotations_and_rejects_unknown_or_duplicate() {
        let valid = "module M { @stable foo = Bar | Baz }";
        let m = parse_str(valid).unwrap();
        assert_eq!(m.dfns[0].annotations, vec!["stable"]);

        let unknown = "module M { @bogus foo = Bar }";
        let err = parse_str(unknown).unwrap_err();
        assert_eq!(
            err.to_string(),
            "Syntax error on line 1: Unknown annotation @bogus (known: @stable)"
        );

        let dup = "module M { @stable @stable foo = Bar }";
        let err = parse_str(dup).unwrap_err();
        assert_eq!(err.to_string(), "Syntax error on line 1: Duplicate annotation @stable");
        assert_eq!(m.version, 1);

        let v2 = parse_str("module M version 2 { foo = Bar }").unwrap();
        assert_eq!(v2.version, 2);

        let zero_ver = parse_str("module M version 0 { foo = Bar }").unwrap_err();
        assert_eq!(zero_ver.to_string(), "Syntax error on line 1: Module version must be >= 1");

        let bad_ver = parse_str("module M version two { foo = Bar }").unwrap_err();
        assert_eq!(bad_ver.to_string(), "Syntax error on line 1: Unmatched Int (found TypeId)");
    }

    #[test]
    fn check_detects_redefined_constructor_and_undefined_type() {
        let src = "module M {
  a = Foo(unknown_t x)
  b = Foo
}";
        let m = parse_str(src).unwrap();
        let errs = check_with_builtins(&m, BUILTIN_TYPES).unwrap_err();
        assert_eq!(
            errs,
            vec![
                "Redefinition of constructor Foo",
                "Defined in a and b",
                "Undefined type unknown_t, used in Foo",
            ]
        );
    }

    #[test]
    fn generates_mini_sql_trees() {
        let tmp_dir = std::env::temp_dir().join(format!("dbt_asdl_test_{}", std::process::id()));
        std::fs::create_dir_all(&tmp_dir).unwrap();
        let spec_path = tmp_dir.join("mini_sql.asdl");
        std::fs::write(&spec_path, MINI_SQL_ASDL).unwrap();

        let paths = generate_trees(&spec_path, tmp_dir.join("gen.rs")).unwrap();
        assert_eq!(paths.len(), 5);

        let shared = std::fs::read_to_string(tmp_dir.join("gen.rs")).unwrap();
        let arena = std::fs::read_to_string(tmp_dir.join("gen_arena.rs")).unwrap();
        let owned = std::fs::read_to_string(tmp_dir.join("gen_owned.rs")).unwrap();
        let views = std::fs::read_to_string(tmp_dir.join("gen_views.rs")).unwrap();
        let sexpr = std::fs::read_to_string(tmp_dir.join("gen_sexpr.rs")).unwrap();

        let _ = std::fs::remove_dir_all(&tmp_dir);

        insta::assert_snapshot!("mini_sql_shared", shared);
        insta::assert_snapshot!("mini_sql_arena", arena);
        insta::assert_snapshot!("mini_sql_owned", owned);
        insta::assert_snapshot!("mini_sql_views", views);
        insta::assert_snapshot!("mini_sql_sexpr", sexpr);
    }

    #[test]
    fn generates_product_edge_cases() {
        let spec = r#"
module Edge {
  span = (int start, int end)
  shared_with_attr = (int x) attributes (span span, bool flag)
  plain_str = (string text, int count) attributes (span span)
  plain_seq = (int* nums, plain_str? opt_str, plain_str req_str)
}
"#;
        let mod_node = parse_str(spec).unwrap();
        let generated = generate(&mod_node, "edge.asdl").unwrap();

        insta::assert_snapshot!("edge_shared", generated.shared);
        insta::assert_snapshot!("edge_arena", generated.arena);
        insta::assert_snapshot!("edge_owned", generated.owned);
        insta::assert_snapshot!("edge_views", generated.views);
        insta::assert_snapshot!("edge_sexpr", generated.sexpr);
    }

    #[test]
    fn validates_restrictions_and_collisions() {
        let unnamed = parse_str("module M { p = (int) }").unwrap();
        assert_eq!(
            generate(&unnamed, "m.asdl").unwrap_err(),
            CodegenError("p: fields must be named (got positional 'int')".to_string())
        );

        let multi_q = parse_str("module M { p = (int?* x) }").unwrap();
        assert_eq!(
            generate(&multi_q, "m.asdl").unwrap_err(),
            CodegenError("p.x: at most one quantifier (? or *) is supported".to_string())
        );

        let attr_simple = parse_str("module M { s = A | B attributes (int x) }").unwrap();
        assert_eq!(
            generate(&attr_simple, "m.asdl").unwrap_err(),
            CodegenError("s: attributes on a field-less sum make no sense".to_string())
        );

        let constant_ty = parse_str("module M { p = (constant c) }").unwrap();
        assert_eq!(
            generate(&constant_ty, "m.asdl").unwrap_err(),
            CodegenError("unsupported field type 'constant' (field 'c')".to_string())
        );

        let attr_kind = parse_str("module M { s = A(int x) attributes (int kind) }").unwrap();
        assert_eq!(
            generate(&attr_kind, "m.asdl").unwrap_err(),
            CodegenError("s: attribute 'kind' collides with a generated method".to_string())
        );

        let node_field = parse_str("module M { s = A(int node) }").unwrap();
        assert_eq!(
            generate(&node_field, "m.asdl").unwrap_err(),
            CodegenError("s.A.node: field name reserved for view structs".to_string())
        );

        let collision = parse_str("module M { ident_list = A }").unwrap();
        assert_eq!(
            generate(&collision, "m.asdl").unwrap_err(),
            CodegenError(
                "generated read-API name 'IdentList' collides with an existing name".to_string()
            )
        );
    }

    #[test]
    fn generates_mini_sql_flat() {
        let tmp_dir =
            std::env::temp_dir().join(format!("dbt_asdl_flat_test_{}", std::process::id()));
        std::fs::create_dir_all(&tmp_dir).unwrap();
        let spec_path = tmp_dir.join("mini_sql.asdl");
        std::fs::write(&spec_path, MINI_SQL_ASDL).unwrap();

        let paths = generate_flat(&spec_path, tmp_dir.join("gen_flat.rs")).unwrap();
        assert_eq!(paths.len(), 2);

        let cpp_paths =
            crate::codegen::cpp::generate_flat(&spec_path, tmp_dir.join("gen_flat.hpp")).unwrap();
        assert_eq!(cpp_paths, vec![tmp_dir.join("gen_flat.hpp")]);

        let flat = std::fs::read_to_string(tmp_dir.join("gen_flat.rs")).unwrap();
        let convert = std::fs::read_to_string(tmp_dir.join("gen_convert.rs")).unwrap();
        let cpp_flat = std::fs::read_to_string(tmp_dir.join("gen_flat.hpp")).unwrap();

        let _ = std::fs::remove_dir_all(&tmp_dir);

        assert!(!flat.contains("Sql"), "the flat target must not mention Sql");
        insta::assert_snapshot!("mini_sql_flat", flat);
        insta::assert_snapshot!("mini_sql_convert", convert);
        insta::assert_snapshot!("mini_sql_cpp_flat", cpp_flat);
    }

    #[test]
    fn validates_flat_restrictions() {
        let scalar_and_simple_sum_seq = parse_str(
            "module M { op = Add | Sub @stable p = (byte* bs, int* is, long* ls, bool* flags, op* ops) file = (p* ps) }",
        )
        .unwrap();
        let flat = generate_flat_from_module(&scalar_and_simple_sum_seq, "m.asdl", fnv1a64(b""))
            .expect("scalar and simple-sum sequences are supported");
        assert!(
            flat.flat
                .contains("pub fn push_byte_seq(&mut self, items: &[u8]) -> FlatSeq")
        );
        assert!(
            flat.flat
                .contains("pub fn push_int_seq(&mut self, items: &[i32]) -> FlatSeq")
        );
        assert!(
            flat.flat
                .contains("pub fn push_long_seq(&mut self, items: &[i64]) -> FlatSeq")
        );
        assert!(
            flat.flat
                .contains("pub fn push_bool_seq(&mut self, items: &[u8]) -> FlatSeq")
        );
        assert!(
            flat.flat
                .contains("pub fn push_op_seq(&mut self, items: &[u8]) -> FlatSeq")
        );
        assert!(flat.flat.contains("ops: FlatSeq<op(u8)>"));
        assert!(flat.convert.contains("b.push_byte_seq(r.bs())"));
        assert!(flat.convert.contains("b.push_int_seq(r.is())"));
        assert!(flat.convert.contains("b.push_long_seq(r.ls())"));
        assert!(flat.convert.contains("b.push_bool_seq(&items)"));
        assert!(flat.convert.contains("b.push_op_seq(&items)"));

        let without_file = parse_str("module M { p = (int x) }").unwrap();
        let flat_no_file = generate_flat_from_module(&without_file, "m.asdl", fnv1a64(b""))
            .expect("flat layout does not require a 'file' root type");
        assert!(
            flat_no_file
                .flat
                .contains("pub fn push_p(&mut self, r: P) -> PRef")
        );
        assert!(
            flat_no_file
                .convert
                .contains("pub fn lower_p(b: &mut TreeBuilder, r: ast::P) -> PRef")
        );
    }
}
