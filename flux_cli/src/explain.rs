//! `flux explain E0xx`: the long explanation of a diagnostic code, taken
//! from the book's error-code chapter (`docs/flux/src/error-codes.md`).

const CHAPTER: &str = include_str!("../../docs/flux/src/error-codes.md");

/// The section for `code` (`E002`, `e2`, or `2`), without its heading, or
/// `None` for a code the chapter does not have.
pub fn explain(code: &str) -> Option<String> {
    let digits = code.trim().trim_start_matches(['E', 'e']);
    let number: u32 = digits.parse().ok()?;
    let heading = format!("## E{number:03}");
    let start = CHAPTER.find(&format!("{heading}\n"))?;
    let body = &CHAPTER[start + heading.len()..];
    let end = body.find("\n## ").unwrap_or(body.len());
    let text = body[..end].trim();
    // Show code blocks as plain indented text; their fences are for the book.
    let mut out = format!("E{number:03}\n\n");
    let mut in_code = false;
    for line in text.lines() {
        if line.starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if in_code {
            out.push_str("    ");
        }
        out.push_str(line);
        out.push('\n');
    }
    Some(out)
}

/// The highest code the chapter explains.
pub fn last_code() -> String {
    CHAPTER
        .lines()
        .filter_map(|l| l.strip_prefix("## E"))
        .filter_map(|n| n.trim().parse::<u32>().ok())
        .max()
        .map_or("E000".into(), |n| format!("E{n:03}"))
}

#[cfg(test)]
mod tests {
    use super::explain;

    #[test]
    fn finds_a_code_in_any_spelling() {
        let text = explain("E001").expect("E001");
        assert!(text.starts_with("E001\n"), "{text}");
        assert!(text.contains("undefined name"), "{text}");
        assert!(text.contains("    let doubled = totl * 2"), "{text}");
        assert_eq!(explain("e1"), explain("1"));
        assert!(explain("E999").is_none());
        assert!(explain("nonsense").is_none());
    }
}
