use std::collections::HashSet;

use flux_ir::{Inst, IrStmt, IrValue};
use flux_jit::{code_is_jit_numeric, stmts_are_jit_numeric};

#[test]
fn numeric_code_accepts_int_arithmetic() {
    let code = vec![
        Inst::Const(IrValue::Int(1)),
        Inst::Const(IrValue::Int(2)),
        Inst::Add,
    ];
    assert!(code_is_jit_numeric(&code, &HashSet::new()));
}

#[test]
fn numeric_code_accepts_strings() {
    let code = vec![
        Inst::Const(IrValue::String("a".into())),
        Inst::Const(IrValue::String("b".into())),
        Inst::Add,
    ];
    assert!(code_is_jit_numeric(&code, &HashSet::new()));
}

#[test]
fn numeric_code_accepts_enum_match() {
    let code = vec![
        Inst::MakeUserEnum {
            type_name: "Color".into(),
            variant: "Green".into(),
            has_payload: false,
        },
        Inst::VariantTagEq("Green".into()),
        Inst::Const(IrValue::Int(20)),
        Inst::Select,
    ];
    assert!(code_is_jit_numeric(&code, &HashSet::new()));
}

#[test]
fn numeric_code_falls_back_for_struct_variant_fields() {
    let code = vec![
        Inst::Load("shape".into()),
        Inst::VariantPayloadGet {
            field: Some("w".into()),
        },
    ];
    assert!(!code_is_jit_numeric(&code, &HashSet::new()));
}

#[test]
fn stmts_numeric_accepts_if_but_not_emit_or_loop_exits() {
    let if_stmt = IrStmt::If {
        cond: vec![Inst::Const(IrValue::Bool(true))],
        then_body: vec![IrStmt::Assign {
            target: "x".into(),
            slot: Some(0),
            expr: vec![Inst::Const(IrValue::Int(1))],
        }]
        .into(),
        else_body: None,
    };
    assert!(stmts_are_jit_numeric(&[if_stmt], &HashSet::new()));
    let emit = IrStmt::Emit {
        event: "E".into(),
        arg: None,
    };
    assert!(!stmts_are_jit_numeric(&[emit], &HashSet::new()));
    assert!(!stmts_are_jit_numeric(&[IrStmt::Break], &HashSet::new()));
}
