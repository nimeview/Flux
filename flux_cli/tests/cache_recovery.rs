//! Invalid compile-cache entries must behave exactly like a cache miss:
//! the program is recompiled from source and the result is unchanged.

use std::fs;
use std::path::Path;

use flux_cli::cache::{deserialize_program, serialize_program, CompileCache};
use flux_cli::pipeline::CompileOptions;
use flux_cli::pipeline::{compile, compile_file, compile_file_with_options, source_from};
use flux_diagnostics::DiagnosticBag;
use flux_hir::lower_program;
use flux_ir::{lower_to_ir, CompiledProgram};
use flux_resolve::resolve_file;
use flux_runtime::ExecutionMode;
use flux_syntax::parse_program;

const SOURCE: &str = "let mut a = 1\nlet b = a + 41\n";

fn lower(src: &str) -> CompiledProgram {
    let mut diag = DiagnosticBag::new();
    let ast = parse_program(flux_diagnostics::SourceId(0), src, &mut diag).unwrap();
    let hir = lower_program(&ast, &mut diag).unwrap();
    lower_to_ir(&hir, flux_diagnostics::SourceId(0), &mut diag).unwrap()
}

/// A fresh project (so no in-process cache exists for it) with `main.fl`.
fn project() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("flux.toml"), "[package]\nname = \"c\"\n").unwrap();
    let main = dir.path().join("main.fl");
    fs::write(&main, SOURCE).unwrap();
    (dir, main)
}

fn key_for(main: &Path) -> String {
    let mut diag = DiagnosticBag::new();
    CompileCache::cache_key_resolved(&resolve_file(main, &mut diag).unwrap())
}

fn assert_compiles_correctly(main: &Path) {
    let compiled = compile_file(main, ExecutionMode::Vm).expect("compile");
    assert_eq!(compiled.executor.get_number("b"), Some(42.0));
}

fn cache_entries(root: &Path) -> Vec<String> {
    fs::read_dir(root.join(".flux").join("cache"))
        .map(|dir| {
            dir.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn blob_rejected_when_any_header_or_payload_byte_is_wrong() {
    let blob = serialize_program(&lower(SOURCE));
    assert!(deserialize_program(&blob).is_some());
    for i in 0..blob.len() {
        let mut bad = blob.clone();
        bad[i] ^= 0x01;
        assert!(
            deserialize_program(&bad).is_none(),
            "flipped byte {i} accepted"
        );
    }
}

#[test]
fn truncated_blob_is_a_miss_at_every_length() {
    let blob = serialize_program(&lower(SOURCE));
    for len in 0..blob.len() {
        assert!(deserialize_program(&blob[..len]).is_none(), "len {len}");
    }
}

#[test]
fn blob_from_another_compiler_or_layout_is_a_miss() {
    let blob = serialize_program(&lower(SOURCE));
    // Layout: magic(6) version(1) fp_len(1) fingerprint checksum(32) payload.
    let fp_len = blob[7] as usize;
    let mut other_compiler = blob.clone();
    other_compiler[8 + fp_len - 1] ^= 0x20;
    assert!(deserialize_program(&other_compiler).is_none());

    let mut old_layout = b"FLXIR\0".to_vec();
    old_layout.push(2);
    old_layout.extend_from_slice(&blob[8 + fp_len + 32..]); // version 2: magic, version, payload
    assert!(deserialize_program(&old_layout).is_none());
}

#[test]
fn corrupt_entries_on_disk_are_recompiled_and_replaced() {
    let valid = serialize_program(&lower(SOURCE));
    let mut flipped = valid.clone();
    let last = flipped.len() - 1;
    flipped[last] ^= 0xFF;
    let cases: [(&str, Vec<u8>); 4] = [
        ("garbage", b"not a cache entry".to_vec()),
        ("empty", Vec::new()),
        ("truncated", valid[..valid.len() / 2].to_vec()),
        ("flipped", flipped),
    ];
    for (name, bytes) in cases {
        let (dir, main) = project();
        let key = key_for(&main);
        CompileCache::for_project(dir.path()).put(&key, bytes);
        assert_compiles_correctly(&main);
        let rewritten = CompileCache::for_project(dir.path()).get_program(&key);
        assert!(rewritten.is_some(), "{name}: entry was not replaced");
    }
}

#[test]
fn decodable_but_invalid_ir_is_rebuilt() {
    let (dir, main) = project();
    let key = key_for(&main);
    // `b` still loads `a`, which no longer exists: IR validation rejects it.
    let mut broken = lower(SOURCE);
    for node in &mut broken.nodes {
        if node.name == "a" {
            node.name = "renamed".into();
        }
    }
    assert!(flux_runtime::validate_program(&broken).is_err());
    CompileCache::for_project(dir.path()).put_program(&key, &broken);
    assert_compiles_correctly(&main);
}

#[test]
fn no_cache_mode_neither_reads_nor_writes_entries() {
    let (dir, main) = project();
    let key = key_for(&main);
    let mut broken = lower(SOURCE);
    broken.nodes.retain(|node| node.name != "b");
    CompileCache::for_project(dir.path()).put_program(&key, &broken);
    let before = cache_entries(dir.path());

    let compiled = compile_file_with_options(
        &main,
        CompileOptions::run(ExecutionMode::Vm).without_cache(),
    )
    .expect("compile");
    // A valid-but-wrong entry would drop `b` if it were read.
    assert_eq!(compiled.executor.get_number("b"), Some(42.0));
    assert_eq!(cache_entries(dir.path()), before, "no-cache wrote entries");
}

#[test]
fn cache_file_names_are_hashes_not_source_text() {
    let (dir, main) = project();
    assert_compiles_correctly(&main);
    // In-memory compiles used to key entries on the raw source text.
    let in_memory = dir.path().join("scratch.fl");
    compile(source_from(in_memory.display().to_string(), "let z = 5\n")).expect("compile");
    let entries = cache_entries(dir.path());
    assert!(entries.len() >= 2, "{entries:?}");
    for name in entries {
        let stem = name.strip_suffix(".ir").unwrap_or_else(|| panic!("{name}"));
        assert!(
            stem.len() == 64 && stem.bytes().all(|b| b.is_ascii_hexdigit()),
            "unexpected cache file name {name:?}"
        );
    }
}
