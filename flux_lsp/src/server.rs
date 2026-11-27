//! Stdio LSP server: diagnostics + hover + completion, wired up by `flux lsp-server`.

use std::collections::{HashMap, HashSet};
use std::error::Error;

use lsp_server::{Connection, Message, Request as ServerRequest, RequestId, Response};
use lsp_types::notification::{
    DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, DidSaveTextDocument,
    Notification, PublishDiagnostics,
};
use lsp_types::request::{
    CodeActionRequest, Completion, DocumentHighlightRequest, DocumentSymbolRequest, Formatting,
    GotoDefinition, HoverRequest, InlayHintRequest, References, Rename, Request,
    SignatureHelpRequest,
};
use lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, CodeActionParams,
    CodeActionProviderCapability, CompletionItem, CompletionItemKind, CompletionOptions,
    CompletionParams, Diagnostic, DiagnosticSeverity, DidChangeTextDocumentParams,
    DidCloseTextDocumentParams, DidOpenTextDocumentParams, DidSaveTextDocumentParams,
    DocumentFormattingParams, DocumentHighlight, DocumentHighlightParams, DocumentSymbol,
    DocumentSymbolParams, DocumentSymbolResponse, Documentation, GotoDefinitionParams,
    GotoDefinitionResponse, Hover, HoverContents, HoverParams, HoverProviderCapability, InlayHint,
    InlayHintKind, InlayHintLabel, InlayHintParams, Location, MarkupContent, MarkupKind,
    NumberOrString, OneOf, ParameterInformation, ParameterLabel, Position,
    PublishDiagnosticsParams, Range, ReferenceParams, RenameParams, ServerCapabilities,
    SignatureHelp, SignatureHelpOptions, SignatureHelpParams, SignatureInformation, SymbolKind,
    TextDocumentSyncCapability, TextDocumentSyncKind, TextDocumentSyncOptions,
    TextDocumentSyncSaveOptions, TextEdit, Url, WorkspaceEdit,
};

use flux_fmt::format_source;

use flux_hir::HirItem;

use crate::analysis::{
    completion_prefix, definition_at_position, describe_type, hover_for_name,
    hover_range_for_symbol, inferred_bindings, local_type, member_completions, outline_symbols,
    position_to_offset, rename_edits_for_symbol, span_to_lsp_range, symbol_at_position,
    user_signature, word_at_offset, MemberKind, OutlineKind,
};
use crate::assist::{
    enclosing_call_at, import_edit, imports, is_imported, names_in_scope, signature_params,
    std_doc, PRELUDE, STD_DOCS, TYPE_NAMES,
};
use crate::documents::DocumentStore;
use crate::position::offset_to_position;

const KEYWORDS: &[&str] = &[
    "fn", "track", "act", "watch", "on", "event", "emit", "let", "mut", "derived", "const", "if",
    "else", "while", "for", "in", "return", "break", "continue", "trait", "impl", "struct", "enum",
    "import", "match", "true", "false", "snap", "with",
];

/// Server capabilities advertised during the `initialize` handshake.
pub fn server_capabilities() -> ServerCapabilities {
    ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Options(
            TextDocumentSyncOptions {
                open_close: Some(true),
                change: Some(TextDocumentSyncKind::INCREMENTAL),
                save: Some(TextDocumentSyncSaveOptions::Supported(true)),
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
        references_provider: Some(OneOf::Left(true)),
        document_highlight_provider: Some(OneOf::Left(true)),
        signature_help_provider: Some(SignatureHelpOptions {
            trigger_characters: Some(vec!["(".to_string(), ",".to_string()]),
            ..Default::default()
        }),
        inlay_hint_provider: Some(OneOf::Left(true)),
        code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
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
                    let Some(params) = notification_params::<DidOpenTextDocumentParams>(not) else {
                        continue;
                    };
                    let uri = params.text_document.uri;
                    store.open(
                        uri.clone(),
                        params.text_document.version,
                        params.text_document.text,
                    );
                    publish_diagnostics(connection, &mut store, &uri)?;
                    // Its buffer may be what another document was missing.
                    publish_dependents(connection, &mut store, &uri)?;
                }
                DidChangeTextDocument::METHOD => {
                    let Some(params) = notification_params::<DidChangeTextDocumentParams>(not)
                    else {
                        continue;
                    };
                    let uri = params.text_document.uri;
                    store.apply_changes(&uri, params.text_document.version, params.content_changes);
                    publish_diagnostics(connection, &mut store, &uri)?;
                    publish_dependents(connection, &mut store, &uri)?;
                }
                DidSaveTextDocument::METHOD => {
                    let Some(params) = notification_params::<DidSaveTextDocumentParams>(not) else {
                        continue;
                    };
                    let uri = params.text_document.uri;
                    publish_diagnostics(connection, &mut store, &uri)?;
                    publish_dependents(connection, &mut store, &uri)?;
                }
                DidCloseTextDocument::METHOD => {
                    let Some(params) = notification_params::<DidCloseTextDocumentParams>(not)
                    else {
                        continue;
                    };
                    let uri = params.text_document.uri;
                    // Dependents read this buffer; they now read the file again.
                    let dependents = store.dependents(&uri);
                    store.close(&uri);
                    for params in close_diagnostics(&mut store, &uri) {
                        send_diagnostics(connection, params)?;
                    }
                    for dependent in dependents {
                        publish_diagnostics(connection, &mut store, &dependent)?;
                    }
                }
                _ => {}
            },
            Message::Response(_) => {}
        }
    }
    Ok(())
}

