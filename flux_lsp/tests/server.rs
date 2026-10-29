//! End-to-end test of the stdio LSP server, driven over an in-memory
//! `lsp_server::Connection` pair instead of real stdio pipes.

use std::thread;

use lsp_server::{Connection, Message, Notification, Request, RequestId};
use lsp_types::notification::{
    DidChangeTextDocument, DidOpenTextDocument, Exit, Initialized, Notification as _,
    PublishDiagnostics,
};
use lsp_types::request::{Completion, GotoDefinition, HoverRequest, Initialize, Request as _};
use lsp_types::{
    CompletionParams, DidChangeTextDocumentParams, DidOpenTextDocumentParams, GotoDefinitionParams,
    GotoDefinitionResponse, HoverParams, InitializeParams, InitializedParams, PartialResultParams,
    Position, PublishDiagnosticsParams, Range, TextDocumentContentChangeEvent,
    TextDocumentIdentifier, TextDocumentItem, TextDocumentPositionParams, Url,
    VersionedTextDocumentIdentifier, WorkDoneProgressParams,
};

fn test_uri() -> Url {
    Url::parse("file:///tmp/flux_lsp_server_test.fl").unwrap()
}

fn start_server() -> (Connection, thread::JoinHandle<()>) {
    let (client, server) = Connection::memory();
    let handle = thread::spawn(move || {
        let caps = serde_json::to_value(flux_lsp::server::server_capabilities()).unwrap();
        server
            .initialize(caps)
            .expect("server initialize handshake");
        flux_lsp::server::main_loop(&server).expect("main loop");
    });

    // Drive the client side of the same handshake before returning control.
    client
        .sender
        .send(Message::Request(Request::new(
            RequestId::from(1),
            Initialize::METHOD.to_string(),
            serde_json::to_value(InitializeParams::default()).unwrap(),
        )))
        .unwrap();
    let _ = client.receiver.recv().expect("initialize response");
    client
        .sender
        .send(Message::Notification(Notification::new(
            Initialized::METHOD.to_string(),
            InitializedParams {},
        )))
        .unwrap();

    (client, handle)
}

fn did_open(client: &Connection, text: &str) {
    did_open_uri(client, test_uri(), text);
}

fn did_open_uri(client: &Connection, uri: Url, text: &str) {
    client
        .sender
        .send(Message::Notification(Notification::new(
            DidOpenTextDocument::METHOD.to_string(),
            DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri,
                    language_id: "flux".to_string(),
                    version: 1,
                    text: text.to_string(),
                },
            },
        )))
        .unwrap();
}

fn recv_diagnostics(client: &Connection) -> PublishDiagnosticsParams {
    loop {
        match client.receiver.recv().expect("expected a message") {
            Message::Notification(n) if n.method == PublishDiagnostics::METHOD => {
                return serde_json::from_value(n.params).unwrap();
            }
            _ => continue,
        }
    }
}

#[test]
fn reports_undefined_name_diagnostic_with_range() {
    let (client, handle) = start_server();
    did_open(&client, "let x = y + 1\n");

    let diags = recv_diagnostics(&client);
    assert_eq!(diags.diagnostics.len(), 1, "{:?}", diags.diagnostics);
    let diag = &diags.diagnostics[0];
    assert_eq!(
        diag.code,
        Some(lsp_types::NumberOrString::String("E001".to_string()))
    );
    assert_eq!(diag.range.start.line, 0);
    // "let x = " is 8 characters, so the undefined `y` starts at column 8 (0-based).
    assert_eq!(diag.range.start.character, 8);
    assert!(diag.range.end.character > diag.range.start.character);

    shutdown(&client, handle);
}

#[test]
fn clean_program_has_no_diagnostics_and_hover_shows_type() {
    let (client, handle) = start_server();
    did_open(
        &client,
        "import io\nlet threshold = 3\nlet doubled = threshold * 2\nwatch { print(doubled) }\n",
    );

    let diags = recv_diagnostics(&client);
    assert!(diags.diagnostics.is_empty(), "{:?}", diags.diagnostics);

    client
        .sender
        .send(Message::Request(Request::new(
            RequestId::from(2),
            HoverRequest::METHOD.to_string(),
            serde_json::to_value(HoverParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: test_uri() },
                    position: Position::new(2, 4),
                },
                work_done_progress_params: WorkDoneProgressParams::default(),
            })
            .unwrap(),
        )))
        .unwrap();

    let hover = loop {
        match client.receiver.recv().expect("expected a message") {
            Message::Response(resp) if resp.id == RequestId::from(2) => break resp,
            _ => continue,
        }
    };
    let result = hover.result.expect("hover result");
    let text = result.to_string();
    assert!(text.contains("derived"), "hover text was: {text}");
    assert!(text.contains("Int"), "hover text was: {text}");

    shutdown(&client, handle);
}

