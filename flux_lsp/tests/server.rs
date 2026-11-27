//! End-to-end test of the stdio LSP server, driven over an in-memory
//! `lsp_server::Connection` pair instead of real stdio pipes.

use std::thread;

use lsp_server::{Connection, Message, Notification, Request, RequestId};
use lsp_types::notification::{
    DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, DidSaveTextDocument, Exit,
    Initialized, Notification as _, PublishDiagnostics,
};
use lsp_types::request::{
    Completion, GotoDefinition, HoverRequest, Initialize, Rename, Request as _,
};
use lsp_types::{
    CompletionParams, DidChangeTextDocumentParams, DidCloseTextDocumentParams,
    DidOpenTextDocumentParams, DidSaveTextDocumentParams, GotoDefinitionParams,
    GotoDefinitionResponse, HoverParams, InitializeParams, InitializedParams, PartialResultParams,
    Position, PublishDiagnosticsParams, Range, RenameParams, TextDocumentContentChangeEvent,
    TextDocumentIdentifier, TextDocumentItem, TextDocumentPositionParams, Url,
    VersionedTextDocumentIdentifier, WorkDoneProgressParams, WorkspaceEdit,
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
    // Simulate an unsaved edit that still depends on the imported `arith` module.
    let unsaved = format!("{saved}let z = m::double(y)\nimport io\nwatch {{ print(z) }}\n");

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

/// A two-file project on disk; paths are canonical because the resolver
/// reports canonical module paths.
fn import_project(util: &str) -> (tempfile::TempDir, Url, Url) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    std::fs::write(root.join("util.fl"), util).unwrap();
    std::fs::write(root.join("main.fl"), "").unwrap();
    let main = Url::from_file_path(root.join("main.fl")).unwrap();
    let util = Url::from_file_path(root.join("util.fl")).unwrap();
    (dir, main, util)
}

fn change_full(client: &Connection, uri: Url, version: i32, text: &str) {
    client
        .sender
        .send(Message::Notification(Notification::new(
            DidChangeTextDocument::METHOD.to_string(),
            DidChangeTextDocumentParams {
                text_document: VersionedTextDocumentIdentifier { uri, version },
                content_changes: vec![TextDocumentContentChangeEvent {
                    range: None,
                    range_length: None,
                    text: text.to_string(),
                }],
            },
        )))
        .unwrap();
}

#[test]
fn imported_module_error_is_published_on_its_own_uri() {
    let (_dir, main, util) = import_project("\npub fn f(a: Int) -> Int {\n  missing + a\n}\n");
    let (client, handle) = start_server();
    did_open_uri(
        &client,
        main.clone(),
        "import util { f }\nlet x = f(1)\nlet y = x\n",
    );

    let root = recv_diagnostics(&client);
    assert_eq!(root.uri, main);
    assert!(
        root.diagnostics
            .iter()
            .all(|d| d.message.contains("unused")),
        "root must not carry the imported error: {:?}",
        root.diagnostics
    );
    let imported = recv_diagnostics(&client);
    assert_eq!(imported.uri, util);
    assert_eq!(imported.diagnostics.len(), 1, "{:?}", imported.diagnostics);
    let range = imported.diagnostics[0].range;
    // `  missing + a` on line 2 (0-based); `missing` starts at column 2.
    assert_eq!((range.start.line, range.start.character), (2, 2));
    assert_eq!((range.end.line, range.end.character), (2, 9));

    // Dropping the import clears the imported file's diagnostics.
    change_full(&client, main.clone(), 2, "let y = 1\nlet z = y\n");
    let root = recv_diagnostics(&client);
    assert_eq!(root.uri, main);
    let cleared = recv_diagnostics(&client);
    assert_eq!(cleared.uri, util);
    assert!(cleared.diagnostics.is_empty(), "{:?}", cleared.diagnostics);

    shutdown(&client, handle);
}

#[test]
fn closing_a_document_clears_its_and_imported_diagnostics() {
    let (_dir, main, util) = import_project("pub fn f() -> Int { nope }\n");
    let (client, handle) = start_server();
    did_open_uri(&client, main.clone(), "let y = 1 +\nimport util { f }\n");
    let _ = recv_diagnostics(&client);
    change_full(&client, main.clone(), 2, "import util { f }\nlet y = f()\n");
    let _ = recv_diagnostics(&client);
    let imported = recv_diagnostics(&client);
    assert_eq!(imported.uri, util);
    assert!(!imported.diagnostics.is_empty());

    client
        .sender
        .send(Message::Notification(Notification::new(
            DidCloseTextDocument::METHOD.to_string(),
            DidCloseTextDocumentParams {
                text_document: TextDocumentIdentifier { uri: main.clone() },
            },
        )))
        .unwrap();
    let closed = recv_diagnostics(&client);
    assert_eq!(closed.uri, main);
    assert!(closed.diagnostics.is_empty());
    let closed_import = recv_diagnostics(&client);
    assert_eq!(closed_import.uri, util);
    assert!(closed_import.diagnostics.is_empty());

    shutdown(&client, handle);
}

// Positions below are UTF-16 code units: `é` is 1 unit (2 bytes) and `𝒳`
// (U+1D4B3) is 2 units (4 bytes, 1 scalar), so byte- or scalar-based columns
// would land elsewhere.

#[test]
fn diagnostic_range_counts_utf16_units() {
    let (client, handle) = start_server();
    did_open(&client, "let s = \"é𝒳\" + y\n");
    let diags = recv_diagnostics(&client);
    assert_eq!(diags.diagnostics.len(), 1, "{:?}", diags.diagnostics);
    let range = diags.diagnostics[0].range;
    assert_eq!(
        range,
        Range::new(Position::new(0, 16), Position::new(0, 17))
    );
    shutdown(&client, handle);
}

