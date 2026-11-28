//! "Did you mean": the closest known name to a misspelled one.

/// The candidate nearest to `name` by edit distance (insert, delete,
/// replace, swap two neighbours), if one is close enough to be a typo: at
/// most one edit for names up to four characters, a third of the length
/// beyond. Case differences count as edits but are preferred on a tie. A
/// candidate of three or more characters that starts `name` (`len` for
/// `lenght`) also counts, as the farthest choice.
pub fn did_you_mean<'a>(
    name: &str,
    candidates: impl IntoIterator<Item = &'a str>,
) -> Option<String> {
    let limit = (name.chars().count() / 3).max(1);
    // (distance, not the same up to case, not missing a letter, name)
    let mut best: Option<(usize, bool, bool, &str)> = None;
    for candidate in candidates {
        if candidate == name || candidate.is_empty() {
            continue;
        }
        let mut distance = edit_distance(name, candidate);
        if distance > limit {
            if candidate.chars().count() >= 3 && name.starts_with(candidate) {
                distance = limit + 1;
            } else {
                continue;
            }
        }
        let key = (
            distance,
            !candidate.eq_ignore_ascii_case(name),
            // `printn` for `println`: a letter left out is the likelier slip.
            !is_subsequence(name, candidate),
            candidate,
        );
        if best.is_none_or(|b| key < b) {
            best = Some(key);
        }
    }
    best.map(|(_, _, _, c)| c.to_string())
}

/// Whether `short`'s characters appear in `long` in order.
fn is_subsequence(short: &str, long: &str) -> bool {
    let mut rest = long.chars();
    short.chars().all(|c| rest.any(|l| l == c))
}

/// E014's help: the module's export spelled like `name`, or its exports.
pub fn export_help<'a>(
    module: &str,
    name: &str,
    exports: impl IntoIterator<Item = &'a str>,
) -> String {
    let mut exports: Vec<&str> = exports.into_iter().collect();
    if let Some(close) = did_you_mean(name, exports.iter().copied()) {
        return format!("`{module}` has a name with a similar spelling: `{close}`");
    }
    exports.sort_unstable();
    exports.dedup();
    match exports.len() {
        0 => format!("`{module}` exports nothing; mark what other files may use `pub`"),
        n if n <= 24 => format!("`{module}` exports: {}", exports.join(", ")),
        n => format!(
            "`{module}` exports {n} names, such as {}",
            exports[..12].join(", ")
        ),
    }
}

/// Optimal string alignment distance over characters.
fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let (n, m) = (a.len(), b.len());
    if n.abs_diff(m) > 3 {
        return n.abs_diff(m);
    }
    let mut d = vec![vec![0usize; m + 1]; n + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, cell) in d[0].iter_mut().enumerate() {
        *cell = j;
    }
    for i in 1..=n {
        for j in 1..=m {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            d[i][j] = (d[i - 1][j] + 1)
                .min(d[i][j - 1] + 1)
                .min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                d[i][j] = d[i][j].min(d[i - 2][j - 2] + 1);
            }
        }
    }
    d[n][m]
}

#[cfg(test)]
mod tests {
    use super::did_you_mean;

    #[test]
    fn finds_typos() {
        assert_eq!(did_you_mean("totl", ["total", "x"]), Some("total".into()));
        assert_eq!(
            did_you_mean("lenght", ["len", "length"]),
            Some("length".into())
        );
        assert_eq!(
            did_you_mean("pritnln", ["println", "print"]),
            Some("println".into())
        );
        assert_eq!(
            did_you_mean("Count", ["count", "mount"]),
            Some("count".into())
        );
        assert_eq!(
            did_you_mean("printn", ["print", "println"]),
            Some("println".into())
        );
    }

    #[test]
    fn ignores_far_names() {
        assert_eq!(did_you_mean("x", ["y"]), Some("y".into()));
        assert_eq!(did_you_mean("value", ["total", "items"]), None);
        assert_eq!(did_you_mean("abc", []), None);
    }
}