#[test]
fn completion_lists_keywords_and_module_bindings() {
    let (client, handle) = start_server();
    did_open(
        &client,
        "let budget = 100\nlet spent = 40\nlet remaining = budget - spent\n",
    );
    let _ = recv_diagnostics(&client);

    client
        .sender
        .send(Message::Request(Request::new(
            RequestId::from(3),
            Completion::METHOD.to_string(),
            serde_json::to_value(CompletionParams {
                text_document_position: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: test_uri() },
                    position: Position::new(2, 0),
                },
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: PartialResultParams::default(),
                context: None,
            })
            .unwrap(),
        )))
        .unwrap();

    let resp = loop {
        match client.receiver.recv().expect("expected a message") {
            Message::Response(resp) if resp.id == RequestId::from(3) => break resp,
            _ => continue,
        }
    };
    let items: Vec<lsp_types::CompletionItem> =
        serde_json::from_value(resp.result.expect("completion result")).unwrap();
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(labels.contains(&"watch"), "{labels:?}");
    assert!(labels.contains(&"budget"), "{labels:?}");
    assert!(labels.contains(&"remaining"), "{labels:?}");

    shutdown(&client, handle);
}

#[test]
fn resolves_imports_for_open_document_with_unsaved_edits() {
    let (client, handle) = start_server();
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../programs/ok/modules/import_math.fl");
    let uri = Url::from_file_path(&path).expect("valid file uri");
    let saved = std::fs::read_to_string(&path).expect("read import_math.fl");
    // Simulate an unsaved edit that still depends on the imported `math` module.
    let unsaved = format!("{saved}let z = m.double(y)\nimport io\nwatch {{ print(z) }}\n");

    did_open_uri(&client, uri, &unsaved);
    let diags = recv_diagnostics(&client);
    assert!(diags.diagnostics.is_empty(), "{:?}", diags.diagnostics);

    shutdown(&client, handle);
}

#[test]
fn goto_definition_finds_binding() {
    let (client, handle) = start_server();
    did_open(&client, "let threshold = 3\nlet doubled = threshold * 2\n");
    let _ = recv_diagnostics(&client);

    client
        .sender
        .send(Message::Request(Request::new(
            RequestId::from(4),
            GotoDefinition::METHOD.to_string(),
            serde_json::to_value(GotoDefinitionParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: test_uri() },
                    position: Position::new(1, 14),
                },
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: PartialResultParams::default(),
            })
            .unwrap(),
        )))
        .unwrap();

    let resp = loop {
        match client.receiver.recv().expect("expected a message") {
            Message::Response(resp) if resp.id == RequestId::from(4) => break resp,
            _ => continue,
        }
    };
    let location: GotoDefinitionResponse =
        serde_json::from_value(resp.result.expect("definition result")).unwrap();
    match location {
        GotoDefinitionResponse::Scalar(loc) => {
            assert_eq!(loc.uri, test_uri());
            assert_eq!(loc.range.start.line, 0);
        }
        _ => panic!("expected scalar location"),
    }

    shutdown(&client, handle);
}

#[test]
fn member_completion_after_dot() {
    let (client, handle) = start_server();
    did_open(
        &client,
        "struct Point { x: Int, y: Int }\nlet p = Point { x: 1, y: 2 }\nlet scale = p.x\n",
    );
    let _ = recv_diagnostics(&client);

    client
        .sender
        .send(Message::Request(Request::new(
            RequestId::from(5),
            Completion::METHOD.to_string(),
            serde_json::to_value(CompletionParams {
                text_document_position: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: test_uri() },
                    position: Position::new(2, 13),
                },
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: PartialResultParams::default(),
                context: None,
            })
            .unwrap(),
        )))
        .unwrap();

    let resp = loop {
        match client.receiver.recv().expect("expected a message") {
            Message::Response(resp) if resp.id == RequestId::from(5) => break resp,
            _ => continue,
        }
    };
    let items: Vec<lsp_types::CompletionItem> =
        serde_json::from_value(resp.result.expect("completion result")).unwrap();
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(labels.contains(&"x"), "{labels:?}");
    assert!(labels.contains(&"y"), "{labels:?}");

    shutdown(&client, handle);
}

#[test]
fn incremental_edit_updates_diagnostics() {
    let (client, handle) = start_server();
    let uri = test_uri();
    did_open(&client, "let x = y + 1\n");
    let bad = recv_diagnostics(&client);
    assert_eq!(bad.diagnostics.len(), 1);

    client
        .sender
        .send(Message::Notification(Notification::new(
            DidChangeTextDocument::METHOD.to_string(),
            DidChangeTextDocumentParams {
                text_document: VersionedTextDocumentIdentifier {
                    uri: uri.clone(),
                    version: 2,
                },
                content_changes: vec![TextDocumentContentChangeEvent {
                    range: Some(Range::new(Position::new(0, 8), Position::new(0, 9))),
                    range_length: None,
                    text: "1".to_string(),
                }],
            },
        )))
        .unwrap();

    let fixed = recv_diagnostics(&client);
    assert!(
        fixed
            .diagnostics
            .iter()
            .all(|d| d.severity != Some(lsp_types::DiagnosticSeverity::ERROR)),
        "expected no errors after fix, got {:?}",
        fixed.diagnostics
    );
    assert_eq!(fixed.version, Some(2));

    shutdown(&client, handle);
}

fn shutdown(client: &Connection, handle: thread::JoinHandle<()>) {
    client
        .sender
        .send(Message::Request(Request::new(
            RequestId::from(999),
            "shutdown".to_string(),
            serde_json::Value::Null,
        )))
        .unwrap();
    let _ = client.receiver.recv().expect("shutdown response");
    client
        .sender
        .send(Message::Notification(Notification::new(
            Exit::METHOD.to_string(),
            (),
        )))
        .unwrap();
    handle
        .join()
        .expect("server thread should exit cleanly after shutdown");
}
