//! Line diffs between two texts (page revisions), grouped into hunks with context.

use serde::Serialize;

/// Past this many cells in the table of common lines, the changed middle is shown as all
/// removed and then all added. Pages are at most 64 KiB, so this rarely matters.
const MAX_CELLS: usize = 4_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Change {
    Same,
    Removed,
    Added,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DiffLine {
    pub change: Change,
    pub text: String,
}

/// A run of changed lines with up to `context` unchanged lines around it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Hunk {
    /// Where the hunk starts in the old and new text, counting lines from 1.
    pub old_line: usize,
    pub new_line: usize,
    pub lines: Vec<DiffLine>,
}

/// Every line of `old` and `new`, marked same, removed or added (a longest common
/// subsequence; removals before additions where both happen).
pub fn diff_lines(old: &str, new: &str) -> Vec<DiffLine> {
    let a: Vec<&str> = old.lines().collect();
    let b: Vec<&str> = new.lines().collect();
    let prefix = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let suffix = a[prefix..]
        .iter()
        .rev()
        .zip(b[prefix..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let (mid_a, mid_b) = (&a[prefix..a.len() - suffix], &b[prefix..b.len() - suffix]);
    let line = |change, text: &str| DiffLine {
        change,
        text: text.to_owned(),
    };
    let mut out: Vec<DiffLine> = a[..prefix].iter().map(|t| line(Change::Same, t)).collect();
    let (n, m) = (mid_a.len(), mid_b.len());
    if (n + 1).saturating_mul(m + 1) > MAX_CELLS {
        out.extend(mid_a.iter().map(|t| line(Change::Removed, t)));
        out.extend(mid_b.iter().map(|t| line(Change::Added, t)));
    } else {
        // common[i][j]: the longest common subsequence of mid_a[i..] and mid_b[j..].
        let width = m + 1;
        let mut common = vec![0u32; (n + 1) * width];
        for i in (0..n).rev() {
            for j in (0..m).rev() {
                common[i * width + j] = if mid_a[i] == mid_b[j] {
                    common[(i + 1) * width + j + 1] + 1
                } else {
                    common[(i + 1) * width + j].max(common[i * width + j + 1])
                };
            }
        }
        let (mut i, mut j) = (0, 0);
        while i < n || j < m {
            if i < n && j < m && mid_a[i] == mid_b[j] {
                out.push(line(Change::Same, mid_a[i]));
                i += 1;
                j += 1;
            } else if j == m || (i < n && common[(i + 1) * width + j] >= common[i * width + j + 1])
            {
                out.push(line(Change::Removed, mid_a[i]));
                i += 1;
            } else {
                out.push(line(Change::Added, mid_b[j]));
                j += 1;
            }
        }
    }
    out.extend(a[a.len() - suffix..].iter().map(|t| line(Change::Same, t)));
    out
}

/// The changed parts of `lines`, each with up to `context` unchanged lines on either side;
/// hunks that would touch are merged. No changes, no hunks.
pub fn hunks(lines: &[DiffLine], context: usize) -> Vec<Hunk> {
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        if line.change == Change::Same {
            continue;
        }
        let (start, end) = (index.saturating_sub(context), (index + context + 1).min(lines.len()));
        match ranges.last_mut() {
            Some(last) if start <= last.1 => last.1 = end,
            _ => ranges.push((start, end)),
        }
    }
    // Line numbers at each index: count what came before in each text.
    let (mut old_line, mut new_line, mut at) = (1, 1, 0);
    let mut out = Vec::new();
    for (start, end) in ranges {
        for line in &lines[at..start] {
            old_line += usize::from(line.change != Change::Added);
            new_line += usize::from(line.change != Change::Removed);
        }
        out.push(Hunk {
            old_line,
            new_line,
            lines: lines[start..end].to_vec(),
        });
        at = start;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marks(lines: &[DiffLine]) -> String {
        lines
            .iter()
            .map(|line| {
                let mark = match line.change {
                    Change::Same => ' ',
                    Change::Removed => '-',
                    Change::Added => '+',
                };
                format!("{mark}{}", line.text)
            })
            .collect::<Vec<_>>()
            .join("|")
    }

    #[test]
    fn marks_removed_and_added_lines() {
        assert_eq!(marks(&diff_lines("a\nb\nc\n", "a\nb\nc\n")), " a| b| c");
        assert_eq!(marks(&diff_lines("a\nb\nc", "a\nx\nc\nd")), " a|-b|+x| c|+d");
        assert_eq!(marks(&diff_lines("", "new")), "+new");
        assert_eq!(marks(&diff_lines("a\nb\na\nb", "b\na\nb\na")), "-a| b| a| b|+a");
    }

    #[test]
    fn hunks_keep_context_and_line_numbers() {
        let old: String = (1..=20).map(|n| format!("{n}\n")).collect();
        let new: String = (1..=20)
            .map(|n| match n {
                3 => "three\n".to_owned(),
                15 => String::new(),
                16 => "16\n16b\n".to_owned(),
                _ => format!("{n}\n"),
            })
            .collect();
        let lines = diff_lines(&old, &new);
        let found = hunks(&lines, 2);
        assert_eq!(found.len(), 2);
        assert_eq!((found[0].old_line, found[0].new_line), (1, 1));
        assert_eq!(marks(&found[0].lines), " 1| 2|-3|+three| 4| 5");
        assert_eq!((found[1].old_line, found[1].new_line), (13, 13));
        assert_eq!(marks(&found[1].lines), " 13| 14|-15| 16|+16b| 17| 18");
        // Close changes merge into one hunk.
        assert_eq!(hunks(&lines, 6).len(), 1);
        assert!(hunks(&diff_lines("same", "same"), 3).is_empty());
    }
}
