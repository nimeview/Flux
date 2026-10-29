use miette::SourceSpan;

/// Identifier for a loaded source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SourceId(pub u32);

/// A single source file with its path and contents.
#[derive(Debug, Clone)]
pub struct SourceFile {
    pub id: SourceId,
    pub path: String,
    pub contents: String,
}

impl miette::SourceCode for SourceFile {
    fn read_span<'a>(
        &'a self,
        span: &SourceSpan,
        context_lines_before: usize,
        context_lines_after: usize,
    ) -> Result<Box<dyn miette::SpanContents<'a> + 'a>, miette::MietteError> {
        self.contents
            .read_span(span, context_lines_before, context_lines_after)
    }
}

/// Byte range in a source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Span {
    pub source: SourceId,
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub const DUMMY: Self = Self {
        source: SourceId(0),
        start: 0,
        end: 0,
    };

    pub fn new(source: SourceId, start: usize, end: usize) -> Self {
        Self { source, start, end }
    }

    pub fn to_miette(&self) -> SourceSpan {
        SourceSpan::new(self.start.into(), self.end.saturating_sub(self.start))
    }

    pub fn merge(a: Self, b: Self) -> Self {
        Self {
            source: a.source,
            start: a.start.min(b.start),
            end: a.end.max(b.end),
        }
    }

    pub fn len(&self) -> usize {
        self.end.saturating_sub(self.start)
    }

    pub fn is_empty(&self) -> bool {
        self.start >= self.end
    }
}

/// Convert a byte offset in `contents` to 1-based `(line, column)`.
pub fn offset_to_line_col(offset: usize, contents: &str) -> (usize, usize) {
    let lines: Vec<&str> = contents.lines().collect();
    offset_to_line_col_lines(offset, &lines)
}

/// Convert 1-based `(line, column)` in `contents` to a byte offset.
pub fn line_col_to_offset(line: usize, column: usize, contents: &str) -> usize {
    let mut offset = 0;
    for (i, l) in contents.split('\n').enumerate() {
        if i + 1 == line {
            let chars: Vec<char> = l.chars().collect();
            let take = column.saturating_sub(1).min(chars.len());
            offset += chars[..take].iter().map(|c| c.len_utf8()).sum::<usize>();
            return offset;
        }
        offset += l.len() + 1;
    }
    contents.len()
}

fn offset_to_line_col_lines(offset: usize, lines: &[&str]) -> (usize, usize) {
    let mut pos = 0;
    for (i, line) in lines.iter().enumerate() {
        let next = pos + line.len() + 1;
        if offset < next {
            return (i + 1, offset.saturating_sub(pos) + 1);
        }
        pos = next;
    }
    (lines.len().max(1), 1)
}
