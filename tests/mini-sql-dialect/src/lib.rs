//! Test dialect shared library generated from `tests/fixtures/mini_sql.asdl`.

#![allow(missing_docs, dead_code)]

use bumpalo::collections::Vec as BVec;
use bumpalo::{Bump, vec as bvec};
use dbt_asdl::{FlatStr, FlatTree, Identifier, NONE_U8, OptBool, OptByte, OptInt, OptLong};

mod shared {
    include!(concat!(env!("OUT_DIR"), "/gen.rs"));
}

pub mod arena {
    include!(concat!(env!("OUT_DIR"), "/gen_arena.rs"));
}

pub mod owned {
    include!(concat!(env!("OUT_DIR"), "/gen_owned.rs"));
}

mod gen_views {
    include!(concat!(env!("OUT_DIR"), "/gen_views.rs"));
}

mod gen_sexpr {
    include!(concat!(env!("OUT_DIR"), "/gen_sexpr.rs"));
}

pub mod ast {
    pub use crate::gen_views::*;
    pub use crate::shared::*;
}

mod gen_flat {
    include!(concat!(env!("OUT_DIR"), "/gen_flat.rs"));
}
pub use gen_flat::*;

#[cfg(feature = "convert")]
mod gen_convert {
    include!(concat!(env!("OUT_DIR"), "/gen_convert.rs"));
}
#[cfg(feature = "convert")]
pub use gen_convert::convert_file;

fn arena_span(start: i64, end: i64) -> ast::Span {
    ast::Span {
        start: ast::CodeLocation {
            lineno: Some(1),
            col_offset: Some(start as i32),
            index: Some(start),
        },
        end: ast::CodeLocation {
            lineno: Some(1),
            col_offset: Some(end as i32),
            index: Some(end),
        },
    }
}

fn build_via_convert() -> *mut FlatTree {
    let b = Bump::new();
    let ident_expr = b.alloc(arena::Expr::Identifier(b.alloc(arena::expr::Identifier {
        span: arena_span(0, 8),
        parts: bvec![in &b; Identifier::from("db"), Identifier::from("users"), Identifier::from("id")],
    })));
    let num_lit = b.alloc(arena::Literal::Number(b.alloc(arena::literal::Number {
        span: arena_span(11, 13),
        text: b.alloc_str("42"),
    })));
    let num_expr = b.alloc(arena::Expr::Literal(b.alloc(arena::expr::Literal {
        span: arena_span(11, 13),
        value: num_lit,
    })));
    let cond = b.alloc(arena::Expr::BinaryOp(b.alloc(arena::expr::BinaryOp {
        span: arena_span(0, 13),
        left: ident_expr,
        op: ast::BinOp::Eq,
        right: num_expr,
    })));

    let str_lit = b.alloc(arena::Literal::String(b.alloc(arena::literal::String {
        span: arena_span(19, 25),
        value: b.alloc_str("ok\n\"1\""),
    })));
    let then_expr = b.alloc(arena::Expr::Literal(b.alloc(arena::expr::Literal {
        span: arena_span(19, 25),
        value: str_lit,
    })));

    let null_lit = b.alloc(arena::Literal::Null(b.alloc(arena::literal::Null {
        span: arena_span(31, 35),
    })));
    let else_expr = b.alloc(arena::Expr::Literal(b.alloc(arena::expr::Literal {
        span: arena_span(31, 35),
        value: null_lit,
    })));

    let wt1 = b.alloc(arena::WhenThen {
        when: cond,
        then: then_expr,
        op: Some(ast::BinOp::Add),
    });
    let wt2 = b.alloc(arena::WhenThen {
        when: ident_expr,
        then: num_expr,
        op: None,
    });

    let case_expr = b.alloc(arena::Expr::Case(b.alloc(arena::expr::Case {
        span: arena_span(0, 40),
        operand: Some(ident_expr),
        cases: bvec![in &b; &*wt1, &*wt2],
        r#else: Some(else_expr),
    })));

    let int_ty = b.alloc(arena::DataType::Int(b.alloc(arena::data_type::Int {
        span: arena_span(44, 50),
        nullable: true,
        unsigned: Some(false),
        width: Some(64),
    })));
    let cast_int = b.alloc(arena::Expr::Cast(b.alloc(arena::expr::Cast {
        span: arena_span(0, 51),
        e: case_expr,
        to: int_ty,
        notes: bvec![in &b; &*b.alloc_str("narrow"), &*b.alloc_str("checked")],
    })));

    let bool_ty = b.alloc(arena::DataType::Bool(b.alloc(arena::data_type::Bool {
        span: arena_span(60, 64),
        nullable: false,
    })));
    let cast_bool = b.alloc(arena::Expr::Cast(b.alloc(arena::expr::Cast {
        span: arena_span(55, 65),
        e: ident_expr,
        to: bool_ty,
        notes: BVec::new_in(&b),
    })));

    let extra_span = ast::Span {
        start: ast::CodeLocation {
            lineno: None,
            col_offset: None,
            index: None,
        },
        end: ast::CodeLocation {
            lineno: Some(2),
            col_offset: Some(10),
            index: Some(80),
        },
    };

    let file = b.alloc(arena::File {
        exprs: bvec![in &b; &*cast_int, &*cast_bool],
        spans: bvec![in &b; arena_span(0, 65), extra_span],
    });
    convert_file(ast::FileRef::Arena(file))
}

