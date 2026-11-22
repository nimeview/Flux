use miette::Diagnostic;
use thiserror::Error;

use crate::{SourceId, Span};

/// Non-fatal compiler warnings (compilation may still succeed).
#[derive(Debug, Clone, Diagnostic, Error)]
pub enum FluxWarning {
    #[error("unused binding `{name}`")]
    #[diagnostic(
        code(flux::E015),
        help("remove the binding or reference it from a derived value, watch, or handler")
    )]
    UnusedBinding {
        name: String,
        #[label("never read")]
        span: miette::SourceSpan,
        source_id: SourceId,
    },
    /// An older form that still works. `replacement` is the exact text that
    /// replaces `span`, applied by the editor's quick fix and `flux migrate`.
    #[error("{message}")]
    #[diagnostic(code(flux::E026))]
    Deprecated {
        message: String,
        replacement: String,
        #[label("replace with `{replacement}`")]
        span: miette::SourceSpan,
        source_id: SourceId,
    },
}

/// A replacement of the text at `span`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fix {
    pub span: Span,
    pub replacement: String,
}

impl FluxWarning {
    pub fn source_id(&self) -> SourceId {
        match self {
            Self::UnusedBinding { source_id, .. } | Self::Deprecated { source_id, .. } => {
                *source_id
            }
        }
    }

    pub fn code(&self) -> &'static str {
        match self {
            Self::UnusedBinding { .. } => "E015",
            Self::Deprecated { .. } => "E026",
        }
    }

    pub fn label_span(&self) -> Span {
        let source_id = self.source_id();
        let miette_span = match self {
            Self::UnusedBinding { span, .. } | Self::Deprecated { span, .. } => *span,
        };
        span_from_miette(source_id, miette_span)
    }

    /// The edit that brings the code up to date, if the warning has one.
    pub fn fix(&self) -> Option<Fix> {
        match self {
            Self::Deprecated { replacement, .. } => Some(Fix {
                span: self.label_span(),
                replacement: replacement.clone(),
            }),
            Self::UnusedBinding { .. } => None,
        }
    }
}

pub(crate) fn span_from_miette(source_id: SourceId, span: miette::SourceSpan) -> Span {
    let start = span.offset();
    Span {
        source: source_id,
        start,
        end: start + span.len(),
    }
}

/// Applies `fixes` to `text`, each given by byte range and replacement. A fix
/// that overlaps one already taken, or whose range is not on character
/// boundaries, is skipped. Returns the new text and how many fixes were applied.
pub fn apply_fixes(text: &str, fixes: &[(usize, usize, String)]) -> (String, usize) {
    let mut sorted: Vec<&(usize, usize, String)> = fixes.iter().collect();
    sorted.sort_by_key(|(start, end, _)| (*start, *end));
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    let mut applied = 0;
    for (start, end, replacement) in sorted {
        let (start, end) = (*start, *end);
        let fits = start >= at
            && start <= end
            && end <= text.len()
            && text.is_char_boundary(start)
            && text.is_char_boundary(end);
        if !fits {
            continue;
        }
        out.push_str(&text[at..start]);
        out.push_str(replacement);
        at = end;
        applied += 1;
    }
    out.push_str(&text[at..]);
    (out, applied)
}

#[cfg(test)]
mod tests {
    use super::apply_fixes;

    fn fix(start: usize, end: usize, text: &str) -> (usize, usize, String) {
        (start, end, text.to_string())
    }

    #[test]
    fn applies_in_order_whatever_order_they_come_in() {
        let (out, n) = apply_fixes(
            "rfn a\nrfn b\n",
            &[fix(6, 9, "track fn"), fix(0, 3, "track fn")],
        );
        assert_eq!(out, "track fn a\ntrack fn b\n");
        assert_eq!(n, 2);
    }

    #[test]
    fn skips_overlaps_and_bad_ranges() {
        let (out, n) = apply_fixes(
            "abcdef",
            &[
                fix(1, 4, "X"),
                fix(2, 3, "Y"),
                fix(5, 9, "Z"),
                fix(4, 2, "W"),
            ],
        );
        assert_eq!(out, "aXef");
        assert_eq!(n, 1);
    }

    #[test]
    fn respects_character_boundaries() {
        // `é` is two bytes; a range inside it is not applied.
        let (out, n) = apply_fixes("é a", &[fix(1, 2, "x"), fix(3, 4, "b")]);
        assert_eq!(out, "é b");
        assert_eq!(n, 1);
    }

    #[test]
    fn empty_ranges_insert() {
        let (out, _) = apply_fixes("ab", &[fix(1, 1, "-")]);
        assert_eq!(out, "a-b");
    }
}
