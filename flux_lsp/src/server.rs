//! Stdio LSP server: diagnostics + hover + completion, wired up by `flux lsp-server`.

use std::collections::HashMap;
use std::error::Error;

use lsp_server::{Connection, Message, Request as ServerRequest, RequestId, Response};
use lsp_types::notification::{
    DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, Notification,
    PublishDiagnostics,
};
use lsp_types::request::{
    Completion, DocumentSymbolRequest, Formatting, GotoDefinition, HoverRequest, Rename, Request,
};
use lsp_types::{
    CompletionItem, CompletionItemKind, CompletionOptions, CompletionParams, Diagnostic,
    DiagnosticSeverity, DidChangeTextDocumentParams, DidCloseTextDocumentParams,
    DidOpenTextDocumentParams, DocumentFormattingParams, DocumentSymbol, DocumentSymbolParams,
    DocumentSymbolResponse, GotoDefinitionParams, GotoDefinitionResponse, Hover, HoverContents,
    HoverParams, HoverProviderCapability, Location, MarkupContent, MarkupKind, NumberOrString,
    OneOf, Position, PublishDiagnosticsParams, Range, RenameParams, ServerCapabilities, SymbolKind,
    TextDocumentSyncCapability, TextDocumentSyncKind, TextDocumentSyncOptions, TextEdit, Url,
    WorkspaceEdit,
};

use flux_fmt::format_source;

use flux_hir::HirItem;

use crate::analysis::{
    completion_prefix, definition_at_position, describe_type, hover_for_name,
    hover_range_for_symbol, member_completions, outline_symbols, position_to_offset,
    rename_edits_for_symbol, span_to_lsp_range, symbol_at_position, word_at_offset, MemberKind,
    OutlineKind,
};
use crate::documents::DocumentStore;

const KEYWORDS: &[&str] = &[
    "fn", "rfn", "watch", "on", "event", "emit", "let", "mut", "derived", "const", "if", "else",
    "while", "for", "in", "return", "break", "continue", "trait", "impl", "struct", "enum",
    "import", "match", "true", "false", "snap",
];

/// Server capabilities advertised during the `initialize` handshake.
pub fn server_capabilities() -> ServerCapabilities {
    ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Options(
            TextDocumentSyncOptions {
                open_close: Some(true),
                change: Some(TextDocumentSyncKind::INCREMENTAL),
                save: None,
                will_save: None,
                will_save_wait_until: None,
            },
        )),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        definition_provider: Some(OneOf::Left(true)),
        completion_provider: Some(CompletionOptions {
            trigger_characters: Some(vec![".".to_string()]),
            ..Default::default()
        }),
        document_formatting_provider: Some(OneOf::Left(true)),
        document_symbol_provider: Some(OneOf::Left(true)),
        rename_provider: Some(OneOf::Left(true)),
        ..Default::default()
    }
}

/// Runs the Flux language server over stdio until the client disconnects.
pub fn run() -> Result<(), Box<dyn Error + Sync + Send>> {
    let (connection, io_threads) = Connection::stdio();
    let server_capabilities = serde_json::to_value(server_capabilities())?;
    let init_params = connection.initialize(server_capabilities)?;
    let _init_params: lsp_types::InitializeParams = serde_json::from_value(init_params)?;

    main_loop(&connection)?;
    io_threads.join()?;
    Ok(())
}

