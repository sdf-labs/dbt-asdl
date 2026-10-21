//! End-to-end integration tests for the AST and `Sexpr` code generated from
//! `tests/fixtures/mini_sql.asdl`.

#![allow(missing_docs, dead_code, unexpected_cfgs)]

use bumpalo::collections::Vec as BVec;
use bumpalo::{Bump, vec as bvec};
use dbt_asdl::{Identifier, Sexpr};

mod shared {
    include!(concat!(env!("OUT_DIR"), "/gen.rs"));
}

pub mod arena {
    include!(concat!(env!("OUT_DIR"), "/gen_arena.rs"));
}

pub mod owned {
    include!(concat!(env!("OUT_DIR"), "/gen_owned.rs"));
}

mod gen_sexpr {
    include!(concat!(env!("OUT_DIR"), "/gen_sexpr.rs"));
}

mod gen_views {
    include!(concat!(env!("OUT_DIR"), "/gen_views.rs"));
}

pub use gen_views::*;
pub use shared::*;

fn loc(lineno: Option<i32>, col_offset: Option<i32>, index: Option<i64>) -> CodeLocation {
    CodeLocation {
        lineno,
        col_offset,
        index,
    }
}

fn span_at(start: i64, end: i64) -> Span {
    Span {
        start: loc(Some(1), Some(start as i32), Some(start)),
        end: loc(Some(1), Some(end as i32), Some(end)),
    }
}

fn unknown_span() -> Span {
    Span {
        start: loc(None, None, None),
        end: loc(None, None, None),
    }
}

fn al<T>(b: &Bump, x: T) -> &T {
    b.alloc(x)
}

fn a_ident<'a>(b: &'a Bump, span: Span, parts: &[&str]) -> arena::ExprRef<'a> {
    al(
        b,
        arena::Expr::Identifier(b.alloc(arena::expr::Identifier {
            span,
            parts: BVec::from_iter_in(parts.iter().map(|s| Identifier::from(s)), b),
        })),
    )
}

fn a_number<'a>(b: &'a Bump, span: Span, text: &str) -> arena::ExprRef<'a> {
    al(
        b,
        arena::Expr::Literal(b.alloc(arena::expr::Literal {
            span,
            value: al(
                b,
                arena::Literal::Number(b.alloc(arena::literal::Number {
                    span,
                    text: b.alloc_str(text),
                })),
            ),
        })),
    )
}

fn a_string<'a>(b: &'a Bump, span: Span, value: &str) -> arena::ExprRef<'a> {
    al(
        b,
        arena::Expr::Literal(b.alloc(arena::expr::Literal {
            span,
            value: al(
                b,
                arena::Literal::String(b.alloc(arena::literal::String {
                    span,
                    value: b.alloc_str(value),
                })),
            ),
        })),
    )
}

fn a_null<'a>(b: &'a Bump, span: Span) -> arena::ExprRef<'a> {
    al(
        b,
        arena::Expr::Literal(b.alloc(arena::expr::Literal {
            span,
            value: al(b, arena::Literal::Null(b.alloc(arena::literal::Null { span }))),
        })),
    )
}

fn o_ident(span: Span, parts: &[&str]) -> owned::Expr {
    owned::Expr::Identifier(Box::new(owned::expr::Identifier {
        span,
        parts: parts.iter().map(|s| Identifier::from(s)).collect(),
    }))
}

fn o_number(span: Span, text: &str) -> owned::Expr {
    owned::Expr::Literal(Box::new(owned::expr::Literal {
        span,
        value: Box::new(owned::Literal::Number(Box::new(owned::literal::Number {
            span,
            text: text.into(),
        }))),
    }))
}

fn o_string(span: Span, value: &str) -> owned::Expr {
    owned::Expr::Literal(Box::new(owned::expr::Literal {
        span,
        value: Box::new(owned::Literal::String(Box::new(owned::literal::String {
            span,
            value: value.into(),
        }))),
    }))
}

fn o_null(span: Span) -> owned::Expr {
    owned::Expr::Literal(Box::new(owned::expr::Literal {
        span,
        value: Box::new(owned::Literal::Null(Box::new(owned::literal::Null { span }))),
    }))
}

