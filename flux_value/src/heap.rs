//! Thread-safe heap pools for tagged values that do not fit in a `u64` data word.
//!
//! Lifetime policy: a handle is valid for the [`HeapScope`] it was created in.
//! Callers open a scope around one exchange with compiled code (convert inputs,
//! run, convert results back to owned [`crate::Value`]s); when the outermost
//! scope on a thread ends, the arrays and enums it allocated are freed and its
//! string references are released. Allocations made outside any scope (e.g.
//! string constants interned while compiling) live for the whole process.
//! Ids are never reused, so a stale handle reads as missing, never as another value.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use crate::{ArithError, TaggedVal};

#[derive(Clone, Debug)]
struct EnumValue {
    type_name: String,
    variant: String,
    payload: Option<TaggedVal>,
}

#[derive(Default)]
struct HeapStores {
    strings: HashMap<u64, String>,
    string_ids: HashMap<String, u64>,
    enums: HashMap<u64, EnumValue>,
    arrays: HashMap<u64, Vec<TaggedVal>>,
    /// Open-scope references to strings that are not process-lifetime.
    /// A string without an entry here is pinned (interned outside a scope).
    string_refs: HashMap<u64, usize>,
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn stores() -> &'static Mutex<HeapStores> {
    static STORES: OnceLock<Mutex<HeapStores>> = OnceLock::new();
    STORES.get_or_init(|| Mutex::new(HeapStores::default()))
}

fn alloc_id() -> u64 {
    NEXT_ID.fetch_add(1, Ordering::SeqCst)
}

#[derive(Clone, Copy)]
enum ScopedEntry {
    Array(u64),
    Enum(u64),
    StringRef(u64),
}

#[derive(Default)]
struct ScopeState {
    depth: usize,
    entries: Vec<ScopedEntry>,
}

// Scratch buffer for in-flight `MakeArray` lowering (one builder per OS thread).
thread_local! {
    static ARRAY_BUILDER: RefCell<Vec<TaggedVal>> = const { RefCell::new(Vec::new()) };
    static SCOPE: RefCell<ScopeState> = RefCell::new(ScopeState::default());
}

/// Guard for one exchange with compiled code; see the module docs. Scopes
/// nest: only the outermost one on a thread frees anything. Not `Send`.
#[must_use = "the scope ends (and frees its allocations) when the guard drops"]
pub struct HeapScope {
    _not_send: std::marker::PhantomData<*const ()>,
}

pub fn heap_scope() -> HeapScope {
    SCOPE.with(|s| s.borrow_mut().depth += 1);
    HeapScope {
        _not_send: std::marker::PhantomData,
    }
}

impl Drop for HeapScope {
    fn drop(&mut self) {
        let entries = SCOPE.with(|s| {
            let mut state = s.borrow_mut();
            state.depth -= 1;
            if state.depth == 0 {
                std::mem::take(&mut state.entries)
            } else {
                Vec::new()
            }
        });
        if entries.is_empty() {
            return;
        }
        let Ok(mut guard) = stores().lock() else {
            return;
        };
        for entry in entries {
            match entry {
                ScopedEntry::Array(id) => {
                    guard.arrays.remove(&id);
                }
                ScopedEntry::Enum(id) => {
                    guard.enums.remove(&id);
                }
                ScopedEntry::StringRef(id) => {
                    let Some(refs) = guard.string_refs.get_mut(&id) else {
                        continue; // pinned while the scope was open
                    };
                    *refs -= 1;
                    if *refs == 0 {
                        guard.string_refs.remove(&id);
                        if let Some(text) = guard.strings.remove(&id) {
                            guard.string_ids.remove(&text);
                        }
                    }
                }
            }
        }
    }
}

/// Records `entry` in the open scope; returns `false` when there is none.
fn record(entry: ScopedEntry) -> bool {
    SCOPE.with(|s| {
        let mut state = s.borrow_mut();
        if state.depth == 0 {
            return false;
        }
        state.entries.push(entry);
        true
    })
}

pub fn intern_string(s: &str) -> u64 {
    let mut guard = stores().lock().expect("flux_value heap mutex poisoned");
    let id = match guard.string_ids.get(s) {
        Some(&id) => id,
        None => {
            let id = alloc_id();
            guard.strings.insert(id, s.to_string());
            guard.string_ids.insert(s.to_string(), id);
            // New strings start pinned; a scoped intern below unpins it.
            guard.string_refs.insert(id, 0);
            id
        }
    };
    let pinned = !guard.string_refs.contains_key(&id);
    if !pinned {
        if record(ScopedEntry::StringRef(id)) {
            *guard.string_refs.get_mut(&id).expect("scoped string") += 1;
        } else {
            guard.string_refs.remove(&id); // interned outside a scope: pin
        }
    }
    id
}

