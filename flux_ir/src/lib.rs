mod codegen;
mod lower;
mod resolve;

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

pub use codegen::{Codegen, IrCodegen};
pub use lower::lower_to_ir;
pub use resolve::resolve_locals;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NodeId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeKind {
    Source,
    Derived,
    /// Declared event (`event Click`).
    Event,
    /// Side-effect node (`watch` / `on` handler).
    Effect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HandlerKind {
    Watch,
    On,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EffectTrigger {
    /// Runs when the named event is emitted.
    OnEvent(String),
    /// Runs when reactive bindings change (`watch` / `watch target`).
    Watch { target: Option<String> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectMeta {
    pub trigger: EffectTrigger,
    pub handler_index: usize,
    pub handler_kind: HandlerKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IrNode {
    pub name: String,
    pub kind: NodeKind,
    pub deps: Vec<NodeId>,
    pub init: Option<IrValue>,
    pub code: Vec<Inst>,
    /// Present for [`NodeKind::Effect`] nodes.
    pub effect: Option<EffectMeta>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IrValue {
    Int(i128),
    Float(f64),
    Bool(bool),
    String(String),
    Unit,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Inst {
    Const(IrValue),
    /// A binding, function, or builtin by name. Locals are `LoadLocal` once
    /// [`resolve_locals`] has run.
    Load(String),
    /// A local's frame slot. An unbound slot (a `let` not yet run) reads
    /// `name` as a global instead, exactly like a name-based lookup.
    LoadLocal {
        slot: u32,
        name: String,
    },
    /// Moves a local out (it is unbound until reassigned). Only emitted for
    /// `x = push(x, v)` / `x = set(x, i, v)` when the other arguments do not
    /// read `x`, so the array is not shared and is updated in place.
    Take(String),
    /// `Take` of a local's frame slot; resolved like `LoadLocal`.
    TakeLocal {
        slot: u32,
        name: String,
    },
    /// Statements run in the current locals, pushing nothing: a match arm
    /// block with control flow. A `return`/`break`/`continue` inside leaves
    /// the expression and acts on the enclosing function or loop.
    Block(Vec<IrStmt>),
    /// `?`: replaces `Ok(v)`/`Some(v)` with `v`; on `Err`/`None` returns that
    /// value from the enclosing function.
    Try,
    /// `xs[i]`: pops the index, then the array; pushes the element, or fails
    /// with `IndexOutOfBounds` outside `0..len`.
    Index,
    Add,
    Sub,
    Mul,
    Div,
    /// Integer remainder (sign of the dividend) or IEEE float remainder.
    Rem,
    Neg,
    Not,
    CmpEq,
    CmpNe,
    CmpLt,
    CmpLe,
    CmpGt,
    CmpGe,
    ShortAnd(Vec<Inst>),
    ShortOr(Vec<Inst>),
    /// Lazy conditional: condition on stack; only the taken branch runs.
    LazyIf {
        then_branch: Vec<Inst>,
        else_branch: Vec<Inst>,
    },
    Select,
    MakeArray(usize),
    EnumTagEq(String),
    VariantTagEq(String),
    VariantPayloadGet {
        field: Option<String>,
    },
    /// Binds a pattern variable; `slot` is assigned by [`resolve_locals`].
    BindLocal {
        name: String,
        slot: u32,
    },
    MakeUserEnum {
        type_name: String,
        variant: String,
        has_payload: bool,
    },
    MakeUserStruct {
        type_name: String,
        fields: Vec<String>,
    },
    FieldGet(String),
    /// Pops `captures` values and pushes a closure over function `name`,
    /// which takes them after its own parameters.
    MakeClosure {
        name: String,
        captures: usize,
    },
    Call {
        name: String,
        argc: usize,
    },
    /// Calls a function value: pops `argc` arguments, then the function (its
    /// name, or a closure); pushes the result.
    CallValue {
        argc: usize,
    },
    /// Explicit cast to a named type (`x as Int32`).
    Cast {
        ty: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IrStmt {
    /// `slot` is the target's frame slot, or `None` for a reactive source
    /// (assigned by name); set by [`resolve_locals`].
    Assign {
        target: String,
        slot: Option<u32>,
        expr: Vec<Inst>,
    },
    /// `slot` is set by [`resolve_locals`].
    Let {
        name: String,
        slot: u32,
        expr: Vec<Inst>,
    },
    Emit {
        event: String,
        arg: Option<Vec<Inst>>,
    },
    Expr {
        code: Vec<Inst>,
    },
    If {
        cond: Vec<Inst>,
        then_body: Vec<IrStmt>,
        else_body: Option<Vec<IrStmt>>,
    },
    While {
        cond: Vec<Inst>,
        body: Vec<IrStmt>,
    },
    /// `slot` (the loop variable's) is set by [`resolve_locals`].
    For {
        var: String,
        slot: u32,
        iter: Vec<Inst>,
        body: Vec<IrStmt>,
    },
    Return {
        expr: Option<Vec<Inst>>,
    },
    Break,
    Continue,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompiledFunction {
    pub name: String,
    pub reactive: bool,
    pub params: Vec<String>,
    pub body: Vec<IrStmt>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompiledWatch {
    pub target: Option<String>,
    pub deps: Vec<String>,
    pub body: Vec<IrStmt>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompiledOnHandler {
    pub event: String,
    pub param: Option<String>,
    pub body: Vec<IrStmt>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompiledProgram {
    pub nodes: Vec<IrNode>,
    pub init_stmts: Vec<IrStmt>,
    pub functions: Vec<CompiledFunction>,
    pub watches: Vec<CompiledWatch>,
    pub on_handlers: Vec<CompiledOnHandler>,
    pub events: Vec<String>,
}

impl CompiledProgram {
    pub fn function_map(&self) -> HashMap<&str, &CompiledFunction> {
        self.functions
            .iter()
            .map(|f| (f.name.as_str(), f))
            .collect()
    }

    pub fn handlers_for<'a>(&'a self, event: &str) -> Vec<&'a CompiledOnHandler> {
        self.on_handlers
            .iter()
            .filter(|h| h.event == event)
            .collect()
    }

    pub fn event_node<'a>(&'a self, name: &str) -> Option<&'a IrNode> {
        self.nodes
            .iter()
            .find(|n| n.kind == NodeKind::Event && n.name == name)
    }

    pub fn effect_nodes(&self) -> impl Iterator<Item = &IrNode> {
        self.nodes.iter().filter(|n| n.kind == NodeKind::Effect)
    }
}