#[test]
fn rename_edits_use_utf16_columns() {
    let (client, handle) = start_server();
    did_open(&client, "let cd = \"é\"\nlet t = \"é𝒳\" + cd\n");
    let _ = recv_diagnostics(&client);
    client
        .sender
        .send(Message::Request(Request::new(
            RequestId::from(7),
            Rename::METHOD.to_string(),
            serde_json::to_value(RenameParams {
                text_document_position: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: test_uri() },
                    position: Position::new(1, 16),
                },
                new_name: "ef".to_string(),
                work_done_progress_params: WorkDoneProgressParams::default(),
            })
            .unwrap(),
        )))
        .unwrap();
    let resp = loop {
        match client.receiver.recv().expect("expected a message") {
            Message::Response(resp) if resp.id == RequestId::from(7) => break resp,
            _ => continue,
        }
    };
    let edit: WorkspaceEdit = serde_json::from_value(resp.result.expect("rename result")).unwrap();
    let mut ranges: Vec<Range> = edit
        .changes
        .expect("changes")
        .values()
        .flatten()
        .map(|e| e.range)
        .collect();
    ranges.sort_by_key(|r| (r.start.line, r.start.character));
    assert_eq!(
        ranges,
        vec![
            Range::new(Position::new(0, 4), Position::new(0, 6)),
            Range::new(Position::new(1, 16), Position::new(1, 18)),
        ]
    );
    shutdown(&client, handle);
}

#[test]
fn incremental_edit_after_astral_char_uses_utf16() {
    let (client, handle) = start_server();
    did_open(&client, "let s = \"𝒳a\"\nlet t = s\n");
    let _ = recv_diagnostics(&client);
    // Insert `b` between `𝒳` (units 9-10) and `a` (unit 11).
    client
        .sender
        .send(Message::Notification(Notification::new(
            DidChangeTextDocument::METHOD.to_string(),
            DidChangeTextDocumentParams {
                text_document: VersionedTextDocumentIdentifier {
                    uri: test_uri(),
                    version: 2,
                },
                content_changes: vec![TextDocumentContentChangeEvent {
                    range: Some(Range::new(Position::new(0, 11), Position::new(0, 11))),
                    range_length: None,
                    text: "b".to_string(),
                }],
            },
        )))
        .unwrap();
    let diags = recv_diagnostics(&client);
    assert!(
        diags
            .diagnostics
            .iter()
            .all(|d| d.severity != Some(lsp_types::DiagnosticSeverity::ERROR)),
        "edit landed in the wrong place: {:?}",
        diags.diagnostics
    );
    shutdown(&client, handle);
}

#[test]
fn saving_an_import_refreshes_the_unchanged_root() {
    let (dir, main, util) = import_project("pub fn f() -> Int { 1 }\n");
    let (client, handle) = start_server();
    let root_text = "import util { f }\nlet x = f()\nlet y = x\n";
    did_open_uri(&client, main.clone(), root_text);
    let clean = recv_diagnostics(&client);
    assert_eq!(clean.uri, main);
    assert!(clean
        .diagnostics
        .iter()
        .all(|d| d.severity != Some(lsp_types::DiagnosticSeverity::ERROR)));

    // The import is opened, edited to break the root's use of `f`, and saved.
    let broken = "pub fn f(a: Int) -> Int { a }\n";
    did_open_uri(&client, util.clone(), "pub fn f() -> Int { 1 }\n");
    let _ = recv_diagnostics(&client);
    std::fs::write(dir.path().canonicalize().unwrap().join("util.fl"), broken).unwrap();
    change_full(&client, util.clone(), 2, broken);
    client
        .sender
        .send(Message::Notification(Notification::new(
            DidSaveTextDocument::METHOD.to_string(),
            DidSaveTextDocumentParams {
                text_document: TextDocumentIdentifier { uri: util.clone() },
                text: None,
            },
        )))
        .unwrap();

    // The root was never edited, yet a fresh publish for it reports the error.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let refreshed = loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        let msg = client
            .receiver
            .recv_timeout(remaining)
            .expect("root was not re-published after its import changed");
        let Message::Notification(n) = msg else {
            continue;
        };
        if n.method != PublishDiagnostics::METHOD {
            continue;
        }
        let diags: PublishDiagnosticsParams = serde_json::from_value(n.params).unwrap();
        if diags.uri == main
            && diags
                .diagnostics
                .iter()
                .any(|d| d.severity == Some(lsp_types::DiagnosticSeverity::ERROR))
        {
            break diags;
        }
    };
    assert_eq!(
        refreshed.diagnostics[0].range.start.line, 1,
        "{:?}",
        refreshed.diagnostics
    );

    shutdown(&client, handle);
}

/// Publications until `want` has one matching `done`; the rest are returned too.
fn publications_until(
    client: &Connection,
    done: impl Fn(&PublishDiagnosticsParams) -> bool,
) -> Vec<PublishDiagnosticsParams> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let mut seen = Vec::new();
    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        let msg = client
            .receiver
            .recv_timeout(remaining)
            .unwrap_or_else(|_| panic!("no matching publication; saw {seen:?}"));
        let Message::Notification(n) = msg else {
            continue;
        };
        if n.method != PublishDiagnostics::METHOD {
            continue;
        }
        let diags: PublishDiagnosticsParams = serde_json::from_value(n.params).unwrap();
        let finished = done(&diags);
        seen.push(diags);
        if finished {
            return seen;
        }
    }
}

fn has_error(diags: &PublishDiagnosticsParams) -> bool {
    diags
        .diagnostics
        .iter()
        .any(|d| d.severity == Some(lsp_types::DiagnosticSeverity::ERROR))
}

