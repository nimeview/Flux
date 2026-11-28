mod arith;
mod convert;
mod display;
mod heap;
mod ops;
mod tagged;
mod width;

pub use arith::{
    binary, binary_i64, bit_not, bitwise, cast, compare, int_from_i128, int_to_i128, negate,
    promote_int, values_equal, ArithError, BitOp, NumOp,
};
pub use convert::{is_tag_representable, tagged_to_value, value_to_tagged};
pub use display::{binding_display_name, render, type_display_name, TUPLE_TYPE};
pub use heap::{
    array_finish, array_get, array_len, array_push, clear_for_tests, concat_strings, enum_payload,
    enum_variant_is, get_array, get_enum, get_string, heap_scope, heap_stats, int_range,
    intern_string, make_int_range_array, store_enum, HeapScope, HeapStats, MAX_CALL_DEPTH,
    MAX_COLLECTION_LEN, MAX_STRING_LEN,
};
pub use ops::{float_from_f64, float_to_f64, int_from_i64, int_to_i64};
pub use tagged::{
    float_tag, int_tag, tag_float_width, tag_int_width, tag_kind, TaggedVal, TAG_ARRAY, TAG_BOOL,
    TAG_CHAR, TAG_ENUM, TAG_ERROR, TAG_FLOAT, TAG_INT, TAG_STRING, TAG_UNIT,
};
pub use width::{FloatWidth, IntWidth, OverflowError};

use std::collections::HashMap;

/// Canonical runtime value representation shared across VM, stdlib, and CLI output.
#[derive(Debug, PartialEq)]
pub enum Value {
    Int {
        width: IntWidth,
        bits: u64,
    },
    Float {
        width: FloatWidth,
        bits: u64,
    },
    Char(u32),
    Bool(bool),
    String(String),
    /// Shared and copied on write: reading or passing an array is O(1);
    /// mutation (`Arc::make_mut`) copies only if another value still holds it.
    Array(std::sync::Arc<Vec<Value>>),
    /// Structs and enums are behind a pointer so that `Value` stays 32
    /// bytes (inline, they made it 72, and every stack push/pop a slow block
    /// copy). They are immutable once built, so copies share them, like arrays.
    UserStruct(std::sync::Arc<StructValue>),
    UserEnum(std::sync::Arc<EnumValue>),
    /// A closure with captures; one without is its function's name, a `String`.
    Closure(std::sync::Arc<ClosureValue>),
    Option(Option<Box<Value>>),
    Result(Result<Box<Value>, Box<Value>>),
    /// `Map[K, V]`, ordered by key; each entry keeps the key's own value
    /// (with its integer width). Shared and copied on write, like arrays.
    Map(std::sync::Arc<std::collections::BTreeMap<MapKey, (Value, Value)>>),
    /// Raw bytes (`read_bytes`, `bytes([…])`): one byte each, shared and
    /// copied on write like arrays.
    Bytes(std::sync::Arc<Vec<u8>>),
    /// An instance of a `live fn` component: a handle. Copies are the same
    /// instance; when the last one goes, the executor that made it drops
    /// the instance's state and subscriptions (see [`InstanceRef`]).
    Instance(std::sync::Arc<InstanceRef>),
    Unit,
}

impl Clone for Value {
    /// Numbers, characters, and Booleans are copied inline; the VM clones a
    /// value on every read of a local, so this path is hot.
    #[inline]
    fn clone(&self) -> Self {
        match self {
            Self::Int { width, bits } => Self::Int {
                width: *width,
                bits: *bits,
            },
            Self::Float { width, bits } => Self::Float {
                width: *width,
                bits: *bits,
            },
            Self::Bool(b) => Self::Bool(*b),
            Self::Char(c) => Self::Char(*c),
            Self::Unit => Self::Unit,
            _ => self.clone_shared(),
        }
    }
}

