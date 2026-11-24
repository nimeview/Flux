//! Transitive side effects and `snap` reads of functions, used to keep derived
//! values and `const` initializers pure. It runs after type inference, so a
//! method call counts the method the checker resolved.
//!
//! A function counts when it runs: called, or passed to a call, which may
//! call it. Storing a function in a local, a field, or a tuple only holds it,
//! and the holder is followed to where it is called: locals (including
//! `match` and `for` bindings, closure captures, and handler parameters, which
//! hold what `emit` passes), module bindings (their initializer and every
//! assignment), and the functions a call returns. A struct or tuple holds all
//! of its parts' functions together, whichever part is used.

use std::collections::{HashMap, HashSet};

use flux_diagnostics::Span;
use flux_hir::{
    HirBlock, HirElse, HirExpr, HirIf, HirItem, HirProgram, HirStmt, SymbolId, SymbolKind,
};

/// Method calls the type checker resolved, by enclosing function and span.
pub(super) type MethodCalls = HashMap<(Option<SymbolId>, Span), SymbolId>;

/// A function or holder, or what a function returns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Node {
    /// Running this function, or a function this binding holds.
    Sym(SymbolId),
    /// Running a function that this function returns.
    Ret(SymbolId),
}

pub(super) type Held = HashSet<Node>;

/// Which nodes have side effects, or read a value with `snap`, when run.
pub(super) struct Purity {
    pub effectful: HashSet<Node>,
    pub snapshotting: HashSet<Node>,
}

/// What flows between items: the functions each closure captures (by
/// capture position) and each event carries (by value position).
#[derive(Default, PartialEq)]
struct Flow {
    captures: HashMap<SymbolId, Vec<Held>>,
    emitted: HashMap<SymbolId, Vec<Held>>,
}

pub(super) fn analyze(
    program: &HirProgram,
    builtin_symbols: &HashSet<SymbolId>,
    method_calls: &MethodCalls,
) -> Purity {
    // Closures and handlers receive what other items give them; repeat until
    // that settles (nested closures take one round per level).
    let mut flow = Flow::default();
    let mut result = scan_items(program, builtin_symbols, method_calls, &flow);
    for _ in 0..8 {
        if result.flow == flow {
            break;
        }
        flow = std::mem::take(&mut result.flow);
        result = scan_items(program, builtin_symbols, method_calls, &flow);
    }
    let mut effectful: HashSet<Node> = builtin_symbols
        .iter()
        .copied()
        .filter(|id| is_effect_builtin(*id, &program.names, builtin_symbols))
        .map(Node::Sym)
        .collect();
    effectful.extend(result.effectful);
    let mut snapshotting = result.snapshotting;
    spread(&result.graph, &mut effectful);
    spread(&result.graph, &mut snapshotting);
    Purity {
        effectful,
        snapshotting,
    }
}

struct Scanned {
    graph: HashMap<Node, Held>,
    effectful: HashSet<Node>,
    snapshotting: HashSet<Node>,
    flow: Flow,
}

fn scan_items(
    program: &HirProgram,
    builtins: &HashSet<SymbolId>,
    method_calls: &MethodCalls,
    flow: &Flow,
) -> Scanned {
    let mut out = Scanned {
        graph: HashMap::new(),
        effectful: HashSet::new(),
        snapshotting: HashSet::new(),
        flow: Flow::default(),
    };
    for item in &program.items {
        let owner = match item {
            HirItem::Fn(function) => Some(function.name),
            _ => None,
        };
        let mut scan = Scanner {
            program,
            builtins,
            method_calls,
            owner,
            calls: HashSet::new(),
            returns: HashSet::new(),
            aliases: HashMap::new(),
            assigned: HashMap::new(),
            flow: Flow::default(),
            effect: false,
            snap: false,
        };
        match item {
            HirItem::Fn(function) => {
                // A closure's captured locals hold what the creating code's did.
                if let Some(held) = flow.captures.get(&function.name) {
                    for (local, held) in function.captures.iter().zip(held) {
                        scan.aliases.insert(*local, held.clone());
                    }
                }
                scan.block(&function.body, true);
                let node = Node::Sym(function.name);
                out.graph
                    .entry(node)
                    .or_default()
                    .extend(scan.calls.drain());
                out.graph
                    .entry(Node::Ret(function.name))
                    .or_default()
                    .extend(scan.returns.drain());
                if scan.effect {
                    out.effectful.insert(node);
                }
                if scan.snap {
                    out.snapshotting.insert(node);
                }
            }
            HirItem::Binding(binding) => {
                let held = scan.held(&binding.value);
                out.graph
                    .entry(Node::Sym(binding.name))
                    .or_default()
                    .extend(held);
            }
            HirItem::SourceAssign(assign) => {
                let held = scan.held(&assign.value);
                scan.assigned.entry(assign.name).or_default().extend(held);
            }
            HirItem::On(on) => {
                // A handler's parameters hold what `emit` passes.
                if let Some(held) = flow.emitted.get(&on.event) {
                    for (param, held) in on.params.iter().zip(held) {
                        scan.aliases.insert(*param, held.clone());
                    }
                }
                scan.block(&on.body, false);
            }
            HirItem::Watch(watch) => scan.block(&watch.body, false),
            HirItem::InitIf(branch) => scan.if_stmt(branch, false),
            HirItem::Struct(_)
            | HirItem::Enum(_)
            | HirItem::Trait(_)
            | HirItem::Impl(_)
            | HirItem::Event(_) => {}
        }
        for (binding, held) in scan.assigned {
            out.graph
                .entry(Node::Sym(binding))
                .or_default()
                .extend(held);
        }
        merge(&mut out.flow.captures, scan.flow.captures);
        merge(&mut out.flow.emitted, scan.flow.emitted);
    }
    out
}

