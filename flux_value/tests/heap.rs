use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::thread;

use flux_value::{
    clear_for_tests, get_array, get_string, heap_scope, heap_stats, intern_string, tagged_to_value,
    value_to_tagged, HeapStats, Value,
};

static TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn heap_test_lock() -> MutexGuard<'static, ()> {
    TEST_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .expect("heap test lock poisoned")
}

#[test]
fn value_tagged_roundtrip_primitives() {
    let _guard = heap_test_lock();
    clear_for_tests();
    let cases = [
        Value::int_i64(42),
        Value::float_f64(1.5),
        Value::Bool(true),
        Value::String("hello".into()),
        Value::Unit,
    ];
    for v in cases {
        let tv = value_to_tagged(&v);
        assert_eq!(tagged_to_value(tv), v);
    }
}

#[test]
fn value_tagged_roundtrip_enum_and_array() {
    let _guard = heap_test_lock();
    clear_for_tests();
    let en = Value::UserEnum {
        type_name: "Color".into(),
        variant: "Green".into(),
        payload: None,
    };
    assert_eq!(tagged_to_value(value_to_tagged(&en)), en);

    let arr = Value::Array(vec![Value::int_i64(1), Value::int_i64(2)]);
    assert_eq!(tagged_to_value(value_to_tagged(&arr)), arr);
}

#[test]
fn intern_string_is_stable_across_threads() {
    let _guard = heap_test_lock();
    clear_for_tests();
    let hello = Arc::new("shared".to_string());
    let handles: Vec<_> = (0..4)
        .map(|_| {
            let s = hello.clone();
            thread::spawn(move || intern_string(&s))
        })
        .collect();
    let ids: Vec<u64> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert!(ids.windows(2).all(|w| w[0] == w[1]));
    assert_eq!(get_string(ids[0]), Some("shared".into()));
}

#[test]
fn heap_stores_are_visible_from_other_threads() {
    let _guard = heap_test_lock();
    clear_for_tests();
    let id = thread::spawn(|| intern_string("cross-thread"))
        .join()
        .unwrap();
    assert_eq!(get_string(id), Some("cross-thread".into()));
}

#[test]
fn scoped_allocations_are_freed_when_outermost_scope_ends() {
    let _guard = heap_test_lock();
    clear_for_tests();
    let outer = heap_scope();
    let array = value_to_tagged(&Value::Array(vec![Value::int_i64(1)]));
    let inner = heap_scope();
    let text = intern_string("scoped-only");
    drop(inner);
    // An inner scope frees nothing: handles stay valid until the outermost ends.
    assert_eq!(get_string(text).as_deref(), Some("scoped-only"));
    assert!(get_array(array.data).is_some());
    drop(outer);
    assert_eq!(get_string(text), None);
    assert_eq!(get_array(array.data), None);
    assert_eq!(heap_stats(), HeapStats::default());
}

#[test]
fn strings_interned_outside_a_scope_are_pinned() {
    let _guard = heap_test_lock();
    clear_for_tests();
    let constant = intern_string("compile-time");
    {
        let _scope = heap_scope();
        assert_eq!(intern_string("compile-time"), constant);
        let _promoted = intern_string("later-pinned");
    }
    assert_eq!(get_string(constant).as_deref(), Some("compile-time"));
    // A scoped string that is later interned outside a scope becomes pinned.
    let scoped = {
        let _scope = heap_scope();
        let id = intern_string("promoted");
        assert_eq!(intern_string_unscoped("promoted"), id);
        id
    };
    assert_eq!(get_string(scoped).as_deref(), Some("promoted"));
}

fn intern_string_unscoped(s: &str) -> u64 {
    thread::scope(|t| t.spawn(|| intern_string(s)).join().unwrap())
}

#[test]
fn shared_scoped_string_survives_other_threads_scope() {
    let _guard = heap_test_lock();
    clear_for_tests();
    let _mine = heap_scope();
    let id = intern_string("shared-text");
    thread::scope(|t| {
        t.spawn(|| {
            let _theirs = heap_scope();
            assert_eq!(intern_string("shared-text"), id);
        })
        .join()
        .unwrap();
    });
    // The other thread's scope ended; this thread still holds a reference.
    assert_eq!(get_string(id).as_deref(), Some("shared-text"));
}
