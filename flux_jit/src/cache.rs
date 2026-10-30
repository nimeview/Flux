//! JIT compile cache keyed by serialized IR + ABI version (thread-local).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Mutex, OnceLock};

use flux_ir::CompiledProgram;
use sha2::{Digest, Sha256};

use crate::JitEngine;

/// Bump when JIT slot ABI, codegen, or host heap layout changes.
pub const JIT_CACHE_VERSION: u8 = 6;

thread_local! {
    static JIT_CACHE: RefCell<HashMap<String, Rc<JitEngine>>> = RefCell::new(HashMap::new());
}

fn compile_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

pub fn cache_key(program: &CompiledProgram) -> String {
    let bytes = bincode::serialize(program).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update([JIT_CACHE_VERSION]);
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// Return a shared [`JitEngine`] for `program`, compiling on first use (per thread).
pub fn get_or_compile(program: &CompiledProgram) -> Rc<JitEngine> {
    let key = cache_key(program);
    if let Some(hit) = JIT_CACHE.with(|cache| cache.borrow().get(&key).cloned()) {
        return hit;
    }

    let _compile_guard = compile_lock()
        .lock()
        .expect("flux_jit compile mutex poisoned");

    JIT_CACHE.with(|cache| {
        let mut guard = cache.borrow_mut();
        if let Some(hit) = guard.get(&key) {
            return hit.clone();
        }
        let engine = Rc::new(JitEngine::new(program));
        guard.insert(key, engine.clone());
        engine
    })
}

pub fn clear_for_tests() {
    JIT_CACHE.with(|cache| cache.borrow_mut().clear());
}
