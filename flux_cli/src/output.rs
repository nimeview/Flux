use flux_diagnostics::{DiagnosticBag, FluxWarning, SourceFile, SourceId};
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
pub fn report_warnings(
    primary: &SourceFile,
    sources: &[SourceFile],
    warnings: &[FluxWarning],
) -> Result<(), ()> {
    if warnings.is_empty() {
        return Ok(());
    }
    let bag = DiagnosticBag {
        errors: Vec::new(),
        warnings: warnings.to_vec(),
    };
    report_errors(primary, sources, &bag)
}

/// Renders every diagnostic against the source its `SourceId` names, falling
/// back to `primary` when that file was never read (e.g. a missing import).
#[allow(clippy::result_unit_err)]
pub fn report_errors(
    primary: &SourceFile,
    sources: &[SourceFile],
    bag: &DiagnosticBag,
) -> Result<(), ()> {
    let handler = GraphicalReportHandler::new();
    for warning in &bag.warnings {
        let source = source_for(primary, sources, warning.source_id());
        render_diagnostic(&handler, source, warning.clone())?;
    }
    if bag.errors.is_empty() {
        return Ok(());
    }
    for error in &bag.errors {
        let source = source_for(primary, sources, error.source_id());
        render_diagnostic(&handler, source, error.clone())?;
    }
    Err(())
}

/// Reports a failed compilation. Always returns `Err`, even if the bag is
/// unexpectedly empty, so a failure can never exit as success.
#[allow(clippy::result_unit_err)]
pub fn report_failure(
    primary: &SourceFile,
    sources: &[SourceFile],
    bag: &DiagnosticBag,
) -> Result<(), ()> {
    if report_errors(primary, sources, bag).is_ok() {
        eprintln!("error: {}: compilation failed", primary.path);
    }
    Err(())
}

fn source_for<'a>(
    primary: &'a SourceFile,
    sources: &'a [SourceFile],
    id: SourceId,
) -> &'a SourceFile {
    sources.iter().find(|s| s.id == id).unwrap_or(primary)
}

fn render_diagnostic<D: miette::Diagnostic + Send + Sync + 'static>(
    handler: &GraphicalReportHandler,
    source: &SourceFile,
    diagnostic: D,
) -> Result<(), ()> {
    eprint!("{}", render_with(handler, source, diagnostic)?);
    Ok(())
}

/// Longest source line shown in a snippet; miette cannot lay out much longer
/// lines (it panics), so such a diagnostic gets a `path:line:column` instead.
const MAX_SNIPPET_LINE: usize = 1000;

fn render_with<D: miette::Diagnostic + Send + Sync + 'static>(
    handler: &GraphicalReportHandler,
    source: &SourceFile,
    diagnostic: D,
) -> Result<String, ()> {
    let offsets: Vec<usize> = diagnostic
        .labels()
        .map(|labels| labels.map(|l| l.offset()).collect())
        .unwrap_or_default();
    if let Some(&offset) = offsets
        .iter()
        .find(|&&o| line_around(&source.contents, o).len() > MAX_SNIPPET_LINE)
    {
        let mut output = String::new();
        handler
            .render_report(&mut output, &diagnostic)
            .map_err(|e| eprintln!("{e:?}"))?;
        let before = &source.contents[..offset.min(source.contents.len())];
        let line = before.matches('\n').count() + 1;
        let column = before.len() - before.rfind('\n').map_or(0, |i| i + 1) + 1;
        output.push_str(&format!(
            "  at {}:{line}:{column} (line too long to show)\n",
            source.path
        ));
        return Ok(output);
    }
    let report = Report::new(diagnostic).with_source_code(source.clone());
    let mut output = String::new();
    handler
        .render_report(&mut output, report.as_ref())
        .into_diagnostic()
        .map_err(|e| eprintln!("{e:?}"))
        .map_err(|_| ())?;
    Ok(output)
}

/// The line of `contents` that contains byte `offset`.
fn line_around(contents: &str, offset: usize) -> &str {
    let offset = offset.min(contents.len());
    let start = contents[..offset].rfind('\n').map_or(0, |i| i + 1);
    let end = contents[offset..]
        .find('\n')
        .map_or(contents.len(), |i| offset + i);
    &contents[start..end]
}
