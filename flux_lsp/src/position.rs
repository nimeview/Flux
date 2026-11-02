//! The one place LSP positions and Flux byte offsets are converted.
//!
//! LSP `Position.character` counts UTF-16 code units (the protocol default
//! `positionEncoding`); Flux spans are UTF-8 byte offsets. Lines end at `\n`.

/// 0-based `(line, UTF-16 character)` for a byte `offset` into `text`.
/// Offsets past the end clamp to the end; offsets inside a character snap to its start.
pub fn offset_to_position(text: &str, offset: usize) -> (u32, u32) {
    let mut offset = offset.min(text.len());
    while !text.is_char_boundary(offset) {
        offset -= 1;
    }
    let before = &text[..offset];
    let line_start = before.rfind('\n').map_or(0, |i| i + 1);
    let line = before.matches('\n').count();
    let character = before[line_start..].encode_utf16().count();
    (line as u32, character as u32)
}

/// [`offset_to_position`] for many offsets into one text: finding the line
/// once per diagnostic was quadratic on a file with many of them.
pub struct LineIndex<'a> {
    text: &'a str,
    line_starts: Vec<usize>,
}

impl<'a> LineIndex<'a> {
    pub fn new(text: &'a str) -> Self {
        let mut line_starts = vec![0];
        line_starts.extend(text.match_indices('\n').map(|(i, _)| i + 1));
        Self { text, line_starts }
    }

    pub fn position(&self, offset: usize) -> (u32, u32) {
        let mut offset = offset.min(self.text.len());
        while !self.text.is_char_boundary(offset) {
            offset -= 1;
        }
        let line = self.line_starts.partition_point(|&start| start <= offset) - 1;
        let character = self.text[self.line_starts[line]..offset]
            .encode_utf16()
            .count();
        (line as u32, character as u32)
    }

    /// [`byte_range_to_lsp`] for this text.
    pub fn range(&self, start: usize, end: usize) -> (u32, u32, u32, u32) {
        let end = widened_end(self.text, start, end);
        let (sl, sc) = self.position(start);
        let (el, ec) = self.position(end);
        (sl, sc, el, ec)
    }
}

/// Byte offset into `text` for a 0-based LSP `(line, UTF-16 character)`.
/// A line past the end maps to the end of `text`; a character past the end of
/// its line maps to the line end; one inside a surrogate pair snaps to the
/// start of that character.
pub fn position_to_offset(text: &str, line: u32, character: u32) -> usize {
    let mut line_start = 0;
    for _ in 0..line {
        match text[line_start..].find('\n') {
            Some(i) => line_start += i + 1,
            None => return text.len(),
        }
    }
    let line_text = text[line_start..].split('\n').next().unwrap_or("");
    let mut units = 0;
    for (i, c) in line_text.char_indices() {
        let next = units + c.len_utf16() as u32;
        if next > character {
            return line_start + i;
        }
        units = next;
    }
    line_start + line_text.len()
}

/// 0-based `(start_line, start_char, end_line, end_char)` for a byte range.
/// Empty ranges widen to one character so editors can show them.
pub fn byte_range_to_lsp(text: &str, start: usize, end: usize) -> (u32, u32, u32, u32) {
    let end = widened_end(text, start, end);
    let (sl, sc) = offset_to_position(text, start);
    let (el, ec) = offset_to_position(text, end);
    (sl, sc, el, ec)
}

fn widened_end(text: &str, start: usize, end: usize) -> usize {
    if end > start {
        return end;
    }
    let mut at = start.min(text.len());
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    text[at..]
        .chars()
        .next()
        .map_or(start, |c| start + c.len_utf8())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_line_index_agrees_with_the_direct_conversion() {
        let index = LineIndex::new(TEXT);
        for offset in 0..=TEXT.len() + 2 {
            assert_eq!(
                index.position(offset),
                offset_to_position(TEXT, offset),
                "{offset}"
            );
            assert_eq!(
                index.range(offset, offset + 1),
                byte_range_to_lsp(TEXT, offset, offset + 1),
                "{offset}"
            );
        }
    }

    // `é` is 2 bytes / 1 unit; `𝒳` (U+1D4B3) is 4 bytes / 2 units.
    const TEXT: &str = "let é = 1\nlet 𝒳x = \"𝒳\"\n";

    #[test]
    fn round_trips_every_char_boundary() {
        for (offset, _) in TEXT.char_indices() {
            let (line, character) = offset_to_position(TEXT, offset);
            assert_eq!(
                position_to_offset(TEXT, line, character),
                offset,
                "{offset}"
            );
        }
    }

    #[test]
    fn counts_utf16_units() {
        let x = TEXT.find('x').unwrap();
        assert_eq!(offset_to_position(TEXT, x), (1, 6));
        assert_eq!(position_to_offset(TEXT, 1, 6), x);
        let eq = TEXT.find('=').unwrap();
        assert_eq!(offset_to_position(TEXT, eq), (0, 6));
    }

    #[test]
    fn clamps_out_of_range_positions() {
        let astral = TEXT.find('𝒳').unwrap();
        // Character 5 is the low surrogate of `𝒳`: snap to its start.
        assert_eq!(position_to_offset(TEXT, 1, 5), astral);
        assert_eq!(position_to_offset(TEXT, 0, 99), TEXT.find('\n').unwrap());
        assert_eq!(position_to_offset(TEXT, 9, 0), TEXT.len());
        assert_eq!(offset_to_position(TEXT, astral + 1), (1, 4));
        assert_eq!(offset_to_position(TEXT, 999), (2, 0));
    }
}
