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
    let end = if end > start {
        end
    } else {
        text[start.min(text.len())..]
            .chars()
            .next()
            .map_or(start, |c| start + c.len_utf8())
    };
    let (sl, sc) = offset_to_position(text, start);
    let (el, ec) = offset_to_position(text, end);
    (sl, sc, el, ec)
}

#[cfg(test)]
mod tests {
    use super::*;

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
