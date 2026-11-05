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
    enclosing_call, import_edit, imports, is_imported, names_in_scope, signature_params, std_doc,
    PRELUDE, STD_DOCS, TYPE_NAMES,
};
use crate::documents::DocumentStore;
use crate::position::offset_to_position;

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

/// Every `publishDiagnostics` that analyzing `uri` produces. Diagnostics in
/// imported files go to those files' URIs (unless open: their own analysis owns
/// them), and imported files that no longer have any get an empty list.
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
                Ok(target) if !store.is_open(&target) => target,
                _ => continue,
            },
        };
        by_uri.entry(target).or_default().push(Diagnostic {
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
        });
    }
    let own = by_uri.remove(uri).unwrap_or_default();
    let reported: HashSet<Url> = by_uri.keys().cloned().collect();
    let stale = store.replace_foreign(uri, by_uri);
    let mut out = vec![PublishDiagnosticsParams {
        version: store.version(uri),
        uri: uri.clone(),
        diagnostics: own,
    }];
    // An open file's diagnostics come from its own analysis.
    for target in reported.union(&stale).filter(|t| !store.is_open(t)) {
        out.push(PublishDiagnosticsParams {
            version: None,
            uri: target.clone(),
            diagnostics: store.foreign_diagnostics(target),
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
        .filter(|target| !store.is_open(target))
        .collect();
    targets.sort_by_key(|u| u.to_string());
    let mut out = vec![PublishDiagnosticsParams {
        uri: uri.clone(),
        diagnostics: Vec::new(),
        version: None,
    }];
    for target in targets {
        out.push(PublishDiagnosticsParams {
            diagnostics: store.foreign_diagnostics(&target),
            uri: target,
            version: None,
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

    // A name the program defines itself hides the library's.
    let own_name = analysis.as_ref().is_some_and(|a| {
        a.hir.as_ref().is_some_and(|hir| {
            hir.symbols
                .values()
                .any(|s| s.name == word && s.kind != flux_hir::SymbolKind::Builtin)
        })
    });
    if !own_name {
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
    let user = analysis
        .as_ref()
        .and_then(|a| hover_for_name(a.hir.as_ref()?, a.typed.as_ref(), word));
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

    for (variant, owner) in &hir.enum_variants {
        if !variant.starts_with("__") && seen.insert(variant.clone()) {
            items.push(CompletionItem {
                label: variant.clone(),
                kind: Some(CompletionItemKind::ENUM_MEMBER),
                detail: Some(owner.clone()),
                ..Default::default()
            });
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
    let (name, argument) = enclosing_call(&text, offset)?;
    let analysis = store.analyze(&uri);
    let user = analysis.as_ref().and_then(|a| user_signature(a, &name));
    let (label, parameters, documentation) = match user {
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