fn flat_span(b: &mut TreeBuilder, start: i64, end: i64) -> SpanRef {
    let s = b.push_code_location(CodeLocation {
        lineno: OptInt::from(Some(1)),
        col_offset: OptInt::from(Some(start as i32)),
        index: OptLong::from(Some(start)),
    });
    let e = b.push_code_location(CodeLocation {
        lineno: OptInt::from(Some(1)),
        col_offset: OptInt::from(Some(end as i32)),
        index: OptLong::from(Some(end)),
    });
    b.push_span(Span { start: s, end: e })
}

fn build_via_builder() -> *mut FlatTree {
    let mut b = TreeBuilder::new();

    let span_ident = flat_span(&mut b, 0, 8);
    let p_db = b.push_str("db");
    let p_users = b.push_str("users");
    let p_id = b.push_str("id");
    let parts = b.push_identifier_seq(&[p_db, p_users, p_id]);
    let ident_expr = b.push_expr_identifier(expr::Identifier {
        span: span_ident,
        parts,
    });

    let span_num_lit = flat_span(&mut b, 11, 13);
    let text_42 = b.push_str("42");
    let num_lit = b.push_literal_number(literal::Number {
        span: span_num_lit,
        text: text_42,
    });
    let span_num_expr = flat_span(&mut b, 11, 13);
    let num_expr = b.push_expr_literal(expr::Literal {
        span: span_num_expr,
        value: num_lit,
    });

    let span_cond = flat_span(&mut b, 0, 13);
    let cond = b.push_expr_binary_op(expr::BinaryOp {
        span: span_cond,
        left: ident_expr,
        op: ast::BinOp::Eq as u8,
        right: num_expr,
    });

    let span_str_lit = flat_span(&mut b, 19, 25);
    let val_ok = b.push_str("ok\n\"1\"");
    let str_lit = b.push_literal_string(literal::String {
        span: span_str_lit,
        value: val_ok,
    });
    let span_then = flat_span(&mut b, 19, 25);
    let then_expr = b.push_expr_literal(expr::Literal {
        span: span_then,
        value: str_lit,
    });

    let span_null_lit = flat_span(&mut b, 31, 35);
    let null_lit = b.push_literal_null(literal::Null {
        span: span_null_lit,
    });
    let span_else = flat_span(&mut b, 31, 35);
    let else_expr = b.push_expr_literal(expr::Literal {
        span: span_else,
        value: null_lit,
    });

    let wt1 = b.push_when_then(WhenThen {
        when: cond,
        then: then_expr,
        op: ast::BinOp::Add as u8,
    });
    let wt2 = b.push_when_then(WhenThen {
        when: ident_expr,
        then: num_expr,
        op: NONE_U8,
    });
    let cases = b.push_when_then_seq(&[wt1, wt2]);

    let span_case = flat_span(&mut b, 0, 40);
    let case_expr = b.push_expr_case(expr::Case {
        span: span_case,
        operand: ident_expr,
        cases,
        r#else: else_expr,
    });

    let span_int_ty = flat_span(&mut b, 44, 50);
    let int_ty = b.push_data_type_int(data_type::Int {
        span: span_int_ty,
        nullable: 1,
        unsigned: OptBool::from(Some(false)),
        width: OptByte::from(Some(64)),
    });
    let n_narrow = b.push_str("narrow");
    let n_checked = b.push_str("checked");
    let notes_int = b.push_string_seq(&[n_narrow, n_checked]);
    let span_cast_int = flat_span(&mut b, 0, 51);
    let cast_int = b.push_expr_cast(expr::Cast {
        span: span_cast_int,
        e: case_expr,
        to: int_ty,
        notes: notes_int,
    });

    let span_bool_ty = flat_span(&mut b, 60, 64);
    let bool_ty = b.push_data_type_bool(data_type::Bool {
        span: span_bool_ty,
        nullable: 0,
    });
    let notes_bool = b.push_string_seq(&[]);
    let span_cast_bool = flat_span(&mut b, 55, 65);
    let cast_bool = b.push_expr_cast(expr::Cast {
        span: span_cast_bool,
        e: ident_expr,
        to: bool_ty,
        notes: notes_bool,
    });

    let exprs = b.push_expr_seq(&[cast_int, cast_bool]);

    let file_span_0 = flat_span(&mut b, 0, 65);
    let extra_start = b.push_code_location(CodeLocation {
        lineno: OptInt::NONE,
        col_offset: OptInt::NONE,
        index: OptLong::NONE,
    });
    let extra_end = b.push_code_location(CodeLocation {
        lineno: OptInt::from(Some(2)),
        col_offset: OptInt::from(Some(10)),
        index: OptLong::from(Some(80)),
    });
    let file_span_1 = b.push_span(Span {
        start: extra_start,
        end: extra_end,
    });
    let spans = b.push_span_seq(&[file_span_0, file_span_1]);

    let root = b.push_file(File { exprs, spans });
    b.finish(root)
}

