//! Expression → stack-machine bytecode ([`Inst`]) generation.

use flux_hir::{HirExpr, HirMatchArm, HirProgram};
use flux_syntax::BinaryOp;

use crate::{Inst, IrValue};

use super::runtime_symbol_name;

pub(super) fn lower_expr(expr: &HirExpr, program: &HirProgram) -> Vec<Inst> {
    let mut code = Vec::new();
    emit_expr(expr, program, &mut code);
    code
}

fn emit_match_bindings(
    scrutinee_name: &str,
    arm: &HirMatchArm,
    program: &HirProgram,
    code: &mut Vec<Inst>,
) {
    if arm.bindings.is_empty() {
        return;
    }
    if !arm.variant.is_empty() && arm.binding_fields.is_empty() {
        code.push(Inst::Load(scrutinee_name.to_string()));
        code.push(Inst::VariantPayloadGet { field: None });
        if let Some(sym) = arm.bindings.first() {
            if let Some(name) = runtime_symbol_name(program, sym) {
                code.push(Inst::BindLocal(name));
            }
        }
    } else if arm.variant.is_empty() && arm.bindings.len() == 1 {
        code.push(Inst::Load(scrutinee_name.to_string()));
        if let Some(name) = runtime_symbol_name(program, &arm.bindings[0]) {
            code.push(Inst::BindLocal(name));
        }
    } else if !arm.binding_fields.is_empty() {
        for (sym, field) in arm.bindings.iter().zip(arm.binding_fields.iter()) {
            code.push(Inst::Load(scrutinee_name.to_string()));
            code.push(Inst::VariantPayloadGet {
                field: Some(field.clone()),
            });
            if let Some(name) = runtime_symbol_name(program, sym) {
                code.push(Inst::BindLocal(name));
            }
        }
    }
}

fn emit_match_arm_cond(
    scrutinee_name: &str,
    arm: &HirMatchArm,
    program: &HirProgram,
    code: &mut Vec<Inst>,
) {
    if arm.variant.is_empty() {
        if let Some(guard) = &arm.guard {
            emit_match_bindings(scrutinee_name, arm, program, code);
            emit_expr(guard, program, code);
        } else {
            code.push(Inst::Const(IrValue::Bool(true)));
        }
        return;
    }
    code.push(Inst::Load(scrutinee_name.to_string()));
    code.push(Inst::VariantTagEq(arm.variant.clone()));
    if arm.guard.is_some() {
        let mut guard_code = Vec::new();
        emit_match_bindings(scrutinee_name, arm, program, &mut guard_code);
        if let Some(guard) = &arm.guard {
            emit_expr(guard, program, &mut guard_code);
        }
        code.push(Inst::ShortAnd(guard_code));
    }
}

/// Best-effort receiver type tag for trait method dispatch (e.g. `(42).to_string()`),
/// derived structurally without a full type checker. Returns `None` when the
/// expression's type can't be determined this way (e.g. identifiers, calls).
#[allow(clippy::only_used_in_recursion)]
fn static_type_tag(expr: &HirExpr, program: &HirProgram) -> Option<String> {
    match expr {
        HirExpr::Int { .. } => Some("Int".to_string()),
        HirExpr::Float { .. } => Some("Float".to_string()),
        HirExpr::Bool { .. } => Some("Bool".to_string()),
        HirExpr::String { .. } => Some("String".to_string()),
        HirExpr::StructLit { name, .. } => Some(name.clone()),
        HirExpr::EnumVariant { enum_name, .. } => Some(enum_name.clone()),
        HirExpr::Snap { expr, .. } => static_type_tag(expr, program),
        _ => None,
    }
}

