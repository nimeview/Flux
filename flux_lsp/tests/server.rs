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