fn merge(into: &mut HashMap<SymbolId, Vec<Held>>, from: HashMap<SymbolId, Vec<Held>>) {
    for (key, parts) in from {
        let slots = into.entry(key).or_default();
        if slots.len() < parts.len() {
            slots.resize(parts.len(), Held::new());
        }
        for (slot, part) in slots.iter_mut().zip(parts) {
            slot.extend(part);
        }
    }
}

/// Adds to `marked` every node that reaches a marked one, directly or not.
fn spread(graph: &HashMap<Node, Held>, marked: &mut HashSet<Node>) {
    loop {
        let newly: Vec<_> = graph
            .iter()
            .filter(|(node, next)| {
                !marked.contains(node) && next.iter().any(|n| marked.contains(n))
            })
            .map(|(node, _)| *node)
            .collect();
        if newly.is_empty() {
            return;
        }
        marked.extend(newly);
    }
}

/// Whether `symbol` is a library function with side effects (`println`).
fn is_effect_builtin(
    symbol: SymbolId,
    names: &HashMap<SymbolId, String>,
    builtin_symbols: &HashSet<SymbolId>,
) -> bool {
    builtin_symbols.contains(&symbol)
        && names
            .get(&symbol)
            .is_some_and(|name| flux_std::SIDE_EFFECT_BUILTINS.contains(&name.as_str()))
}

/// The hidden builtins that only build a value from their arguments.
pub(super) fn is_constructor(
    symbol: SymbolId,
    program: &HirProgram,
    builtins: &HashSet<SymbolId>,
) -> bool {
    builtins.contains(&symbol)
        && program.names.get(&symbol).is_some_and(|name| {
            [
                flux_hir::TUPLE,
                flux_hir::TUPLE_CHECK,
                flux_hir::WITH_FIELDS,
                flux_hir::GROUPED,
            ]
            .contains(&name.as_str())
        })
}

/// What running `symbol` runs: the functions a local holds, or itself.
pub(super) fn targets(symbol: SymbolId, aliases: &HashMap<SymbolId, Held>) -> Held {
    match aliases.get(&symbol) {
        Some(held) => held.clone(),
        None => HashSet::from([Node::Sym(symbol)]),
    }
}

/// What the result of running `nodes` holds: the functions they return.
pub(super) fn returned(nodes: &Held) -> Held {
    nodes
        .iter()
        .filter_map(|node| match node {
            Node::Sym(id) => Some(Node::Ret(*id)),
            Node::Ret(_) => None,
        })
        .collect()
}

