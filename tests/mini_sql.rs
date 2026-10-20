//! End-to-end integration tests for the AST and `Sexpr` code generated from
//! `tests/fixtures/mini_sql.asdl`.

#![allow(missing_docs, dead_code)]

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
                al(b, arena::WhenThen { when: cond1, then: then1 }),
                al(b, arena::WhenThen { when: cond2, then: then2 }),
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
            },
            owned::WhenThen {
                when: Box::new(cond2),
                then: Box::new(then2),
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
        },
    );
    let wt_owned = owned::WhenThen {
        when: Box::new(o_ident(unknown_span(), &["k"])),
        then: Box::new(o_number(unknown_span(), "9")),
    };
    assert_eq!(wt_arena.to_owned(), wt_owned);
    let wt_sexpr = "(WhenThen (when (Identifier (parts [\"k\"]))) (then (Literal (value (Number (text \"9\"))))))";
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
            ") ",
            "(WhenThen",
            " (when (Identifier (parts [\"c\"])))",
            " (then (Literal (value (String (value \"ok\\n\")))))",
            ")",
            "]"
        )
    );

    let wt0: WhenThenRef<'_> = cases.get(0).unwrap();
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
}
