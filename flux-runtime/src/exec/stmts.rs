//! Statement execution with structured `return` / `break` / `continue` flow.

use std::collections::HashMap;

use flux_ir::IrStmt;

use crate::graph::RuntimeValue;

use super::{Executor, FlowControl, StmtResult};

/// Result of a statement aborted by a runtime error: unwind like `return`.
const TRAPPED: StmtResult = StmtResult {
    value: RuntimeValue::Unit,
    flow: FlowControl::Return,
};

impl Executor {
    pub(super) fn run_stmts(
        &mut self,
        stmts: &[IrStmt],
        locals: &mut HashMap<String, RuntimeValue>,
    ) -> RuntimeValue {
        match self.run_stmts_flow(stmts, locals) {
            (v, FlowControl::None) => v,
            (v, _) => v,
        }
    }

    pub(super) fn run_stmts_flow(
        &mut self,
        stmts: &[IrStmt],
        locals: &mut HashMap<String, RuntimeValue>,
    ) -> (RuntimeValue, FlowControl) {
        let mut last = RuntimeValue::Unit;
        for stmt in stmts {
            let result = self.run_stmt_flow(stmt, locals);
            if self.trap.is_some() {
                return (RuntimeValue::Unit, FlowControl::Return);
            }
            match result {
                StmtResult {
                    value,
                    flow: FlowControl::None,
                } => last = value,
                StmtResult {
                    value,
                    flow: FlowControl::Return,
                } => return (value, FlowControl::Return),
                StmtResult { value, flow } => return (value, flow),
            }
        }
        (last, FlowControl::None)
    }

    pub(super) fn run_stmt(
        &mut self,
        stmt: &IrStmt,
        locals: &mut HashMap<String, RuntimeValue>,
    ) -> RuntimeValue {
        self.run_stmt_flow(stmt, locals).value
    }

    fn run_stmt_flow(
        &mut self,
        stmt: &IrStmt,
        locals: &mut HashMap<String, RuntimeValue>,
    ) -> StmtResult {
        match stmt {
            IrStmt::Assign { target, expr } => {
                let value = self.eval_expr(expr, locals);
                if self.trap.is_some() {
                    return TRAPPED;
                }
                if self.is_source_binding(target) {
                    self.set_source(target, value.clone());
                } else {
                    locals.insert(target.clone(), value.clone());
                }
                StmtResult {
                    value,
                    flow: FlowControl::None,
                }
            }
            IrStmt::Let { name, expr } => {
                let value = self.eval_expr(expr, locals);
                if self.trap.is_some() {
                    return TRAPPED;
                }
                locals.insert(name.clone(), value.clone());
                StmtResult {
                    value,
                    flow: FlowControl::None,
                }
            }
            IrStmt::Emit { event, arg } => {
                let payload = arg
                    .as_ref()
                    .map(|c| self.eval_expr(c, locals))
                    .unwrap_or(RuntimeValue::Unit);
                if self.trap.is_some() {
                    return TRAPPED;
                }
                self.emit(event, payload);
                StmtResult {
                    value: RuntimeValue::Unit,
                    flow: FlowControl::None,
                }
            }
            IrStmt::Expr { code } => StmtResult {
                value: self.eval_expr(code, locals),
                flow: FlowControl::None,
            },
            IrStmt::Return { expr } => {
                let value = expr
                    .as_ref()
                    .map(|c| self.eval_expr(c, locals))
                    .unwrap_or(RuntimeValue::Unit);
                StmtResult {
                    value,
                    flow: FlowControl::Return,
                }
            }
            IrStmt::Break => StmtResult {
                value: RuntimeValue::Unit,
                flow: FlowControl::Break,
            },
            IrStmt::Continue => StmtResult {
                value: RuntimeValue::Unit,
                flow: FlowControl::Continue,
            },
            IrStmt::If {
                cond,
                then_body,
                else_body,
            } => {
                let cond = self.eval_expr(cond, locals);
                // A failed condition is not `false`: run neither branch.
                if self.trap.is_some() {
                    return TRAPPED;
                }
                if cond.as_bool().unwrap_or(false) {
                    self.run_stmts_flow(then_body, locals)
                } else if let Some(else_body) = else_body {
                    self.run_stmts_flow(else_body, locals)
                } else {
                    (RuntimeValue::Unit, FlowControl::None)
                }
                .into()
            }
            IrStmt::While { cond, body } => {
                let mut last = RuntimeValue::Unit;
                loop {
                    let keep_going = self.eval_expr(cond, locals);
                    if self.trap.is_some() {
                        return TRAPPED;
                    }
                    if !keep_going.as_bool().unwrap_or(false) {
                        break;
                    }
                    let result = self.run_stmts_flow(body, locals);
                    if self.trap.is_some() {
                        return TRAPPED;
                    }
                    match result {
                        (v, FlowControl::None) => last = v,
                        (v, FlowControl::Break) => {
                            last = v;
                            break;
                        }
                        (v, FlowControl::Continue) => {
                            last = v;
                            continue;
                        }
                        (v, FlowControl::Return) => {
                            return StmtResult {
                                value: v,
                                flow: FlowControl::Return,
                            };
                        }
                    }
                }
                StmtResult {
                    value: last,
                    flow: FlowControl::None,
                }
            }
            IrStmt::For { var, iter, body } => {
                let mut last = RuntimeValue::Unit;
                let items = self.eval_expr(iter, locals);
                if self.trap.is_some() {
                    return TRAPPED;
                }
                let items: Vec<RuntimeValue> = match items {
                    RuntimeValue::Array(items) => items,
                    _ => vec![],
                };
                'for_loop: for item in items {
                    locals.insert(var.clone(), item);
                    let result = self.run_stmts_flow(body, locals);
                    if self.trap.is_some() {
                        return TRAPPED;
                    }
                    match result {
                        (v, FlowControl::None) => last = v,
                        (v, FlowControl::Break) => {
                            last = v;
                            break 'for_loop;
                        }
                        (v, FlowControl::Continue) => {
                            last = v;
                            continue;
                        }
                        (v, FlowControl::Return) => {
                            return StmtResult {
                                value: v,
                                flow: FlowControl::Return,
                            };
                        }
                    }
                }
                StmtResult {
                    value: last,
                    flow: FlowControl::None,
                }
            }
        }
    }
}