pub fn get_string(id: u64) -> Option<String> {
    stores()
        .lock()
        .expect("flux_value heap mutex poisoned")
        .strings
        .get(&id)
        .cloned()
}

pub fn store_enum(type_name: &str, variant: &str, payload: Option<TaggedVal>) -> u64 {
    let id = alloc_id();
    record(ScopedEntry::Enum(id));
    stores()
        .lock()
        .expect("flux_value heap mutex poisoned")
        .enums
        .insert(
            id,
            EnumValue {
                type_name: type_name.to_string(),
                variant: variant.to_string(),
                payload,
            },
        );
    id
}

pub fn get_enum(id: u64) -> Option<(String, String, Option<TaggedVal>)> {
    stores()
        .lock()
        .expect("flux_value heap mutex poisoned")
        .enums
        .get(&id)
        .map(|e| (e.type_name.clone(), e.variant.clone(), e.payload))
}

pub fn store_array(items: Vec<TaggedVal>) -> u64 {
    let id = alloc_id();
    record(ScopedEntry::Array(id));
    stores()
        .lock()
        .expect("flux_value heap mutex poisoned")
        .arrays
        .insert(id, items);
    id
}

pub fn get_array(id: u64) -> Option<Vec<TaggedVal>> {
    stores()
        .lock()
        .expect("flux_value heap mutex poisoned")
        .arrays
        .get(&id)
        .cloned()
}

pub fn array_len(id: u64) -> u64 {
    stores()
        .lock()
        .expect("flux_value heap mutex poisoned")
        .arrays
        .get(&id)
        .map(|a| a.len() as u64)
        .unwrap_or(0)
}

pub fn array_get(id: u64, index: u64) -> TaggedVal {
    stores()
        .lock()
        .expect("flux_value heap mutex poisoned")
        .arrays
        .get(&id)
        .and_then(|a| a.get(index as usize).copied())
        .unwrap_or(TaggedVal::UNIT)
}

/// Largest array `range` (and other bulk builders) may produce; larger requests
/// are the runtime error [`ArithError::AllocationLimit`] instead of exhausting memory.
pub const MAX_COLLECTION_LEN: u64 = 10_000_000;

/// The integers `start, start + step, …` strictly before `end` (after, for a
/// negative step). A zero step is empty. Never overflows: lengths are computed in `i128`.
pub fn int_range(start: i64, end: i64, step: i64) -> Result<Vec<i64>, ArithError> {
    let (start, end, step) = (start as i128, end as i128, step as i128);
    let len = if step > 0 && start < end {
        (end - start - 1) / step + 1
    } else if step < 0 && start > end {
        (start - end - 1) / -step + 1
    } else {
        0
    };
    if len > MAX_COLLECTION_LEN as i128 {
        return Err(ArithError::AllocationLimit);
    }
    Ok((0..len).map(|i| (start + i * step) as i64).collect())
}

pub fn make_int_range_array(start: i64, end: i64, step: i64) -> Result<u64, ArithError> {
    let items = int_range(start, end, step)?;
    Ok(store_array(
        items.into_iter().map(TaggedVal::from_int).collect(),
    ))
}

pub fn array_push(tag: u64, data: u64) {
    ARRAY_BUILDER.with(|b| b.borrow_mut().push(TaggedVal { tag, data }));
}

pub fn array_finish(count: u64) -> TaggedVal {
    let items = ARRAY_BUILDER.with(|b| {
        let mut buf = b.borrow_mut();
        let n = count as usize;
        let start = buf.len().saturating_sub(n);
        let items = buf[start..].to_vec();
        buf.truncate(start);
        items
    });
    TaggedVal::from_array_id(store_array(items))
}

/// Live entry counts in the process-wide pools.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HeapStats {
    pub strings: usize,
    pub enums: usize,
    pub arrays: usize,
}

pub fn heap_stats() -> HeapStats {
    let guard = stores().lock().expect("flux_value heap mutex poisoned");
    HeapStats {
        strings: guard.strings.len(),
        enums: guard.enums.len(),
        arrays: guard.arrays.len(),
    }
}

/// Clear all pooled heap values (for tests).
pub fn clear_for_tests() {
    let mut guard = stores().lock().expect("flux_value heap mutex poisoned");
    guard.strings.clear();
    guard.string_ids.clear();
    guard.enums.clear();
    guard.arrays.clear();
    guard.string_refs.clear();
    ARRAY_BUILDER.with(|b| b.borrow_mut().clear());
}
