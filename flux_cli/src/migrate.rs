//! v1 → v2 syntax migration (top-level bindings and `set` statements).

use std::fs;
use std::path::{Path, PathBuf};

/// Migrate Flux source from v1 binding syntax to v2.
pub fn migrate_source(input: &str) -> String {
    let reassigned = collect_reassigned_names(input);
    let lines: Vec<&str> = input.lines().collect();
    let mut out = String::new();

    for (idx, line) in lines.iter().enumerate() {
        if let Some(migrated) = migrate_line(line, &reassigned, &lines, idx) {
            out.push_str(&migrated);
        } else {
            out.push_str(line);
        }
        if idx + 1 < lines.len() {
            out.push('\n');
        }
    }
    if input.ends_with('\n') && !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// Migrate a single file; returns `(output, changed)`.
pub fn migrate_file(path: &Path, write: bool) -> miette::Result<(String, bool)> {
    let input = fs::read_to_string(path)
        .map_err(|e| miette::miette!("failed to read {}: {e}", path.display()))?;
    let output = migrate_source(&input);
    let changed = output != input;
    if write && changed {
        fs::write(path, &output)
            .map_err(|e| miette::miette!("failed to write {}: {e}", path.display()))?;
    }
    Ok((output, changed))
}

/// Migrate all `.fl` source files under `root` recursively.
pub fn migrate_tree(root: &Path, write: bool) -> miette::Result<Vec<(PathBuf, bool)>> {
    let mut results = Vec::new();
    walk_module_files(root, &mut |path| {
        let (_, changed) = migrate_file(path, write)?;
        results.push((path.to_path_buf(), changed));
        Ok(())
    })?;
    Ok(results)
}

fn walk_module_files(
    root: &Path,
    f: &mut dyn FnMut(&Path) -> miette::Result<()>,
) -> miette::Result<()> {
    if root.is_file() {
        if is_module_file(root) {
            f(root)?;
        }
        return Ok(());
    }
    for entry in fs::read_dir(root).map_err(|e| miette::miette!("read {}: {e}", root.display()))? {
        let entry = entry.map_err(|e| miette::miette!("{e}"))?;
        let path = entry.path();
        if path.is_dir() {
            walk_module_files(&path, f)?;
        } else if is_module_file(&path) {
            f(&path)?;
        }
    }
    Ok(())
}

fn is_module_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| ext == flux_syntax::MODULE_EXTENSION)
}

fn collect_reassigned_names(source: &str) -> std::collections::HashSet<String> {
    let mut names = std::collections::HashSet::new();
    let mut in_on_or_watch = 0u32;
    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("on ") || trimmed == "on" {
            if trimmed.contains('{') {
                for part in extract_inline_assignments(trimmed) {
                    names.insert(part);
                }
            }
            let opens = trimmed.matches('{').count() as u32;
            let closes = trimmed.matches('}').count() as u32;
            if opens > closes {
                in_on_or_watch += opens - closes;
            }
            continue;
        }
        if trimmed.starts_with("watch") {
            if trimmed.contains('{') {
                for part in extract_inline_assignments(trimmed) {
                    names.insert(part);
                }
            }
            let opens = trimmed.matches('{').count() as u32;
            let closes = trimmed.matches('}').count() as u32;
            if opens > closes {
                in_on_or_watch += opens - closes;
            }
            continue;
        }
        if trimmed == "}" {
            in_on_or_watch = in_on_or_watch.saturating_sub(1);
            continue;
        }
        if in_on_or_watch > 0 {
            if let Some(name) = parse_assign_name(trimmed) {
                names.insert(name);
            }
            for part in extract_inline_assignments(trimmed) {
                names.insert(part);
            }
            continue;
        }
    }
    names
}

fn parse_assign_name(line: &str) -> Option<String> {
    if line.starts_with("let ") || line.starts_with("set ") {
        return None;
    }
    let eq = line.find('=')?;
    let left = line[..eq].trim();
    if left.is_empty() || left.contains(' ') {
        return None;
    }
    if left.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        Some(left.to_string())
    } else {
        None
    }
}

fn extract_inline_assignments(line: &str) -> Vec<String> {
    let mut names = Vec::new();
    let open = line.find('{');
    let close = line.rfind('}');
    let body = match (open, close) {
        (Some(o), Some(c)) if c > o => &line[o + 1..c],
        _ => return names,
    };
    for segment in body.split(';') {
        if let Some(name) = parse_assign_name(segment.trim()) {
            names.push(name);
        }
    }
    names
}

