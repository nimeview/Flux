//! Updating one part of a value: `p.x = v`, `grid[i][j] = v`, `xs[i] += v`.
//!
//! Arrays, maps, and structs are shared and copied on write, so only the
//! containers on the path that another value still holds are copied. The part
//! changes only once the whole path is known to be valid: a failure leaves the
//! value as it was. For `part op= v` the caller reads the old part with
//! [`get`] before evaluating `v`, then stores the combined value.

use flux_ir::PlaceOp;
use flux_value::{ArithError, MapKey, NumOp};

use crate::RuntimeValue;

/// A step of the path, with its index already evaluated.
pub(super) enum Key<'a> {
    Index(RuntimeValue),
    Field(&'a str),
}

/// The part of `root` that `path` names.
pub(super) fn get(root: &RuntimeValue, path: &[Key<'_>]) -> Result<RuntimeValue, ArithError> {
    let mut part = root;
    for key in path {
        part = match (key, part) {
            (Key::Index(index), RuntimeValue::Array(items)) => index
                .as_int()
                .and_then(|i| usize::try_from(i).ok())
                .and_then(|i| items.get(i))
                .ok_or(ArithError::IndexOutOfBounds)?,
            (Key::Index(index), RuntimeValue::Map(entries)) => {
                let key = MapKey::of(index).ok_or(ArithError::KeyNotFound)?;
                &entries.get(&key).ok_or(ArithError::KeyNotFound)?.1
            }
            (Key::Field(field), RuntimeValue::UserStruct(s)) => {
                s.fields.get(*field).ok_or(ArithError::KeyNotFound)?
            }
            _ => {
                debug_assert!(false, "a place path through a value without parts");
                return Err(ArithError::IndexOutOfBounds);
            }
        };
    }
    Ok(part.clone())
}

/// Replaces the part of `root` that `path` names with `value`.
pub(super) fn update(
    root: &mut RuntimeValue,
    path: &[Key<'_>],
    value: RuntimeValue,
) -> Result<(), ArithError> {
    let Some((first, rest)) = path.split_first() else {
        *root = value;
        return Ok(());
    };
    match (first, root) {
        (Key::Index(index), RuntimeValue::Array(items)) => {
            let at = index
                .as_int()
                .and_then(|i| usize::try_from(i).ok())
                .filter(|&i| i < items.len())
                .ok_or(ArithError::IndexOutOfBounds)?;
            update(&mut std::sync::Arc::make_mut(items)[at], rest, value)
        }
        (Key::Index(index), RuntimeValue::Map(entries)) => {
            let key = MapKey::of(index).ok_or(ArithError::KeyNotFound)?;
            // `m[k] = v` adds `k`; anything deeper needs it to be there.
            if rest.is_empty() {
                if !entries.contains_key(&key)
                    && entries.len() as u64 >= flux_value::MAX_COLLECTION_LEN
                {
                    return Err(ArithError::AllocationLimit);
                }
                std::sync::Arc::make_mut(entries).insert(key, (index.clone(), value));
                return Ok(());
            }
            if !entries.contains_key(&key) {
                return Err(ArithError::KeyNotFound);
            }
            let entry = std::sync::Arc::make_mut(entries)
                .get_mut(&key)
                .ok_or(ArithError::KeyNotFound)?;
            update(&mut entry.1, rest, value)
        }
        (Key::Field(field), RuntimeValue::UserStruct(fields)) => {
            if !fields.fields.contains_key(*field) {
                debug_assert!(false, "field `{field}` checked by the type checker");
                return Err(ArithError::KeyNotFound);
            }
            let part = std::sync::Arc::make_mut(fields)
                .fields
                .get_mut(*field)
                .ok_or(ArithError::KeyNotFound)?;
            update(part, rest, value)
        }
        // The type checker allows only arrays, maps, and structs on a path.
        _ => {
            debug_assert!(false, "a place path through a value without parts");
            Err(ArithError::IndexOutOfBounds)
        }
    }
}

/// `old op value`, as the operator computes it in an expression.
pub(super) fn combine(
    op: PlaceOp,
    old: RuntimeValue,
    value: RuntimeValue,
) -> Result<RuntimeValue, ArithError> {
    let op = match op {
        PlaceOp::Add => NumOp::Add,
        PlaceOp::Sub => NumOp::Sub,
        PlaceOp::Mul => NumOp::Mul,
        PlaceOp::Div => NumOp::Div,
        PlaceOp::Rem => NumOp::Rem,
    };
    let mut stack = vec![old, value];
    super::values::arith_binop(&mut stack, op)?;
    Ok(stack.pop().unwrap_or(RuntimeValue::Unit))
}
