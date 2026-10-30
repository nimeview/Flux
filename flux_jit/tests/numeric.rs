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
fn numeric_code_falls_back_for_enum_match() {
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
    assert!(!code_is_jit_numeric(&code, &HashSet::new()));
}

#[test]
fn stmts_numeric_accepts_if_and_emit() {
    let stmts = vec![
        IrStmt::If {
            cond: vec![Inst::Const(IrValue::Bool(true))],
            then_body: vec![IrStmt::Assign {
                target: "x".into(),
                expr: vec![Inst::Const(IrValue::Int(1))],
            }],
            else_body: None,
        },
        IrStmt::Emit {
            event: "E".into(),
            arg: None,
        },
    ];
    assert!(stmts_are_jit_numeric(&stmts, &HashSet::new()));
}
