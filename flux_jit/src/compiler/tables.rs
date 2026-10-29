use std::collections::HashMap;

use flux_ir::{CompiledProgram, Inst, IrStmt, IrValue};

use flux_value::intern_string as heap_intern_string;

/// Compile-time intern tables passed into Cranelift lowering.
#[derive(Debug, Clone, Default)]
pub struct JitTables {
    pub strings: HashMap<String, u64>,
}

fn intern_string(strings: &mut HashMap<String, u64>, s: &str) {
    strings
        .entry(s.to_string())
        .or_insert_with(|| heap_intern_string(s));
}

fn walk_code(strings: &mut HashMap<String, u64>, code: &[Inst]) {
    for inst in code {
        match inst {
            Inst::Const(IrValue::String(s)) => intern_string(strings, s),
            Inst::MakeUserEnum {
                type_name, variant, ..
            } => {
                intern_string(strings, type_name);
                intern_string(strings, variant);
            }
            Inst::VariantTagEq(v) | Inst::EnumTagEq(v) => intern_string(strings, v),
            Inst::ShortAnd(inner) | Inst::ShortOr(inner) => walk_code(strings, inner),
            Inst::LazyIf {
                then_branch,
                else_branch,
            } => {
                walk_code(strings, then_branch);
                walk_code(strings, else_branch);
            }
            _ => {}
        }
    }
}

fn walk_stmts(strings: &mut HashMap<String, u64>, stmts: &[IrStmt]) {
    for stmt in stmts {
        match stmt {
            IrStmt::Expr { code }
            | IrStmt::Let { expr: code, .. }
            | IrStmt::Assign { expr: code, .. } => walk_code(strings, code),
            IrStmt::Emit {
                arg: Some(code), ..
            } => walk_code(strings, code),
            IrStmt::If {
                cond,
                then_body,
                else_body,
            } => {
                walk_code(strings, cond);
                walk_stmts(strings, then_body);
                if let Some(b) = else_body {
                    walk_stmts(strings, b);
                }
            }
            IrStmt::While { cond, body } => {
                walk_code(strings, cond);
                walk_stmts(strings, body);
            }
            IrStmt::For { iter, body, .. } => {
                walk_code(strings, iter);
                walk_stmts(strings, body);
            }
            IrStmt::Return { expr: Some(code) } => walk_code(strings, code),
            _ => {}
        }
    }
}

impl JitTables {
    pub fn build(program: &CompiledProgram) -> Self {
        let mut strings = HashMap::new();

        for node in &program.nodes {
            walk_code(&mut strings, &node.code);
        }
        for func in &program.functions {
            walk_stmts(&mut strings, &func.body);
        }
        for watch in &program.watches {
            walk_stmts(&mut strings, &watch.body);
        }
        for handler in &program.on_handlers {
            walk_stmts(&mut strings, &handler.body);
        }

        Self { strings }
    }

    pub fn string_id(&self, s: &str) -> Option<u64> {
        self.strings.get(s).copied()
    }
}
