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

    // The end of input (Ctrl-D, or a piped script that ends) quits.
    while let Some(input) = read_input(PROMPT, &stdin)? {
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
                let mut executor = compiled.executor;
                // The session's sources keep what they held: a new line
                // compiles the whole session again from the start.
                if let Some(old) = &session.executor {
                    carry_sources(old, &mut executor, &session.types);
                }
                print_runtime(&executor);
                session.executor = Some(executor);
            }
            Err(failed) => {
                exit_if_asked();
                session.source.truncate(prev_len);
                let _ = report_errors(failed.source(), &failed.sources, &failed.diagnostics);
            }
        }
    }

    Ok(())
}

/// `exit(code)` in what the session ran ends the REPL with that status.
fn exit_if_asked() {
    if let Some(code) = flux_std::take_exit_code() {
        std::process::exit(code);
    }
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
        exit_if_asked();
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
               :emit <Event> [v…] — emit an event with its values\n\
               :quit              — exit\n\
             \n\
             Bindings:\n\
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

/// Copies the value of each source that `new` has too, of the same type,
/// from `old`.
fn carry_sources(old: &Executor, new: &mut Executor, types: &HashMap<String, String>) {
    for (name, ty) in types {
        if !ty.starts_with("Cell<") {
            continue;
        }
        if let Some(value) = old.get_value(name) {
            if new.get_value(name).is_some_and(|current| current != value) {
                new.set_source(name, value);
            }
        }
    }
}

/// How many values the event `name` carries, from its type (`Event<Int64,
/// Int64>`), if the session declares it.
fn event_arity(name: &str, types: &HashMap<String, String>) -> Option<usize> {
    let inner = types.get(name)?.strip_prefix("Event<")?.strip_suffix('>')?;
    let mut depth = 0i32;
    let mut count = usize::from(!inner.trim().is_empty());
    for c in inner.chars() {
        match c {
            '<' | '[' | '(' => depth += 1,
            '>' | ']' | ')' => depth -= 1,
            ',' if depth == 0 => count += 1,
            _ => {}
        }
    }
    Some(count)
}

fn emit_event(rest: &str, session: &mut ReplSession) {
    let (event, values) = match rest.split_once(char::is_whitespace) {
        Some((event, payload)) => (event, split_payload(payload)),
        None => (rest, Vec::new()),
    };
    if event.is_empty() {
        eprintln!("usage: :emit <Event> [values]");
        return;
    }
    let Some(arity) = event_arity(event, &session.types) else {
        eprintln!("unknown event `{event}`: declare it first, as `event {event}`");
        return;
    };
    if values.len() != arity {
        eprintln!(
            "`{event}` carries {arity} value{}, but {} {} given: `:emit {event}{}`",
            if arity == 1 { "" } else { "s" },
            values.len(),
            if values.len() == 1 { "was" } else { "were" },
            " v".repeat(arity)
        );
        return;
    }
    let Some(executor) = session.executor.as_mut() else {
        return;
    };
    let values = values.iter().map(|v| parse_payload(v)).collect();
    executor.emit_values(event, values);
    if let Some(err) = executor.runtime_error() {
        // The failed batch's earlier writes stay, as in a program; the
        // session goes on from them with a fresh executor.
        eprintln!("error: {err}");
        exit_if_asked();
        recover(session);
        return;
    }
    print_runtime(executor);
}

/// After a runtime error, a new executor for the session's source, with the
/// sources' values from the failed one.
fn recover(session: &mut ReplSession) {
    let Some(old) = session.executor.take() else {
        return;
    };
    let source = source_from("repl", session.source.clone());
    if let Ok(compiled) = compile_with_mode(source, session.mode()) {
        let mut executor = compiled.executor;
        carry_sources(&old, &mut executor, &session.types);
        session.executor = Some(executor);
    }
}

/// The values after `:emit Event`: separated by spaces, a `"quoted string"`
/// kept whole.
fn split_payload(text: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for c in text.trim().chars() {
        match c {
            '"' => {
                quoted = !quoted;
                current.push(c);
            }
            c if c.is_whitespace() && !quoted => {
                if !current.is_empty() {
                    values.push(std::mem::take(&mut current));
                }
            }
            c => current.push(c),
        }
    }
    if !current.is_empty() {
        values.push(current);
    }
    values
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

/// One input, continued while braces are open; `None` at the end of input.
fn read_input(primary: &str, stdin: &io::Stdin) -> io::Result<Option<String>> {
    let Some(mut buf) = read_line(primary, stdin)? else {
        return Ok(None);
    };
    if buf.trim().starts_with(':') {
        return Ok(Some(buf));
    }
    while open_braces(&buf) > 0 {
        // Input that ends inside braces is what was read.
        let Some(more) = read_line(CONT_PROMPT, stdin)? else {
            break;
        };
        if !buf.ends_with('\n') {
            buf.push('\n');
        }
        buf.push_str(&more);
    }
    Ok(Some(buf))
}

fn read_line(prompt: &str, stdin: &io::Stdin) -> io::Result<Option<String>> {
    print!("{prompt}");
    io::stdout().flush()?;
    let mut buf = String::new();
    if stdin.read_line(&mut buf)? == 0 {
        return Ok(None);
    }
    Ok(Some(buf))
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
