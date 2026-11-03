use flux_diagnostics::{DiagnosticBag, FluxError, SourceId};
use miette::Diagnostic;

#[test]
fn contextual_mismatch_keeps_e005_identity_and_renders_help() {
    let make = || FluxError::TypeMismatchWithHelp {
        expected: "field `total`".into(),
        found: "Int64".into(),
        span: (12, 7).into(),
        source_id: SourceId(3),
        help: "annotate the closure parameter".into(),
    };
    let ordinary = FluxError::TypeMismatch {
        expected: "field `total`".into(),
        found: "Int64".into(),
        span: (12, 7).into(),
        source_id: SourceId(3),
    };
    let error = make();
    assert_eq!(error.code(), "E005");
    assert_eq!(Diagnostic::code(&error).unwrap().to_string(), "flux::E005");
    assert_eq!(error.source_id(), ordinary.source_id());
    assert_eq!(error.label_span(), ordinary.label_span());
    assert_eq!(error.to_string(), ordinary.to_string());
    assert!(ordinary.help().is_none());
    let report = format!("{:?}", miette::Report::new(error));
    assert!(
        report.contains("help: annotate the closure parameter"),
        "{report}"
    );
    let mut diagnostics = DiagnosticBag::new();
    diagnostics.push(make());
    diagnostics.push(make());
    assert_eq!(diagnostics.errors.len(), 1);
}

#[test]
fn e016_keeps_read_message_and_distinguishes_assignment() {
    for (assigned, expected) in [
        (
            false,
            "`let` without `mut` cannot read reactive binding `c`",
        ),
        (true, "binding `c` is assigned; declare it with `let mut`"),
    ] {
        let error = FluxError::ImmutableLetReadsReactive {
            name: "c".into(),
            assigned,
            span: (0, 1).into(),
            source_id: SourceId(0),
        };
        assert_eq!(error.code(), "E016");
        assert_eq!(error.to_string(), expected);
    }
}