/// A notification's parameters; one that cannot be read is reported and
/// skipped, since a client's mistake must not stop the server.
fn notification_params<P: serde::de::DeserializeOwned>(not: lsp_server::Notification) -> Option<P> {
    match serde_json::from_value(not.params) {
        Ok(params) => Some(params),
        Err(e) => {
            eprintln!("flux lsp: ignoring malformed {}: {e}", not.method);
            None
        }
    }
}

fn handle_request(
    connection: &Connection,
    store: &mut DocumentStore,
    req: ServerRequest,
) -> Result<(), Box<dyn Error + Sync + Send>> {
    let (id, method) = (req.id.clone(), req.method.clone());
    match dispatch_request(connection, store, req) {
        Err(e) if e.is::<serde_json::Error>() => {
            let response = Response::new_err(
                id,
                lsp_server::ErrorCode::InvalidParams as i32,
                format!("{method}: {e}"),
            );
            connection.sender.send(Message::Response(response))?;
            Ok(())
        }
        other => other,
    }
}

fn dispatch_request(
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
        SignatureHelpRequest::METHOD => {
            let (id, params): (RequestId, SignatureHelpParams) =
                cast_request::<SignatureHelpRequest>(req)?;
            let help = compute_signature_help(store, params);
            respond(connection, id, help)?;
        }
        References::METHOD => {
            let (id, params): (RequestId, ReferenceParams) = cast_request::<References>(req)?;
            let found = compute_references(store, params);
            respond(connection, id, found)?;
        }
        DocumentHighlightRequest::METHOD => {
            let (id, params): (RequestId, DocumentHighlightParams) =
                cast_request::<DocumentHighlightRequest>(req)?;
            let found = compute_highlights(store, params);
            respond(connection, id, found)?;
        }
        InlayHintRequest::METHOD => {
            let (id, params): (RequestId, InlayHintParams) = cast_request::<InlayHintRequest>(req)?;
            let hints = compute_inlay_hints(store, params);
            respond(connection, id, Some(hints))?;
        }
        CodeActionRequest::METHOD => {
            let (id, params): (RequestId, CodeActionParams) =
                cast_request::<CodeActionRequest>(req)?;
            let actions = compute_code_actions(store, params);
            respond(connection, id, Some(actions))?;
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
    for params in document_diagnostics(store, uri) {
        send_diagnostics(connection, params)?;
    }
    Ok(())
}

/// Re-publishes open documents that import `uri`; each re-analyzes only if a
/// file it read changed on disk.
fn publish_dependents(
    connection: &Connection,
    store: &mut DocumentStore,
    uri: &Url,
) -> Result<(), Box<dyn Error + Sync + Send>> {
    for dependent in store.dependents(uri) {
        publish_diagnostics(connection, store, &dependent)?;
    }
    Ok(())
}

fn send_diagnostics(
    connection: &Connection,
    params: PublishDiagnosticsParams,
) -> Result<(), Box<dyn Error + Sync + Send>> {
    connection
        .sender
        .send(Message::Notification(lsp_server::Notification {
            method: PublishDiagnostics::METHOD.to_string(),
            params: serde_json::to_value(params)?,
        }))?;
    Ok(())
}

/// One analysis diagnostic as the protocol's.
fn to_lsp_diagnostic(d: crate::analysis::RangedDiagnostic) -> Diagnostic {
    Diagnostic {
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
        data: d.fix.map(|fix| {
            serde_json::json!({
                "title": fix.title,
                "replacement": fix.replacement,
                "range": Range::new(
                    Position::new((fix.start_line - 1) as u32, (fix.start_column - 1) as u32),
                    Position::new((fix.end_line - 1) as u32, (fix.end_column - 1) as u32),
                ),
            })
        }),
        ..Default::default()
    }
}

/// Diagnostics only an importer's analysis finds: a watch cycle (E029) that
/// runs through the imported file and the importer, which the imported
/// file's own analysis cannot see.
fn seen_only_from_importer(code: &str) -> bool {
    code == "E029"
}

/// What open document `uri` shows: its own analysis's diagnostics, plus the
/// ones open importers found in it that it cannot find itself.
fn open_document_diagnostics(store: &mut DocumentStore, uri: &Url) -> Vec<Diagnostic> {
    let mut own: Vec<Diagnostic> = store
        .analyze(uri)
        .map(|analysis| {
            analysis
                .diagnostics
                .into_iter()
                .filter(|d| d.path.is_none())
                .map(to_lsp_diagnostic)
                .collect()
        })
        .unwrap_or_default();
    for d in store.foreign_diagnostics(uri) {
        if !own.contains(&d) {
            own.push(d);
        }
    }
    own
}

/// Every `publishDiagnostics` that analyzing `uri` produces. Diagnostics in
/// imported files go to those files' URIs: a closed file gets them all; an
/// open one keeps its own analysis's, plus those only an importer can find
/// (see [`seen_only_from_importer`]). Files that no longer have any get an
/// empty list.
pub fn document_diagnostics(store: &mut DocumentStore, uri: &Url) -> Vec<PublishDiagnosticsParams> {
    let Some(analysis) = store.analyze(uri) else {
        return Vec::new();
    };
    let mut by_uri: HashMap<Url, Vec<Diagnostic>> = HashMap::new();
    by_uri.insert(uri.clone(), Vec::new());
    for d in analysis.diagnostics {
        let target = match d.path.as_deref() {
            None => uri.clone(),
            Some(path) => match Url::from_file_path(path) {
                Ok(target) if !store.is_open(&target) || seen_only_from_importer(&d.code) => target,
                _ => continue,
            },
        };
        by_uri.entry(target).or_default().push(to_lsp_diagnostic(d));
    }
    let mut own = by_uri.remove(uri).unwrap_or_default();
    let reported: HashSet<Url> = by_uri.keys().cloned().collect();
    let stale = store.replace_foreign(uri, by_uri);
    // What importers found in this file that its own analysis cannot.
    for d in store.foreign_diagnostics(uri) {
        if !own.contains(&d) {
            own.push(d);
        }
    }
    let mut out = vec![PublishDiagnosticsParams {
        version: store.version(uri),
        uri: uri.clone(),
        diagnostics: own,
    }];
    for target in reported.union(&stale) {
        let diagnostics = if store.is_open(target) {
            open_document_diagnostics(store, target)
        } else {
            store.foreign_diagnostics(target)
        };
        out.push(PublishDiagnosticsParams {
            version: store.version(target),
            uri: target.clone(),
            diagnostics,
        });
    }
    // Root first, then imported files in a stable order.
    out.sort_by_key(|p| (p.uri != *uri, p.uri.to_string()));
    out
}

/// Clears diagnostics for a closed document and the imported files it reported on.
pub fn close_diagnostics(store: &mut DocumentStore, uri: &Url) -> Vec<PublishDiagnosticsParams> {
    let mut targets: Vec<Url> = store
        .replace_foreign(uri, HashMap::new())
        .into_iter()
        .collect();
    targets.sort_by_key(|u| u.to_string());
    let mut out = vec![PublishDiagnosticsParams {
        uri: uri.clone(),
        diagnostics: Vec::new(),
        version: None,
    }];
    // An open file loses what this one found in it, and keeps its own.
    for target in targets {
        let diagnostics = if store.is_open(&target) {
            open_document_diagnostics(store, &target)
        } else {
            store.foreign_diagnostics(&target)
        };
        out.push(PublishDiagnosticsParams {
            diagnostics,
            version: store.version(&target),
            uri: target,
        });
    }
    out
}

fn compute_hover(store: &mut DocumentStore, params: HoverParams) -> Option<Hover> {
    let uri = params.text_document_position_params.text_document.uri;
    let position = params.text_document_position_params.position;
    let text = store.text(&uri)?.to_string();
    let offset = position_to_offset(&text, position.line, position.character);
    let word = word_at_offset(&text, offset)?;
    let analysis = store.analyze(&uri);

    // A library method: `xs.sorted()` with the cursor on `sorted`.
    let mut at = offset.min(text.len());
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    let start = text[..at]
        .char_indices()
        .rev()
        .find(|(_, c)| !(c.is_alphanumeric() || *c == '_'))
        .map_or(0, |(i, c)| i + c.len_utf8());
    if let Some(dot) = text[..start].strip_suffix('.').map(str::len) {
        let doc = analysis
            .as_ref()
            .and_then(|a| crate::analysis::library_method_doc(a, &text, dot, word));
        if let Some((signature, doc)) = doc {
            return Some(Hover {
                contents: HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: format!("```flux\n{signature}\n```\n{doc}\n\nLibrary method."),
                }),
                range: None,
            });
        }
    }

    // A method or type function: found by position, as its internal name is
    // not the word under the cursor.
    // A component's member (`n` in its body, `a.n` outside) likewise.
    let method = analysis.as_ref().and_then(|a| {
        let id = crate::analysis::live_member_at(a, &text, offset, word).or_else(|| {
            crate::analysis::symbol_at_position(a, &text, position.line, position.character)
        })?;
        let name = a.hir.as_ref()?.names.get(&id)?;
        if !(name.starts_with("__impl_") || name.starts_with("__live_") || name.contains("::")) {
            return None;
        }
        let ty = a.typed.as_ref()?.types.get(&id)?;
        let ty = match ty {
            flux_types::Type::Cell(inner) | flux_types::Type::Read(inner) => inner.as_ref(),
            other => other,
        };
        Some(format!("{word}: {}", crate::analysis::describe_type(ty)))
    });
    // A name the program defines itself hides the library's.
    let own_name = analysis.as_ref().is_some_and(|a| {
        a.hir.as_ref().is_some_and(|hir| {
            hir.symbols
                .values()
                .any(|s| s.name == word && s.kind != flux_hir::SymbolKind::Builtin)
        })
    });
    if !own_name && method.is_none() {
        if let Some(value) = library_markdown(word) {
            return Some(Hover {
                contents: HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value,
                }),
                range: None,
            });
        }
    }
    let user = method.or_else(|| {
        analysis
            .as_ref()
            .and_then(|a| hover_for_name(a.hir.as_ref()?, a.typed.as_ref(), word))
    });
    let text_str = match keyword_doc(word) {
        Some(doc) => doc.to_string(),
        None => user?,
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
    let offset = position_to_offset(&text, position.line, position.character);
    let mut seen: HashSet<String> = HashSet::new();

    // Everything from here on works from the text, while the file is broken too.
    let analysis = store.analyze(&uri);
    let (locals, item_range) = names_in_scope(&text, offset - prefix.len());
    for name in locals {
        if seen.insert(name.clone()) {
            let detail = analysis
                .as_ref()
                .and_then(|a| local_type(a, &name, &item_range));
            items.push(CompletionItem {
                label: name,
                kind: Some(CompletionItemKind::VARIABLE),
                detail,
                sort_text: Some("0".to_string()),
                ..Default::default()
            });
        }
    }
    library_completions(&text, &prefix, &mut seen, &mut items);
    items.extend(TYPE_NAMES.iter().map(|name| CompletionItem {
        label: name.to_string(),
        kind: Some(CompletionItemKind::CLASS),
        ..Default::default()
    }));

    let Some(analysis) = analysis else {
        return filter_completions(items, &prefix);
    };
    let Some(hir) = &analysis.hir else {
        return filter_completions(items, &prefix);
    };

    // Variants of every enum, the imported modules' included.
    for item in &hir.items {
        let HirItem::Enum(e) = item else { continue };
        let owner = hir
            .names
            .get(&e.name)
            .map_or("", |n| flux_value::type_display_name(n));
        for (variant, _) in &e.variants {
            if !variant.starts_with("__") && seen.insert(variant.clone()) {
                items.push(CompletionItem {
                    label: variant.clone(),
                    kind: Some(CompletionItemKind::ENUM_MEMBER),
                    detail: Some(owner.to_string()),
                    ..Default::default()
                });
            }
        }
    }
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
        // An imported module's type, by the bare name the file uses.
        let imported = hir
            .root_types
            .iter()
            .find(|(_, internal)| *internal == name)
            .map(|(bare, _)| bare);
        let name = match imported {
            Some(bare) => bare,
            None if name.starts_with("__") => continue,
            None => name,
        };
        if !seen.insert(name.clone()) {
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
    let prefix = prefix.to_lowercase();
    items
        .into_iter()
        .filter(|item| item.label.to_lowercase().starts_with(&prefix))
        .collect()
}

/// The documentation of a standard library or prelude name.
fn library_markdown(name: &str) -> Option<String> {
    if let Some((_, module, signature, doc)) = std_doc(name) {
        return Some(format!(
            "```flux\n{signature}\n```\n{doc}\n\nFrom `{module}`."
        ));
    }
    PRELUDE
        .iter()
        .find(|entry| entry.0 == name)
        .map(|(_, signature, doc)| format!("```flux\n{signature}\n```\n{doc}"))
}

fn import_text_edit(text: &str, module: &str, name: &str) -> TextEdit {
    let (line, start, end, new_text) = import_edit(text, module, name);
    TextEdit {
        range: Range::new(Position::new(line, start), Position::new(line, end)),
        new_text,
    }
}

/// The prelude, what the file imports, and, once a name is being typed, the rest of
/// the standard library, with the import added on accept.
fn library_completions(
    text: &str,
    prefix: &str,
    seen: &mut HashSet<String>,
    items: &mut Vec<CompletionItem>,
) {
    let imported = imports(text);
    for (name, signature, doc) in PRELUDE {
        if seen.insert(name.to_string()) {
            items.push(CompletionItem {
                label: name.to_string(),
                kind: Some(CompletionItemKind::CONSTRUCTOR),
                detail: Some(signature.to_string()),
                documentation: Some(Documentation::String(doc.to_string())),
                ..Default::default()
            });
        }
    }
    for (name, module, signature, doc) in STD_DOCS {
        let available = is_imported(&imported, module, name)
            // `assert` and `assert_eq` are exported by `math` too.
            || (*module == "testing" && is_imported(&imported, "math", name));
        if !available && prefix.is_empty() {
            continue;
        }
        if !seen.insert(name.to_string()) {
            continue;
        }
        items.push(CompletionItem {
            label: name.to_string(),
            kind: Some(CompletionItemKind::FUNCTION),
            detail: Some(if available {
                signature.to_string()
            } else {
                format!("{signature} (import from {module})")
            }),
            documentation: Some(Documentation::String(doc.to_string())),
            sort_text: Some(format!("{}{name}", if available { "1" } else { "9" })),
            additional_text_edits: (!available).then(|| vec![import_text_edit(text, module, name)]),
            ..Default::default()
        });
    }
}

fn compute_signature_help(
    store: &mut DocumentStore,
    params: SignatureHelpParams,
) -> Option<SignatureHelp> {
    let uri = params.text_document_position_params.text_document.uri;
    let position = params.text_document_position_params.position;
    let text = store.text(&uri)?.to_string();
    let offset = position_to_offset(&text, position.line, position.character);
    let (name, argument, dot) = enclosing_call_at(&text, offset)?;
    let analysis = store.analyze(&uri);
    // `xs.slice(` : the library method, when the call is on a library value.
    let method = dot
        .and_then(|dot| crate::analysis::library_method_doc(analysis.as_ref()?, &text, dot, &name));
    let user = if method.is_some() {
        None
    } else {
        analysis.as_ref().and_then(|a| user_signature(a, &name))
    };
    let (label, parameters, documentation) = match (method, user) {
        (Some((signature, doc)), _) => (
            signature.to_string(),
            signature_params(signature),
            Some(doc),
        ),
        (None, user) => match user {
            Some((parameters, ret)) => {
                let ret = ret.map(|r| format!(" -> {r}")).unwrap_or_default();
                (
                    format!("{name}({}){ret}", parameters.join(", ")),
                    parameters,
                    None,
                )
            }
            None => {
                let (signature, doc) =
                    std_doc(&name).map(|entry| (entry.2, entry.3)).or_else(|| {
                        PRELUDE
                            .iter()
                            .find(|entry| entry.0 == name)
                            .map(|entry| (entry.1, entry.2))
                    })?;
                (
                    signature.to_string(),
                    signature_params(signature),
                    Some(doc),
                )
            }
        },
    };
    let active = argument.min(parameters.len().saturating_sub(1)) as u32;
    Some(SignatureHelp {
        signatures: vec![SignatureInformation {
            label,
            documentation: documentation.map(|d| Documentation::String(d.to_string())),
            parameters: Some(
                parameters
                    .into_iter()
                    .map(|p| ParameterInformation {
                        label: ParameterLabel::Simple(p),
                        documentation: None,
                    })
                    .collect(),
            ),
            active_parameter: Some(active),
        }],
        active_signature: Some(0),
        active_parameter: Some(active),
    })
}

/// Where a symbol occurs, by file path, as start and end line and column.
type Occurrences = HashMap<String, Vec<(u32, u32, u32, u32)>>;

/// The buffer's path and where the symbol at `position` occurs.
fn symbol_occurrences(
    store: &mut DocumentStore,
    uri: &Url,
    position: Position,
) -> Option<(String, Occurrences)> {
    let text = store.text(uri)?.to_string();
    let analysis = store.analyze(uri)?;
    let symbol = symbol_at_position(&analysis, &text, position.line, position.character)?;
    let buffer_path = uri
        .to_file_path()
        .ok()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "<buffer>".to_string());
    let edits = rename_edits_for_symbol(&analysis, &text, &buffer_path, symbol);
    Some((buffer_path, edits))
}