fn build_arena_expr(b: &Bump, span: Span) -> arena::ExprRef<'_> {
    let cond1 = al(
        b,
        arena::Expr::BinaryOp(b.alloc(arena::expr::BinaryOp {
            span,
            left: a_ident(b, span, &["t", "a"]),
            op: BinOp::Eq,
            right: a_number(b, span, "1"),
        })),
    );
    let sub_expr = al(
        b,
        arena::Expr::BinaryOp(b.alloc(arena::expr::BinaryOp {
            span,
            left: a_ident(b, span, &["b"]),
            op: BinOp::Sub,
            right: a_number(b, span, "2"),
        })),
    );
    let int_ty = al(
        b,
        arena::DataType::Int(b.alloc(arena::data_type::Int {
            span,
            nullable: false,
            unsigned: Some(true),
            width: Some(64),
        })),
    );
    let then1 = al(
        b,
        arena::Expr::Cast(b.alloc(arena::expr::Cast {
            span,
            e: sub_expr,
            to: int_ty,
            notes: bvec![in b; &*b.alloc_str("fast"), &*b.alloc_str("checked")],
        })),
    );

    let cond2 = a_ident(b, span, &["c"]);
    let then2 = a_string(b, span, "ok\n");

    let bool_ty = al(
        b,
        arena::DataType::Bool(b.alloc(arena::data_type::Bool {
            span,
            nullable: true,
        })),
    );
    let else_expr = al(
        b,
        arena::Expr::Cast(b.alloc(arena::expr::Cast {
            span,
            e: a_null(b, span),
            to: bool_ty,
            notes: BVec::new_in(b),
        })),
    );

    al(
        b,
        arena::Expr::Case(b.alloc(arena::expr::Case {
            span,
            operand: Some(a_ident(b, span, &["x"])),
            cases: bvec![
                in b;
                al(b, arena::WhenThen { when: cond1, then: then1, op: Some(BinOp::Add) }),
                al(b, arena::WhenThen { when: cond2, then: then2, op: None }),
            ],
            r#else: Some(else_expr),
        })),
    )
}

fn build_owned_expr(span: Span) -> owned::Expr {
    let cond1 = owned::Expr::BinaryOp(Box::new(owned::expr::BinaryOp {
        span,
        left: Box::new(o_ident(span, &["t", "a"])),
        op: BinOp::Eq,
        right: Box::new(o_number(span, "1")),
    }));
    let sub_expr = owned::Expr::BinaryOp(Box::new(owned::expr::BinaryOp {
        span,
        left: Box::new(o_ident(span, &["b"])),
        op: BinOp::Sub,
        right: Box::new(o_number(span, "2")),
    }));
    let int_ty = owned::DataType::Int(Box::new(owned::data_type::Int {
        span,
        nullable: false,
        unsigned: Some(true),
        width: Some(64),
    }));
    let then1 = owned::Expr::Cast(Box::new(owned::expr::Cast {
        span,
        e: Box::new(sub_expr),
        to: Box::new(int_ty),
        notes: vec!["fast".into(), "checked".into()],
    }));

    let cond2 = o_ident(span, &["c"]);
    let then2 = o_string(span, "ok\n");

    let bool_ty = owned::DataType::Bool(Box::new(owned::data_type::Bool {
        span,
        nullable: true,
    }));
    let else_expr = owned::Expr::Cast(Box::new(owned::expr::Cast {
        span,
        e: Box::new(o_null(span)),
        to: Box::new(bool_ty),
        notes: Vec::new(),
    }));

    owned::Expr::Case(Box::new(owned::expr::Case {
        span,
        operand: Some(Box::new(o_ident(span, &["x"]))),
        cases: vec![
            owned::WhenThen {
                when: Box::new(cond1),
                then: Box::new(then1),
                op: Some(BinOp::Add),
            },
            owned::WhenThen {
                when: Box::new(cond2),
                then: Box::new(then2),
                op: None,
            },
        ],
        r#else: Some(Box::new(else_expr)),
    }))
}

#[test]
fn shared_types_and_sexpr() {
    let empty_loc = loc(None, None, None);
    assert_eq!(empty_loc.lineno(), None);
    assert_eq!(empty_loc.col_offset(), None);
    assert_eq!(empty_loc.index(), None);
    assert_eq!(empty_loc.to_sexpr(), "(CodeLocation)");

    let full_loc = loc(Some(3), Some(12), Some(48));
    assert_eq!(full_loc.lineno(), Some(3));
    assert_eq!(full_loc.col_offset(), Some(12));
    assert_eq!(full_loc.index(), Some(48));
    assert_eq!(full_loc.to_sexpr(), "(CodeLocation (lineno 3) (col_offset 12) (index 48))");

    let sp = Span {
        start: empty_loc,
        end: full_loc,
    };
    assert_eq!(sp.start(), empty_loc);
    assert_eq!(sp.end(), full_loc);
    assert_eq!(
        sp.to_sexpr(),
        "(Span (start (CodeLocation)) (end (CodeLocation (lineno 3) (col_offset 12) (index 48))))"
    );

    assert_eq!(BinOp::Add.to_sexpr(), "Add");
    assert_eq!(BinOp::Sub.to_sexpr(), "Sub");
    assert_eq!(BinOp::Eq.to_sexpr(), "Eq");
}