/// Drives the request/notification loop for an already-initialized [`Connection`].
/// Exposed so tests can run the server against [`Connection::memory`] without stdio.
pub fn main_loop(connection: &Connection) -> Result<(), Box<dyn Error + Sync + Send>> {
    let mut store = DocumentStore::new();

    for msg in &connection.receiver {
        match msg {
            Message::Request(req) => {
                if connection.handle_shutdown(&req)? {
                    return Ok(());
                }
                handle_request(connection, &mut store, req)?;
            }
            Message::Notification(not) => match not.method.as_str() {
                DidOpenTextDocument::METHOD => {
                    let params: DidOpenTextDocumentParams = serde_json::from_value(not.params)?;
                    let uri = params.text_document.uri;
                    store.open(
                        uri.clone(),
                        params.text_document.version,
                        params.text_document.text,
                    );
                    publish_diagnostics(connection, &mut store, &uri)?;
                }
                DidChangeTextDocument::METHOD => {
                    let params: DidChangeTextDocumentParams = serde_json::from_value(not.params)?;
                    let uri = params.text_document.uri;
                    store.apply_changes(&uri, params.text_document.version, params.content_changes);
                    publish_diagnostics(connection, &mut store, &uri)?;
                }
                DidCloseTextDocument::METHOD => {
                    let params: DidCloseTextDocumentParams = serde_json::from_value(not.params)?;
                    store.close(&params.text_document.uri);
                }
                _ => {}
            },
            Message::Response(_) => {}
        }
    }
    Ok(())
}

fn handle_request(
    connection: &Connection,
    store: &mut DocumentStore,
    req: ServerRequest,
) -> Result<(), Box<dyn Error + Sync + Send>> {
    match req.method.as_str() {
        HoverRequest::METHOD => {
            let (id, params): (RequestId, HoverParams) = cast_request::<HoverRequest>(req)?;
            let hover = compute_hover(store, params);
            respond(connection, id, hover)?;
        }
        Completion::METHOD => {
            let (id, params): (RequestId, CompletionParams) = cast_request::<Completion>(req)?;
            let items = compute_completion(store, params);
            respond(connection, id, Some(items))?;
        }
        GotoDefinition::METHOD => {
            let (id, params): (RequestId, GotoDefinitionParams) =
                cast_request::<GotoDefinition>(req)?;
            let location = compute_definition(store, params);
            respond(connection, id, location)?;
        }
        Formatting::METHOD => {
            let (id, params): (RequestId, DocumentFormattingParams) =
                cast_request::<Formatting>(req)?;
            let edits = compute_formatting(store, params);
            respond(connection, id, edits)?;
        }
        DocumentSymbolRequest::METHOD => {
            let (id, params): (RequestId, DocumentSymbolParams) =
                cast_request::<DocumentSymbolRequest>(req)?;
            let symbols = compute_document_symbols(store, params);
            respond(connection, id, symbols)?;
        }
        Rename::METHOD => {
            let (id, params): (RequestId, RenameParams) = cast_request::<Rename>(req)?;
            let edit = compute_rename(store, params);
            respond(connection, id, edit)?;
        }
        _ => {
            connection.sender.send(Message::Response(Response::new_err(
                req.id,
                lsp_server::ErrorCode::MethodNotFound as i32,
                format!("unsupported method: {}", req.method),
            )))?;
        }
    }
    Ok(())
}

fn cast_request<R>(
    req: ServerRequest,
) -> Result<(RequestId, R::Params), Box<dyn Error + Sync + Send>>
where
    R: Request,
    R::Params: serde::de::DeserializeOwned,
{
    let params = serde_json::from_value(req.params)?;
    Ok((req.id, params))
}

fn respond<T: serde::Serialize>(
    connection: &Connection,
    id: RequestId,
    result: T,
) -> Result<(), Box<dyn Error + Sync + Send>> {
    let response = Response::new_ok(id, result);
    connection.sender.send(Message::Response(response))?;
    Ok(())
}

fn publish_diagnostics(
    connection: &Connection,
    store: &mut DocumentStore,
    uri: &Url,
) -> Result<(), Box<dyn Error + Sync + Send>> {
    let Some(analysis) = store.analyze(uri) else {
        return Ok(());
    };
    let diagnostics = analysis
        .diagnostics
        .into_iter()
        .map(|d| Diagnostic {
            range: Range::new(
                Position::new((d.start_line - 1) as u32, (d.start_column - 1) as u32),
                Position::new((d.end_line - 1) as u32, (d.end_column - 1) as u32),
            ),
            severity: Some(match d.level {
                crate::analysis::DiagnosticLevel::Error => DiagnosticSeverity::ERROR,
                crate::analysis::DiagnosticLevel::Warning => DiagnosticSeverity::WARNING,
            }),
            code: Some(NumberOrString::String(d.code)),
            source: Some("flux".to_string()),
            message: d.message,
            ..Default::default()
        })
        .collect();

    let params = PublishDiagnosticsParams {
        uri: uri.clone(),
        diagnostics,
        version: store.version(uri),
    };
    connection
        .sender
        .send(Message::Notification(lsp_server::Notification {
            method: PublishDiagnostics::METHOD.to_string(),
            params: serde_json::to_value(params)?,
        }))?;
    Ok(())
}

