//! Which functions may read reactive values, directly or through calls.
//!
//! Compiled code reads a copy of the graph's values taken when it starts.
//! While a write has left some value stale, such a function runs on the VM,
//! which brings what it reads up to date first; and a compiled body that
//! writes a source and then calls one falls back.

use std::collections::{HashMap, HashSet};

use crate::{CompiledProgram, Inst, IrStmt};

/// Names of the functions that may read a graph node: they name one, call
/// a function that does, or call a function value (which could be anything).
pub fn graph_readers(program: &CompiledProgram, is_node: impl Fn(&str) -> bool) -> HashSet<String> {
    let mut uses: HashMap<&str, HashSet<&str>> = HashMap::new();
    let mut readers = HashSet::new();
    for func in &program.functions {
        let mut names = HashSet::new();
        let mut dynamic = false;
        stmts_names(&func.body, &mut names, &mut dynamic);
        if dynamic || names.iter().any(|n| is_node(n)) {
            readers.insert(func.name.clone());
        }
        uses.insert(func.name.as_str(), names);
    }
    // A function that names a reader reads too, until nothing changes.
    loop {
        let before = readers.len();
        for (name, names) in &uses {
            if !readers.contains(*name) && names.iter().any(|n| readers.contains(*n)) {
                readers.insert(name.to_string());
            }
        }
        if readers.len() == before {
            return readers;
        }
    }
}

fn stmts_names<'a>(stmts: &'a [IrStmt], names: &mut HashSet<&'a str>, dynamic: &mut bool) {
    for stmt in stmts {
        match stmt {
            IrStmt::Assign { expr, .. }
            | IrStmt::Let { expr, .. }
            | IrStmt::Expr { code: expr } => code_names(expr, names, dynamic),
            IrStmt::Emit { args, .. } => {
                for code in args {
                    code_names(code, names, dynamic);
                }
            }
            IrStmt::Return { expr: arg } => {
                if let Some(code) = arg {
                    code_names(code, names, dynamic);
                }
            }
            IrStmt::If {
                cond,
                then_body,
                else_body,
            } => {
                code_names(cond, names, dynamic);
                stmts_names(then_body, names, dynamic);
                if let Some(body) = else_body {
                    stmts_names(body, names, dynamic);
                }
            }
            IrStmt::While { cond, body } => {
                code_names(cond, names, dynamic);
                stmts_names(body, names, dynamic);
            }
            IrStmt::For { iter, body, .. } => {
                code_names(iter, names, dynamic);
                stmts_names(body, names, dynamic);
            }
            IrStmt::Break | IrStmt::Continue => {}
        }
    }
}

fn code_names<'a>(code: &'a [Inst], names: &mut HashSet<&'a str>, dynamic: &mut bool) {
    for inst in code {
        match inst {
            // An unbound local slot reads its name as a global.
            Inst::Load(name)
            | Inst::LoadLocal { name, .. }
            | Inst::Take(name)
            | Inst::TakeLocal { name, .. }
            | Inst::Call { name, .. }
            | Inst::MakeClosure { name, .. } => {
                names.insert(name.as_str());
            }
            Inst::CallValue { .. } => *dynamic = true,
            Inst::Block(stmts) => stmts_names(stmts, names, dynamic),
            Inst::ShortAnd(inner) | Inst::ShortOr(inner) => code_names(inner, names, dynamic),
            Inst::LazyIf {
                then_branch,
                else_branch,
            } => {
                code_names(then_branch, names, dynamic);
                code_names(else_branch, names, dynamic);
            }
            _ => {}
        }
    }
}
