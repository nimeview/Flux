//! Synchronous file and stdin I/O builtins.

use std::fs;
use std::io::{self, Write};

use flux_value::Value;

pub fn read_line() -> Value {
    let mut line = String::new();
    match io::stdin().read_line(&mut line) {
        Ok(_) => {
            let trimmed = line.strip_suffix('\n').unwrap_or(&line).to_string();
            let trimmed = trimmed.strip_suffix('\r').unwrap_or(&trimmed).to_string();
            ok_string(trimmed)
        }
        Err(e) => err_string(e.to_string()),
    }
}

pub fn read_file(path: &str) -> Value {
    match fs::read_to_string(path) {
        Ok(contents) => ok_string(contents),
        Err(e) => err_string(e.to_string()),
    }
}

pub fn write_file(path: &str, contents: &str) -> Value {
    match fs::write(path, contents) {
        Ok(()) => ok_unit(),
        Err(e) => err_string(e.to_string()),
    }
}

pub fn append_file(path: &str, contents: &str) -> Value {
    match fs::OpenOptions::new().create(true).append(true).open(path) {
        Ok(mut file) => match file.write_all(contents.as_bytes()) {
            Ok(()) => ok_unit(),
            Err(e) => err_string(e.to_string()),
        },
        Err(e) => err_string(e.to_string()),
    }
}

pub fn file_exists(path: &str) -> Value {
    Value::Bool(std::path::Path::new(path).exists())
}

fn ok_string(s: String) -> Value {
    Value::Result(Ok(Box::new(Value::String(s))))
}

fn ok_unit() -> Value {
    Value::Result(Ok(Box::new(Value::Unit)))
}

fn err_string(msg: String) -> Value {
    Value::Result(Err(Box::new(Value::String(msg))))
}
