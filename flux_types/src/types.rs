use flux_diagnostics::Span;

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TypeId(pub u32);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    Int,
    Int8,
    Int16,
    Int32,
    Int64,
    UInt8,
    UInt16,
    UInt32,
    UInt64,
    Float,
    Float32,
    Float64,
    Char,
    Bool,
    String,
    Unit,
    Event(Box<Type>),
    Cell(Box<Type>),
    Read(Box<Type>),
    Array(Box<Type>),
    Option(Box<Type>),
    Result {
        ok: Box<Type>,
        err: Box<Type>,
    },
    Struct {
        name: String,
        fields: Vec<(String, Type)>,
    },
    Enum {
        name: String,
        variants: Vec<(String, Option<Type>)>,
    },
    Var(String),
    Func {
        params: Vec<Type>,
        ret: Box<Type>,
    },
    Error,
}

impl Type {
    pub fn is_reactive(&self) -> bool {
        matches!(self, Type::Cell(_) | Type::Read(_))
    }

    pub fn is_error(&self) -> bool {
        matches!(self, Type::Error)
    }

    pub fn is_integer(&self) -> bool {
        matches!(
            self,
            Type::Int
                | Type::Int8
                | Type::Int16
                | Type::Int32
                | Type::Int64
                | Type::UInt8
                | Type::UInt16
                | Type::UInt32
                | Type::UInt64
        )
    }

    pub fn is_float(&self) -> bool {
        matches!(self, Type::Float | Type::Float32 | Type::Float64)
    }

    pub fn display(&self) -> String {
        match self {
            Type::Int => "Int".into(),
            Type::Int8 => "Int8".into(),
            Type::Int16 => "Int16".into(),
            Type::Int32 => "Int32".into(),
            Type::Int64 => "Int64".into(),
            Type::UInt8 => "UInt8".into(),
            Type::UInt16 => "UInt16".into(),
            Type::UInt32 => "UInt32".into(),
            Type::UInt64 => "UInt64".into(),
            Type::Float => "Float".into(),
            Type::Float32 => "Float32".into(),
            Type::Float64 => "Float64".into(),
            Type::Char => "Char".into(),
            Type::Bool => "Bool".into(),
            Type::String => "String".into(),
            Type::Unit => "Unit".into(),
            Type::Event(inner) => format!("Event<{}>", inner.display()),
            Type::Cell(inner) => format!("Cell<{}>", inner.display()),
            Type::Read(inner) => format!("Read<{}>", inner.display()),
            Type::Array(inner) => format!("[{}]", inner.display()),
            Type::Option(inner) => format!("Option<{}>", inner.display()),
            Type::Result { ok, err } => {
                format!("Result<{}, {}>", ok.display(), err.display())
            }
            Type::Struct { name, fields } => {
                let fs = fields
                    .iter()
                    .map(|(n, t)| format!("{n}: {}", t.display()))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{name} {{ {fs} }}")
            }
            Type::Enum { name, variants } => {
                let vs = variants
                    .iter()
                    .map(|(n, p)| match p {
                        Some(t) => format!("{n}({})", t.display()),
                        None => n.clone(),
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("enum {name} {{ {vs} }}")
            }
            Type::Var(name) => name.clone(),
            Type::Func { params, ret } => {
                let ps = params
                    .iter()
                    .map(|p| p.display())
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("fn({ps}) -> {}", ret.display())
            }
            Type::Error => "<error>".into(),
        }
    }

    pub fn unwrap_reactive(&self) -> Option<&Type> {
        match self {
            Type::Cell(t) | Type::Read(t) => Some(t),
            _ => None,
        }
    }
}

pub fn resolve_type_name(name: &str, span: Span) -> Result<Type, (String, Span)> {
    let ty = match name {
        "Int" | "Int64" => Type::Int64,
        "Int8" => Type::Int8,
        "Int16" => Type::Int16,
        "Int32" => Type::Int32,
        "UInt8" => Type::UInt8,
        "UInt16" => Type::UInt16,
        "UInt32" => Type::UInt32,
        "UInt64" => Type::UInt64,
        "Float" | "Float64" => Type::Float64,
        "Float32" => Type::Float32,
        "Char" => Type::Char,
        "Bool" => Type::Bool,
        "String" => Type::String,
        "Unit" => Type::Unit,
        other => return Err((other.to_string(), span)),
    };
    Ok(ty)
}
