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
    /// The types of the values an event carries, in order.
    Event(Vec<Type>),
    Cell(Box<Type>),
    Read(Box<Type>),
    Array(Box<Type>),
    Option(Box<Type>),
    /// `Map[K, V]`; `K` is an integer type, `String`, `Bool`, or `Char`.
    Map {
        key: Box<Type>,
        value: Box<Type>,
    },
    /// `(A, B, …)`: two or more values of these types.
    Tuple(Vec<Type>),
    Result {
        ok: Box<Type>,
        err: Box<Type>,
    },
    /// `args` are the type arguments of a generic struct (`Pair[Int]`), and
    /// `fields` have them substituted; a template's arguments are its
    /// parameters as [`Type::Var`].
    Struct {
        name: String,
        args: Vec<Type>,
        fields: Vec<(String, Type)>,
    },
    /// `variants: []` is a placeholder that names the type without its
    /// variants (see `TypeCheckContext::expand`).
    Enum {
        name: String,
        args: Vec<Type>,
        variants: Vec<(String, Option<Type>)>,
    },
    Var(String),
    /// Element type of an empty array literal `[]` before context fixes it.
    /// An array of `Empty` fits any array type; a binding may not keep it.
    Empty,
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

    /// The type as a person reads it: a type of an imported module by its
    /// declared name (`Point`).
    pub fn display(&self) -> String {
        self.render(false)
    }

    /// The type's identity, for keys: like [`Self::display`], but a type of
    /// an imported module keeps its internal name (`__mod_1__Point`), so two
    /// modules' `Point` types differ.
    pub fn key(&self) -> String {
        self.render(true)
    }

    fn render(&self, internal: bool) -> String {
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
            Type::Event(values) => {
                let values: Vec<String> = values.iter().map(|v| v.render(internal)).collect();
                format!("Event<{}>", values.join(", "))
            }
            Type::Cell(inner) => format!("Cell<{}>", inner.render(internal)),
            Type::Read(inner) => format!("Read<{}>", inner.render(internal)),
            Type::Array(inner) => format!("[{}]", inner.render(internal)),
            Type::Option(inner) => format!("Option<{}>", inner.render(internal)),
            Type::Map { key, value } => {
                format!("Map<{}, {}>", key.render(internal), value.render(internal))
            }
            Type::Tuple(elems) => {
                let elems: Vec<String> = elems.iter().map(|e| e.render(internal)).collect();
                format!("({})", elems.join(", "))
            }
            Type::Result { ok, err } => {
                format!("Result<{}, {}>", ok.render(internal), err.render(internal))
            }
            // A user type is known by its name, which also keeps recursive
            // types readable. Only a struct-variant payload has no name.
            Type::Struct { name, args, .. } | Type::Enum { name, args, .. } if !name.is_empty() => {
                let name = if internal {
                    name.as_str()
                } else {
                    flux_value::type_display_name(name)
                };
                if args.is_empty() {
                    name.to_string()
                } else {
                    let args: Vec<String> = args.iter().map(|a| a.render(internal)).collect();
                    format!("{name}[{}]", args.join(", "))
                }
            }
            Type::Struct { fields, .. } => {
                let fs = fields
                    .iter()
                    .map(|(n, t)| format!("{n}: {}", t.render(internal)))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{{ {fs} }}")
            }
            Type::Enum { .. } => "enum".into(),
            Type::Var(name) => name.clone(),
            Type::Func { params, ret } => {
                let ps = params
                    .iter()
                    .map(|p| p.render(internal))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("fn({ps}) -> {}", ret.render(internal))
            }
            Type::Error => "<error>".into(),
            Type::Empty => "_".into(),
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
