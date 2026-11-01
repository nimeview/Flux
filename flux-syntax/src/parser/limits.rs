//! Nesting limits. The parser and every later compiler pass recurse over the
//! syntax tree, so unbounded nesting would overflow the native stack (a
//! process abort) instead of producing a diagnostic.
//!
//! - [`lexical_nesting`] runs before the grammar, which recurses once per
//!   bracket or unary operator: at most [`MAX_BRACKET_NESTING`] open
//!   brackets, and at most [`MAX_UNARY_RUN`] consecutive `-`/`!`.
//! - [`tree_depth_exceeds`] runs on the parsed tree without recursion: long
//!   operator chains (`1 + 1 + …`) are flat in the source but deep in the tree.

use flux_diagnostics::{SourceId, Span};

use crate::ast::{Block, ElseBranch, Expr, IfStmt, Item, Program, Stmt, StringPart};

pub const MAX_BRACKET_NESTING: usize = 256;
pub const MAX_UNARY_RUN: usize = 256;
/// Deepest syntax tree accepted (as Python bounds its compiler's recursion).
pub const MAX_TREE_DEPTH: usize = 1000;

/// The first place where brackets nest, or unary operators repeat, beyond
/// the limits, with a description. Strings (and their `{…}` interpolation)
/// and `//` comments are followed exactly enough to count only code brackets.
pub(super) fn lexical_nesting(source_id: SourceId, input: &str) -> Option<(Span, String)> {
    #[derive(Clone, Copy, PartialEq)]
    enum Mode {
        Code,
        Str,
    }
    // Each open bracket remembers the mode to return to when it closes; an
    // interpolation `{` opens from string mode.
    let mut stack: Vec<Mode> = Vec::new();
    let mut mode = Mode::Code;
    let mut unary_run = 0usize;
    let bytes = input.as_bytes();
    let mut i = 0;
    let at = |i: usize| Span {
        source: source_id,
        start: i,
        end: i + 1,
    };
    while i < bytes.len() {
        let c = bytes[i];
        match mode {
            Mode::Str => match c {
                b'\\' => i += 1,
                b'"' => mode = stack.pop().unwrap_or(Mode::Code),
                b'{' => {
                    stack.push(Mode::Str);
                    mode = Mode::Code;
                    if stack.len() > MAX_BRACKET_NESTING {
                        return Some((at(i), too_deep()));
                    }
                }
                _ => {}
            },
            Mode::Code => {
                if c == b'/' && bytes.get(i + 1) == Some(&b'/') {
                    while i < bytes.len() && bytes[i] != b'\n' {
                        i += 1;
                    }
                    continue;
                }
                match c {
                    b'-' | b'!' => {
                        unary_run += 1;
                        if unary_run > MAX_UNARY_RUN {
                            return Some((
                                at(i),
                                format!(
                                    "more than {MAX_UNARY_RUN} `-`/`!` in a row; \
                                     simplify the expression"
                                ),
                            ));
                        }
                    }
                    c if c.is_ascii_whitespace() => {}
                    _ => unary_run = 0,
                }
                match c {
                    b'"' => {
                        stack.push(Mode::Code);
                        mode = Mode::Str;
                    }
                    b'(' | b'[' | b'{' => {
                        stack.push(Mode::Code);
                        if stack.len() > MAX_BRACKET_NESTING {
                            return Some((at(i), too_deep()));
                        }
                    }
                    b')' | b']' | b'}' => mode = stack.pop().unwrap_or(Mode::Code),
                    _ => {}
                }
            }
        }
        i += 1;
    }
    None
}

fn too_deep() -> String {
    format!(
        "brackets nest more than {MAX_BRACKET_NESTING} levels deep; \
         split the code into smaller functions or bindings"
    )
}

