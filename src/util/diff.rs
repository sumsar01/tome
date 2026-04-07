/// Number of unchanged context lines to show around each changed hunk.
pub const DIFF_CONTEXT_LINES: usize = 3;

/// Compute a unified diff between two strings, returning the result as a `String`.
///
/// Lines unique to `a` are prefixed with `-`, lines new in `b` with `+`, and
/// unchanged context lines with ` `.  Hunks are separated by `@@ ... @@`.
/// Uses an O(n×m) LCS DP — suitable for documents up to ~10 k lines.
pub fn unified_diff(a: &str, b: &str) -> String {
    unified_diff_ctx(a, b, DIFF_CONTEXT_LINES)
}

/// Like [`unified_diff`] but with a configurable context window.
pub fn unified_diff_ctx(a: &str, b: &str, ctx: usize) -> String {
    let a_lines: Vec<&str> = a.lines().collect();
    let b_lines: Vec<&str> = b.lines().collect();
    let m = a_lines.len();
    let n = b_lines.len();

    // dp[i][j] = length of LCS of a[..i] and b[..j]
    let mut dp = vec![vec![0usize; n + 1]; m + 1];
    for i in 1..=m {
        for j in 1..=n {
            if a_lines[i - 1] == b_lines[j - 1] {
                dp[i][j] = dp[i - 1][j - 1] + 1;
            } else {
                dp[i][j] = dp[i - 1][j].max(dp[i][j - 1]);
            }
        }
    }

    // Backtrack to produce edit operations: ('=', line), ('-', line), ('+', line)
    let mut ops: Vec<(char, &str)> = Vec::new();
    let (mut i, mut j) = (m, n);
    while i > 0 || j > 0 {
        if i > 0 && j > 0 && a_lines[i - 1] == b_lines[j - 1] {
            ops.push(('=', a_lines[i - 1]));
            i -= 1;
            j -= 1;
        } else if j > 0 && (i == 0 || dp[i][j - 1] >= dp[i - 1][j]) {
            ops.push(('+', b_lines[j - 1]));
            j -= 1;
        } else {
            ops.push(('-', a_lines[i - 1]));
            i -= 1;
        }
    }
    ops.reverse();

    // Mark lines that should be printed (changed lines + ctx lines around them)
    let changed: Vec<bool> = ops.iter().map(|(op, _)| *op != '=').collect();
    let mut printed = vec![false; ops.len()];
    for (k, &is_changed) in changed.iter().enumerate() {
        if is_changed {
            let start = k.saturating_sub(ctx);
            let end = (k + ctx + 1).min(ops.len());
            for p in printed.iter_mut().take(end).skip(start) {
                *p = true;
            }
        }
    }

    let mut out = String::new();
    let mut last: Option<usize> = None;
    for (k, (op, line)) in ops.iter().enumerate() {
        if !printed[k] {
            continue;
        }
        if let Some(l) = last {
            if k > l + 1 {
                out.push_str("@@ ... @@\n");
            }
        }
        let prefix = match op {
            '-' => "-",
            '+' => "+",
            _ => " ",
        };
        out.push_str(prefix);
        out.push_str(line);
        out.push('\n');
        last = Some(k);
    }

    if last.is_none() {
        out.push_str("(no differences)\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_texts_produce_no_differences() {
        let result = unified_diff("hello\nworld\n", "hello\nworld\n");
        assert_eq!(result, "(no differences)\n");
    }

    #[test]
    fn single_line_change() {
        let result = unified_diff("hello\nworld\n", "hello\nearth\n");
        assert!(result.contains("-world"));
        assert!(result.contains("+earth"));
    }

    #[test]
    fn addition_only() {
        let result = unified_diff("a\nb\n", "a\nb\nc\n");
        assert!(result.contains("+c"));
        assert!(!result.contains("-"));
    }
}