fn lsp_range((sl, sc, el, ec): (u32, u32, u32, u32)) -> Range {
    Range::new(Position::new(sl, sc), Position::new(el, ec))
}

fn compute_references(store: &mut DocumentStore, params: ReferenceParams) -> Option<Vec<Location>> {
    let uri = params.text_document_position.text_document.uri;
    let position = params.text_document_position.position;
    let (buffer_path, occurrences) = symbol_occurrences(store, &uri, position)?;
    let mut out = Vec::new();
    for (path, ranges) in occurrences {
        let target = if path == buffer_path {
            uri.clone()
        } else {
            Url::from_file_path(&path).ok()?
        };
        out.extend(
            ranges
                .into_iter()
                .map(|range| Location::new(target.clone(), lsp_range(range))),
        );
    }
    out.sort_by_key(|l| {
        (
            l.uri.to_string(),
            l.range.start.line,
            l.range.start.character,
        )
    });
    Some(out)
}

fn compute_highlights(
    store: &mut DocumentStore,
    params: DocumentHighlightParams,
) -> Option<Vec<DocumentHighlight>> {
    let uri = params.text_document_position_params.text_document.uri;
    let position = params.text_document_position_params.position;
    let (buffer_path, mut occurrences) = symbol_occurrences(store, &uri, position)?;
    Some(
        occurrences
            .remove(&buffer_path)?
            .into_iter()
            .map(|range| DocumentHighlight {
                range: lsp_range(range),
                kind: None,
            })
            .collect(),
    )
}