#[test]
fn data_type_and_literal_variants_and_sexpr() {
    let b = &Bump::new();
    let sp1 = span_at(0, 5);
    let sp2 = span_at(10, 20);

    let dt_int_default: arena::DataTypeMut<'_> =
        b.alloc(arena::DataType::Int(b.alloc(arena::data_type::Int {
            span: sp1,
            nullable: false,
            unsigned: None,
            width: None,
        })));
    dt_int_default.set_span(sp2);
    let dt_int_ref = DataTypeRef::from(&*dt_int_default);
    assert_eq!(dt_int_ref.span(), sp2);
    assert_eq!((&*dt_int_default).to_sexpr(), "Int");
    assert_eq!(dt_int_ref.to_sexpr(), "Int");

    dt_int_default.set_nullable(true);
    assert_eq!((&*dt_int_default).to_sexpr(), "(Int (nullable true))");
    assert_eq!(
        dt_int_default.to_owned(),
        owned::DataType::Int(Box::new(owned::data_type::Int {
            span: sp1,
            nullable: true,
            unsigned: None,
            width: None,
        }))
    );

    let mut dt_bool = owned::DataType::Bool(Box::new(owned::data_type::Bool {
        span: sp1,
        nullable: false,
    }));
    assert_eq!(dt_bool.to_sexpr(), "Bool");
    dt_bool.set_span(sp2);
    dt_bool.set_nullable(true);
    let dt_bool_ref = DataTypeRef::from(&dt_bool);
    assert_eq!(dt_bool_ref.kind(), DataTypeKind::Bool);
    assert_eq!(dt_bool_ref.span(), sp2);
    assert!(dt_bool_ref.nullable());
    assert!(dt_bool_ref.as_int().is_none());
    assert!(dt_bool_ref.as_bool().unwrap().nullable());
    assert_eq!(dt_bool.to_sexpr(), "(Bool (nullable true))");
    assert_eq!(dt_bool_ref.to_sexpr(), "(Bool (nullable true))");

    let lit_num: arena::LiteralMut<'_> =
        b.alloc(arena::Literal::Number(b.alloc(arena::literal::Number {
            span: sp1,
            text: "3.14",
        })));
    lit_num.set_span(sp2);
    let lit_num_owned = lit_num.to_owned();
    let lit_num_ref = LiteralRef::from(&lit_num_owned);
    assert_eq!((&*lit_num).to_sexpr(), "(Number (text \"3.14\"))");
    assert_eq!(lit_num_owned.to_sexpr(), "(Number (text \"3.14\"))");
    assert_eq!(lit_num_ref.to_sexpr(), "(Number (text \"3.14\"))");
    assert_eq!(lit_num_ref.kind(), LiteralKind::Number);
    assert_eq!(lit_num_ref.as_number().unwrap().text(), "3.14");
    assert!(lit_num_ref.as_string().is_none());
    assert!(lit_num_ref.as_null().is_none());

    let mut lit_str = owned::Literal::String(Box::new(owned::literal::String {
        span: sp1,
        value: "a\"b\\c\td\r\n\x07".into(),
    }));
    lit_str.set_span(sp2);
    assert_eq!(lit_str.to_sexpr(), "(String (value \"a\\\"b\\\\c\\td\\r\\n\\u{7}\"))");
    assert_eq!(LiteralRef::from(&lit_str).kind(), LiteralKind::String);
    assert_eq!(LiteralRef::from(&lit_str).as_string().unwrap().value(), "a\"b\\c\td\r\n\x07");

    let lit_null = arena::Literal::Null(b.alloc(arena::literal::Null { span: sp1 }));
    assert_eq!((&lit_null).to_sexpr(), "Null");
    assert_eq!(
        lit_null.to_owned(),
        owned::Literal::Null(Box::new(owned::literal::Null { span: sp2 }))
    );
    let lit_null_ref = LiteralRef::from(&lit_null);
    assert_eq!(lit_null_ref.kind(), LiteralKind::Null);
    assert_eq!(lit_null_ref.as_null().unwrap().span(), sp1);
}

