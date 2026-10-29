//! Integration tests for realistic demo programs in `programs/demo/`.

use std::path::PathBuf;

use flux_cli::pipeline::{compile, source_from};
use flux_runtime::RuntimeValue;

fn programs_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../programs")
}

fn read_demo(name: &str) -> (String, String) {
    let path = programs_dir().join("demo").join(name);
    let contents = std::fs::read_to_string(&path).expect("read demo program");
    (format!("demo/{name}"), contents)
}

fn compile_demo(name: &str) -> flux_cli::pipeline::Compiled {
    let (path, contents) = read_demo(name);
    compile(source_from(path, contents))
        .unwrap_or_else(|e| panic!("demo/{name} should compile: {:?}", e.diagnostics.errors))
}

#[test]
fn demo_counter_click_tracking() {
    let mut compiled = compile_demo("counter.fl");
    assert_eq!(compiled.executor.get_number("count"), Some(0.0));
    assert_eq!(compiled.executor.get_number("doubled"), Some(0.0));
    assert_eq!(compiled.executor.get_number("tripled"), Some(0.0));

    for _ in 0..5 {
        compiled.executor.emit("Click", RuntimeValue::Unit);
    }

    assert_eq!(compiled.executor.get_number("count"), Some(5.0));
    assert_eq!(compiled.executor.get_number("doubled"), Some(10.0));
    assert_eq!(compiled.executor.get_number("tripled"), Some(15.0));
}

#[test]
fn demo_thermostat_alerts_on_heat() {
    let mut compiled = compile_demo("thermostat.fl");
    assert_eq!(compiled.executor.get_number("comfort"), Some(24.0));
    assert_eq!(compiled.executor.get_number("alert_count"), Some(0.0));
    assert_eq!(
        compiled.executor.get_value("too_hot"),
        Some(RuntimeValue::Bool(false))
    );

    compiled
        .executor
        .set_source("temperature", RuntimeValue::int_i64(30));
    assert_eq!(compiled.executor.get_number("comfort"), Some(34.0));
    assert_eq!(
        compiled.executor.get_value("too_hot"),
        Some(RuntimeValue::Bool(true))
    );
    assert_eq!(compiled.executor.get_number("alert_count"), Some(1.0));

    compiled.executor.emit("Tick", RuntimeValue::Unit);
    assert_eq!(compiled.executor.get_number("temperature"), Some(31.0));
    assert_eq!(compiled.executor.get_number("alert_count"), Some(2.0));
}

#[test]
fn demo_inventory_reassign_at_init() {
    let compiled = compile_demo("inventory.fl");
    assert_eq!(compiled.executor.get_number("price"), Some(10.0));
    assert_eq!(compiled.executor.get_number("quantity"), Some(5.0));
    assert_eq!(compiled.executor.get_number("total"), Some(50.0));
}

#[test]
fn demo_game_score_events_and_watch() {
    let mut compiled = compile_demo("game_score.fl");
    assert_eq!(compiled.executor.get_number("possessions"), Some(10.0));
    assert_eq!(compiled.executor.get_number("lead"), Some(0.0));

    compiled.executor.emit("ScoreHome", RuntimeValue::Unit);
    compiled.executor.emit("ScoreHome", RuntimeValue::Unit);
    compiled.executor.emit("ScoreAway", RuntimeValue::Unit);

    assert_eq!(compiled.executor.get_number("home"), Some(6.0));
    assert_eq!(compiled.executor.get_number("away"), Some(3.0));
    assert_eq!(compiled.executor.get_number("lead"), Some(3.0));
}

#[test]
fn demo_dashboard_full_feature_smoke() {
    let mut compiled = compile_demo("dashboard.fl");

    assert_eq!(compiled.executor.get_number("version"), Some(1.0));
    assert_eq!(compiled.executor.get_number("threshold"), Some(25.0));
    assert_eq!(
        compiled.executor.get_string("status_label"),
        Some("status:ok".into())
    );
    assert_eq!(compiled.executor.get_number("comfort"), Some(-5.0));
    assert_eq!(compiled.executor.get_number("adjusted"), Some(20.0));
    assert_eq!(compiled.executor.get_number("bonus"), Some(120.0));
    assert_eq!(compiled.executor.get_number("health"), Some(50.0));
    assert_eq!(compiled.executor.get_number("rolling"), Some(15.0));
    assert_eq!(compiled.executor.get_number("alert_count"), Some(0.0));

    assert!(compiled.types.contains_key("temperature"));
    assert!(compiled.types.contains_key("comfort"));

    compiled.executor.emit("Refresh", RuntimeValue::Unit);
    assert_eq!(compiled.executor.get_number("temperature"), Some(21.0));
    assert_eq!(compiled.executor.get_number("alert_count"), Some(0.0));

    compiled
        .executor
        .set_source("temperature", RuntimeValue::int_i64(30));
    assert_eq!(compiled.executor.get_number("alert_count"), Some(1.0));
}

#[test]
fn all_demo_programs_exist_and_compile() {
    for name in [
        "counter.fl",
        "thermostat.fl",
        "inventory.fl",
        "game_score.fl",
        "dashboard.fl",
    ] {
        let path = programs_dir().join("demo").join(name);
        assert!(path.is_file(), "missing demo/{name}");
        compile_demo(name);
    }
}