fn parse(src: &str) -> Result<*mut FlatTree, String> {
    match src.trim() {
        "" => Err("empty input".to_string()),
        "convert" => Ok(build_via_convert()),
        "builder" => Ok(build_via_builder()),
        "panic" => panic!("deliberate panic inside dialect cdylib"),
        other => Err(format!("unsupported query mode: {other}")),
    }
}

dbt_asdl::export_sql_dialect!(sql_dialect_mini, "mini", "0.1.0", parse);

mod legacy_dialect {
    use dbt_asdl::{FlatTree, OptBool, OptByte, OptInt, OptLong};

    mod gen_flat {
        include!(concat!(env!("OUT_DIR"), "/legacy/gen_flat.rs"));
    }
    use gen_flat::*;

    fn flat_span(b: &mut TreeBuilder, start: i64, end: i64) -> SpanRef {
        let s = b.push_code_location(CodeLocation {
            lineno: OptInt::from(Some(1)),
            col_offset: OptInt::from(Some(start as i32)),
            index: OptLong::from(Some(start)),
        });
        let e = b.push_code_location(CodeLocation {
            lineno: OptInt::from(Some(1)),
            col_offset: OptInt::from(Some(end as i32)),
            index: OptLong::from(Some(end)),
        });
        b.push_span(Span { start: s, end: e })
    }

    pub fn parse(src: &str) -> Result<*mut FlatTree, String> {
        if src.trim().is_empty() {
            return Err("empty input".to_string());
        }
        let mut b = TreeBuilder::new();
        let sp = flat_span(&mut b, 0, 20);

        let p_col = b.push_str("col");
        let parts = b.push_identifier_seq(&[p_col]);
        let ident = b.push_expr_identifier(expr::Identifier { span: sp, parts });
        let paren = b.push_expr_paren(expr::Paren {
            span: sp,
            inner: ident,
        });

        let bool_lit = b.push_literal_boolean(literal::Boolean { span: sp, val: 1 });
        let bool_expr = b.push_expr_literal(expr::Literal {
            span: sp,
            value: bool_lit,
        });

        let wt = b.push_when_then(WhenThen {
            when: paren,
            then: bool_expr,
            op: super::ast::BinOp::Eq as u8,
        });
        let cases = b.push_when_then_seq(&[wt]);
        let case_expr = b.push_expr_case(expr::Case {
            span: sp,
            operand: ExprRef::NONE,
            cases,
            r#else: ExprRef::NONE,
        });

        let int_ty = b.push_data_type_int(data_type::Int {
            span: sp,
            nullable: 1,
            unsigned: OptBool::from(Some(true)),
            width: OptByte::from(Some(32)),
        });
        let hint = b.push_str("v1-hint");
        let cast_expr = b.push_expr_cast(expr::Cast {
            span: sp,
            e: case_expr,
            to: int_ty,
            legacy_hint: hint,
        });

        let exprs = b.push_expr_seq(&[cast_expr]);
        let spans = b.push_span_seq(&[sp]);
        let root = b.push_file(File { exprs, spans });
        Ok(b.finish(root))
    }

    dbt_asdl::export_sql_dialect!(sql_dialect_legacy, "legacy", "0.1.0", parse);
}

mod v2_dialect {
    use dbt_asdl::FlatTree;

    mod gen_flat {
        include!(concat!(env!("OUT_DIR"), "/v2/gen_flat.rs"));
    }
    use gen_flat::*;

    pub fn parse(_src: &str) -> Result<*mut FlatTree, String> {
        let mut b = TreeBuilder::new();
        let exprs = b.push_expr_seq(&[]);
        let spans = b.push_span_seq(&[]);
        let root = b.push_file(File { exprs, spans });
        Ok(b.finish(root))
    }

    dbt_asdl::export_sql_dialect!(sql_dialect_v2, "v2", "0.2.0", parse);
}
