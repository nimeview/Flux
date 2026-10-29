use flux_diagnostics::{
    line_col_to_offset, offset_to_line_col, DiagnosticBag, FluxError, FluxWarning, SourceId, Span,
};

fn span(start: usize, end: usize) -> Span {
    Span {
        source: SourceId(0),
        start,
        end,
    }
}

#[test]
fn error_codes_are_stable() {
    let sid = SourceId(1);
    let s = span(0, 1).to_miette();
    let cases: Vec<(&str, FluxError)> = vec![
        (
            "E000",
            FluxError::Parse {
                message: "x".into(),
                span: s,
                source_id: sid,
            },
        ),
        (
            "E001",
            FluxError::UndefinedName {
                name: "x".into(),
                span: s,
                source_id: sid,
            },
        ),
        (
            "E004",
            FluxError::DuplicateDefinition {
                name: "x".into(),
                span: s,
                prev_span: s,
                source_id: sid,
            },
        ),
        (
            "E017",
            FluxError::SideEffectInDerived {
                name: "x".into(),
                span: s,
                source_id: sid,
            },
        ),
    ];
    for (code, err) in cases {
        assert_eq!(err.code(), code);
    }
}

#[test]
fn warning_code_is_e015() {
    let w = FluxWarning::UnusedBinding {
        name: "tmp".into(),
        span: span(2, 5).to_miette(),
        source_id: SourceId(0),
    };
    assert_eq!(w.code(), "E015");
    assert_eq!(w.label_span().start, 2);
    assert_eq!(w.label_span().end, 5);
}

#[test]
fn diagnostic_bag_tracks_warnings_separately() {
    let mut bag = DiagnosticBag::new();
    bag.push_warning(FluxWarning::UnusedBinding {
        name: "x".into(),
        span: span(0, 1).to_miette(),
        source_id: SourceId(0),
    });
    assert!(!bag.has_errors());
    assert!(bag.has_warnings());
    assert!(!bag.is_empty());
}

#[test]
fn offset_line_col_roundtrip() {
    let src = "line one\nline two\nthird";
    let offset = line_col_to_offset(2, 3, src);
    assert_eq!(offset, 11);
    let (line, col) = offset_to_line_col(offset, src);
    assert_eq!((line, col), (2, 3));
}

#[test]
fn offset_to_line_col_first_line() {
    let src = "hello";
    assert_eq!(offset_to_line_col(0, src), (1, 1));
    assert_eq!(offset_to_line_col(3, src), (1, 4));
}
