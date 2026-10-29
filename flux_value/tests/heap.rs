use std::sync::Arc;
use std::thread;

use flux_value::{
    clear_for_tests, get_string, intern_string, tagged_to_value, value_to_tagged, Value,
};

#[test]
fn value_tagged_roundtrip_primitives() {
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
    clear_for_tests();
    let id = thread::spawn(|| intern_string("cross-thread"))
        .join()
        .unwrap();
    assert_eq!(get_string(id), Some("cross-thread".into()));
}