#[test]
fn an_unsaved_edit_to_an_import_updates_the_root() {
    let (_dir, main, util) = import_project("pub fn f() -> Int { 1 }\n");
    let (client, handle) = start_server();
    did_open_uri(
        &client,
        main.clone(),
        "import util { f }\nlet x = f()\nlet y = x\n",
    );
    assert!(!has_error(&recv_diagnostics(&client)));
    did_open_uri(&client, util.clone(), "pub fn f() -> Int { 1 }\n");
    let _ = recv_diagnostics(&client);

    // Nothing is written to disk: the root follows the editor's text.
    change_full(&client, util.clone(), 2, "pub fn f(a: Int) -> Int { a }\n");
    let main_uri = main.clone();
    publications_until(&client, |d| d.uri == main_uri && has_error(d));
    change_full(&client, util.clone(), 3, "pub fn f() -> Int { 2 }\n");
    let main_uri = main.clone();
    publications_until(&client, |d| d.uri == main_uri && !has_error(d));

    // Closing the buffer discards the edit, and the root sees the disk again.
    change_full(&client, util.clone(), 4, "pub fn f(a: Int) -> Int { a }\n");
    let main_uri = main.clone();
    publications_until(&client, |d| d.uri == main_uri && has_error(d));
    client
        .sender
        .send(Message::Notification(Notification::new(
            DidCloseTextDocument::METHOD.to_string(),
            DidCloseTextDocumentParams {
                text_document: TextDocumentIdentifier { uri: util },
            },
        )))
        .unwrap();
    let main_uri = main.clone();
    publications_until(&client, |d| d.uri == main_uri && !has_error(d));
    shutdown(&client, handle);
}

#[test]
fn definition_follows_an_unsaved_edit_to_an_import() {
    let (_dir, main, util) = import_project("pub fn f() -> Int { 1 }\n");
    let (client, handle) = start_server();
    did_open_uri(&client, util.clone(), "\n\n\npub fn f() -> Int { 1 }\n");
    let _ = recv_diagnostics(&client);
    did_open_uri(
        &client,
        main.clone(),
        "import util { f }\nlet x = f()\nlet y = x\n",
    );
    let _ = recv_diagnostics(&client);
    client
        .sender
        .send(Message::Request(Request::new(
            RequestId::from(40),
            GotoDefinition::METHOD.to_string(),
            GotoDefinitionParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: main },
                    position: Position::new(1, 8),
                },
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: PartialResultParams::default(),
            },
        )))
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let location = loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if let Message::Response(r) = client.receiver.recv_timeout(remaining).expect("response") {
            let found: GotoDefinitionResponse = serde_json::from_value(r.result.unwrap()).unwrap();
            let GotoDefinitionResponse::Scalar(location) = found else {
                panic!("{found:?}");
            };
            break location;
        }
    };
    assert_eq!(location.uri, util);
    assert_eq!(location.range.start.line, 3, "{location:?}");
    shutdown(&client, handle);
}

#[test]
fn a_root_does_not_clear_the_diagnostics_of_an_import_opened_later() {
    let (_dir, main, util) = import_project("pub fn f() -> Int { nope }\n");
    let (client, handle) = start_server();
    did_open_uri(
        &client,
        main.clone(),
        "import util { f }\nlet x = f()\nlet y = x\n",
    );
    let util_uri = util.clone();
    publications_until(&client, |d| d.uri == util_uri && has_error(d));
    did_open_uri(&client, util.clone(), "pub fn f() -> Int { nope }\n");
    let util_uri = util.clone();
    publications_until(&client, |d| d.uri == util_uri && has_error(d));
    // The root republishes; the open import's own diagnostics stay.
    change_full(
        &client,
        main.clone(),
        2,
        "import util { f }\nlet x = f()\nlet z = x\n",
    );
    let main_uri = main.clone();
    let mut seen = publications_until(&client, |d| d.uri == main_uri);
    // Publications for imported files follow the root's.
    while let Ok(msg) = client
        .receiver
        .recv_timeout(std::time::Duration::from_millis(300))
    {
        if let Message::Notification(n) = msg {
            seen.push(serde_json::from_value(n.params).unwrap());
        }
    }
    assert!(
        seen.iter().all(|d| d.uri != util || has_error(d)),
        "{seen:?}"
    );
    shutdown(&client, handle);
}

#[test]
fn malformed_messages_do_not_stop_the_server() {
    let (client, handle) = start_server();
    for method in [
        DidOpenTextDocument::METHOD,
        DidChangeTextDocument::METHOD,
        DidSaveTextDocument::METHOD,
        DidCloseTextDocument::METHOD,
    ] {
        client
            .sender
            .send(Message::Notification(Notification::new(
                method.to_string(),
                serde_json::json!({ "nonsense": 1 }),
            )))
            .unwrap();
    }
    client
        .sender
        .send(Message::Request(Request::new(
            RequestId::from(50),
            HoverRequest::METHOD.to_string(),
            serde_json::json!({ "nonsense": 1 }),
        )))
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if let Message::Response(r) = client.receiver.recv_timeout(remaining).expect("response") {
            assert_eq!(r.id, RequestId::from(50));
            assert!(r.error.is_some(), "{r:?}");
            break;
        }
    }
    // The server still works.
    did_open(&client, "let x = nope\n");
    assert!(has_error(&recv_diagnostics(&client)));
    shutdown(&client, handle);
}

#[test]
fn opening_an_import_refreshes_a_root_that_needed_it() {
    // `f` is not on disk yet, and then `new.fl` does not exist there at all.
    let (dir, main, util) = import_project("pub fn g() -> Int { 1 }\n");
    let (client, handle) = start_server();
    did_open_uri(
        &client,
        main.clone(),
        "import util { f }\nlet x = f()\nlet y = x\n",
    );
    let main_uri = main.clone();
    publications_until(&client, |d| d.uri == main_uri && has_error(d));
    did_open_uri(&client, util, "pub fn f() -> Int { 1 }\n");
    let main_uri = main.clone();
    publications_until(&client, |d| d.uri == main_uri && !has_error(d));

    let new_file =
        Url::from_file_path(dir.path().canonicalize().unwrap().join("fresh.fl")).unwrap();
    change_full(
        &client,
        main.clone(),
        2,
        "import fresh { h }\nlet x = h()\nlet y = x\n",
    );
    let main_uri = main.clone();
    publications_until(&client, |d| d.uri == main_uri && has_error(d));
    did_open_uri(&client, new_file, "pub fn h() -> Int { 1 }\n");
    let main_uri = main.clone();
    publications_until(&client, |d| d.uri == main_uri && !has_error(d));
    shutdown(&client, handle);
}