/// Whether the tree is deeper than [`MAX_TREE_DEPTH`] (iteratively), with the
/// span of an expression past the limit.
pub(super) fn tree_depth_exceeds(program: &Program) -> Option<Span> {
    enum Node<'a> {
        Expr(&'a Expr),
        Stmt(&'a Stmt),
        Block(&'a Block),
        If(&'a IfStmt),
    }
    let mut work: Vec<(Node<'_>, usize)> = Vec::new();
    for item in &program.items {
        match item {
            Item::Fn(f) => work.push((Node::Block(&f.body), 1)),
            Item::RFn(f) => work.push((Node::Block(&f.body), 1)),
            Item::Derived(d) => work.push((Node::Expr(&d.value), 1)),
            Item::Const(c) => work.push((Node::Expr(&c.value), 1)),
            Item::Let(l) => work.push((Node::Expr(&l.value), 1)),
            Item::Watch(w) => work.push((Node::Block(&w.body), 1)),
            Item::On(o) => work.push((Node::Block(&o.body), 1)),
            Item::If(i) => work.push((Node::If(i), 1)),
            Item::Impl(i) => {
                for m in &i.methods {
                    work.push((Node::Block(&m.body), 1));
                }
            }
            Item::Import(_)
            | Item::TypeAlias(_)
            | Item::Struct(_)
            | Item::Enum(_)
            | Item::Trait(_)
            | Item::Event(_) => {}
        }
    }
    while let Some((node, depth)) = work.pop() {
        let d = depth + 1;
        match node {
            Node::Expr(e) => {
                if depth > MAX_TREE_DEPTH {
                    // Point at where it starts: the whole expression may be a
                    // huge single line.
                    let span = e.span();
                    return Some(Span {
                        end: span.start + 1,
                        ..span
                    });
                }
                match e {
                    Expr::InterpolatedString { parts, .. } => {
                        for p in parts {
                            if let StringPart::Expr(x) = p {
                                work.push((Node::Expr(x), d));
                            }
                        }
                    }
                    Expr::Call { callee, args, .. } => {
                        work.push((Node::Expr(callee), d));
                        work.extend(args.iter().map(|a| (Node::Expr(a), d)));
                    }
                    Expr::Field { base: x, .. }
                    | Expr::Closure { body: x, .. }
                    | Expr::Unary { expr: x, .. }
                    | Expr::Cast { expr: x, .. }
                    | Expr::Try { expr: x, .. } => work.push((Node::Expr(x), d)),
                    Expr::Binary { left, right, .. } => {
                        work.push((Node::Expr(left), d));
                        work.push((Node::Expr(right), d));
                    }
                    Expr::If {
                        cond,
                        then_expr,
                        else_expr,
                        ..
                    } => {
                        work.push((Node::Expr(cond), d));
                        work.push((Node::Expr(then_expr), d));
                        work.push((Node::Expr(else_expr), d));
                    }
                    Expr::Array { elems, .. } => {
                        work.extend(elems.iter().map(|x| (Node::Expr(x), d)));
                    }
                    Expr::StructLit { fields, .. } => {
                        work.extend(fields.iter().map(|f| (Node::Expr(&f.value), d)));
                    }
                    Expr::Match {
                        scrutinee, arms, ..
                    } => {
                        work.push((Node::Expr(scrutinee), d));
                        for arm in arms {
                            if let Some(g) = &arm.guard {
                                work.push((Node::Expr(g), d));
                            }
                            work.extend(arm.stmts.iter().map(|s| (Node::Stmt(s), d)));
                            if let Some(x) = &arm.expr {
                                work.push((Node::Expr(x), d));
                            }
                        }
                    }
                    Expr::Int { .. }
                    | Expr::Float { .. }
                    | Expr::Bool { .. }
                    | Expr::String { .. }
                    | Expr::Ident { .. } => {}
                }
            }
            Node::Block(b) => work.extend(b.stmts.iter().map(|s| (Node::Stmt(s), d))),
            Node::If(i) => {
                work.push((Node::Expr(&i.cond), d));
                work.push((Node::Block(&i.then_branch), d));
                match &i.else_branch {
                    Some(ElseBranch::If(nested)) => work.push((Node::If(nested), d)),
                    Some(ElseBranch::Block(b)) => work.push((Node::Block(b), d)),
                    None => {}
                }
            }
            Node::Stmt(s) => match s {
                Stmt::Let(l) => work.push((Node::Expr(&l.value), d)),
                Stmt::Assign(a) => work.push((Node::Expr(&a.value), d)),
                Stmt::Emit(e) => work.extend(e.args.iter().map(|a| (Node::Expr(a), d))),
                Stmt::Return(r) => {
                    if let Some(v) = &r.value {
                        work.push((Node::Expr(v), d));
                    }
                }
                Stmt::Expr(e) => work.push((Node::Expr(e), d)),
                Stmt::If(i) => work.push((Node::If(i), d)),
                Stmt::While(w) => {
                    work.push((Node::Expr(&w.cond), d));
                    work.push((Node::Block(&w.body), d));
                }
                Stmt::For(f) => {
                    work.push((Node::Expr(&f.iter), d));
                    work.push((Node::Block(&f.body), d));
                }
                Stmt::Watch(w) => work.push((Node::Block(&w.body), d)),
                Stmt::On(o) => work.push((Node::Block(&o.body), d)),
                Stmt::Break(_) | Stmt::Continue(_) => {}
            },
        }
    }
    None
}
