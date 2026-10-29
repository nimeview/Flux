//! Backward-compatible re-exports — cache lives in `flux_driver`.

pub use flux_driver::{
    cache_key_for_path, cache_matches, deserialize_program, serialize_program, CompileCache,
};
