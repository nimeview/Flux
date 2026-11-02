//! Statement execution with structured `return` / `break` / `continue` flow.

use flux_ir::IrStmt;

use crate::graph::RuntimeValue;
use crate::vm::debug_assert_vm;

use super::{Executor, FlowControl, Locals, StmtResult};

/// Result of a statement aborted by a runtime error: unwind like `return`.
const TRAPPED: StmtResult = StmtResult {
    value: RuntimeValue::Unit,
    flow: FlowControl::Return,
};

impl Executor {
    pub(super) fn run_stmts(&mut self, stmts: &[IrStmt], locals: &mut Locals) -> RuntimeValue {
        match self.run_stmts_flow(stmts, locals) {
            (v, FlowControl::None) => v,
            (v, _) => v,
        }
    }

    /// A statement cut short by its expression: a runtime error, or
    /// `return`/`break`/`continue` from a match arm block inside it.
    fn interrupted(&mut self) -> Option<StmtResult> {
        if self.trap.is_some() {
            return Some(TRAPPED);
        }
        self.pending_flow
            .take()
            .map(|(value, flow)| StmtResult { value, flow })
    }

    pub(super) fn run_stmts_flow(
        &mut self,
        stmts: &[IrStmt],
        locals: &mut Locals,
    ) -> (RuntimeValue, FlowControl) {
        let mut last = RuntimeValue::Unit;
        for stmt in stmts {
            // Release the previous statement's value first: holding it would
            // keep an array shared and force copy-on-write to copy.
            drop(std::mem::replace(&mut last, RuntimeValue::Unit));
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

    pub(super) fn run_stmt(&mut self, stmt: &IrStmt, locals: &mut Locals) -> RuntimeValue {
        self.run_stmt_flow(stmt, locals).value
    }

    fn run_stmt_flow(&mut self, stmt: &IrStmt, locals: &mut Locals) -> StmtResult {
        match stmt {
            IrStmt::Assign { target, slot, expr } => {
                let value = self.eval_expr(expr, locals);
                if let Some(stop) = self.interrupted() {
                    return stop;
                }
                // `resolve_locals` leaves only reactive sources without a slot.
                match slot {
                    Some(slot) => locals.set(*slot, value.clone()),
                    None => {
                        debug_assert_vm(
                            self.is_source_binding(target),
                            &format!("assignment to `{target}` has no slot"),
                        );
                        self.set_source(target, value.clone());
                    }
                }
                StmtResult {
                    value,
                    flow: FlowControl::None,
                }
            }
            IrStmt::Let { slot, expr, .. } => {
                let value = self.eval_expr(expr, locals);
                if let Some(stop) = self.interrupted() {
                    return stop;
                }
                locals.set(*slot, value.clone());
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
                if let Some(stop) = self.interrupted() {
                    return stop;
                }
                self.emit(event, payload);
                StmtResult {
                    value: RuntimeValue::Unit,
                    flow: FlowControl::None,
                }
            }
            IrStmt::Expr { code } => {
                let value = self.eval_expr(code, locals);
                if let Some(stop) = self.interrupted() {
                    return stop;
                }
                StmtResult {
                    value,
                    flow: FlowControl::None,
                }
            }
            IrStmt::Return { expr } => {
                let value = expr
                    .as_ref()
                    .map(|c| self.eval_expr(c, locals))
                    .unwrap_or(RuntimeValue::Unit);
                if let Some(stop) = self.interrupted() {
                    return stop;
                }
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
                if let Some(stop) = self.interrupted() {
                    return stop;
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
                    if let Some(stop) = self.interrupted() {
                        return stop;
                    }
                    if !keep_going.as_bool().unwrap_or(false) {
                        break;
                    }
                    drop(std::mem::replace(&mut last, RuntimeValue::Unit));
                    let result = self.run_stmts_flow(body, locals);
                    if let Some(stop) = self.interrupted() {
                        return stop;
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
            IrStmt::For {
                slot, iter, body, ..
            } => {
                let mut last = RuntimeValue::Unit;
                let items = self.eval_expr(iter, locals);
                if let Some(stop) = self.interrupted() {
                    return stop;
                }
                let items: Vec<RuntimeValue> = match items {
                    // The loop owns its elements; this copies only if the array
                    // is still shared (e.g. a named local), never on a temporary.
                    RuntimeValue::Array(items) => {
                        std::sync::Arc::try_unwrap(items).unwrap_or_else(|shared| (*shared).clone())
                    }
                    _ => vec![],
                };
                'for_loop: for item in items {
                    locals.set(*slot, item);
                    drop(std::mem::replace(&mut last, RuntimeValue::Unit));
                    let result = self.run_stmts_flow(body, locals);
                    if let Some(stop) = self.interrupted() {
                        return stop;
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