/// The functions `expr`'s value holds, following locals in `aliases`.
/// A call's value holds what its function returns.
pub(super) fn held(
    expr: &HirExpr,
    aliases: &HashMap<SymbolId, Held>,
    program: &HirProgram,
    builtins: &HashSet<SymbolId>,
    method_calls: &MethodCalls,
    owner: Option<SymbolId>,
) -> Held {
    let each = |e: &HirExpr| held(e, aliases, program, builtins, method_calls, owner);
    match expr {
        HirExpr::Ident { symbol, .. } => targets(*symbol, aliases),
        HirExpr::Closure { fn_symbol, .. } => HashSet::from([Node::Sym(*fn_symbol)]),
        HirExpr::Array { elems, .. } => elems.iter().flat_map(each).collect(),
        HirExpr::StructLit { fields, .. } => fields.iter().flat_map(|(_, e)| each(e)).collect(),
        HirExpr::EnumVariant {
            payload: Some(value),
            ..
        } => each(value),
        HirExpr::If {
            then_expr,
            else_expr,
            ..
        } => each(then_expr).into_iter().chain(each(else_expr)).collect(),
        HirExpr::Match { arms, .. } => arms.iter().flat_map(|a| each(&a.expr)).collect(),
        HirExpr::Index { base: inner, .. }
        | HirExpr::Field { base: inner, .. }
        | HirExpr::Snap { expr: inner, .. }
        | HirExpr::Try { expr: inner, .. } => each(inner),
        HirExpr::Call { callee, args, span } => match callee.as_ref() {
            HirExpr::Ident { symbol, .. } if is_constructor(*symbol, program, builtins) => {
                args.iter().flat_map(each).collect()
            }
            HirExpr::Ident { symbol, .. } => returned(&targets(*symbol, aliases)),
            HirExpr::Field { .. } => match method_calls.get(&(owner, *span)) {
                Some(method) => HashSet::from([Node::Ret(*method)]),
                None => returned(&each(callee)),
            },
            other => returned(&each(other)),
        },
        HirExpr::Int { .. }
        | HirExpr::Float { .. }
        | HirExpr::Bool { .. }
        | HirExpr::String { .. }
        | HirExpr::EnumVariant { payload: None, .. }
        | HirExpr::Unary { .. }
        | HirExpr::Binary { .. }
        | HirExpr::Cast { .. }
        | HirExpr::ToText { .. } => HashSet::new(),
    }
}

struct Scanner<'a> {
    program: &'a HirProgram,
    builtins: &'a HashSet<SymbolId>,
    method_calls: &'a MethodCalls,
    owner: Option<SymbolId>,
    /// What this code runs.
    calls: Held,
    /// The functions this code returns.
    returns: Held,
    /// The functions each local holds.
    aliases: HashMap<SymbolId, Held>,
    /// The functions this code assigns to each module binding.
    assigned: HashMap<SymbolId, Held>,
    /// What this code gives closures and events.
    flow: Flow,
    effect: bool,
    snap: bool,
}

