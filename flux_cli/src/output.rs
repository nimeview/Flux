use flux_diagnostics::{DiagnosticBag, FluxWarning, SourceFile};
use flux_runtime::{EffectTrigger, Executor, HandlerKind, NodeKind, RuntimeValue};
use miette::{GraphicalReportHandler, IntoDiagnostic, Report};

pub fn print_value(name: &str, value: &RuntimeValue) {
    println!("{name} = {}", value.display_string());
}
pub fn format_runtime(executor: &Executor) -> String {
    format_runtime_with(executor, false)
}

/// Renders `Source`/`Derived` bindings, and — when `verbose` is set — also
/// lists `Event` declarations and `watch`/`on` effect handlers wired into the graph.
pub fn format_runtime_with(executor: &Executor, verbose: bool) -> String {
    let mut out = String::new();
    for node in &executor.graph.nodes {
        match &node.kind {
            NodeKind::Source | NodeKind::Derived { .. } => {
                out.push_str(&node.name);
                out.push_str(" = ");
                out.push_str(&node.value.display_string());
                out.push('\n');
            }
            NodeKind::Event if verbose => {
                out.push_str("event ");
                out.push_str(&node.name);
                out.push('\n');
            }
            NodeKind::Effect { meta } if verbose => {
                let kind = match meta.handler_kind {
                    HandlerKind::Watch => "watch",
                    HandlerKind::On => "on",
                };
                let target = match &meta.trigger {
                    EffectTrigger::OnEvent(event) => event.clone(),
                    EffectTrigger::Watch { target: Some(t) } => t.clone(),
                    EffectTrigger::Watch { target: None } => "*".to_string(),
                };
                out.push_str(&format!("{kind} {target} (effect)\n"));
            }
            _ => {}
        }
    }
    out
}

pub fn print_runtime(executor: &Executor) {
    print_runtime_with(executor, false);
}

pub fn print_runtime_with(executor: &Executor, verbose: bool) {
    print!("{}", format_runtime_with(executor, verbose));
}

#[cfg(feature = "jit")]
pub fn print_jit_stats(stats: &flux_runtime::JitStats) {
    eprintln!("--- JIT stats ---");
    eprintln!("compiled: {}", stats.compiled);
    eprintln!("vm_fallback: {}", stats.fallback);
    if !stats.reasons.is_empty() {
        eprintln!("fallback reasons:");
        for (reason, count) in &stats.reasons {
            eprintln!("  {reason}: {count}");
        }
    }
    if !stats.fallback_nodes.is_empty() {
        eprintln!("fallback nodes: {}", stats.fallback_nodes.join(", "));
    }
}

#[cfg(not(feature = "jit"))]
pub fn print_jit_stats(_stats: &()) {}

#[allow(clippy::result_unit_err)]
pub fn report_warnings(source: &SourceFile, warnings: &[FluxWarning]) -> Result<(), ()> {
    if warnings.is_empty() {
        return Ok(());
    }
    let bag = DiagnosticBag {
        errors: Vec::new(),
        warnings: warnings.to_vec(),
    };
    report_errors(source, &bag)
}

#[allow(clippy::result_unit_err)]
pub fn report_errors(source: &SourceFile, bag: &DiagnosticBag) -> Result<(), ()> {
    let handler = GraphicalReportHandler::new();
    for warning in &bag.warnings {
        render_diagnostic(&handler, source, warning.clone())?;
    }
    if bag.errors.is_empty() {
        return Ok(());
    }
    for error in &bag.errors {
        render_diagnostic(&handler, source, error.clone())?;
    }
    Err(())
}

fn render_diagnostic<D: miette::Diagnostic + Send + Sync + 'static>(
    handler: &GraphicalReportHandler,
    source: &SourceFile,
    diagnostic: D,
) -> Result<(), ()> {
    let report = Report::new(diagnostic).with_source_code(source.clone());
    let mut output = String::new();
    handler
        .render_report(&mut output, report.as_ref())
        .into_diagnostic()
        .map_err(|e| eprintln!("{e:?}"))
        .map_err(|_| ())?;
    eprint!("{output}");
    Ok(())
}