fn compute_hover(store: &mut DocumentStore, params: HoverParams) -> Option<Hover> {
    let uri = params.text_document_position_params.text_document.uri;
    let position = params.text_document_position_params.position;
    let text = store.text(&uri)?.to_string();
    let offset = position_to_offset(&text, position.line, position.character);
    let word = word_at_offset(&text, offset)?;
    let analysis = store.analyze(&uri);

    let text_str = if let Some(doc) = keyword_doc(word) {
        doc.to_string()
    } else {
        let analysis = analysis.as_ref()?;
        let hir = analysis.hir.as_ref()?;
        hover_for_name(hir, analysis.typed.as_ref(), word)?
    };

    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: format!("```flux\n{text_str}\n```"),
        }),
        range: analysis
            .as_ref()
            .and_then(|a| hover_range_for_symbol(a, &text, word))
            .map(|(sl, sc, el, ec)| Range::new(Position::new(sl, sc), Position::new(el, ec))),
    })
}

fn compute_completion(store: &mut DocumentStore, params: CompletionParams) -> Vec<CompletionItem> {
    let uri = params.text_document_position.text_document.uri;
    let position = params.text_document_position.position;
    let text = match store.text(&uri) {
        Some(t) => t.to_string(),
        None => return keyword_completions(),
    };

    let prefix = completion_prefix(&text, position.line, position.character);

    if let Some(analysis) = store.analyze(&uri) {
        let member_items = member_completions(&analysis, &text, position.line, position.character);
        if !member_items.is_empty() {
            return member_items
                .into_iter()
                .map(|m| CompletionItem {
                    label: m.label,
                    kind: Some(match m.kind {
                        MemberKind::Field => CompletionItemKind::FIELD,
                        MemberKind::Method => CompletionItemKind::METHOD,
                    }),
                    detail: m.detail,
                    ..Default::default()
                })
                .collect();
        }
    }

    let mut items = keyword_completions();

    let Some(analysis) = store.analyze(&uri) else {
        return filter_completions(items, &prefix);
    };
    let Some(hir) = &analysis.hir else {
        return filter_completions(items, &prefix);
    };

    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for item in &hir.items {
        let (symbol, kind) = match item {
            HirItem::Binding(b) => (b.name, CompletionItemKind::VARIABLE),
            HirItem::Fn(f) => (f.name, CompletionItemKind::FUNCTION),
            HirItem::Event(e) => (e.name, CompletionItemKind::EVENT),
            HirItem::Struct(s) => (s.name, CompletionItemKind::STRUCT),
            HirItem::Enum(e) => (e.name, CompletionItemKind::ENUM),
            _ => continue,
        };
        let Some(name) = hir.names.get(&symbol) else {
            continue;
        };
        if name.starts_with("__") || !seen.insert(name.clone()) {
            continue;
        }
        let detail = analysis
            .typed
            .as_ref()
            .and_then(|t| t.types.get(&symbol))
            .map(describe_type);
        items.push(CompletionItem {
            label: name.clone(),
            kind: Some(kind),
            detail,
            ..Default::default()
        });
    }
    filter_completions(items, &prefix)
}

fn filter_completions(items: Vec<CompletionItem>, prefix: &str) -> Vec<CompletionItem> {
    if prefix.is_empty() {
        return items;
    }
    items
        .into_iter()
        .filter(|item| item.label.starts_with(prefix))
        .collect()
}

