//! Frame slots for locals.
//!
//! Each function, `on` handler, `watch`, derived-node expression, and the
//! module's initialization statements gets a frame. Parameters take slots
//! `0..n` in order; every other name the body binds (`let`, `for`, a pattern
//! variable, or an assignment to a name that is not a reactive source) gets
//! the next slot at its first binding. Loads and takes of those names become
//! [`Inst::LoadLocal`] / [`Inst::TakeLocal`].
//!
//! The VM behaves exactly as with name-keyed locals: a slot is unbound until
//! its binding runs, and an unbound slot reads its name as a global. Slots
//! are derived from names alone, so the pass is idempotent.

use std::collections::{HashMap, HashSet};

use crate::{CompiledProgram, Inst, IrStmt, NodeKind};

/// Assigns frame slots in every body of `program`; see the module docs.
pub fn resolve_locals(program: &mut CompiledProgram) {
    let sources: HashSet<String> = program
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Source)
        .map(|n| n.name.clone())
        .collect();
    for f in &mut program.functions {
        resolve_stmts(&f.params, &mut f.body, &sources);
    }
    for h in &mut program.on_handlers {
        resolve_stmts(h.param.as_slice(), &mut h.body, &sources);
    }
    for w in &mut program.watches {
        resolve_stmts(&[], &mut w.body, &sources);
    }
    resolve_stmts(&[], &mut program.init_stmts, &sources);
    for node in &mut program.nodes {
        let mut slots = Slots::new(&[], &sources);
        slots.collect_code(&node.code);
        slots.rewrite_code(&mut node.code);
    }
}

fn resolve_stmts(params: &[String], body: &mut [IrStmt], sources: &HashSet<String>) {
    let mut slots = Slots::new(params, sources);
    slots.collect_stmts(body);
    slots.rewrite_stmts(body);
}

struct Slots<'a> {
    map: HashMap<String, u32>,
    sources: &'a HashSet<String>,
}

impl<'a> Slots<'a> {
    fn new(params: &[String], sources: &'a HashSet<String>) -> Self {
        let mut slots = Self {
            map: HashMap::new(),
            sources,
        };
        for p in params {
            slots.bind(p);
        }
        slots
    }

    fn bind(&mut self, name: &str) {
        let next = self.map.len() as u32;
        self.map.entry(name.to_string()).or_insert(next);
    }

    fn slot(&self, name: &str) -> u32 {
        self.map[name]
    }

    fn collect_stmts(&mut self, stmts: &[IrStmt]) {
        for stmt in stmts {
            match stmt {
                IrStmt::Assign { target, expr, .. } => {
                    self.collect_code(expr);
                    if !self.sources.contains(target) {
                        self.bind(target);
                    }
                }
                IrStmt::Let { name, expr, .. } => {
                    self.collect_code(expr);
                    self.bind(name);
                }
                IrStmt::Emit { arg, .. } => {
                    if let Some(arg) = arg {
                        self.collect_code(arg);
                    }
                }
                IrStmt::Expr { code } => self.collect_code(code),
                IrStmt::Return { expr } => {
                    if let Some(expr) = expr {
                        self.collect_code(expr);
                    }
                }
                IrStmt::If {
                    cond,
                    then_body,
                    else_body,
                } => {
                    self.collect_code(cond);
                    self.collect_stmts(then_body);
                    if let Some(else_body) = else_body {
                        self.collect_stmts(else_body);
                    }
                }
                IrStmt::While { cond, body } => {
                    self.collect_code(cond);
                    self.collect_stmts(body);
                }
                IrStmt::For {
                    var, iter, body, ..
                } => {
                    self.collect_code(iter);
                    self.bind(var);
                    self.collect_stmts(body);
                }
                IrStmt::Break | IrStmt::Continue => {}
            }
        }
    }

    fn collect_code(&mut self, code: &[Inst]) {
        for inst in code {
            match inst {
                Inst::BindLocal { name, .. } => self.bind(name),
                Inst::Block(stmts) => self.collect_stmts(stmts),
                Inst::ShortAnd(inner) | Inst::ShortOr(inner) => self.collect_code(inner),
                Inst::LazyIf {
                    then_branch,
                    else_branch,
                } => {
                    self.collect_code(then_branch);
                    self.collect_code(else_branch);
                }
                _ => {}
            }
        }
    }

    fn rewrite_stmts(&self, stmts: &mut [IrStmt]) {
        for stmt in stmts {
            match stmt {
                IrStmt::Assign { target, slot, expr } => {
                    self.rewrite_code(expr);
                    // A source is assigned by name even if a local shares
                    // the name: the VM checks sources first.
                    *slot = if self.sources.contains(target) {
                        None
                    } else {
                        self.map.get(target.as_str()).copied()
                    };
                }
                IrStmt::Let { name, slot, expr } => {
                    self.rewrite_code(expr);
                    *slot = self.slot(name);
                }
                IrStmt::Emit { arg, .. } => {
                    if let Some(arg) = arg {
                        self.rewrite_code(arg);
                    }
                }
                IrStmt::Expr { code } => self.rewrite_code(code),
                IrStmt::Return { expr } => {
                    if let Some(expr) = expr {
                        self.rewrite_code(expr);
                    }
                }
                IrStmt::If {
                    cond,
                    then_body,
                    else_body,
                } => {
                    self.rewrite_code(cond);
                    self.rewrite_stmts(then_body);
                    if let Some(else_body) = else_body {
                        self.rewrite_stmts(else_body);
                    }
                }
                IrStmt::While { cond, body } => {
                    self.rewrite_code(cond);
                    self.rewrite_stmts(body);
                }
                IrStmt::For {
                    var,
                    slot,
                    iter,
                    body,
                } => {
                    self.rewrite_code(iter);
                    *slot = self.slot(var);
                    self.rewrite_stmts(body);
                }
                IrStmt::Break | IrStmt::Continue => {}
            }
        }
    }

    fn rewrite_code(&self, code: &mut [Inst]) {
        for inst in code {
            match inst {
                Inst::Load(name) | Inst::LoadLocal { name, .. } => {
                    if let Some(&slot) = self.map.get(name.as_str()) {
                        let name = std::mem::take(name);
                        *inst = Inst::LoadLocal { slot, name };
                    }
                }
                Inst::Take(name) | Inst::TakeLocal { name, .. } => {
                    if let Some(&slot) = self.map.get(name.as_str()) {
                        let name = std::mem::take(name);
                        *inst = Inst::TakeLocal { slot, name };
                    }
                }
                Inst::BindLocal { name, slot } => *slot = self.slot(name),
                Inst::Block(stmts) => self.rewrite_stmts(stmts),
                Inst::ShortAnd(inner) | Inst::ShortOr(inner) => self.rewrite_code(inner),
                Inst::LazyIf {
                    then_branch,
                    else_branch,
                } => {
                    self.rewrite_code(then_branch);
                    self.rewrite_code(else_branch);
                }
                _ => {}
            }
        }
    }
}