fn emit_expr(expr: &HirExpr, program: &HirProgram, code: &mut Vec<Inst>) {
    match expr {
        HirExpr::Int { value, .. } => code.push(Inst::Const(IrValue::Int(*value))),
        HirExpr::Float { value, .. } => code.push(Inst::Const(IrValue::Float(*value))),
        HirExpr::Bool { value, .. } => code.push(Inst::Const(IrValue::Bool(*value))),
        HirExpr::String { value, .. } => code.push(Inst::Const(IrValue::String(value.clone()))),
        HirExpr::Ident { symbol, .. } => {
            if let Some(name) = runtime_symbol_name(program, symbol) {
                code.push(Inst::Load(name));
            }
        }
        HirExpr::Snap { expr, .. } => emit_expr(expr, program, code),
        HirExpr::Unary {
            op: flux_syntax::UnaryOp::Neg,
            expr,
            ..
        } => {
            emit_expr(expr, program, code);
            code.push(Inst::Neg);
        }
        HirExpr::Unary {
            op: flux_syntax::UnaryOp::Not,
            expr,
            ..
        } => {
            emit_expr(expr, program, code);
            code.push(Inst::Not);
        }
        HirExpr::Binary {
            op, left, right, ..
        } => {
            emit_expr(left, program, code);
            match op {
                BinaryOp::And => {
                    let mut right_code = Vec::new();
                    emit_expr(right, program, &mut right_code);
                    code.push(Inst::ShortAnd(right_code));
                }
                BinaryOp::Or => {
                    let mut right_code = Vec::new();
                    emit_expr(right, program, &mut right_code);
                    code.push(Inst::ShortOr(right_code));
                }
                other => {
                    emit_expr(right, program, code);
                    code.push(match other {
                        BinaryOp::Add => Inst::Add,
                        BinaryOp::Sub => Inst::Sub,
                        BinaryOp::Mul => Inst::Mul,
                        BinaryOp::Div => Inst::Div,
                        BinaryOp::Rem => Inst::Rem,
                        BinaryOp::Eq => Inst::CmpEq,
                        BinaryOp::Ne => Inst::CmpNe,
                        BinaryOp::Lt => Inst::CmpLt,
                        BinaryOp::Le => Inst::CmpLe,
                        BinaryOp::Gt => Inst::CmpGt,
                        BinaryOp::Ge => Inst::CmpGe,
                        BinaryOp::And | BinaryOp::Or => unreachable!(),
                    });
                }
            }
        }
        HirExpr::If {
            cond,
            then_expr,
            else_expr,
            ..
        } => {
            emit_expr(cond, program, code);
            let mut then_branch = Vec::new();
            emit_expr(then_expr, program, &mut then_branch);
            let mut else_branch = Vec::new();
            emit_expr(else_expr, program, &mut else_branch);
            code.push(Inst::LazyIf {
                then_branch,
                else_branch,
            });
        }
        HirExpr::Call { callee, args, .. } => {
            if let HirExpr::Field { base, field, .. } = callee.as_ref() {
                emit_expr(base, program, code);
                for arg in args {
                    emit_expr(arg, program, code);
                }
                let fn_id = if let Some(tag) = static_type_tag(base, program) {
                    program.trait_impls.get(&(tag, field.clone())).copied()
                } else {
                    program
                        .trait_impls
                        .iter()
                        .find(|((_, method), _)| method == field)
                        .map(|(_, id)| *id)
                };
                if let Some(name) = fn_id.and_then(|id| runtime_symbol_name(program, &id)) {
                    code.push(Inst::Call {
                        name,
                        argc: 1 + args.len(),
                    });
                }
                return;
            }
            if let HirExpr::Closure { fn_symbol, .. } = callee.as_ref() {
                if let Some(name) = runtime_symbol_name(program, fn_symbol) {
                    for arg in args {
                        emit_expr(arg, program, code);
                    }
                    code.push(Inst::Call {
                        name,
                        argc: args.len(),
                    });
                }
                return;
            }
            if let HirExpr::Ident { symbol, .. } = callee.as_ref() {
                if let Some(name) = runtime_symbol_name(program, symbol) {
                    for arg in args {
                        emit_expr(arg, program, code);
                    }
                    code.push(Inst::Call {
                        name,
                        argc: args.len(),
                    });
                }
            }
        }
        HirExpr::Array { elems, .. } => {
            for elem in elems {
                emit_expr(elem, program, code);
            }
            code.push(Inst::MakeArray(elems.len()));
        }
        HirExpr::EnumVariant {
            enum_name,
            variant,
            payload,
            ..
        } => {
            if let Some(p) = payload {
                emit_expr(p, program, code);
            }
            code.push(Inst::MakeUserEnum {
                type_name: enum_name.clone(),
                variant: variant.clone(),
                has_payload: payload.is_some(),
            });
        }
        HirExpr::StructLit { name, fields, .. } => {
            let field_names: Vec<String> = fields.iter().map(|(n, _)| n.clone()).collect();
            for (_, expr) in fields {
                emit_expr(expr, program, code);
            }
            code.push(Inst::MakeUserStruct {
                type_name: name.clone(),
                fields: field_names,
            });
        }
        HirExpr::Match {
            scrutinee,
            arms,
            span,
        } => {
            if arms.is_empty() {
                code.push(Inst::Const(IrValue::Unit));
                return;
            }
            // NUL cannot occur in a source identifier, so this cannot collide
            // with a user local or module binding.
            let temp = format!("\0flux_match_{}_{}", span.source.0, span.start);
            emit_expr(scrutinee, program, code);
            code.push(Inst::BindLocal(temp.clone()));

            // `LazyIf` ensures that only the selected arm executes. The initial
            // Unit is an unreachable fallback after the type check verifies
            // that every enum variant (or a catch-all) is covered.
            let mut result = vec![Inst::Const(IrValue::Unit)];
            for arm in arms.iter().rev() {
                let mut then_code = Vec::new();
                emit_match_bindings(&temp, arm, program, &mut then_code);
                emit_expr(&arm.expr, program, &mut then_code);
                let mut cond_code = Vec::new();
                emit_match_arm_cond(&temp, arm, program, &mut cond_code);
                let else_branch = std::mem::take(&mut result);
                result = cond_code;
                result.push(Inst::LazyIf {
                    then_branch: then_code,
                    else_branch,
                });
            }
            code.extend(result);
        }
        HirExpr::Cast { expr, ty, .. } => {
            emit_expr(expr, program, code);
            code.push(Inst::Cast { ty: ty.clone() });
        }
        HirExpr::Field { base, field, .. } => {
            emit_expr(base, program, code);
            code.push(Inst::FieldGet(field.clone()));
        }
        HirExpr::Closure { fn_symbol, .. } => {
            if let Some(name) = runtime_symbol_name(program, fn_symbol) {
                code.push(Inst::Const(IrValue::String(name)));
            }
        }
    }
}
