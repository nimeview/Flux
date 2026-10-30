use std::collections::HashSet;

use flux_ir::{Inst, IrStmt};

pub(crate) fn is_numeric_code(code: &[Inst], jit_fns: &HashSet<String>) -> bool {
    code.iter().all(|i| match i {
        Inst::Const(_)
        | Inst::Load(_)
        | Inst::Add
        | Inst::Sub
        | Inst::Mul
        | Inst::Div
        | Inst::Rem
        | Inst::Neg
        | Inst::Not
        | Inst::CmpEq
        | Inst::CmpNe
        | Inst::CmpLt
        | Inst::CmpLe
        | Inst::CmpGt
        | Inst::CmpGe
        | Inst::Select => true,
        Inst::Call { name, .. } => {
            jit_fns.contains(name)
                || name == "range"
                || flux_std::PURE_INTRINSICS.contains(&name.as_str())
        }
        Inst::ShortAnd(r) | Inst::ShortOr(r) => is_numeric_code(r, jit_fns),
        Inst::Cast { .. } => true,
        Inst::LazyIf {
            then_branch,
            else_branch,
        } => is_numeric_code(then_branch, jit_fns) && is_numeric_code(else_branch, jit_fns),
        Inst::MakeArray(_) => true,
        // Pattern matching currently lowers through VM-local bindings. Keep it
        // on the reference VM until the JIT has equivalent branch-local scope.
        Inst::EnumTagEq(_)
        | Inst::VariantTagEq(_)
        | Inst::VariantPayloadGet { .. }
        | Inst::BindLocal(_) => false,
        Inst::MakeUserEnum { .. } => true,
        Inst::MakeUserStruct { .. } | Inst::FieldGet(_) => false,
    })
}

pub(crate) fn stmts_numeric(stmts: &[IrStmt], jit_fns: &HashSet<String>) -> bool {
    stmts.iter().all(|s| match s {
        IrStmt::Expr { code } => is_numeric_code(code, jit_fns),
        IrStmt::Let { expr, .. } => is_numeric_code(expr, jit_fns),
        IrStmt::Assign { expr, .. } => is_numeric_code(expr, jit_fns),
        IrStmt::Emit { arg, .. } => arg
            .as_ref()
            .map(|c| is_numeric_code(c, jit_fns))
            .unwrap_or(true),
        IrStmt::If {
            cond,
            then_body,
            else_body,
        } => {
            is_numeric_code(cond, jit_fns)
                && stmts_numeric(then_body, jit_fns)
                && else_body
                    .as_ref()
                    .map(|b| stmts_numeric(b, jit_fns))
                    .unwrap_or(true)
        }
        IrStmt::While { cond, body } => {
            is_numeric_code(cond, jit_fns) && stmts_numeric(body, jit_fns)
        }
        IrStmt::For { iter, body, .. } => {
            is_numeric_code(iter, jit_fns) && stmts_numeric(body, jit_fns)
        }
        IrStmt::Return { expr } => expr
            .as_ref()
            .map(|c| is_numeric_code(c, jit_fns))
            .unwrap_or(true),
        IrStmt::Break | IrStmt::Continue => true,
    })
}

pub(crate) fn func_is_numeric(body: &[IrStmt], jit_fns: &HashSet<String>) -> bool {
    stmts_numeric(body, jit_fns)
}

pub(crate) fn max_call_argc_in_code(code: &[Inst]) -> u32 {
    let mut max = 0u32;
    for inst in code {
        match inst {
            Inst::Call { argc, .. } => max = max.max(*argc as u32),
            Inst::ShortAnd(r) | Inst::ShortOr(r) => max = max.max(max_call_argc_in_code(r)),
            Inst::LazyIf {
                then_branch,
                else_branch,
            } => {
                max = max
                    .max(max_call_argc_in_code(then_branch))
                    .max(max_call_argc_in_code(else_branch));
            }
            _ => {}
        }
    }
    max
}

pub(crate) fn max_call_argc_in_stmts(stmts: &[IrStmt]) -> u32 {
    let mut max = 0u32;
    for stmt in stmts {
        max = max.max(match stmt {
            IrStmt::Expr { code } => max_call_argc_in_code(code),
            IrStmt::Let { expr, .. } | IrStmt::Assign { expr, .. } => max_call_argc_in_code(expr),
            IrStmt::Emit { arg, .. } => arg.as_ref().map(|c| max_call_argc_in_code(c)).unwrap_or(0),
            IrStmt::If {
                cond,
                then_body,
                else_body,
            } => max_call_argc_in_code(cond)
                .max(max_call_argc_in_stmts(then_body))
                .max(
                    else_body
                        .as_ref()
                        .map(|b| max_call_argc_in_stmts(b))
                        .unwrap_or(0),
                ),
            IrStmt::While { cond, body } => {
                max_call_argc_in_code(cond).max(max_call_argc_in_stmts(body))
            }
            IrStmt::For { iter, body, .. } => {
                max_call_argc_in_code(iter).max(max_call_argc_in_stmts(body))
            }
            IrStmt::Return { expr } => expr.as_ref().map(|c| max_call_argc_in_code(c)).unwrap_or(0),
            IrStmt::Break | IrStmt::Continue => 0,
        });
    }
    max
}