fn compute_formatting(
    store: &DocumentStore,
    params: DocumentFormattingParams,
) -> Option<Vec<TextEdit>> {
    let uri = params.text_document.uri;
    let text = store.text(&uri)?;
    let formatted = format_source(text).ok()?;
    if formatted == *text {
        return Some(Vec::new());
    }
    let end_line = text.lines().count().max(1) as u32;
    let end_char = text
        .lines()
        .last()
        .map(|l| l.chars().count() as u32)
        .unwrap_or(0);
    Some(vec![TextEdit {
        range: Range::new(
            Position::new(0, 0),
            Position::new(end_line.saturating_sub(1), end_char),
        ),
        new_text: formatted,
    }])
}

fn compute_document_symbols(
    store: &mut DocumentStore,
    params: DocumentSymbolParams,
) -> Option<DocumentSymbolResponse> {
    let uri = params.text_document.uri;
    let text = store.text(&uri)?.to_string();
    let analysis = store.analyze(&uri)?;
    let hir = analysis.hir.as_ref()?;
    let symbols: Vec<DocumentSymbol> = outline_symbols(hir)
        .into_iter()
        .filter_map(|sym| {
            let (sl, sc, el, ec) = span_to_lsp_range(&text, &sym.span, &analysis.source_paths)?;
            Some(DocumentSymbol {
                name: sym.name,
                detail: None,
                kind: match sym.kind {
                    OutlineKind::Variable => SymbolKind::VARIABLE,
                    OutlineKind::Function => SymbolKind::FUNCTION,
                    OutlineKind::Event => SymbolKind::EVENT,
                    OutlineKind::Struct => SymbolKind::STRUCT,
                    OutlineKind::Enum => SymbolKind::ENUM,
                    OutlineKind::Trait => SymbolKind::INTERFACE,
                },
                tags: None,
                range: Range::new(Position::new(sl, sc), Position::new(el, ec)),
                selection_range: Range::new(Position::new(sl, sc), Position::new(el, ec)),
                children: None,
                #[allow(deprecated)]
                deprecated: None,
            })
        })
        .collect();
    Some(DocumentSymbolResponse::Nested(symbols))
}

fn compute_rename(store: &mut DocumentStore, params: RenameParams) -> Option<WorkspaceEdit> {
    let uri = params.text_document_position.text_document.uri;
    let position = params.text_document_position.position;
    let new_name = params.new_name.trim().to_string();
    if new_name.is_empty() || !is_ident(&new_name) {
        return None;
    }
    let text = store.text(&uri)?.to_string();
    let analysis = store.analyze(&uri)?;
    let symbol_id = symbol_at_position(&analysis, &text, position.line, position.character)?;
    let buffer_path = uri
        .to_file_path()
        .ok()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "<buffer>".to_string());
    let edits_by_path = rename_edits_for_symbol(&analysis, &text, &buffer_path, symbol_id);
    if edits_by_path.is_empty() {
        return None;
    }

    let mut changes = HashMap::new();
    for (path, ranges) in edits_by_path {
        let target_uri = if path == buffer_path {
            uri.clone()
        } else {
            Url::from_file_path(&path).ok()?
        };
        let edits: Vec<TextEdit> = ranges
            .into_iter()
            .map(|(sl, sc, el, ec)| TextEdit {
                range: Range::new(Position::new(sl, sc), Position::new(el, ec)),
                new_text: new_name.clone(),
            })
            .collect();
        changes.insert(target_uri, edits);
    }

    Some(WorkspaceEdit {
        changes: Some(changes),
        document_changes: None,
        change_annotations: None,
    })
}

fn is_ident(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_alphabetic() || first == '_') && chars.all(|c| c.is_alphanumeric() || c == '_')
}