#[test]
fn arena_and_owned_trees_equality_conversion_and_sexpr() {
    let b = &Bump::new();
    let arena_expr = build_arena_expr(b, span_at(1, 50));
    let arena_shifted = build_arena_expr(b, span_at(100, 200));
    let owned_expr = build_owned_expr(unknown_span());

    assert_eq!(arena_expr, arena_shifted);
    assert_eq!(arena_expr.to_owned(), owned_expr);

    let mut owned_diff = build_owned_expr(unknown_span());
    if let owned::Expr::Case(case_payload) = &mut owned_diff {
        case_payload.r#else = None;
    }
    assert_ne!(owned_expr, owned_diff);

    owned_diff.set_span(span_at(5, 9));
    assert_eq!(ExprRef::from(&owned_diff).span(), span_at(5, 9));

    let wt_arena = al(
        b,
        arena::WhenThen {
            when: a_ident(b, span_at(1, 2), &["k"]),
            then: a_number(b, span_at(3, 4), "9"),
            op: Some(BinOp::Add),
        },
    );
    let wt_owned = owned::WhenThen {
        when: Box::new(o_ident(unknown_span(), &["k"])),
        then: Box::new(o_number(unknown_span(), "9")),
        op: Some(BinOp::Add),
    };
    assert_eq!(wt_arena.to_owned(), wt_owned);
    let wt_sexpr = "(WhenThen (when (Identifier (parts [\"k\"]))) (then (Literal (value (Number (text \"9\"))))) (op Add))";
    assert_eq!(wt_arena.to_sexpr(), wt_sexpr);
    assert_eq!(wt_owned.to_sexpr(), wt_sexpr);
    assert_eq!(WhenThenRef::from(wt_arena).to_sexpr(), wt_sexpr);

    let expected_sexpr = concat!(
        "(Case",
        " (operand (Identifier (parts [\"x\"])))",
        " (cases [",
        "(WhenThen",
        " (when (BinaryOp (left (Identifier (parts [\"t\" \"a\"]))) (op Eq) (right (Literal (value (Number (text \"1\")))))))",
        " (then (Cast (e (BinaryOp (left (Identifier (parts [\"b\"]))) (op Sub) (right (Literal (value (Number (text \"2\"))))))) (to (Int (unsigned true) (width 64))) (notes [\"fast\" \"checked\"])))",
        " (op Add)",
        ") ",
        "(WhenThen",
        " (when (Identifier (parts [\"c\"])))",
        " (then (Literal (value (String (value \"ok\\n\")))))",
        ")",
        "])",
        " (else (Cast (e (Literal (value Null))) (to (Bool (nullable true)))))",
        ")"
    );

    assert_eq!(arena_expr.to_sexpr(), expected_sexpr);
    assert_eq!(owned_expr.to_sexpr(), expected_sexpr);
    assert_eq!(ExprRef::from(&owned_expr).to_sexpr(), expected_sexpr);

    let bare_ident = a_ident(b, unknown_span(), &[]);
    assert_eq!(bare_ident.to_sexpr(), "Identifier");

    let bare_case = al(
        b,
        arena::Expr::Case(b.alloc(arena::expr::Case {
            span: unknown_span(),
            operand: None,
            cases: BVec::new_in(b),
            r#else: None,
        })),
    );
    assert_eq!(bare_case.to_sexpr(), "Case");
}

fn inspect_case_tree(root: ExprRef<'_>) {
    assert_eq!(root.kind(), ExprKind::Case);
    assert!(root.as_identifier().is_none());
    assert!(root.as_literal().is_none());
    assert!(root.as_binary_op().is_none());
    assert!(root.as_cast().is_none());

    let case = root.as_case().expect("root is a Case");
    let operand = case.operand().expect("has operand");
    assert_eq!(operand.kind(), ExprKind::Identifier);
    let op_parts: Vec<&str> = operand
        .as_identifier()
        .unwrap()
        .parts()
        .iter()
        .map(Identifier::as_str)
        .collect();
    assert_eq!(op_parts, ["x"]);

    let cases: WhenThenList<'_> = case.cases();
    assert!(!cases.is_empty());
    assert_eq!(cases.len(), 2);
    assert_eq!(cases.iter().len(), 2);
    assert!(cases.get(2).is_none());
    assert_eq!(
        cases.to_sexpr(),
        concat!(
            "[",
            "(WhenThen",
            " (when (BinaryOp (left (Identifier (parts [\"t\" \"a\"]))) (op Eq) (right (Literal (value (Number (text \"1\")))))))",
            " (then (Cast (e (BinaryOp (left (Identifier (parts [\"b\"]))) (op Sub) (right (Literal (value (Number (text \"2\"))))))) (to (Int (unsigned true) (width 64))) (notes [\"fast\" \"checked\"])))",
            " (op Add)",
            ") ",
            "(WhenThen",
            " (when (Identifier (parts [\"c\"])))",
            " (then (Literal (value (String (value \"ok\\n\")))))",
            ")",
            "]"
        )
    );

    let wt0: WhenThenRef<'_> = cases.get(0).unwrap();
    assert_eq!(wt0.op(), Some(BinOp::Add));
    let bin = wt0.when().as_binary_op().unwrap();
    assert_eq!(bin.op(), BinOp::Eq);
    let lhs_parts: Vec<&str> = bin
        .left()
        .as_identifier()
        .unwrap()
        .parts()
        .iter()
        .map(Identifier::as_str)
        .collect();
    assert_eq!(lhs_parts, ["t", "a"]);
    assert_eq!(
        bin.right()
            .as_literal()
            .unwrap()
            .value()
            .as_number()
            .unwrap()
            .text(),
        "1"
    );

    let cast1 = wt0.then().as_cast().unwrap();
    assert_eq!(cast1.e().as_binary_op().unwrap().op(), BinOp::Sub);
    let dt_int = cast1.to().as_int().unwrap();
    assert!(!dt_int.nullable());
    assert_eq!(dt_int.unsigned(), Some(true));
    assert_eq!(dt_int.width(), Some(64));

    let notes = cast1.notes();
    assert!(!notes.is_empty());
    assert_eq!(notes.len(), 2);
    assert_eq!(notes.get(0), Some("fast"));
    assert_eq!(notes.get(1), Some("checked"));
    assert_eq!(notes.get(2), None);
    let notes_vec: Vec<&str> = notes.into_iter().collect();
    assert_eq!(notes_vec, ["fast", "checked"]);
    assert_eq!(notes.to_sexpr(), "[\"fast\" \"checked\"]");

    let collected: Vec<WhenThenRef<'_>> = cases.into_iter().collect();
    assert_eq!(collected.len(), 2);
    let wt1 = collected[1];
    assert_eq!(wt1.op(), None);
    assert_eq!(
        wt1.then()
            .as_literal()
            .unwrap()
            .value()
            .as_string()
            .unwrap()
            .value(),
        "ok\n"
    );

    let else_expr = case.r#else().expect("has else");
    match else_expr.view() {
        ExprView::Cast(c) => {
            assert!(c.notes().is_empty());
            assert_eq!(c.e().as_literal().unwrap().value().kind(), LiteralKind::Null);
            match c.to().view() {
                DataTypeView::Bool(b_view) => assert!(b_view.nullable()),
                DataTypeView::Int(_) => panic!("expected Bool data type"),
            }
        }
        _ => panic!("expected Cast in else branch"),
    }
}