impl Scanner<'_> {
    fn held(&self, expr: &HirExpr) -> Held {
        held(
            expr,
            &self.aliases,
            self.program,
            self.builtins,
            self.method_calls,
            self.owner,
        )
    }

    fn run(&mut self, nodes: Held) {
        for node in &nodes {
            if let Node::Sym(id) = node {
                if is_effect_builtin(*id, &self.program.names, self.builtins) {
                    self.effect = true;
                }
            }
        }
        self.calls.extend(nodes);
    }

    fn block(&mut self, block: &HirBlock, returns: bool) {
        let last = block.stmts.len().saturating_sub(1);
        for (i, stmt) in block.stmts.iter().enumerate() {
            self.stmt(stmt, returns && i == last);
        }
    }

    fn stmt(&mut self, stmt: &HirStmt, returns: bool) {
        match stmt {
            HirStmt::Let(binding) => {
                self.expr(&binding.value, false);
                let held = self.held(&binding.value);
                self.aliases.insert(binding.name, held);
            }
            HirStmt::Assign(assign) => {
                for index in assign.indices() {
                    self.expr(index, false);
                }
                self.expr(&assign.value, false);
                let held = self.held(&assign.value);
                let module =
                    self.program.symbols.get(&assign.name).is_some_and(|s| {
                        matches!(s.kind, SymbolKind::Source | SymbolKind::Derived)
                    });
                if module {
                    self.effect = true;
                    self.assigned.entry(assign.name).or_default().extend(held);
                } else {
                    self.aliases.entry(assign.name).or_default().extend(held);
                }
            }
            HirStmt::Emit(emit) => {
                self.effect = true;
                let parts: Vec<Held> = emit.args.iter().map(|a| self.held(a)).collect();
                merge(&mut self.flow.emitted, HashMap::from([(emit.event, parts)]));
                for arg in &emit.args {
                    self.expr(arg, false);
                }
            }
            HirStmt::Return(ret) => {
                if let Some(value) = &ret.value {
                    self.give_back(value);
                }
            }
            HirStmt::Expr(expr) if returns => self.give_back(expr),
            HirStmt::Expr(expr) => self.expr(expr, false),
            HirStmt::If(branch) => self.if_stmt(branch, returns),
            HirStmt::While(loop_stmt) => {
                self.expr(&loop_stmt.cond, false);
                self.block(&loop_stmt.body, false);
            }
            HirStmt::For(loop_stmt) => {
                self.expr(&loop_stmt.iter, false);
                let elems = self.held(&loop_stmt.iter);
                self.aliases.insert(loop_stmt.name, elems);
                self.block(&loop_stmt.body, false);
            }
            HirStmt::Watch(_) | HirStmt::On(_) => self.effect = true,
            HirStmt::Break(_) | HirStmt::Continue(_) => {}
        }
    }

    /// `expr` is the value this function returns.
    fn give_back(&mut self, expr: &HirExpr) {
        self.expr(expr, false);
        let held = self.held(expr);
        self.returns.extend(held);
    }

    fn if_stmt(&mut self, branch: &HirIf, returns: bool) {
        self.expr(&branch.cond, false);
        self.block(&branch.then_branch, returns);
        match &branch.else_branch {
            Some(HirElse::If(nested)) => self.if_stmt(nested, returns),
            Some(HirElse::Block(block)) => self.block(block, returns),
            None => {}
        }
    }

    /// `runs`: whether a function that is `expr`'s value is called here (as
    /// a callee or an argument).
    fn expr(&mut self, expr: &HirExpr, runs: bool) {
        match expr {
            HirExpr::Ident { symbol, .. } => {
                if runs {
                    let targets = targets(*symbol, &self.aliases);
                    self.run(targets);
                }
            }
            HirExpr::Closure {
                fn_symbol,
                captures,
                ..
            } => {
                let held: Vec<Held> = captures
                    .iter()
                    .map(|c| targets(*c, &self.aliases))
                    .collect();
                merge(&mut self.flow.captures, HashMap::from([(*fn_symbol, held)]));
                if runs {
                    self.run(HashSet::from([Node::Sym(*fn_symbol)]));
                }
            }
            HirExpr::Snap { expr: inner, .. } => {
                self.snap = true;
                self.expr(inner, runs);
            }
            HirExpr::Call { callee, args, span } => {
                if let HirExpr::Ident { symbol, .. } = callee.as_ref() {
                    if is_constructor(*symbol, self.program, self.builtins) {
                        args.iter().for_each(|a| self.expr(a, runs));
                        return;
                    }
                }
                let ran = match callee.as_ref() {
                    HirExpr::Ident { symbol, .. } => targets(*symbol, &self.aliases),
                    HirExpr::Field { base, .. } => {
                        match self.method_calls.get(&(self.owner, *span)) {
                            Some(method) => {
                                self.expr(base, false);
                                HashSet::from([Node::Sym(*method)])
                            }
                            // Not a method: `op.f(x)` calls the function in
                            // field `f`.
                            None => {
                                self.expr(callee, true);
                                HashSet::new()
                            }
                        }
                    }
                    other => {
                        self.expr(other, true);
                        HashSet::new()
                    }
                };
                // Calling the result runs what the function returns.
                if runs {
                    self.run(returned(&ran));
                }
                self.run(ran);
                // A function passed along may be called by whatever receives it.
                args.iter().for_each(|a| self.expr(a, true));
            }
            HirExpr::Unary { expr: inner, .. }
            | HirExpr::Cast { expr: inner, .. }
            | HirExpr::ToText { expr: inner, .. } => self.expr(inner, false),
            HirExpr::Try { expr: inner, .. } | HirExpr::Field { base: inner, .. } => {
                self.expr(inner, runs)
            }
            HirExpr::Index { base, index, .. } => {
                self.expr(base, runs);
                self.expr(index, false);
            }
            HirExpr::Binary { left, right, .. } => {
                self.expr(left, false);
                self.expr(right, false);
            }
            HirExpr::If {
                cond,
                then_expr,
                else_expr,
                ..
            } => {
                self.expr(cond, false);
                self.expr(then_expr, runs);
                self.expr(else_expr, runs);
            }
            HirExpr::Array { elems, .. } => elems.iter().for_each(|e| self.expr(e, runs)),
            HirExpr::StructLit { fields, .. } => {
                fields.iter().for_each(|(_, e)| self.expr(e, runs))
            }
            HirExpr::EnumVariant { payload, .. } => {
                if let Some(value) = payload {
                    self.expr(value, runs);
                }
            }
            HirExpr::Match {
                scrutinee, arms, ..
            } => {
                self.expr(scrutinee, false);
                // What an arm binds is part of what the scrutinee holds.
                let parts = self.held(scrutinee);
                for arm in arms {
                    for binding in &arm.bindings {
                        self.aliases.insert(*binding, parts.clone());
                    }
                    if let Some(guard) = &arm.guard {
                        self.expr(guard, false);
                    }
                    self.block(&arm.body, false);
                    self.expr(&arm.expr, runs);
                }
            }
            HirExpr::Int { .. }
            | HirExpr::Float { .. }
            | HirExpr::Bool { .. }
            | HirExpr::String { .. } => {}
        }
    }
}
