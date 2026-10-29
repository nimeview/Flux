use flux_value::Value;

#[test]
fn numeric_coercions() {
    assert_eq!(Value::int_i64(42).as_int(), Some(42));
    assert_eq!(Value::float_f64(3.9).as_int(), Some(3));
    assert_eq!(Value::Bool(true).as_int(), None);

    assert_eq!(Value::float_f64(1.5).as_float(), Some(1.5));
    assert_eq!(Value::int_i64(7).as_float(), Some(7.0));
    assert_eq!(Value::String("x".into()).as_float(), None);
}

#[test]
fn kind_name_and_is_numeric() {
    assert!(Value::int_i64(1).is_numeric());
    assert!(Value::float_f64(1.0).is_numeric());
    assert!(!Value::Bool(true).is_numeric());
    assert_eq!(Value::Unit.kind_name(), "Unit");
    assert_eq!(Value::String("a".into()).kind_name(), "String");
}

#[test]
fn display_string_renders_nested_values() {
    assert_eq!(Value::int_i64(1).display_string(), "1");
    assert_eq!(Value::String("hi".into()).display_string(), "\"hi\"");
    assert_eq!(
        Value::Array(vec![Value::int_i64(1), Value::Bool(false)]).display_string(),
        "[1, false]"
    );
    assert_eq!(
        Value::Option(Some(Box::new(Value::int_i64(9)))).display_string(),
        "Some(9)"
    );
}