impl Value {
    #[inline(never)]
    fn clone_shared(&self) -> Self {
        match self {
            Self::String(s) => Self::String(s.clone()),
            Self::Array(a) => Self::Array(a.clone()),
            Self::UserStruct(s) => Self::UserStruct(s.clone()),
            Self::UserEnum(e) => Self::UserEnum(e.clone()),
            Self::Closure(c) => Self::Closure(c.clone()),
            Self::Option(o) => Self::Option(o.clone()),
            Self::Result(r) => Self::Result(r.clone()),
            Self::Map(m) => Self::Map(m.clone()),
            Self::Bytes(b) => Self::Bytes(b.clone()),
            Self::Instance(i) => Self::Instance(i.clone()),
            Self::Int { width, bits } => Self::Int {
                width: *width,
                bits: *bits,
            },
            Self::Float { width, bits } => Self::Float {
                width: *width,
                bits: *bits,
            },
            Self::Bool(b) => Self::Bool(*b),
            Self::Char(c) => Self::Char(*c),
            Self::Unit => Self::Unit,
        }
    }
}

/// Where dropped instances are reported: the executor that owns their
/// state empties it at its next safe point.
#[derive(Debug, Default)]
pub struct InstanceMailbox {
    dropped: std::sync::Mutex<Vec<u64>>,
    /// Whether `dropped` may have entries: checked without the lock.
    any: std::sync::atomic::AtomicBool,
}

impl InstanceMailbox {
    /// The instances dropped since the last call.
    pub fn take(&self) -> Vec<u64> {
        if !self.any.load(std::sync::atomic::Ordering::Acquire) {
            return Vec::new();
        }
        let mut dropped = self.dropped.lock().unwrap_or_else(|e| e.into_inner());
        self.any.store(false, std::sync::atomic::Ordering::Release);
        std::mem::take(&mut *dropped)
    }

    /// Whether instance `id` was dropped and not yet taken.
    pub fn contains(&self, id: u64) -> bool {
        if !self.any.load(std::sync::atomic::Ordering::Acquire) {
            return false;
        }
        self.dropped
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains(&id)
    }

    pub fn is_empty(&self) -> bool {
        self.dropped
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_empty()
    }
}

/// The identity of a component instance. Dropping the last handle tells its
/// executor, through the mailbox, to release the instance.
#[derive(Debug)]
pub struct InstanceRef {
    pub id: u64,
    /// The component's internal name.
    pub type_name: String,
    pub mailbox: std::sync::Arc<InstanceMailbox>,
}

impl Drop for InstanceRef {
    fn drop(&mut self) {
        let mut dropped = self
            .mailbox
            .dropped
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        dropped.push(self.id);
        self.mailbox
            .any
            .store(true, std::sync::atomic::Ordering::Release);
    }
}

impl PartialEq for InstanceRef {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClosureValue {
    pub function: String,
    pub captured: Vec<Value>,
}

/// A type's, variant's, or field's name, shared: building a value copies a
/// pointer, not the text.
pub type Name = std::sync::Arc<str>;

#[derive(Debug, Clone)]
pub struct StructValue {
    pub type_name: Name,
    pub fields: Fields,
}

#[derive(Debug, Clone)]
pub struct EnumValue {
    pub type_name: Name,
    pub variant: Name,
    pub payload: Option<Value>,
}

/// A struct's fields, sorted by name: a lookup compares a few short names,
/// and equal structs hold their fields in the same order. Up to two live
/// inside the struct itself, so a pair, a tuple of two, or a tree node is
/// one allocation. (A `HashMap` cost one for the table and one per name.)
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Fields(smallvec::SmallVec<[(Name, Value); 2]>);

impl Fields {
    pub fn with_capacity(n: usize) -> Self {
        Self(smallvec::SmallVec::with_capacity(n))
    }

    /// The fields of `pairs`, in any order; a later duplicate name wins.
    pub fn from_pairs(pairs: impl IntoIterator<Item = (Name, Value)>) -> Self {
        let mut fields: smallvec::SmallVec<[(Name, Value); 2]> = pairs.into_iter().collect();
        // Stable, so of two equal names the later stays last.
        fields.sort_by(|a, b| a.0.cmp(&b.0));
        let mut i = 1;
        while i < fields.len() {
            if fields[i].0 == fields[i - 1].0 {
                fields.remove(i - 1);
            } else {
                i += 1;
            }
        }
        Self(fields)
    }

    fn position(&self, name: &str) -> Result<usize, usize> {
        self.0.binary_search_by(|(n, _)| (**n).cmp(name))
    }

