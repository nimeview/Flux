use std::collections::HashMap;
use std::io::{self, Write};

use flux_runtime::{ExecutionMode, Executor, RuntimeValue};

use crate::output::{print_runtime, report_errors};
use crate::pipeline::{compile_with_mode, source_from};

const PROMPT: &str = "flux> ";
const CONT_PROMPT: &str = "...> ";

struct ReplSession {
    source: String,
    types: HashMap<String, String>,
    executor: Option<Executor>,
    jit: bool,
}

impl ReplSession {
    fn new(jit: bool) -> Self {
        Self {
            source: String::new(),
            types: HashMap::new(),
            executor: None,
            jit,
        }
    }

    fn mode(&self) -> ExecutionMode {
        if self.jit {
            ExecutionMode::Jit
        } else {
            ExecutionMode::Vm
        }
    }
}

pub fn run(jit: bool) -> io::Result<()> {
    let stdin = io::stdin();
    let mut session = ReplSession::new(jit);

    eprintln!(
        "Flux REPL (v{}){}. Type :help for commands.\n",
        env!("CARGO_PKG_VERSION"),
        if jit { " [JIT]" } else { "" }
    );

    loop {
        let input = read_input(PROMPT, &stdin)?;
        let trimmed = input.trim();

        if trimmed.is_empty() {
            continue;
        }

        if trimmed.starts_with(':') {
            match handle_command(trimmed, &mut session) {
                CommandResult::Continue => {}
                CommandResult::Quit => break,
            }
            continue;
        }

        if !session.source.is_empty() && !session.source.ends_with('\n') {
            session.source.push('\n');
        }
        let prev_len = session.source.len();
        session.source.push_str(trimmed);
        session.source.push('\n');

        let source = source_from("repl", session.source.clone());
        match compile_with_mode(source, session.mode()) {
            Ok(compiled) => {
                session.types = compiled.types;
                print_runtime(&compiled.executor);
                session.executor = Some(compiled.executor);
            }
            Err(failed) => {
                session.source.truncate(prev_len);
                let _ = report_errors(failed.source(), &failed.sources, &failed.diagnostics);
            }
        }
    }

    Ok(())
}

enum CommandResult {
    Continue,
    Quit,
}

fn handle_command(line: &str, session: &mut ReplSession) -> CommandResult {
    if line == ":quit" || line == ":q" || line == ":exit" {
        return CommandResult::Quit;
    }
    if line == ":reset" || line == ":clear" {
        *session = ReplSession::new(session.jit);
        eprintln!("session cleared");
        return CommandResult::Continue;
    }
    if line == ":show" {
        if session.source.is_empty() {
            eprintln!("(empty session)");
        } else {
            eprintln!("--- session ---");
            eprintln!("{}", session.source);
        }
        return CommandResult::Continue;
    }
    if line == ":types" {
        if session.types.is_empty() {
            eprintln!("(no bindings typed yet)");
        } else {
            let mut names: Vec<_> = session.types.keys().collect();
            names.sort();
            for name in names {
                eprintln!("{name}: {}", session.types[name]);
            }
        }
        return CommandResult::Continue;
    }
    if let Some(name) = line.strip_prefix(":type ") {
        let name = name.trim();
        if name.is_empty() {
            eprintln!("usage: :type <name>");
        } else if let Some(ty) = session.types.get(name) {
            eprintln!("{name}: {ty}");
        } else {
            eprintln!("unknown binding: {name}");
        }
        return CommandResult::Continue;
    }
    if let Some(rest) = line.strip_prefix(":emit ") {
        emit_event(rest.trim(), session);
        return CommandResult::Continue;
    }
    if line == ":help" || line == ":h" {
        eprintln!(
            "Commands:\n\
               :help              — this message\n\
               :show              — print accumulated source\n\
               :reset / :clear     — clear session\n\
               :types             — list all binding types\n\
               :type <name>       — show type of a binding\n\
               :emit <Event> [v]  — emit an event with an optional payload\n\
               :quit              — exit\n\
             \n\
             Bindings (v2 syntax):\n\
               let mut count = 0     — mutable source binding\n\
               count = count + 1     — assign only inside on/watch\n\
               let doubled = count * 2   — derived (immutable)\n\
             \n\
             Multiline: open {{ without closing on the same input block."
        );
        return CommandResult::Continue;
    }

    eprintln!("unknown command: {line} (try :help)");
    CommandResult::Continue
}

fn emit_event(rest: &str, session: &mut ReplSession) {
    let Some(executor) = session.executor.as_mut() else {
        eprintln!("no compiled program yet — declare `event {rest}` and some `on` handlers first");
        return;
    };
    let (event, payload) = match rest.split_once(char::is_whitespace) {
        Some((event, payload)) => (event, parse_payload(payload)),
        None => (rest, RuntimeValue::Unit),
    };
    if event.is_empty() {
        eprintln!("usage: :emit <Event> [payload]");
        return;
    }
    executor.emit(event, payload);
    print_runtime(executor);
}

/// Parses a REPL `:emit` payload literal (int, float, bool, quoted string, or
/// a bare word treated as a string). This is intentionally minimal — it is not
/// the Flux expression parser, just enough to poke events interactively.
fn parse_payload(text: &str) -> RuntimeValue {
    let text = text.trim();
    if let Ok(i) = text.parse::<i64>() {
        return RuntimeValue::int_i64(i);
    }
    if let Ok(f) = text.parse::<f64>() {
        return RuntimeValue::float_f64(f);
    }
    match text {
        "true" => return RuntimeValue::Bool(true),
        "false" => return RuntimeValue::Bool(false),
        _ => {}
    }
    if let Some(inner) = text.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
        return RuntimeValue::String(inner.to_string());
    }
    RuntimeValue::String(text.to_string())
}

fn read_input(primary: &str, stdin: &io::Stdin) -> io::Result<String> {
    let mut buf = read_line(primary, stdin)?;
    if buf.trim().starts_with(':') {
        return Ok(buf);
    }
    while open_braces(&buf) > 0 {
        let more = read_line(CONT_PROMPT, stdin)?;
        if !buf.ends_with('\n') {
            buf.push('\n');
        }
        buf.push_str(&more);
    }
    Ok(buf)
}

fn read_line(prompt: &str, stdin: &io::Stdin) -> io::Result<String> {
    print!("{prompt}");
    io::stdout().flush()?;
    let mut buf = String::new();
    stdin.read_line(&mut buf)?;
    Ok(buf)
}

fn open_braces(s: &str) -> i32 {
    let mut depth = 0i32;
    for ch in s.chars() {
        match ch {
            '{' => depth += 1,
            '}' => depth -= 1,
            _ => {}
        }
    }
    depth
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_braces_counts_nesting() {
        assert_eq!(open_braces("fn f() { let x = 1"), 1);
        assert_eq!(open_braces("fn f() { }"), 0);
    }
}