fn migrate_line(
    line: &str,
    reassigned: &std::collections::HashSet<String>,
    all_lines: &[&str],
    line_idx: usize,
) -> Option<String> {
    if is_inside_string_or_comment(line) {
        return None;
    }
    let trimmed = line.trim();
    let indent = line.strip_suffix(trimmed).unwrap_or("");

    if let Some(migrated) = migrate_for_int_literal(trimmed) {
        return Some(format!("{indent}{migrated}"));
    }

    if let Some(rest) = trimmed.strip_prefix("set ") {
        if let Some((name, rhs)) = split_binding(rest) {
            return Some(format!("{indent}let mut {name} = {rhs}"));
        }
    }

    if is_top_level_context(all_lines, line_idx) {
        if let Some((name, rhs)) = split_top_level_binding(trimmed) {
            if reassigned.contains(&name) {
                return Some(format!("{indent}let mut {name} = {rhs}"));
            }
            return Some(format!("{indent}let {name} = {rhs}"));
        }
    }

    None
}

/// `for i in 10` → `for i in range(10)` when the iterator is an integer literal.
fn migrate_for_int_literal(line: &str) -> Option<String> {
    let rest = line.strip_prefix("for ")?;
    let in_pos = rest.find(" in ")?;
    let name = rest[..in_pos].trim();
    if name.is_empty() || name.contains(' ') {
        return None;
    }
    let after_in = rest[in_pos + 4..].trim();
    let iter = after_in
        .split(|c: char| c.is_whitespace() || c == '{')
        .next()?
        .trim();
    if iter.is_empty() || !iter.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let suffix = after_in.strip_prefix(iter)?.trim_start();
    Some(format!("for {name} in range({iter}){suffix}"))
}

fn is_top_level_context(lines: &[&str], idx: usize) -> bool {
    let mut depth = 0i32;
    for (i, line) in lines.iter().enumerate() {
        if i >= idx {
            break;
        }
        let t = line.trim();
        depth += t.matches('{').count() as i32;
        depth -= t.matches('}').count() as i32;
    }
    depth == 0
}

fn split_top_level_binding(line: &str) -> Option<(String, String)> {
    if line.starts_with("let ")
        || line.starts_with("fn ")
        || line.starts_with("rfn ")
        || line.starts_with("struct ")
        || line.starts_with("enum ")
        || line.starts_with("trait ")
        || line.starts_with("impl ")
        || line.starts_with("import ")
        || line.starts_with("event ")
        || line.starts_with("derived ")
        || line.starts_with("const ")
        || line.starts_with("type ")
        || line.starts_with("watch")
        || line.starts_with("on ")
        || line.starts_with("//")
    {
        return None;
    }
    split_binding(line)
}

fn split_binding(line: &str) -> Option<(String, String)> {
    let eq = line.find('=')?;
    let name = line[..eq].trim();
    let rhs = line[eq + 1..].trim();
    if name.is_empty() || name.contains(' ') {
        return None;
    }
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return None;
    }
    Some((name.to_string(), rhs.to_string()))
}

fn is_inside_string_or_comment(line: &str) -> bool {
    line.trim_start().starts_with("//")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_top_level_source_to_let_mut_when_reassigned() {
        let input = "counter = 0\non Click { counter = counter + 1 }\n";
        let out = migrate_source(input);
        assert!(out.contains("let mut counter = 0"));
        assert!(out.contains("counter = counter + 1"));
    }

    #[test]
    fn migrates_derived_binding_to_let() {
        let input = "counter = 0\ndoubled = counter * 2\n";
        let out = migrate_source(input);
        assert!(out.contains("let counter = 0"));
        assert!(out.contains("let doubled = counter * 2"));
    }

    #[test]
    fn migrates_set_to_let_mut() {
        let input = "fn f() {\n  set n = 0\n}\n";
        let out = migrate_source(input);
        assert!(out.contains("let mut n = 0"));
    }

    #[test]
    fn preserves_comments_and_strings() {
        let input = "// counter = old\nx = 1\n";
        let out = migrate_source(input);
        assert!(out.contains("// counter = old"));
        assert!(out.contains("let x = 1"));
    }

    #[test]
    fn migrates_for_int_count_to_range() {
        let input = "for i in 10 { print(i) }\n";
        let out = migrate_source(input);
        assert!(out.contains("for i in range(10)"));
    }

    #[test]
    fn does_not_merge_lines() {
        let input = "a = 1\nb = 2\n";
        let out = migrate_source(input);
        assert_eq!(out.lines().count(), 2);
    }
}