    pub fn get(&self, name: &str) -> Option<&Value> {
        self.position(name).ok().map(|i| &self.0[i].1)
    }

    pub fn get_mut(&mut self, name: &str) -> Option<&mut Value> {
        self.position(name).ok().map(|i| &mut self.0[i].1)
    }

    /// Sets `name` to `value`, adding the field if it is new.
    pub fn insert(&mut self, name: impl Into<Name> + AsRef<str>, value: Value) {
        match self.position(name.as_ref()) {
            Ok(i) => self.0[i].1 = value,
            Err(i) => self.0.insert(i, (name.into(), value)),
        }
    }

    pub fn contains_key(&self, name: &str) -> bool {
        self.position(name).is_ok()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Names and values, by name.
    pub fn iter(&self) -> impl Iterator<Item = (&Name, &Value)> {
        self.0.iter().map(|(n, v)| (n, v))
    }

    pub fn keys(&self) -> impl Iterator<Item = &Name> {
        self.0.iter().map(|(n, _)| n)
    }

    pub fn values(&self) -> impl Iterator<Item = &Value> {
        self.0.iter().map(|(_, v)| v)
    }

    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut Value> {
        self.0.iter_mut().map(|(_, v)| v)
    }
}

impl std::ops::Index<&str> for Fields {
    type Output = Value;
    fn index(&self, name: &str) -> &Value {
        self.get(name).expect("no such field")
    }
}

impl From<HashMap<String, Value>> for Fields {
    fn from(map: HashMap<String, Value>) -> Self {
        Self::from_pairs(map.into_iter().map(|(k, v)| (Name::from(k), v)))
    }
}

impl FromIterator<(String, Value)> for Fields {
    fn from_iter<I: IntoIterator<Item = (String, Value)>>(iter: I) -> Self {
        Self::from_pairs(iter.into_iter().map(|(k, v)| (Name::from(k), v)))
    }
}

impl FromIterator<(Name, Value)> for Fields {
    fn from_iter<I: IntoIterator<Item = (Name, Value)>>(iter: I) -> Self {
        Self::from_pairs(iter)
    }
}

/// `"0"`, `"1"`, …: a tuple's field names, shared.
pub fn index_name(i: usize) -> Name {
    static NAMES: std::sync::OnceLock<Vec<Name>> = std::sync::OnceLock::new();
    let names = NAMES.get_or_init(|| (0..32).map(|i| Name::from(i.to_string())).collect());
    names
        .get(i)
        .cloned()
        .unwrap_or_else(|| Name::from(i.to_string()))
}

/// The type name all tuples share.
pub fn tuple_type_name() -> Name {
    static NAME: std::sync::OnceLock<Name> = std::sync::OnceLock::new();
    NAME.get_or_init(|| Name::from(display::TUPLE_TYPE)).clone()
}

/// Runs `f`, which recurses into a struct or enum's parts, on a stack that
/// grows when low. A recursive type's value can be nested as deep as a loop
/// builds it (no call limit applies), and a native overflow aborts the process.
pub(crate) fn deep_recursion<R>(f: impl FnOnce() -> R) -> R {
    stacker::maybe_grow(64 * 1024, 1024 * 1024, f)
}

impl PartialEq for StructValue {
    fn eq(&self, other: &Self) -> bool {
        deep_recursion(|| self.type_name == other.type_name && self.fields == other.fields)
    }
}

impl PartialEq for EnumValue {
    fn eq(&self, other: &Self) -> bool {
        deep_recursion(|| {
            self.type_name == other.type_name
                && self.variant == other.variant
                && self.payload == other.payload
        })
    }
}

impl Value {
    /// Whether dropping this value can recurse into further values.
    fn holds_values(&self) -> bool {
        !matches!(
            self,
            Self::Int { .. }
                | Self::Float { .. }
                | Self::Char(_)
                | Self::Bool(_)
                | Self::String(_)
                | Self::Instance(_)
                | Self::Unit
        )
    }
}

// Dropping the last reference to a long recursive value recurses through it.
impl Drop for StructValue {
    fn drop(&mut self) {
        if self.fields.values().any(Value::holds_values) {
            let fields = std::mem::take(&mut self.fields);
            deep_recursion(|| drop(fields));
        }
    }
}

impl Drop for EnumValue {
    fn drop(&mut self) {
        if self.payload.as_ref().is_some_and(Value::holds_values) {
            let payload = self.payload.take();
            deep_recursion(|| drop(payload));
        }
    }
}

/// A map key: integers (any width, by value), `String`, `Bool`, or `Char`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum MapKey {
    Bool(bool),
    Int(i128),
    Char(u32),
    String(String),
}

impl MapKey {
    /// The key for `v`, or `None` for a value that cannot be a key.
    pub fn of(v: &Value) -> Option<MapKey> {
        match v {
            Value::Bool(b) => Some(MapKey::Bool(*b)),
            Value::Int { width, bits } => Some(MapKey::Int(int_to_i128(*width, *bits))),
            Value::Char(c) => Some(MapKey::Char(*c)),
            Value::String(s) => Some(MapKey::String(s.clone())),
            _ => None,
        }
    }
}

impl Value {
    pub fn user_struct(type_name: impl Into<Name>, fields: impl Into<Fields>) -> Self {
        Self::UserStruct(std::sync::Arc::new(StructValue {
            type_name: type_name.into(),
            fields: fields.into(),
        }))
    }