#[test]
fn views_api_runs_identically_over_arena_and_owned_trees() {
    let b = &Bump::new();
    let arena_expr = build_arena_expr(b, span_at(1, 50));
    let owned_expr = build_owned_expr(span_at(1, 50));

    inspect_case_tree(ExprRef::from(arena_expr));
    inspect_case_tree(ExprRef::from(&owned_expr));

    let arena_file = al(
        b,
        arena::File {
            exprs: bvec![in b; arena_expr],
            spans: bvec![in b; span_at(1, 50)],
        },
    );
    let owned_file = owned::File {
        exprs: vec![owned_expr],
        spans: vec![span_at(1, 50)],
    };
    assert_eq!(arena_file.to_owned(), owned_file);
    assert_eq!(arena_file.to_sexpr(), owned_file.to_sexpr());

    let file_ref = FileRef::from(arena_file);
    assert_eq!(file_ref.exprs().len(), 1);
    assert_eq!(file_ref.spans(), &[span_at(1, 50)]);
    inspect_case_tree(file_ref.exprs().get(0).unwrap());
}

mod gen_flat {
    include!(concat!(env!("OUT_DIR"), "/gen_flat.rs"));
}

fn mini_sql_dialect_cdylib() -> std::path::PathBuf {
    use std::path::PathBuf;
    use std::process::Command;

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let child_manifest = manifest_dir.join("tests/mini-sql-dialect/Cargo.toml");
    let exe = std::env::current_exe().expect("test executable path");
    let outer_target = exe
        .parent()
        .and_then(|p| p.parent())
        .expect("target/<profile>/deps parent");
    let child_target = outer_target.join("mini-sql-dialect-target");

    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let status = Command::new(cargo)
        .args(["build", "--manifest-path"])
        .arg(&child_manifest)
        .arg("--target-dir")
        .arg(&child_target)
        .arg("--offline")
        .status()
        .expect("failed to invoke cargo build for mini-sql-dialect");
    assert!(status.success(), "building mini-sql-dialect cdylib failed");

    let dylib_name = format!(
        "{}mini_sql_dialect{}",
        std::env::consts::DLL_PREFIX,
        std::env::consts::DLL_SUFFIX
    );
    child_target.join("debug").join(dylib_name)
}

fn child_node<'t>(node: dbt_asdl::Node<'t>, field: &str) -> dbt_asdl::Node<'t> {
    match node.field(field) {
        Some(dbt_asdl::Value::Node(n)) => n,
        _ => panic!("expected Node for field '{field}'"),
    }
}

