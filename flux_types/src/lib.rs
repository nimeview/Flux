mod builtins;
mod check;
mod mono;
#[allow(dead_code)]
mod registry;
mod types;

pub use builtins::{builtin_fn_type, is_builtin_fn, BUILTIN_FNS};
pub use check::state::TypeCheckContext;
pub use check::{typecheck_program, TypedProgram};
pub use mono::monomorphize_program;
pub use types::Type;