    /// A tuple of `values`: a struct with fields "0", "1", ….
    pub fn tuple(values: Vec<Value>) -> Self {
        let fields = values
            .into_iter()
            .enumerate()
            .map(|(i, v)| (index_name(i), v));
        Self::user_struct(tuple_type_name(), Fields::from_pairs(fields))
    }

    pub fn user_enum(
        type_name: impl Into<Name>,
        variant: impl Into<Name>,
        payload: Option<Value>,
    ) -> Self {
        Self::UserEnum(std::sync::Arc::new(EnumValue {
            type_name: type_name.into(),
            variant: variant.into(),
            payload,
        }))
    }

    pub fn array(items: Vec<Value>) -> Self {
        Self::Array(std::sync::Arc::new(items))
    }

    pub fn int_i64(n: i64) -> Self {
        Self::Int {
            width: IntWidth::I64,
            bits: n as u64,
        }
    }

    pub fn float_f64(f: f64) -> Self {
        Self::Float {
            width: FloatWidth::F64,
            bits: f.to_bits(),
        }
    }

    pub fn as_float(&self) -> Option<f64> {
        match self {
            Self::Float { width, bits } => Some(float_to_f64(*width, *bits)),
            Self::Int { width, bits } => Some(int_to_i128(*width, *bits) as f64),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_int(&self) -> Option<i64> {
        match self {
            Self::Int { width, bits } => i64::try_from(int_to_i128(*width, *bits)).ok(),
            Self::Float { width, bits } => Some(float_to_f64(*width, *bits) as i64),
            _ => None,
        }
    }

    pub fn is_numeric(&self) -> bool {
        matches!(self, Self::Int { .. } | Self::Float { .. })
    }

    pub fn kind_name(&self) -> &'static str {
        match self {
            Self::Int { width, .. } => match width {
                IntWidth::I64 => "Int",
                other => {
                    let _ = other;
                    "Int"
                }
            },
            Self::Float { width, .. } => match width {
                FloatWidth::F64 => "Float",
                FloatWidth::F32 => "Float32",
            },
            Self::Char(_) => "Char",
            Self::Bool(_) => "Bool",
            Self::String(_) => "String",
            Self::Array(_) => "Array",
            Self::UserStruct(_) => "Struct",
            Self::UserEnum(_) => "Enum",
            Self::Closure(_) => "Closure",
            Self::Option(_) => "Option",
            Self::Result(_) => "Result",
            Self::Map(_) => "Map",
            Self::Bytes(_) => "Bytes",
            Self::Instance(_) => "Instance",
            Self::Unit => "Unit",
        }
    }

    /// Human-readable representation for print/display builtins and CLI output.
    pub fn display_string(&self) -> String {
        render(self)
    }
}

#[cfg(test)]
mod size_tests {
    /// Every operand push/pop moves a `Value`; past 64 bytes the move
    /// became a block copy that dominated the VM loop.
    #[test]
    fn value_stays_small() {
        assert!(std::mem::size_of::<super::Value>() <= 32);
    }
}
