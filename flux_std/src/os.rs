//! The operating system, for command-line programs: the `env`, `fs`, `path`,
//! and `process` modules, and `read_stdin` in `io`. Expected failures are
//! `Err` with a message that names the file or program, as for `read_file`.

use std::cell::Cell;
use std::io::Read;
use std::path::Path;

use flux_value::{ArithError, Value};

fn ok(v: Value) -> Value {
    Value::Result(Ok(Box::new(v)))
}

fn err(message: String) -> Value {
    Value::Result(Err(Box::new(Value::String(message))))
}

fn ok_unit_or(path: &str, result: std::io::Result<()>) -> Value {
    match result {
        Ok(()) => ok(Value::Unit),
        Err(e) => err(format!("{path}: {e}")),
    }
}

fn some_string(s: Option<&str>) -> Value {
    Value::Option(s.map(|s| Box::new(Value::String(s.to_string()))))
}

/// All of stdin, until its end.
pub fn read_stdin() -> Value {
    let mut text = String::new();
    match std::io::stdin().read_to_string(&mut text) {
        Ok(_) => ok(Value::String(text)),
        Err(e) => err(format!("stdin: {e}")),
    }
}

/// The value of an environment variable, if it is set (and is text).
pub fn env_var(name: &str) -> Value {
    some_string(std::env::var(name).ok().as_deref())
}

pub fn current_dir() -> Value {
    match std::env::current_dir() {
        Ok(dir) => ok(Value::String(dir.display().to_string())),
        Err(e) => err(format!("current directory: {e}")),
    }
}

/// The names of a directory's entries, sorted.
pub fn list_dir(path: &str) -> Value {
    let entries = match std::fs::read_dir(path) {
        Ok(entries) => entries,
        Err(e) => return err(format!("{path}: {e}")),
    };
    let mut names = Vec::new();
    for entry in entries {
        match entry {
            Ok(entry) => names.push(entry.file_name().to_string_lossy().into_owned()),
            Err(e) => return err(format!("{path}: {e}")),
        }
    }
    names.sort();
    ok(Value::array(names.into_iter().map(Value::String).collect()))
}

pub fn is_dir(path: &str) -> Value {
    Value::Bool(Path::new(path).is_dir())
}

/// Creates a directory and any missing parents; an existing one is fine.
pub fn create_dir(path: &str) -> Value {
    ok_unit_or(path, std::fs::create_dir_all(path))
}

pub fn remove_file(path: &str) -> Value {
    ok_unit_or(path, std::fs::remove_file(path))
}

/// Removes an empty directory.
pub fn remove_dir(path: &str) -> Value {
    ok_unit_or(path, std::fs::remove_dir(path))
}

pub fn path_join(base: &str, part: &str) -> Value {
    Value::String(Path::new(base).join(part).display().to_string())
}

pub fn file_name(path: &str) -> Value {
    some_string(Path::new(path).file_name().and_then(|s| s.to_str()))
}

pub fn extension(path: &str) -> Value {
    some_string(Path::new(path).extension().and_then(|s| s.to_str()))
}

/// The path without its last part; `None` for a root or a bare name's parent
/// that is empty.
pub fn parent(path: &str) -> Value {
    let parent = Path::new(path)
        .parent()
        .map(|p| p.display().to_string())
        .filter(|p| !p.is_empty());
    some_string(parent.as_deref())
}

/// Runs `program` with `args` (no shell), waits, and returns its exit code,
/// stdout, and stderr. A program ended by a signal has exit code -1.
pub fn run(program: &str, args: &[Value]) -> Value {
    let args: Vec<&str> = args
        .iter()
        .filter_map(|a| match a {
            Value::String(s) => Some(s.as_str()),
            _ => None,
        })
        .collect();
    match std::process::Command::new(program)
        .args(&args)
        .stdin(std::process::Stdio::null())
        .output()
    {
        Ok(out) => ok(Value::tuple(vec![
            Value::int_i64(i64::from(out.status.code().unwrap_or(-1))),
            Value::String(String::from_utf8_lossy(&out.stdout).into_owned()),
            Value::String(String::from_utf8_lossy(&out.stderr).into_owned()),
        ])),
        Err(e) => err(format!("{program}: {e}")),
    }
}

thread_local! {
    static EXIT_CODE: Cell<Option<i32>> = const { Cell::new(None) };
}

/// `exit(code)`: stops the program with `code`. It is raised as
/// [`ArithError::Exit`], which unwinds like a runtime error; the command
/// line then exits with the code that [`take_exit_code`] returns.
pub fn exit(code: Option<i64>) -> Result<Value, ArithError> {
    let code = code
        .and_then(|c| u8::try_from(c).ok())
        .ok_or(ArithError::BadExitStatus)?;
    // The first `exit` wins; the program stops there.
    EXIT_CODE.with(|c| {
        if c.get().is_none() {
            c.set(Some(i32::from(code)));
        }
    });
    Err(ArithError::Exit)
}

/// The status a call of `exit` asked for, if one did since the last call.
pub fn take_exit_code() -> Option<i32> {
    EXIT_CODE.with(Cell::take)
}
