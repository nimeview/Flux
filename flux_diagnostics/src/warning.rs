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
    /// An older form whose replacement cannot be written as one edit.
    #[error("{message}")]
    #[diagnostic(code(flux::E026))]
    OldForm {
        message: String,
        #[label("older form")]
        span: miette::SourceSpan,
        source_id: SourceId,
    },
    /// Watches that wake each other in a cycle.
    #[error("{message}")]
    #[diagnostic(
        code(flux::E029),
        help("each watch runs at most once per batch, so the last change in the cycle does not wake the watch it reaches; compute the value with `let`, or change it in an `on` handler")
    )]
    WatchCycle {
        message: String,
        #[label("this watch")]
        span: miette::SourceSpan,
        source_id: SourceId,
    },
    /// A collection function called for a result nobody uses.
    #[error("the result of `{name}` is not used")]
    #[diagnostic(code(flux::E030), help("{help}"))]
    UnusedResult {
        name: String,
        help: String,
        #[label("this value is dropped")]
        span: miette::SourceSpan,
        source_id: SourceId,
    },
    /// A file beside the importer has a library module's name: `import`
    /// of that name means the library module, never the file.
    #[error("`{file}` is not imported: `{name}` is a library module")]
    #[diagnostic(
        code(flux::E035),
        help("rename the file to import it; `import {name}` always means the library")
    )]
    ShadowedLibraryModule {
        name: String,
        file: String,
        #[label("the library module `{name}`")]
        span: miette::SourceSpan,
        source_id: SourceId,
    },
    /// A watch changes a source.
    #[error("this watch changes `{name}`")]
    #[diagnostic(
        code(flux::E028),
        help("a watch reacts to changes; compute `{name}` with `let`, or `emit` an event whose `on` handler changes it")
    )]
    WatchWrites {
        name: String,
        #[label("changes state")]
        span: miette::SourceSpan,
        source_id: SourceId,
    },
}

/// A replacement of the text at `span`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fix {
    /// What the fix does, for an editor's menu.
    pub title: String,
    pub span: Span,
    pub replacement: String,
}

impl FluxWarning {
    pub fn source_id(&self) -> SourceId {
        match self {
            Self::UnusedBinding { source_id, .. }
            | Self::Deprecated { source_id, .. }
            | Self::WatchWrites { source_id, .. }
            | Self::ShadowedLibraryModule { source_id, .. }
            | Self::WatchCycle { source_id, .. }
            | Self::UnusedResult { source_id, .. }
            | Self::OldForm { source_id, .. } => *source_id,
        }
    }

    pub fn code(&self) -> &'static str {
        match self {
            Self::UnusedBinding { .. } => "E015",
            Self::Deprecated { .. } | Self::OldForm { .. } => "E026",
            Self::WatchWrites { .. } => "E028",
            Self::ShadowedLibraryModule { .. } => "E035",
            Self::WatchCycle { .. } => "E029",
            Self::UnusedResult { .. } => "E030",
        }
    }

    pub fn label_span(&self) -> Span {
        let source_id = self.source_id();
        let miette_span = match self {
            Self::UnusedBinding { span, .. }
            | Self::Deprecated { span, .. }
            | Self::WatchWrites { span, .. }
            | Self::ShadowedLibraryModule { span, .. }
            | Self::WatchCycle { span, .. }
            | Self::UnusedResult { span, .. }
            | Self::OldForm { span, .. } => *span,
        };
        span_from_miette(source_id, miette_span)
    }

    /// The edit that brings the code up to date, if the warning has one.
    pub fn fix(&self) -> Option<Fix> {
        match self {
            Self::Deprecated { replacement, .. } => Some(Fix {
                title: format!("Replace with `{replacement}`"),
                span: self.label_span(),
                replacement: replacement.clone(),
            }),
            Self::UnusedBinding { .. }
            | Self::WatchWrites { .. }
            | Self::ShadowedLibraryModule { .. }
            | Self::WatchCycle { .. }
            | Self::UnusedResult { .. }
            | Self::OldForm { .. } => None,
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