fn compute_inlay_hints(store: &mut DocumentStore, params: InlayHintParams) -> Vec<InlayHint> {
    let uri = params.text_document.uri;
    let Some(text) = store.text(&uri).map(str::to_string) else {
        return Vec::new();
    };
    let Some(analysis) = store.analyze(&uri) else {
        return Vec::new();
    };
    inferred_bindings(&analysis, &text)
        .into_iter()
        .filter_map(|(offset, ty)| {
            let (line, character) = offset_to_position(&text, offset);
            let position = Position::new(line, character);
            let wanted = params.range.start <= position && position <= params.range.end;
            wanted.then(|| InlayHint {
                position,
                label: InlayHintLabel::String(format!(": {ty}")),
                kind: Some(InlayHintKind::TYPE),
                text_edits: None,
                tooltip: None,
                padding_left: None,
                padding_right: None,
                data: None,
            })
        })
        .collect()
}

/// A quick fix for each undefined standard library name: import it.
fn compute_code_actions(
    store: &mut DocumentStore,
    params: CodeActionParams,
) -> Vec<CodeActionOrCommand> {
    let uri = params.text_document.uri;
    let Some(text) = store.text(&uri).map(str::to_string) else {
        return Vec::new();
    };
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for diagnostic in &params.context.diagnostics {
        // A warning with a fix carries its edit; the range defaults to the
        // diagnostic's own.
        if let Some(data) = diagnostic.data.as_ref() {
            let Some(replacement) = data.get("replacement").and_then(|v| v.as_str()) else {
                continue;
            };
            let range = data
                .get("range")
                .and_then(|r| serde_json::from_value::<Range>(r.clone()).ok())
                .unwrap_or(diagnostic.range);
            let title = data
                .get("title")
                .and_then(|t| t.as_str())
                .map(str::to_string)
                .unwrap_or_else(|| format!("Replace with `{replacement}`"));
            out.push(CodeActionOrCommand::CodeAction(CodeAction {
                title,
                kind: Some(CodeActionKind::QUICKFIX),
                diagnostics: Some(vec![diagnostic.clone()]),
                edit: Some(WorkspaceEdit {
                    changes: Some(HashMap::from([(
                        uri.clone(),
                        vec![TextEdit {
                            range,
                            new_text: replacement.to_string(),
                        }],
                    )])),
                    document_changes: None,
                    change_annotations: None,
                }),
                is_preferred: Some(true),
                ..Default::default()
            }));
            continue;
        }
        let Some(name) = diagnostic
            .message
            .strip_prefix("undefined name `")
            .and_then(|rest| rest.strip_suffix('`'))
        else {
            continue;
        };
        let Some((name, module, ..)) = std_doc(name) else {
            continue;
        };
        if !seen.insert(*name) {
            continue;
        }
        let edit = import_text_edit(&text, module, name);
        out.push(CodeActionOrCommand::CodeAction(CodeAction {
            title: format!("Import `{name}` from `{module}`"),
            kind: Some(CodeActionKind::QUICKFIX),
            diagnostics: Some(vec![diagnostic.clone()]),
            edit: Some(WorkspaceEdit {
                changes: Some(HashMap::from([(uri.clone(), vec![edit])])),
                document_changes: None,
                change_annotations: None,
            }),
            is_preferred: Some(true),
            ..Default::default()
        }));
    }
    out
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
    let (end_line, end_char) = crate::position::offset_to_position(text, text.len());
    Some(vec![TextEdit {
        range: Range::new(Position::new(0, 0), Position::new(end_line, end_char)),
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
            let (sl, sc, el, ec) = span_to_lsp_range(&text, &sym.span, &analysis)?;
            // A component: a class with its members.
            let template = hir.templates.iter().find(|t| t.name == sym.name);
            let children = template.map(|t| {
                t.members
                    .iter()
                    .filter_map(|(name, id, _)| {
                        let span = hir.symbols.get(id)?.span;
                        let (sl, sc, el, ec) = span_to_lsp_range(&text, &span, &analysis)?;
                        let kind = match analysis.typed.as_ref().and_then(|t| t.types.get(id)) {
                            Some(flux_types::Type::Func { .. }) => SymbolKind::METHOD,
                            Some(flux_types::Type::Event(_)) => SymbolKind::EVENT,
                            _ => SymbolKind::FIELD,
                        };
                        let range = Range::new(Position::new(sl, sc), Position::new(el, ec));
                        Some(DocumentSymbol {
                            name: name.clone(),
                            detail: None,
                            kind,
                            tags: None,
                            range,
                            selection_range: range,
                            children: None,
                            #[allow(deprecated)]
                            deprecated: None,
                        })
                    })
                    .collect()
            });
            Some(DocumentSymbol {
                name: sym.name,
                detail: template.map(|_| "live fn".to_string()),
                kind: match sym.kind {
                    _ if template.is_some() => SymbolKind::CLASS,
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
                children,
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
        span_to_lsp_range(&text, &loc.span, &analysis)?;
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

    /// A watch cycle through two files is found by the importer's analysis,
    /// at a watch in the imported file: it shows there even when that file
    /// is open (its own analysis cannot see the cycle), and goes when the
    /// importer closes.
    #[test]
    fn a_cycle_through_an_open_imported_file_shows_in_it() {
        let dir = std::env::temp_dir().join(format!("flux-lsp-cycle-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let shared = "pub event Bump\n\npub let mut a = 0\n\npub let mut b = 0\n\non Bump {\n    a += 1\n}\n\npub act fn set_b(v: Int) {\n    b = v\n}\n\nwatch a {\n    set_b(a)\n}\n";
        let main = "import shared { a, b, Bump, set_b }\n\nwatch b {\n    emit Bump()\n}\n\nfn main() {\n}\n";
        std::fs::write(dir.join("shared.fl"), shared).unwrap();
        std::fs::write(dir.join("main.fl"), main).unwrap();
        let shared_uri =
            Url::from_file_path(dir.join("shared.fl").canonicalize().unwrap()).unwrap();
        let main_uri = Url::from_file_path(dir.join("main.fl").canonicalize().unwrap()).unwrap();
        let mut store = DocumentStore::new();
        store.open(shared_uri.clone(), 1, shared.to_string());
        store.open(main_uri.clone(), 1, main.to_string());
        let codes = |params: &[PublishDiagnosticsParams], uri: &Url| -> Vec<String> {
            params
                .iter()
                .filter(|p| p.uri == *uri)
                .flat_map(|p| p.diagnostics.iter())
                .filter_map(|d| match &d.code {
                    Some(NumberOrString::String(c)) => Some(c.clone()),
                    _ => None,
                })
                .collect()
        };
        let published = document_diagnostics(&mut store, &main_uri);
        assert!(
            codes(&published, &shared_uri).contains(&"E029".to_string()),
            "{published:?}"
        );
        // The imported file's own publish keeps it.
        let own = document_diagnostics(&mut store, &shared_uri);
        assert!(
            codes(&own, &shared_uri).contains(&"E029".to_string()),
            "{own:?}"
        );
        // Closing the importer takes it away.
        store.close(&main_uri);
        let closed = close_diagnostics(&mut store, &main_uri);
        assert!(
            !codes(&closed, &shared_uri).contains(&"E029".to_string()),
            "{closed:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The editor knows a component's members: hover and definition of a
    /// member by its name in the body and through an instance (`a.n`,
    /// `emit a.Inc`), completion after `a.`, and the component in the outline.
    #[test]
    fn live_fn_members_in_the_editor() {
        let src = "import io { println }\n\nlive fn Counter(start: Int) {\n    pub let mut n = start\n    pub derived double = n * 2\n    pub event Inc(by: Int)\n\n    on Inc(by) {\n        n += by\n    }\n}\n\nact fn main() {\n    let a = Counter(1)\n    emit a.Inc(2)\n    println(\"{a.n}\")\n    let d = a.double\n}\n";
        let dir = std::env::temp_dir().join(format!("flux-lsp-live-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("main.fl"), src).unwrap();
        let uri = Url::from_file_path(dir.join("main.fl").canonicalize().unwrap()).unwrap();
        let mut store = DocumentStore::new();
        store.open(uri.clone(), 1, src.to_string());
        let at = |line: u32, character: u32| lsp_types::TextDocumentPositionParams {
            text_document: lsp_types::TextDocumentIdentifier { uri: uri.clone() },
            position: Position::new(line, character),
        };
        let hover = |store: &mut DocumentStore, line, ch| match compute_hover(
            store,
            HoverParams {
                text_document_position_params: at(line, ch),
                work_done_progress_params: Default::default(),
            },
        )
        .map(|h| h.contents)
        {
            Some(HoverContents::Markup(m)) => m.value,
            other => format!("{other:?}"),
        };
        let definition_line = |store: &mut DocumentStore, line, ch| match compute_definition(
            store,
            GotoDefinitionParams {
                text_document_position_params: at(line, ch),
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            },
        ) {
            Some(GotoDefinitionResponse::Scalar(l)) => {
                Some((l.range.start.line, l.range.start.character))
            }
            _ => None,
        };
        // `n` in the handler, `a.n`, `a.double`, `Inc` in `emit a.Inc`.
        assert!(
            hover(&mut store, 8, 8).contains("n: "),
            "{}",
            hover(&mut store, 8, 8)
        );
        assert_eq!(definition_line(&mut store, 8, 8), Some((3, 16)));
        assert!(hover(&mut store, 15, 17).contains("n: "));
        assert_eq!(definition_line(&mut store, 15, 17), Some((3, 16)));
        assert!(hover(&mut store, 16, 15).contains("double: Int64"));
        assert_eq!(definition_line(&mut store, 16, 15), Some((4, 16)));
        assert!(hover(&mut store, 14, 12).contains("Inc: event"));
        assert_eq!(definition_line(&mut store, 14, 12), Some((5, 14)));
        let completion = compute_completion(
            &mut store,
            CompletionParams {
                text_document_position: at(16, 14),
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
                context: None,
            },
        );
        let labels: Vec<String> = completion.iter().map(|c| c.label.clone()).collect();
        assert_eq!(labels, ["n", "double", "Inc"]);
        let Some(DocumentSymbolResponse::Nested(symbols)) = compute_document_symbols(
            &mut store,
            DocumentSymbolParams {
                text_document: lsp_types::TextDocumentIdentifier { uri: uri.clone() },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            },
        ) else {
            panic!("no symbols");
        };
        let counter = symbols
            .iter()
            .find(|s| s.name == "Counter")
            .expect("Counter");
        assert_eq!(counter.kind, SymbolKind::CLASS);
        let members: Vec<&str> = counter
            .children
            .iter()
            .flatten()
            .map(|c| c.name.as_str())
            .collect();
        assert_eq!(members, ["n", "double", "Inc"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

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