#[test]
fn closing_one_root_keeps_what_another_reports_on_an_import() {
    let (dir, main, util) = import_project("pub fn f() -> Int { nope }\n");
    let other = Url::from_file_path(dir.path().canonicalize().unwrap().join("other.fl")).unwrap();
    let (client, handle) = start_server();
    let root_text = "import util { f }\nlet x = f()\nlet y = x\n";
    did_open_uri(&client, main.clone(), root_text);
    let util_uri = util.clone();
    publications_until(&client, |d| d.uri == util_uri && has_error(d));
    did_open_uri(&client, other, root_text);
    let util_uri = util.clone();
    publications_until(&client, |d| d.uri == util_uri && has_error(d));
    client
        .sender
        .send(Message::Notification(Notification::new(
            DidCloseTextDocument::METHOD.to_string(),
            DidCloseTextDocumentParams {
                text_document: TextDocumentIdentifier { uri: main },
            },
        )))
        .unwrap();
    while let Ok(msg) = client
        .receiver
        .recv_timeout(std::time::Duration::from_millis(300))
    {
        if let Message::Notification(n) = msg {
            let diags: PublishDiagnosticsParams = serde_json::from_value(n.params).unwrap();
            assert!(diags.uri != util || has_error(&diags), "{diags:?}");
        }
    }
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

/// Sends a request by its raw parameters and returns the result as JSON.
fn ask(client: &Connection, id: i32, method: &str, params: serde_json::Value) -> serde_json::Value {
    client
        .sender
        .send(Message::Request(Request::new(
            RequestId::from(id),
            method.to_string(),
            params,
        )))
        .unwrap();
    loop {
        match client.receiver.recv().expect("expected a message") {
            Message::Response(resp) if resp.id == RequestId::from(id) => {
                return resp.result.expect("a result");
            }
            _ => continue,
        }
    }
}

fn at(line: u32, character: u32) -> serde_json::Value {
    serde_json::json!({
        "textDocument": { "uri": test_uri() },
        "position": { "line": line, "character": character },
    })
}

const ASSIST_SOURCE: &str = "import io { println }\n\nstruct Item {\n    price: Int\n}\n\nfn total(i: Item, extra: Int) -> Int {\n    i.price + extra\n}\n\nfn main() {\n    let it = Item { price: 2 }\n    println(\"{total(it, 1)}\")\n    \n}\n";

fn labels(result: &serde_json::Value) -> Vec<String> {
    result
        .as_array()
        .expect("items")
        .iter()
        .map(|item| item["label"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn completion_offers_locals_imports_and_library_names() {
    let (client, handle) = start_server();
    did_open(&client, ASSIST_SOURCE);
    let _ = recv_diagnostics(&client);

    let found = labels(&ask(&client, 2, "textDocument/completion", at(13, 4)));
    for name in [
        "it", "println", "total", "Item", "Ok", "None", "Int", "match",
    ] {
        assert!(found.iter().any(|l| l == name), "no {name} in {found:?}");
    }
    // Library names not imported yet wait until a name is being typed.
    assert!(!found.iter().any(|l| l == "str_len"), "{found:?}");
    // Nothing from outside this function.
    assert!(!found.iter().any(|l| l == "extra"), "{found:?}");

    shutdown(&client, handle);
}

#[test]
fn completing_an_unimported_name_adds_its_import() {
    let (client, handle) = start_server();
    let text = "import io { println }\n\nfn main() {\n    le\n}\n";
    did_open(&client, text);
    let _ = recv_diagnostics(&client);

    let result = ask(&client, 2, "textDocument/completion", at(3, 6));
    let len = result
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["label"] == "len")
        .expect("len offered");
    assert_eq!(
        len["additionalTextEdits"][0]["newText"],
        "import collections { len }\n"
    );

    shutdown(&client, handle);
}

#[test]
fn signature_help_covers_user_functions_and_the_library() {
    let (client, handle) = start_server();
    did_open(&client, ASSIST_SOURCE);
    let _ = recv_diagnostics(&client);

    // Inside `total(it, 1)`, on the second argument.
    let help = ask(&client, 2, "textDocument/signatureHelp", at(12, 24));
    assert_eq!(
        help["signatures"][0]["label"],
        "total(i: Item, extra: Int64) -> Int64"
    );
    assert_eq!(help["activeParameter"], 1);
    // Inside `println(`.
    let help = ask(&client, 3, "textDocument/signatureHelp", at(12, 12));
    assert_eq!(help["signatures"][0]["label"], "println(text: String)");

    shutdown(&client, handle);
}

#[test]
fn references_and_highlights_find_every_use() {
    let (client, handle) = start_server();
    did_open(&client, ASSIST_SOURCE);
    let _ = recv_diagnostics(&client);

    let mut params = at(6, 4);
    params["context"] = serde_json::json!({ "includeDeclaration": true });
    let found = ask(&client, 2, "textDocument/references", params);
    assert_eq!(found.as_array().unwrap().len(), 2, "{found}");
    let found = ask(&client, 3, "textDocument/documentHighlight", at(6, 4));
    assert_eq!(found.as_array().unwrap().len(), 2, "{found}");

    shutdown(&client, handle);
}

#[test]
fn inlay_hints_show_only_types_not_written() {
    let (client, handle) = start_server();
    did_open(&client, ASSIST_SOURCE);
    let _ = recv_diagnostics(&client);

    let hints = ask(
        &client,
        2,
        "textDocument/inlayHint",
        serde_json::json!({
            "textDocument": { "uri": test_uri() },
            "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 20, "character": 0 } },
        }),
    );
    let hints = hints.as_array().unwrap();
    assert_eq!(hints.len(), 1, "{hints:?}");
    assert_eq!(hints[0]["label"], ": Item");
    assert_eq!(hints[0]["position"]["line"], 11);

    shutdown(&client, handle);
}

#[test]
fn hover_documents_the_library_unless_the_program_owns_the_name() {
    let (client, handle) = start_server();
    did_open(&client, ASSIST_SOURCE);
    let _ = recv_diagnostics(&client);
    let hover = ask(&client, 2, "textDocument/hover", at(12, 6)).to_string();
    assert!(hover.contains("From `io`"), "{hover}");
    shutdown(&client, handle);

    let (client, handle) = start_server();
    did_open(
        &client,
        "fn len(a: Int) -> Int {\n    a\n}\n\nfn main() {\n    let _b = len(1)\n}\n",
    );
    let _ = recv_diagnostics(&client);
    let hover = ask(&client, 2, "textDocument/hover", at(5, 14)).to_string();
    assert!(!hover.contains("From `collections`"), "{hover}");
    shutdown(&client, handle);
}

#[test]
fn quick_fix_imports_an_undefined_library_name() {
    let (client, handle) = start_server();
    let text = "fn main() {\n    println(\"x\")\n}\n";
    did_open(&client, text);
    let diagnostics = recv_diagnostics(&client).diagnostics;
    assert!(!diagnostics.is_empty());

    let actions = ask(
        &client,
        2,
        "textDocument/codeAction",
        serde_json::json!({
            "textDocument": { "uri": test_uri() },
            "range": diagnostics[0].range,
            "context": { "diagnostics": diagnostics },
        }),
    );
    let action = &actions[0];
    assert_eq!(action["title"], "Import `println` from `io`");
    let edit = action["edit"]["changes"][test_uri().as_str()][0].clone();
    assert_eq!(edit["newText"], "import io { println }\n\n");

    shutdown(&client, handle);
}

#[test]
fn quick_fix_applies_the_replacement_a_warning_carries() {
    let (client, handle) = start_server();
    let text = "rfn f() -> Int {\n    1\n}\n";
    did_open(&client, text);
    let _ = recv_diagnostics(&client);

    // The compiler attaches `data.replacement` to an older form's warning.
    let warning = serde_json::json!({
        "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 3 } },
        "severity": 2,
        "code": "E026",
        "source": "flux",
        "message": "`rfn` is now written `track fn`",
        "data": { "replacement": "track fn" },
    });
    let actions = ask(
        &client,
        2,
        "textDocument/codeAction",
        serde_json::json!({
            "textDocument": { "uri": test_uri() },
            "range": warning["range"],
            "context": { "diagnostics": [warning] },
        }),
    );
    let action = &actions[0];
    assert_eq!(action["title"], "Replace with `track fn`");
    let edit = &action["edit"]["changes"][test_uri().as_str()][0];
    assert_eq!(edit["newText"], "track fn");
    assert_eq!(edit["range"]["end"]["character"], 3);

    shutdown(&client, handle);
}

#[test]
fn quick_fix_for_a_write_outside_act_fn_edits_the_fn_keyword() {
    let (client, handle) = start_server();
    let text = "let mut n = 0\npub fn bump() {\n    n = 1\n}\n";
    did_open(&client, text);
    let diags = recv_diagnostics(&client);
    let warning = diags
        .diagnostics
        .iter()
        .find(|d| d.code == Some(lsp_types::NumberOrString::String("E027".to_string())))
        .expect("an E027 warning")
        .clone();
    // The warning points at the write; its fix edits the `fn` keyword.
    assert_eq!(warning.range.start.line, 2);
    let warning = serde_json::to_value(&warning).unwrap();
    let actions = ask(
        &client,
        2,
        "textDocument/codeAction",
        serde_json::json!({
            "textDocument": { "uri": test_uri() },
            "range": warning["range"],
            "context": { "diagnostics": [warning] },
        }),
    );
    let action = &actions[0];
    assert_eq!(action["title"], "Make `bump` an `act fn`");
    let edit = &action["edit"]["changes"][test_uri().as_str()][0];
    assert_eq!(edit["newText"], "act fn");
    assert_eq!(edit["range"]["start"]["line"], 1);
    assert_eq!(edit["range"]["start"]["character"], 4);
    assert_eq!(edit["range"]["end"]["character"], 6);

    shutdown(&client, handle);
}

/// The edits of renaming the name at `line`/`character`, as `(line, start, end)`.
fn rename_ranges(text: &str, line: u32, character: u32) -> Vec<(u64, u64, u64)> {
    let (client, handle) = start_server();
    did_open(&client, text);
    let _ = recv_diagnostics(&client);
    let result = ask(
        &client,
        2,
        "textDocument/rename",
        serde_json::json!({
            "textDocument": { "uri": test_uri() },
            "position": { "line": line, "character": character },
            "newName": "renamed",
        }),
    );
    shutdown(&client, handle);
    let mut out: Vec<(u64, u64, u64)> = result["changes"][test_uri().as_str()]
        .as_array()
        .expect("edits")
        .iter()
        .map(|e| {
            let r = &e["range"];
            (
                r["start"]["line"].as_u64().unwrap(),
                r["start"]["character"].as_u64().unwrap(),
                r["end"]["character"].as_u64().unwrap(),
            )
        })
        .collect();
    out.sort();
    out
}

#[test]
fn rename_of_watch_prev_and_handler_values_edits_only_the_names() {
    let text = "let mut n = 0\nwatch n(old) {\n    let copy = old\n}\nfn main() {\n}\n";
    assert_eq!(rename_ranges(text, 2, 16), [(1, 8, 11), (2, 15, 18)]);
    let text = "event Move(dx: Int, dy: Int)\nlet mut x = 0\non Move(dx, dy) {\n    x = x + dx\n}\nfn main() {\n}\n";
    assert_eq!(rename_ranges(text, 3, 13), [(2, 8, 10), (3, 12, 14)]);
    // A watch's target and a handler's event: only the name in the head.
    let text =
        "event Tick\nlet mut n = 0\non Tick {\n    n = n + 1\n}\nwatch n {\n}\nfn main() {\n}\n";
    let edits = rename_ranges(text, 1, 8);
    assert!(edits.contains(&(5, 6, 7)), "{edits:?}");
    assert!(edits.iter().all(|(_, s, e)| e - s == 1), "{edits:?}");
}

#[test]
fn rename_of_a_method_edits_its_definition_and_calls() {
    let text = "struct P {\n    x: Int\n}\nimpl P {\n    fn len(self) -> Int {\n        self.x\n    }\n}\nfn main() {\n    let p = P { x: 1 }\n    let n = p.len()\n}\n";
    // From the call and from the definition.
    let from_call = rename_ranges(text, 10, 15);
    let from_def = rename_ranges(text, 4, 8);
    assert_eq!(from_call, from_def);
    assert!(from_call.contains(&(4, 7, 10)), "{from_call:?}");
    assert!(from_call.contains(&(10, 14, 17)), "{from_call:?}");
}

#[test]
fn rename_of_a_type_function_edits_only_its_name() {
    let text = "struct P {\n    x: Int\n}\nimpl P {\n    fn new() -> Self {\n        Self { x: 0 }\n    }\n}\nfn main() {\n    let p = P::new()\n}\n";
    let edits = rename_ranges(text, 9, 15);
    assert_eq!(edits, [(4, 7, 10), (9, 15, 18)]);
}

#[test]
fn hover_on_a_method_call_shows_the_method() {
    let (client, handle) = start_server();
    did_open(
        &client,
        "struct P {\n    x: Int\n}\nimpl P {\n    fn len(self) -> Int {\n        self.x\n    }\n}\nfn main() {\n    let p = P { x: 1 }\n    let _n = p.len()\n}\n",
    );
    let _ = recv_diagnostics(&client);
    let hover = ask(&client, 2, "textDocument/hover", at(10, 16)).to_string();
    assert!(hover.contains("len: fn("), "{hover}");
    assert!(!hover.contains("From `collections`"), "{hover}");
    shutdown(&client, handle);
}

#[test]
fn rename_of_a_type_function_reaches_self_paths() {
    let text = "struct C {\n    n: Int\n}\nimpl C {\n    fn zero() -> Self {\n        Self { n: 0 }\n    }\n    fn origin() -> Self {\n        Self::zero()\n    }\n}\nfn main() {\n    let c = C::zero()\n}\n";
    let edits = rename_ranges(text, 12, 15);
    assert_eq!(edits, [(4, 7, 11), (8, 14, 18), (12, 15, 19)]);
}

#[test]
fn completion_after_a_dot_offers_library_methods() {
    let (client, handle) = start_server();
    let text = "fn main() {\n    let xs = [3, 1]\n    let n = xs.len()\n}\n";
    did_open(&client, text);
    let _ = recv_diagnostics(&client);
    let result = ask(&client, 2, "textDocument/completion", at(2, 15)).to_string();
    assert!(result.contains("\"sorted\""), "{result}");
    assert!(result.contains("\"push\""), "{result}");
    assert!(!result.contains("\"split\""), "{result}");
    shutdown(&client, handle);
}

#[test]
fn hover_and_signature_help_know_library_methods() {
    let (client, handle) = start_server();
    let text = "fn main() {\n    let xs = [3, 1]\n    let s = \"a,b\"\n    let ys = xs.slice(0, 1)\n    let n = s.len()\n}\n";
    did_open(&client, text);
    let _ = recv_diagnostics(&client);
    // `slice` and `len` on an array and on a string.
    let slice = ask(&client, 2, "textDocument/hover", at(3, 17)).to_string();
    assert!(
        slice.contains("xs.slice(start: Int, end: Int) -> [T]"),
        "{slice}"
    );
    let len = ask(&client, 3, "textDocument/hover", at(4, 15)).to_string();
    assert!(len.contains("s.len() -> Int"), "{len}");
    assert!(len.contains("number of characters"), "{len}");
    // Inside `xs.slice(0, |1)`: the second parameter.
    let help = ask(&client, 4, "textDocument/signatureHelp", at(3, 25)).to_string();
    assert!(help.contains("xs.slice(start: Int, end: Int)"), "{help}");
    assert!(help.contains("\"activeParameter\":1"), "{help}");
    shutdown(&client, handle);
}

#[test]
fn library_method_help_follows_the_receiver_and_the_open_call() {
    let (client, handle) = start_server();
    let text = "struct P {\n    xs: [Int],\n}\nimpl Int {\n    fn abs(self) -> String {\n        \"custom\"\n    }\n}\nfn total(a: Int, b: Int) -> Int {\n    a + b\n}\nfn main() {\n    let s = \"💡hello\"\n    let p = P { xs: [1] }\n    let xs = \"abc\"\n    let n = p.xs.len()\n    let m = 1\n    let c = m.abs()\n    let t = total(xs.len(), 2)\n}\n";
    did_open(&client, text);
    let _ = recv_diagnostics(&client);
    // Inside a string after an emoji: no crash.
    let _ = ask(&client, 2, "textDocument/hover", at(12, 17));
    // `p.xs` is the field, an array, not the local string `xs`.
    let field = ask(&client, 3, "textDocument/hover", at(15, 18)).to_string();
    assert!(field.contains("xs.len() -> Int"), "{field}");
    assert!(field.contains("number of elements"), "{field}");
    // The program's own `Int.abs` hides the library's.
    let own = ask(&client, 4, "textDocument/hover", at(17, 15)).to_string();
    assert!(!own.contains("n.abs() -> N"), "{own}");
    // After the finished inner `xs.len()`: the outer `total`.
    let help = ask(&client, 5, "textDocument/signatureHelp", at(18, 28)).to_string();
    assert!(help.contains("total("), "{help}");
    assert!(help.contains("\"activeParameter\":1"), "{help}");
    shutdown(&client, handle);
}

#[test]
fn definition_of_a_shadowing_local_is_the_local() {
    let (client, handle) = start_server();
    let text = "const limit = 10\nfn f() -> Int {\n    let limit = 3\n    limit + 1\n}\nfn main() {\n    let n = f() + limit\n}\n";
    did_open(&client, text);
    let _ = recv_diagnostics(&client);
    let local = ask(&client, 2, "textDocument/definition", at(3, 5));
    assert_eq!(local["range"]["start"]["line"], 2, "{local}");
    let module = ask(&client, 3, "textDocument/definition", at(6, 19));
    assert_eq!(module["range"]["start"]["line"], 0, "{module}");
    shutdown(&client, handle);
}

#[test]
fn an_imported_type_is_found_in_its_own_module_not_a_same_named_one() {
    // `util` and `other` both declare `Point`; the root imports `util`'s.
    let (dir, main, util) = import_project("pub struct Point {\n    pub x: Int\n}\n");
    let root = dir.path().canonicalize().unwrap();
    std::fs::write(
        root.join("other.fl"),
        "\n\npub struct Point {\n    pub y: Int\n}\npub fn n() -> Int {\n    1\n}\n",
    )
    .unwrap();
    let text = "import util { Point }\nimport other { n }\nstruct Holder {\n    p: Point\n}\nfn main() {}\n";
    std::fs::write(root.join("main.fl"), text).unwrap();
    let (client, handle) = start_server();
    did_open_uri(&client, main.clone(), text);
    let _ = recv_diagnostics(&client);
    let position = |id: i32, method: &str| {
        ask(
            &client,
            id,
            method,
            serde_json::json!({
                "textDocument": { "uri": main.clone() },
                "position": { "line": 3, "character": 8 },
            }),
        )
    };
    let location = position(2, "textDocument/definition");
    assert_eq!(location["uri"], util.to_string(), "{location}");
    let completion = ask(
        &client,
        3,
        "textDocument/completion",
        serde_json::json!({
            "textDocument": { "uri": main.clone() },
            "position": { "line": 3, "character": 7 },
        }),
    );
    assert!(completion.to_string().contains("\"Point\""), "{completion}");
    shutdown(&client, handle);
}

#[test]
fn a_local_named_like_an_imported_type_and_a_qualified_type_resolve_right() {
    let (dir, main, util) = import_project("pub struct Point {\n    pub x: Int\n}\n");
    let root = dir.path().canonicalize().unwrap();
    std::fs::write(
        root.join("other.fl"),
        "\n\npub struct Point {\n    pub y: Int\n}\npub fn n() -> Int {\n    1\n}\n",
    )
    .unwrap();
    let other = Url::from_file_path(root.join("other.fl")).unwrap();
    let text = "import util { Point }\nimport other\nstruct Holder {\n    p: other::Point\n}\nfn f(Point: Int) -> Int {\n    Point\n}\nfn main() {}\n";
    std::fs::write(root.join("main.fl"), text).unwrap();
    let (client, handle) = start_server();
    did_open_uri(&client, main.clone(), text);
    let _ = recv_diagnostics(&client);
    let at_main = |line: u32, character: u32| {
        serde_json::json!({
            "textDocument": { "uri": main.clone() },
            "position": { "line": line, "character": character },
        })
    };
    let location = ask(&client, 2, "textDocument/definition", at_main(3, 15));
    assert_eq!(location["uri"], other.to_string(), "{location}");
    assert_eq!(location["range"]["start"]["line"], 2, "{location}");
    let hover = ask(&client, 3, "textDocument/hover", at_main(6, 5)).to_string();
    assert!(hover.contains("Point: Int64"), "{hover}");
    let _ = util;
    shutdown(&client, handle);
}

/// A generic struct's fields have the types of its arguments: `p.` offers
/// `a: Int64` for a `Pair[Int]`, and hover shows the instance type.
#[test]
fn members_and_hover_of_a_generic_type_use_its_arguments() {
    let (client, handle) = start_server();
    let text = "struct Pair[T] { a: T, b: T }\nfn main() {\n    let p = Pair { a: 1, b: 2 }\n    let n = p.a\n}\n";
    did_open(&client, text);
    let _ = recv_diagnostics(&client);
    let at = |line: u32, character: u32| {
        serde_json::json!({
            "textDocument": { "uri": test_uri() },
            "position": { "line": line, "character": character },
        })
    };
    let items = ask(&client, 2, "textDocument/completion", at(3, 14)).to_string();
    assert!(items.contains("\"label\":\"a\""), "{items}");
    assert!(items.contains("Int64"), "{items}");
    assert!(!items.contains("\"T\""), "{items}");
    let hover = ask(&client, 3, "textDocument/hover", at(2, 8)).to_string();
    assert!(hover.contains("Pair[Int64]"), "{hover}");
    shutdown(&client, handle);
}

/// A user-typed field of a generic instance offers its own fields.
#[test]
fn a_field_of_a_generic_instance_offers_its_members() {
    let (client, handle) = start_server();
    let text = "struct P { n: Int }\nstruct B[T] { x: T }\nfn main() {\n    let b = B { x: P { n: 1 } }\n    let p = b.x\n    let m = p.n\n}\n";
    did_open(&client, text);
    let _ = recv_diagnostics(&client);
    let at = serde_json::json!({
        "textDocument": { "uri": test_uri() },
        "position": { "line": 5, "character": 14 },
    });
    let items = ask(&client, 2, "textDocument/completion", at).to_string();
    assert!(items.contains("\"label\":\"n\""), "{items}");
    shutdown(&client, handle);
}

/// The methods of a generic impl are offered on its instances.
#[test]
fn a_generic_impls_methods_are_offered_on_an_instance() {
    let (client, handle) = start_server();
    let text = "struct Pair[T] { a: T, b: T }\nimpl[T] Pair[T] {\n    fn swap(self) -> Pair[T] {\n        Pair { a: self.b, b: self.a }\n    }\n}\nfn main() {\n    let p = Pair { a: 1, b: 2 }\n    let q = p.swap()\n}\n";
    did_open(&client, text);
    let diagnostics = recv_diagnostics(&client);
    assert!(diagnostics.diagnostics.is_empty(), "{diagnostics:?}");
    let at = serde_json::json!({
        "textDocument": { "uri": test_uri() },
        "position": { "line": 8, "character": 14 },
    });
    let items = ask(&client, 2, "textDocument/completion", at).to_string();
    assert!(items.contains("\"label\":\"swap\""), "{items}");
    shutdown(&client, handle);
}

/// Renaming a generic impl's method renames its calls on every instance,
/// each of which runs its own specialization.
#[test]
fn rename_of_a_generic_method_reaches_every_instance() {
    let text = "struct P[T] { x: T }\nimpl[T] P[T] {\n    fn get(self) -> T {\n        self.x\n    }\n}\nfn main() {\n    let a = P { x: 1 }.get()\n    let b = P { x: true }.get()\n}\n";
    let ranges = rename_ranges(text, 7, 24);
    assert_eq!(ranges, [(2, 7, 10), (7, 23, 26), (8, 26, 29)], "{ranges:?}");
}

/// Hover on a call of a generic method shows the instance's signature.
#[test]
fn hover_on_a_generic_method_call_shows_the_instance() {
    let (client, handle) = start_server();
    let text = "struct P[T] { x: T }\nimpl[T] P[T] {\n    fn get(self) -> T {\n        self.x\n    }\n}\nfn main() {\n    let p = P { x: 1 }\n    let n = p.get()\n}\n";
    did_open(&client, text);
    let _ = recv_diagnostics(&client);
    let at = serde_json::json!({
        "textDocument": { "uri": test_uri() },
        "position": { "line": 8, "character": 15 },
    });
    let hover = ask(&client, 2, "textDocument/hover", at).to_string();
    assert!(!hover.contains("<error>"), "{hover}");
    assert!(hover.contains("Int64"), "{hover}");
    shutdown(&client, handle);
}

/// A module with its own handlers and watches has no errors in the editor,
/// and writing its source from an importer is E032 there.
#[test]
fn a_reactive_module_checks_and_its_sources_are_read_only_outside() {
    let (_dir, main, util) = import_project(
        "pub let mut count = 0\npub event Inc(by: Int)\non Inc(by) {\n    count += by\n}\nwatch count {\n    let seen = count\n}\n",
    );
    let (client, handle) = start_server();
    let util_text = std::fs::read_to_string(util.to_file_path().unwrap()).unwrap();
    did_open_uri(&client, util.clone(), &util_text);
    let module = recv_diagnostics(&client);
    assert!(module.diagnostics.is_empty(), "{module:?}");
    did_open_uri(
        &client,
        main,
        "import util { count, Inc }\nevent Go\non Go {\n    count = 1\n}\nfn main() {\n}\n",
    );
    let root = recv_diagnostics(&client);
    assert!(
        root.diagnostics
            .iter()
            .any(|d| d.message.contains("belongs to another module")),
        "{root:?}"
    );
    shutdown(&client, handle);
}

/// Renaming an imported module's event renames it where it is declared,
/// handled, and emitted, also written `c::E`.
#[test]
fn rename_of_a_module_event_reaches_qualified_uses() {
    let (_dir, main, util) = import_project("pub event E(n: Int)\n");
    let root = util.to_file_path().unwrap().parent().unwrap().to_path_buf();
    let text = "import util as c\non c::E(n) {\n}\nact fn main() {\n    emit c::E(1)\n}\n";
    std::fs::write(root.join("main.fl"), text).unwrap();
    let (client, handle) = start_server();
    did_open_uri(&client, main.clone(), text);
    let _ = recv_diagnostics(&client);
    let result = ask(
        &client,
        2,
        "textDocument/rename",
        serde_json::json!({
            "textDocument": { "uri": main.clone() },
            "position": { "line": 1, "character": 6 },
            "newName": "F",
        }),
    );
    shutdown(&client, handle);
    let edits = &result["changes"][main.as_str()];
    let ranges: Vec<(u64, u64, u64)> = edits
        .as_array()
        .expect("edits in main")
        .iter()
        .map(|e| {
            let r = &e["range"];
            (
                r["start"]["line"].as_u64().unwrap(),
                r["start"]["character"].as_u64().unwrap(),
                r["end"]["character"].as_u64().unwrap(),
            )
        })
        .collect();
    let mut ranges = ranges;
    ranges.sort();
    assert_eq!(ranges, [(1, 6, 7), (4, 12, 13)], "{result}");
    assert!(result["changes"][util.as_str()].is_array(), "{result}");
}

/// A module alias spelled like the event keeps its name on rename.
#[test]
fn rename_of_an_event_leaves_an_alias_of_the_same_spelling() {
    let (_dir, main, util) = import_project("pub event E(n: Int)\n");
    let root = util.to_file_path().unwrap().parent().unwrap().to_path_buf();
    let text = "import util as E\non E::E(n) {\n}\nact fn main() {\n    emit E::E(1)\n}\n";
    std::fs::write(root.join("main.fl"), text).unwrap();
    let (client, handle) = start_server();
    did_open_uri(&client, main.clone(), text);
    let _ = recv_diagnostics(&client);
    let result = ask(
        &client,
        2,
        "textDocument/rename",
        serde_json::json!({
            "textDocument": { "uri": main.clone() },
            "position": { "line": 1, "character": 6 },
            "newName": "F",
        }),
    );
    shutdown(&client, handle);
    let mut ranges: Vec<(u64, u64)> = result["changes"][main.as_str()]
        .as_array()
        .expect("edits in main")
        .iter()
        .map(|e| {
            let r = &e["range"];
            (
                r["start"]["line"].as_u64().unwrap(),
                r["start"]["character"].as_u64().unwrap(),
            )
        })
        .collect();
    ranges.sort();
    assert_eq!(ranges, [(1, 6), (4, 12)], "{result}");
}

/// Another module's private fields are not offered after `.`.
#[test]
fn completion_leaves_out_another_modules_private_fields() {
    let (dir, main, _util) = import_project(
        "pub struct Account {\n    pub owner: String,\n    balance: Int\n}\npub fn open() -> Account {\n    Account { owner: \"a\", balance: 0 }\n}\n",
    );
    let root = dir.path().canonicalize().unwrap();
    let text =
        "import util { Account, open }\nfn main() {\n    let a = open()\n    let o = a.owner\n}\n";
    std::fs::write(root.join("main.fl"), text).unwrap();
    let (client, handle) = start_server();
    did_open_uri(&client, main.clone(), text);
    let _ = recv_diagnostics(&client);
    let at = serde_json::json!({
        "textDocument": { "uri": main },
        "position": { "line": 3, "character": 14 },
    });
    let items = ask(&client, 2, "textDocument/completion", at).to_string();
    assert!(items.contains("\"label\":\"owner\""), "{items}");
    assert!(!items.contains("\"label\":\"balance\""), "{items}");
    shutdown(&client, handle);
}