fn keyword_completions() -> Vec<CompletionItem> {
    let mut items: Vec<CompletionItem> = KEYWORDS
        .iter()
        .map(|kw| CompletionItem {
            label: kw.to_string(),
            kind: Some(CompletionItemKind::KEYWORD),
            ..Default::default()
        })
        .collect();
    items.push(snippet("let mut", "let mut ${1:name} = ${2:0}"));
    items.push(snippet("derived", "derived ${1:name} = ${2:expr}"));
    items.push(snippet("watch", "watch ${1:target} {\n\t$0\n}"));
    items.push(snippet("on", "on ${1:Event} {\n\t$0\n}"));
    items.push(snippet(
        "fn",
        "fn ${1:name}(${2:args}) -> ${3:Type} {\n\t$0\n}",
    ));
    items
}

fn compute_definition(
    store: &mut DocumentStore,
    params: GotoDefinitionParams,
) -> Option<GotoDefinitionResponse> {
    let uri = params.text_document_position_params.text_document.uri;
    let position = params.text_document_position_params.position;
    let text = store.text(&uri)?.to_string();
    let analysis = store.analyze(&uri)?;
    let loc = definition_at_position(&analysis, &text, position.line, position.character)?;
    let (start_line, start_col, end_line, end_col) =
        span_to_lsp_range(&text, &loc.span, &analysis.source_paths)?;
    let target_uri = if loc.path == "<buffer>" {
        uri
    } else {
        Url::from_file_path(&loc.path).ok()?
    };
    Some(GotoDefinitionResponse::Scalar(Location {
        uri: target_uri,
        range: Range::new(
            Position::new(start_line, start_col),
            Position::new(end_line, end_col),
        ),
    }))
}

fn snippet(label: &str, body: &str) -> CompletionItem {
    CompletionItem {
        label: label.to_string(),
        kind: Some(CompletionItemKind::SNIPPET),
        insert_text: Some(body.to_string()),
        insert_text_format: Some(lsp_types::InsertTextFormat::SNIPPET),
        ..Default::default()
    }
}

fn keyword_doc(word: &str) -> Option<&'static str> {
    Some(match word {
        "fn" => "fn — pure function; reactive reads are forbidden (use `snap`)",
        "rfn" => "rfn — reactive function; reads of source/derived bindings are tracked",
        "watch" => "watch target { ... } — re-runs the block whenever `target` changes",
        "on" => "on Event { ... } — runs the block each time `Event` is emitted",
        "event" => "event Name(payload?) — declares an event type",
        "emit" => "emit Event(args?) — fires an event, running its `on` handlers",
        "trait" => "trait Name { fn method(...) -> T; } — declares shared behavior",
        "impl" => "impl Trait for Type { ... } — implements a trait for a type",
        "snap" => "snap(expr) — takes a non-reactive snapshot of a reactive value",
        "let" => "let name = expr — immutable binding; use `let mut` for writable sources",
        "mut" => "mut — marks a `let` binding as assignable in `on`/`watch` handlers",
        "derived" => "derived name = expr — explicit derived binding (reactive)",
        "const" => "const NAME = expr — compile-time constant module binding",
        "if" => "if cond { ... } else { ... } — conditional expression or statement",
        "else" => "else — alternative branch for `if`",
        "while" => "while cond { ... } — loop while condition is true",
        "for" => "for name in iter { ... } — iterate a range or array",
        "in" => "in — used with `for name in iter`",
        "return" => "return expr — exit the current function with a value",
        "break" => "break — exit the innermost loop",
        "continue" => "continue — skip to the next loop iteration",
        "struct" => "struct Name { field: Type, ... } — product type",
        "enum" => "enum Name { Variant, ... } — sum type",
        "import" => "import path { sym, ... } — bring symbols from another module",
        "match" => "match expr { pat => val, ... } — pattern match",
        "true" | "false" => "boolean literal",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advertises_incremental_sync() {
        let caps = server_capabilities();
        match caps.text_document_sync {
            Some(TextDocumentSyncCapability::Options(opts)) => {
                assert_eq!(opts.change, Some(TextDocumentSyncKind::INCREMENTAL));
                assert_eq!(opts.open_close, Some(true));
            }
            other => panic!("expected incremental sync options, got {other:?}"),
        }
    }
}
