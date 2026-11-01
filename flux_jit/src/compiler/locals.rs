//! Function-local bindings as Cranelift `Variable`s.
//!
//! A local reassigned inside a loop or a branch needs a different SSA value
//! on each path; `Variable`s let the frontend insert the block parameters.
//! (Storing a plain SSA `Value` per name fed the loop header its pre-loop value
//! forever, so `while i < n { i = i + 1 }` never ended.)

use std::collections::HashMap;

use cranelift::prelude::*;

#[derive(Default)]
pub(crate) struct JitLocals {
    vars: HashMap<String, (Variable, Variable)>,
}

impl JitLocals {
    pub(crate) fn contains(&self, name: &str) -> bool {
        self.vars.contains_key(name)
    }

    /// The current `(tag, data)` of `name` at the builder's position.
    pub(crate) fn get(&self, name: &str, b: &mut FunctionBuilder<'_>) -> Option<(Value, Value)> {
        let &(tag, data) = self.vars.get(name)?;
        Some((b.use_var(tag), b.use_var(data)))
    }

    /// Bind or reassign `name` at the builder's position.
    pub(crate) fn set(
        &mut self,
        name: &str,
        (tag, data): (Value, Value),
        b: &mut FunctionBuilder<'_>,
    ) {
        let &mut (tag_var, data_var) = self
            .vars
            .entry(name.to_string())
            .or_insert_with(|| (b.declare_var(types::I64), b.declare_var(types::I64)));
        b.def_var(tag_var, tag);
        b.def_var(data_var, data);
    }

    pub(crate) fn remove(&mut self, name: &str) {
        self.vars.remove(name);
    }
}
