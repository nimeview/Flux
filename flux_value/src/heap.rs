//! Thread-safe heap pools for tagged values that do not fit in a `u64` data word.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use crate::TaggedVal;

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
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn stores() -> &'static Mutex<HeapStores> {
    static STORES: OnceLock<Mutex<HeapStores>> = OnceLock::new();
    STORES.get_or_init(|| Mutex::new(HeapStores::default()))
}

fn alloc_id() -> u64 {
    NEXT_ID.fetch_add(1, Ordering::SeqCst)
}

// Scratch buffer for in-flight `MakeArray` lowering (one builder per OS thread).
thread_local! {
    static ARRAY_BUILDER: RefCell<Vec<TaggedVal>> = const { RefCell::new(Vec::new()) };
}

pub fn intern_string(s: &str) -> u64 {
    let mut guard = stores().lock().expect("flux_value heap mutex poisoned");
    if let Some(&id) = guard.string_ids.get(s) {
        return id;
    }
    let id = alloc_id();
    guard.strings.insert(id, s.to_string());
    guard.string_ids.insert(s.to_string(), id);
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

pub fn make_int_range_array(start: i64, end: i64, step: i64) -> u64 {
    if step == 0 {
        return store_array(vec![]);
    }
    let mut items = Vec::new();
    if step > 0 {
        let mut i = start;
        while i < end {
            items.push(TaggedVal::from_int(i));
            let Some(next) = i.checked_add(step) else {
                break;
            };
            if next <= i {
                break;
            }
            i = next;
        }
    } else {
        let mut i = start;
        while i > end {
            items.push(TaggedVal::from_int(i));
            let Some(next) = i.checked_add(step) else {
                break;
            };
            if next >= i {
                break;
            }
            i = next;
        }
    }
    store_array(items)
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

/// Clear all pooled heap values (for tests).
pub fn clear_for_tests() {
    let mut guard = stores().lock().expect("flux_value heap mutex poisoned");
    guard.strings.clear();
    guard.string_ids.clear();
    guard.enums.clear();
    guard.arrays.clear();
    ARRAY_BUILDER.with(|b| b.borrow_mut().clear());
}