#[test]
fn flat_abi_loads_cdylib_and_exercises_tree_across_boundary() {
    use dbt_asdl::{Dialect, LoadError, NONE_U8, Value};
    use gen_flat::{DataTypeTag, StableNodeExt};

    let dylib_path = mini_sql_dialect_cdylib();

    match unsafe {
        Dialect::load(std::path::Path::new("/nonexistent/libfoo.so"), "mini", gen_flat::ABI_VERSION)
    } {
        Err(LoadError::Open(_)) => {}
        Err(other) => panic!("expected LoadError::Open, got {other:?}"),
        Ok(_) => panic!("expected LoadError::Open"),
    }

    match unsafe { Dialect::load(&dylib_path, "nosuch", gen_flat::ABI_VERSION) } {
        Err(LoadError::MissingSymbol(sym)) => assert_eq!(sym, "sql_dialect_nosuch"),
        Err(other) => panic!("expected LoadError::MissingSymbol, got {other:?}"),
        Ok(_) => panic!("expected LoadError::MissingSymbol"),
    }

    let dialect = unsafe { Dialect::load(&dylib_path, "mini", gen_flat::ABI_VERSION) }
        .expect("load mini-sql-dialect shared library");
    assert_eq!(dialect.name(), "mini");
    assert_eq!(dialect.dialect_version(), "0.1.0");
    assert_eq!(dialect.descriptor().runtime_abi_version, dbt_asdl::RUNTIME_ABI_VERSION);
    assert_eq!(dialect.descriptor().abi_version, gen_flat::ABI_VERSION);
    assert_ne!(dialect.schema_hash(), 0);

    match dialect.parse("") {
        Err(e) => assert_eq!(e, "empty input"),
        Ok(_) => panic!("expected empty input error"),
    }
    match dialect.parse("bogus") {
        Err(e) => assert_eq!(e, "unsupported query mode: bogus"),
        Ok(_) => panic!("expected unsupported query mode error"),
    }
    match dialect.parse("panic") {
        Err(e) => assert_eq!(e, "panic in parse"),
        Ok(_) => panic!("expected panic in parse error"),
    }
    assert!(matches!(
        unsafe { dbt_asdl::Tree::from_raw(std::ptr::null_mut()) },
        Err(dbt_asdl::AbiError::NullTree)
    ));

    let tree_from_convert = dialect.parse("convert").expect("parse via convert_file");
    let tree_from_builder = dialect.parse("builder").expect("parse via TreeBuilder");

    assert_eq!(tree_from_convert.runtime_abi_version(), dbt_asdl::RUNTIME_ABI_VERSION);
    assert_eq!(tree_from_convert.abi_version(), gen_flat::ABI_VERSION);
    assert_eq!(tree_from_convert.schema_hash(), dialect.schema_hash());
    assert_eq!(tree_from_builder.runtime_abi_version(), dbt_asdl::RUNTIME_ABI_VERSION);
    assert_eq!(tree_from_builder.abi_version(), gen_flat::ABI_VERSION);
    assert_eq!(tree_from_builder.schema_hash(), dialect.schema_hash());

    let expected_sexpr = concat!(
        "(File",
        " (exprs [",
        "(Cast",
        " (e (Case",
        " (operand (Identifier (parts [\"db\" \"users\" \"id\"])))",
        " (cases [",
        "(WhenThen",
        " (when (BinaryOp (left (Identifier (parts [\"db\" \"users\" \"id\"]))) (op Eq) (right (Literal (value (Number (text \"42\")))))))",
        " (then (Literal (value (String (value \"ok\\n\\\"1\\\"\")))))",
        " (op Add)",
        ") ",
        "(WhenThen",
        " (when (Identifier (parts [\"db\" \"users\" \"id\"])))",
        " (then (Literal (value (Number (text \"42\")))))",
        ")",
        "])",
        " (else (Literal (value Null)))",
        "))",
        " (to (Int (nullable true) (unsigned false) (width 64)))",
        " (notes [\"narrow\" \"checked\"])",
        ") ",
        "(Cast",
        " (e (Identifier (parts [\"db\" \"users\" \"id\"])))",
        " (to Bool)",
        ")",
        "])",
        ")"
    );

    assert_eq!(tree_from_convert.to_sexpr(), expected_sexpr);
    assert_eq!(tree_from_builder.to_sexpr(), expected_sexpr);

    for tree in [&tree_from_convert, &tree_from_builder] {
        let root = tree.root();
        assert_eq!(root.type_name(), "file");
        assert_eq!(root.kind(), None);
        assert_eq!(root.kind_id(), None);
        assert!(root.field("nonexistent").is_none());
        assert!(root.as_data_type().is_none());
        assert!(root.as_when_then().is_none());

        let Some(Value::Seq(exprs)) = root.field("exprs") else {
            panic!("expected exprs sequence");
        };
        assert!(!exprs.is_empty());
        assert_eq!(exprs.len(), 2);
        assert!(exprs.get(2).is_none());
        assert_eq!(exprs.elem_type(), tree.type_by_name("expr").unwrap());
        assert_eq!(root.fields().len(), 2);
        assert!(root.field_at(0).is_some());
        assert!(root.field_at(2).is_none());

        let Some(Value::Node(cast0)) = exprs.get(0) else {
            panic!("expected first expr node");
        };
        assert_eq!(cast0.type_name(), "expr");
        assert_eq!(cast0.kind(), Some("Cast"));
        assert_eq!(cast0.kind_id(), tree.kind_by_name("Cast"));
        assert!(tree.kind_by_name("NoSuchCtor").is_none());

        let Some(Value::Seq(notes)) = cast0.field("notes") else {
            panic!("expected notes sequence");
        };
        let note_strs: Vec<&str> = notes
            .iter()
            .map(|v| match v {
                Value::Str(s) => s,
                _ => panic!("expected Str in notes"),
            })
            .collect();
        assert_eq!(note_strs, ["narrow", "checked"]);

        let to0 = child_node(cast0, "to");
        assert_eq!(to0.type_name(), "data_type");
        assert_eq!(to0.kind(), Some("Int"));
        assert!(matches!(to0.field("nullable"), Some(Value::Bool(true))));
        assert!(matches!(to0.field("unsigned"), Some(Value::Bool(false))));
        assert!(matches!(to0.field("width"), Some(Value::Byte(64))));

        let dt0 = to0.as_data_type().expect("@stable data_type projection");
        assert_eq!(dt0.node().index(), to0.index());
        assert_eq!(dt0.tag(), Some(DataTypeTag::Int));
        assert!(dt0.as_bool().is_none());
        let dt0_int = dt0.as_int().expect("Int payload");
        assert_eq!(dt0_int.nullable, 1);
        assert_eq!(dt0_int.unsigned.get(), Some(false));
        assert_eq!(dt0_int.width.get(), Some(64));
        assert!(!dt0_int.span.is_none());

        let case_node = child_node(cast0, "e");
        assert_eq!(case_node.kind(), Some("Case"));
        let operand = child_node(case_node, "operand");
        assert_eq!(operand.kind(), Some("Identifier"));
        let Some(Value::Seq(parts)) = operand.field("parts") else {
            panic!("expected parts sequence");
        };
        let part_strs: Vec<&str> = parts
            .iter()
            .map(|v| match v {
                Value::Str(s) => s,
                _ => panic!("expected Str in parts"),
            })
            .collect();
        assert_eq!(part_strs, ["db", "users", "id"]);

        let Some(Value::Seq(cases)) = case_node.field("cases") else {
            panic!("expected cases sequence");
        };
        assert_eq!(cases.len(), 2);

        let Some(Value::Node(wt0_node)) = cases.get(0) else {
            panic!("expected wt0 node");
        };
        assert_eq!(wt0_node.type_name(), "when_then");
        assert_eq!(wt0_node.kind(), None);
        let Some(Value::SimpleSum(op_ty, op_tag)) = wt0_node.field("op") else {
            panic!("expected SimpleSum for wt0.op");
        };
        assert_eq!(tree.simple_variant_name(op_ty, op_tag), Some("Add"));

        let wt0_proj = wt0_node
            .as_when_then()
            .expect("@stable when_then projection");
        assert!(!wt0_proj.when.is_none());
        assert!(!wt0_proj.then.is_none());
        assert_eq!(wt0_proj.op, BinOp::Add as u8);

        let when0 = child_node(wt0_node, "when");
        assert_eq!(when0.kind(), Some("BinaryOp"));
        let Some(Value::SimpleSum(bin_ty, bin_tag)) = when0.field("op") else {
            panic!("expected SimpleSum for BinaryOp.op");
        };
        assert_eq!(tree.simple_variant_name(bin_ty, bin_tag), Some("Eq"));

        let Some(Value::Node(wt1_node)) = cases.get(1) else {
            panic!("expected wt1 node");
        };
        assert!(matches!(wt1_node.field("op"), Some(Value::None)));
        let wt1_proj = wt1_node
            .as_when_then()
            .expect("@stable when_then projection");
        assert_eq!(wt1_proj.op, NONE_U8);

        let Some(Value::Node(cast1)) = exprs.get(1) else {
            panic!("expected second expr node");
        };
        let to1 = child_node(cast1, "to");
        let dt1 = to1.as_data_type().expect("@stable data_type projection");
        assert_eq!(dt1.tag(), Some(DataTypeTag::Bool));
        assert!(dt1.as_int().is_none());
        let dt1_bool = dt1.as_bool().expect("Bool payload");
        assert_eq!(dt1_bool.nullable, 0);

        let Some(Value::Seq(spans)) = root.field("spans") else {
            panic!("expected spans sequence");
        };
        assert_eq!(spans.len(), 2);
        let Some(Value::Node(span0)) = spans.get(0) else {
            panic!("expected span0 node");
        };
        let start0 = child_node(span0, "start");
        assert!(matches!(start0.field("lineno"), Some(Value::Int(1))));
        assert!(matches!(start0.field("col_offset"), Some(Value::Int(0))));
        assert!(matches!(start0.field("index"), Some(Value::Long(0))));

        let Some(Value::Node(span1)) = spans.get(1) else {
            panic!("expected span1 node");
        };
        let start1 = child_node(span1, "start");
        assert!(matches!(start1.field("lineno"), Some(Value::None)));
        assert!(matches!(start1.field("col_offset"), Some(Value::None)));
        assert!(matches!(start1.field("index"), Some(Value::None)));
    }

    let freeze = gen_flat::freeze_stable_layout();
    assert!(freeze.starts_with("abi_version: 1\n"));
    assert!(freeze.contains("type data_type: sum\n"));
    assert!(freeze.contains("type when_then: product size "));
    assert!(freeze.contains("type bin_op: simple_sum (referenced by a stable field)\n"));
    assert!(freeze.contains("variant 0: Int size 8 align 4\n"));
    assert!(freeze.contains("nullable: u8 @4\n"));
    assert!(freeze.contains("unsigned: OptBool @5\n"));
    assert!(freeze.contains("width: OptByte @6\n"));
    assert_eq!(std::mem::size_of::<gen_flat::CodeLocation>(), 16);
}

#[test]
fn flat_abi_version_drift_and_incompatible_version_refusal() {
    use dbt_asdl::{AbiError, Dialect, LoadError, Value};
    use gen_flat::{DataTypeTag, StableNodeExt};

    let dylib_path = mini_sql_dialect_cdylib();

    match unsafe { Dialect::load(&dylib_path, "v2", gen_flat::ABI_VERSION) } {
        Err(LoadError::Abi(AbiError::AbiVersion { found, expected })) => {
            assert_eq!(found, 2);
            assert_eq!(expected, 1);
        }
        Err(other) => panic!("expected AbiError::AbiVersion, got {other:?}"),
        Ok(_) => panic!("expected version 2 library to be refused by version 1 host"),
    }

    let v2_dialect = unsafe { Dialect::load(&dylib_path, "v2", 2) }
        .expect("load v2 dialect when expecting ABI version 2");
    assert_eq!(v2_dialect.descriptor().abi_version, 2);

    let current = unsafe { Dialect::load(&dylib_path, "mini", gen_flat::ABI_VERSION) }.unwrap();
    let legacy = unsafe { Dialect::load(&dylib_path, "legacy", gen_flat::ABI_VERSION) }
        .expect("host built from newer .asdl loads dialect built from older compatible .asdl");

    assert_eq!(legacy.descriptor().abi_version, gen_flat::ABI_VERSION);
    assert_ne!(legacy.schema_hash(), current.schema_hash());

    let tree = legacy.parse("SELECT 1").expect("parse with legacy dialect");
    assert_eq!(tree.abi_version(), gen_flat::ABI_VERSION);
    assert_eq!(tree.schema_hash(), legacy.schema_hash());

    let expected_legacy_sexpr = concat!(
        "(File (exprs [(Cast",
        " (e (Case (cases [(WhenThen",
        " (when (Paren (inner (Identifier (parts [\"col\"])))))",
        " (then (Literal (value (Boolean (val true)))))",
        " (op Eq))])))",
        " (to (Int (nullable true) (unsigned true) (width 32)))",
        " (legacy_hint \"v1-hint\"))]))"
    );
    assert_eq!(tree.to_sexpr(), expected_legacy_sexpr);

    let root = tree.root();
    let Some(Value::Seq(exprs)) = root.field("exprs") else {
        panic!("expected exprs sequence");
    };
    let Some(Value::Node(cast)) = exprs.get(0) else {
        panic!("expected Cast node");
    };
    assert_eq!(cast.kind(), Some("Cast"));

    assert!(cast.field("notes").is_none());
    assert!(matches!(cast.field("legacy_hint"), Some(Value::Str("v1-hint"))));

    assert!(tree.kind_by_name("Paren").is_some());
    assert!(tree.kind_by_name("Boolean").is_some());
    assert!(tree.kind_by_name("Null").is_none());

    let case_node = child_node(cast, "e");
    let Some(Value::Seq(cases)) = case_node.field("cases") else {
        panic!("expected cases sequence");
    };
    let Some(Value::Node(wt_node)) = cases.get(0) else {
        panic!("expected WhenThen node");
    };

    let wt_proj = wt_node
        .as_when_then()
        .expect("@stable when_then projection works across drifted schema");
    assert_eq!(wt_proj.op, BinOp::Eq as u8);

    let when_node = child_node(wt_node, "when");
    assert_eq!(when_node.kind(), Some("Paren"));
    let inner = child_node(when_node, "inner");
    assert_eq!(inner.kind(), Some("Identifier"));

    let then_node = child_node(wt_node, "then");
    let lit_node = child_node(then_node, "value");
    assert_eq!(lit_node.kind(), Some("Boolean"));
    assert!(matches!(lit_node.field("val"), Some(Value::Bool(true))));

    let to_node = child_node(cast, "to");
    let dt = to_node
        .as_data_type()
        .expect("@stable data_type projection works across drifted schema");
    assert_eq!(dt.tag(), Some(DataTypeTag::Int));
    let dt_int = dt.as_int().expect("Int payload");
    assert_eq!(dt_int.nullable, 1);
    assert_eq!(dt_int.unsigned.get(), Some(true));
    assert_eq!(dt_int.width.get(), Some(32));
}
