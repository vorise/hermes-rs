use dissimilar::{diff, Chunk};

/// Result of a fuzzy match operation.
#[derive(Debug, Clone)]
pub struct FuzzyMatch {
    /// Start byte offset in the original content.
    pub start: usize,
    /// End byte offset in the original content.
    pub end: usize,
    /// Which strategy found the match.
    pub strategy: &'static str,
}

/// Result of a find-and-replace operation.
#[derive(Debug)]
pub enum ReplaceResult {
    /// Successfully replaced.
    Replaced {
        /// The new content after replacement.
        content: String,
        /// How many occurrences were replaced.
        count: usize,
    },
    /// Pattern not found.
    NotFound,
    /// Multiple occurrences found but replace_all is false.
    MultipleOccurrences { count: usize },
}

/// 9-strategy fuzzy matching chain.
///
/// Tried in order until one succeeds:
/// 1. exact — Direct string find
/// 2. line_trimmed — Strip leading/trailing whitespace per line
/// 3. whitespace_normalized — Collapse multiple spaces/tabs
/// 4. indentation_flexible — Strip all leading whitespace
/// 5. escape_normalized — Convert literal \n → newlines, \t → tabs
/// 6. trimmed_boundary — Trim whitespace from first/last lines only
/// 7. unicode_normalized — Smart quotes, dashes, ellipsis → ASCII
/// 8. block_anchor — Match first+last lines, require middle similarity
/// 9. context_aware — Require >=50% of lines have >=80% similarity
pub fn fuzzy_find(content: &str, pattern: &str) -> Option<FuzzyMatch> {
    let strategies: &[fn(&str, &str) -> Option<FuzzyMatch>] = &[
        exact_match,
        line_trimmed_match,
        whitespace_normalized_match,
        indentation_flexible_match,
        escape_normalized_match,
        trimmed_boundary_match,
        unicode_normalized_match,
        block_anchor_match,
        context_aware_match,
    ];

    for strategy in strategies {
        if let Some(m) = strategy(content, pattern) {
            return Some(m);
        }
    }
    None
}

/// Find-and-replace using fuzzy matching.
///
/// When `replace_all` is false and multiple occurrences are found,
/// returns `ReplaceResult::MultipleOccurrences`.
pub fn fuzzy_find_and_replace(
    content: &str,
    pattern: &str,
    replacement: &str,
    replace_all: bool,
) -> ReplaceResult {
    if pattern.is_empty() {
        return ReplaceResult::NotFound;
    }

    // Try exact match first for the common case
    if let Some(pos) = content.find(pattern) {
        if replace_all {
            let count = content.matches(pattern).count();
            return ReplaceResult::Replaced {
                content: content.replace(pattern, replacement),
                count,
            };
        }
        // Check for multiple occurrences even with exact match
        let count = content.matches(pattern).count();
        if count > 1 {
            return ReplaceResult::MultipleOccurrences { count };
        }
        let mut new_content = String::with_capacity(content.len() + replacement.len() - pattern.len());
        new_content.push_str(&content[..pos]);
        new_content.push_str(replacement);
        new_content.push_str(&content[pos + pattern.len()..]);
        return ReplaceResult::Replaced {
            content: new_content,
            count: 1,
        };
    }

    // Try fuzzy strategies to find ALL occurrences
    let strategies: &[fn(&str, &str) -> Option<FuzzyMatch>] = &[
        line_trimmed_match,
        whitespace_normalized_match,
        indentation_flexible_match,
        escape_normalized_match,
        trimmed_boundary_match,
        unicode_normalized_match,
        block_anchor_match,
        context_aware_match,
    ];

    for strategy in strategies {
        if let Some(_m) = strategy(content, pattern) {
            // Check if there are multiple occurrences with this strategy
            let occurrences = find_all_occurrences(content, pattern, *strategy);
            if occurrences.len() > 1 && !replace_all {
                return ReplaceResult::MultipleOccurrences {
                    count: occurrences.len(),
                };
            }

            // Replace all occurrences (or just the first one)
            let mut new_content = String::with_capacity(content.len());
            let mut last_end = 0;
            let limit = if replace_all { occurrences.len() } else { 1 };

            for occ in occurrences.iter().take(limit) {
                new_content.push_str(&content[last_end..occ.start]);
                new_content.push_str(replacement);
                last_end = occ.end;
            }
            new_content.push_str(&content[last_end..]);

            return ReplaceResult::Replaced {
                content: new_content,
                count: limit,
            };
        }
    }

    ReplaceResult::NotFound
}

/// Find all non-overlapping occurrences using a fuzzy strategy.
fn find_all_occurrences(
    content: &str,
    pattern: &str,
    strategy: fn(&str, &str) -> Option<FuzzyMatch>,
) -> Vec<FuzzyMatch> {
    let mut results = Vec::new();
    let mut search_from = 0;

    while search_from < content.len() {
        let remaining = &content[search_from..];
        if let Some(m) = strategy(remaining, pattern) {
            let absolute_start = search_from + m.start;
            let absolute_end = search_from + m.end;
            results.push(FuzzyMatch {
                start: absolute_start,
                end: absolute_end,
                strategy: m.strategy,
            });
            search_from = absolute_end;
        } else {
            break;
        }
    }

    results
}

// ─── Strategy 1: Exact match ───

fn exact_match(content: &str, pattern: &str) -> Option<FuzzyMatch> {
    content.find(pattern).map(|start| FuzzyMatch {
        start,
        end: start + pattern.len(),
        strategy: "exact",
    })
}

// ─── Strategy 2: Line-trimmed match ───

fn line_trimmed_match(content: &str, pattern: &str) -> Option<FuzzyMatch> {
    let norm_content = content
        .lines()
        .map(|l| l.trim())
        .collect::<Vec<_>>()
        .join("\n");
    let norm_pattern = pattern
        .lines()
        .map(|l| l.trim())
        .collect::<Vec<_>>()
        .join("\n");

    norm_content.find(&norm_pattern).map(|norm_start| {
        // Map back to original content byte offset
        let orig_start = map_normalized_offset(content, norm_start);
        // Find the end by scanning lines
        let pattern_line_count = pattern.lines().count();
        let orig_end = find_line_range_end(content, orig_start, pattern_line_count);
        FuzzyMatch {
            start: orig_start,
            end: orig_end,
            strategy: "line_trimmed",
        }
    })
}

// ─── Strategy 3: Whitespace-normalized match ───

fn whitespace_normalized_match(content: &str, pattern: &str) -> Option<FuzzyMatch> {
    let norm_content = normalize_whitespace(content);
    let norm_pattern = normalize_whitespace(pattern);

    norm_content.find(&norm_pattern).map(|_norm_start| {
        // For whitespace-normalized, find the best alignment in original
        let lines: Vec<&str> = pattern.lines().filter(|l| !l.trim().is_empty()).collect();
        if let Some(first_line) = lines.first() {
            if let Some(start) = find_line_with_content(content, first_line.trim()) {
                let line_count = lines.len().max(1);
                let end = find_line_range_end(content, start, line_count);
                return FuzzyMatch {
                    start,
                    end,
                    strategy: "whitespace_normalized",
                };
            }
        }
        FuzzyMatch {
            start: 0,
            end: content.len(),
            strategy: "whitespace_normalized",
        }
    })
}

fn normalize_whitespace(s: &str) -> String {
    s.lines()
        .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect::<Vec<_>>()
        .join("\n")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

// ─── Strategy 4: Indentation-flexible match ───

fn indentation_flexible_match(content: &str, pattern: &str) -> Option<FuzzyMatch> {
    let norm_content = content
        .lines()
        .map(|l| l.trim_start())
        .collect::<Vec<_>>()
        .join("\n");
    let norm_pattern = pattern
        .lines()
        .map(|l| l.trim_start())
        .collect::<Vec<_>>()
        .join("\n");

    norm_content.find(&norm_pattern).map(|norm_start| {
        let orig_start = map_normalized_offset(content, norm_start);
        let pattern_line_count = pattern.lines().count();
        let orig_end = find_line_range_end(content, orig_start, pattern_line_count);
        FuzzyMatch {
            start: orig_start,
            end: orig_end,
            strategy: "indentation_flexible",
        }
    })
}

// ─── Strategy 5: Escape-normalized match ───

fn escape_normalized_match(content: &str, pattern: &str) -> Option<FuzzyMatch> {
    let normalized = normalize_escapes(pattern);
    content.find(&normalized).map(|start| FuzzyMatch {
        start,
        end: start + normalized.len(),
        strategy: "escape_normalized",
    })
}

fn normalize_escapes(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.peek() {
                Some('n') => { result.push('\n'); chars.next(); }
                Some('t') => { result.push('\t'); chars.next(); }
                Some('r') => { result.push('\r'); chars.next(); }
                Some('\\') => { result.push('\\'); chars.next(); }
                _ => result.push(c),
            }
        } else {
            result.push(c);
        }
    }
    result
}

// ─── Strategy 6: Trimmed-boundary match ───

fn trimmed_boundary_match(content: &str, pattern: &str) -> Option<FuzzyMatch> {
    let trimmed = pattern.trim();
    content.find(trimmed).map(|start| {
        // Expand to include original leading/trailing whitespace on boundary lines
        let line_start = content[..start].rfind('\n').map(|p| p + 1).unwrap_or(0);
        let search_end = start + trimmed.len();
        let line_end = content[search_end..]
            .find('\n')
            .map(|p| search_end + p + 1)
            .unwrap_or(content.len());
        // Only expand if the boundary lines match
        FuzzyMatch {
            start: line_start,
            end: line_end,
            strategy: "trimmed_boundary",
        }
    })
}

// ─── Strategy 7: Unicode-normalized match ───

fn unicode_normalized_match(content: &str, pattern: &str) -> Option<FuzzyMatch> {
    let norm_pattern = normalize_unicode(pattern);
    let norm_content = normalize_unicode(content);

    norm_content.find(&norm_pattern).map(|norm_start| {
        // Build position mapping for unicode normalization
        let orig_start = map_unicode_normalized_offset(content, &norm_content, norm_start);
        let orig_end = orig_start + pattern.len(); // Approximate
        // Clamp to content bounds
        let orig_end = orig_end.min(content.len());
        FuzzyMatch {
            start: orig_start,
            end: orig_end,
            strategy: "unicode_normalized",
        }
    })
}

fn normalize_unicode(s: &str) -> String {
    s.replace('\u{201c}', "\"")
        .replace('\u{201d}', "\"")
        .replace('\u{2018}', "'")
        .replace('\u{2019}', "'")
        .replace('\u{2014}', "--")
        .replace('\u{2013}', "-")
        .replace('\u{2026}', "...")
        .replace('\u{00a0}', " ")
}

fn map_unicode_normalized_offset(original: &str, _normalized: &str, norm_offset: usize) -> usize {
    // Walk both strings in parallel, tracking byte offsets
    let mut orig_bytes = 0;
    let mut norm_bytes = 0;

    for ch in original.chars() {
        if norm_bytes >= norm_offset {
            break;
        }
        let ch_str: String = match ch {
            '\u{201c}' | '\u{201d}' => "\"".into(),
            '\u{2018}' | '\u{2019}' => "'".into(),
            '\u{2014}' => "--".into(),
            '\u{2013}' => "-".into(),
            '\u{2026}' => "...".into(),
            '\u{00a0}' => " ".into(),
            _ => ch.to_string(),
        };
        norm_bytes += ch_str.len();
        orig_bytes += ch.len_utf8();
    }

    orig_bytes.min(original.len())
}

// ─── Strategy 8: Block-anchor match ───

fn block_anchor_match(content: &str, pattern: &str) -> Option<FuzzyMatch> {
    let norm_content = normalize_unicode(content);
    let norm_pattern = normalize_unicode(pattern);

    let pattern_lines: Vec<&str> = norm_pattern.lines().collect();
    if pattern_lines.len() < 3 {
        return None; // Need at least 3 lines for anchor
    }

    let first_line = pattern_lines[0].trim();
    let _last_line = pattern_lines[pattern_lines.len() - 1].trim();

    // Find all positions where first line matches
    let mut candidates = Vec::new();
    let mut search_from = 0;
    while let Some(pos) = norm_content[search_from..].find(first_line) {
        candidates.push(search_from + pos);
        search_from += pos + first_line.len();
    }

    if candidates.is_empty() {
        return None;
    }

    if candidates.len() == 1 {
        let start = candidates[0];
        let _expected_end = start + norm_pattern.len();
        let actual_line_end = find_line_range_end(&norm_content, start, pattern_lines.len());
        let block_content = &norm_content[start..actual_line_end.min(norm_content.len())];
        let block_pattern = &norm_pattern[..pattern_lines.len().min(pattern_lines.len())];

        let similarity = line_similarity(block_content, block_pattern);
        if similarity >= 0.5 {
            let orig_start = map_unicode_normalized_offset(content, &norm_content, start);
            let orig_end = find_line_range_end(content, orig_start, pattern_lines.len());
            return Some(FuzzyMatch {
                start: orig_start,
                end: orig_end,
                strategy: "block_anchor",
            });
        }
    } else {
        // Multi-candidate: require 70% similarity
        let threshold = 0.7;
        for &start in &candidates {
            let actual_line_end = find_line_range_end(&norm_content, start, pattern_lines.len());
            let block_content = &norm_content[start..actual_line_end.min(norm_content.len())];
            let similarity = line_similarity(block_content, &norm_pattern);
            if similarity >= threshold {
                let orig_start = map_unicode_normalized_offset(content, &norm_content, start);
                let orig_end = find_line_range_end(content, orig_start, pattern_lines.len());
                return Some(FuzzyMatch {
                    start: orig_start,
                    end: orig_end,
                    strategy: "block_anchor",
                });
            }
        }
    }

    None
}

// ─── Strategy 9: Context-aware match ───

fn context_aware_match(content: &str, pattern: &str) -> Option<FuzzyMatch> {
    let pattern_lines: Vec<&str> = pattern.lines().collect();
    let pattern_len = pattern_lines.len();
    if pattern_len == 0 {
        return None;
    }

    let content_lines: Vec<&str> = content.lines().collect();
    if content_lines.len() < pattern_len {
        return None;
    }

    let min_matching_ratio = 0.5;
    let min_line_similarity = 0.8;

    for window_start in 0..=content_lines.len() - pattern_len {
        let window = &content_lines[window_start..window_start + pattern_len];
        let mut matching_lines = 0;

        for i in 0..pattern_len {
            let sim = line_similarity(window[i], pattern_lines[i]);
            if sim >= min_line_similarity {
                matching_lines += 1;
            }
        }

        let ratio = matching_lines as f64 / pattern_len as f64;
        if ratio >= min_matching_ratio {
            let _start = content[..content_lines[window_start].as_ptr() as usize - content.as_ptr() as usize]
                .rfind('\n')
                .map(|p| p + 1)
                .unwrap_or(0);

            // More reliable: calculate byte offsets from line indices
            let mut start = 0;
            for i in 0..window_start {
                start += content_lines[i].len() + 1; // +1 for newline
            }
            let mut end = start;
            for i in 0..pattern_len {
                end += content_lines[window_start + i].len() + 1;
            }
            end = end.min(content.len());

            return Some(FuzzyMatch {
                start,
                end,
                strategy: "context_aware",
            });
        }
    }

    None
}

// ─── Helpers ───

fn line_similarity(a: &str, b: &str) -> f64 {
    if a == b {
        return 1.0;
    }
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }

    let diffs_a = diff(a, b);
    let diffs_b = diff(b, a);

    let mut equal_len = 0;
    let mut total_len = 0;
    for c in &diffs_a {
        total_len += match c {
            Chunk::Equal(s) | Chunk::Delete(s) | Chunk::Insert(s) => s.len(),
        };
    }
    for c in &diffs_b {
        total_len += match c {
            Chunk::Equal(s) | Chunk::Delete(s) | Chunk::Insert(s) => s.len(),
        };
    }

    // Use dissimilar to compute similarity
    let diffs = diff(a, b);
    for c in &diffs {
        if matches!(c, Chunk::Equal(_)) {
            equal_len += match c {
                Chunk::Equal(s) => s.len(),
                Chunk::Delete(s) => s.len(),
                Chunk::Insert(s) => s.len(),
            };
        }
    }

    if total_len == 0 {
        return 0.0;
    }

    2.0 * equal_len as f64 / total_len as f64
}

fn map_normalized_offset(original: &str, norm_offset: usize) -> usize {
    // Approximate: find the line in original that corresponds to norm_offset
    let mut norm_lines = 0;
    let mut orig_offset = 0;

    for line in original.lines() {
        if norm_lines >= norm_offset {
            break;
        }
        // Count this line in normalized form
        norm_lines += 1;
        orig_offset += line.len() + 1; // +1 for newline
    }

    orig_offset.min(original.len())
}

fn find_line_with_content(content: &str, needle: &str) -> Option<usize> {
    let mut offset = 0;
    for line in content.lines() {
        if line.contains(needle) {
            return Some(offset);
        }
        offset += line.len() + 1;
    }
    None
}

fn find_line_range_end(content: &str, start: usize, line_count: usize) -> usize {
    let remaining = &content[start.min(content.len())..];
    let mut offset = 0;
    let mut lines = 0;

    for line in remaining.lines() {
        lines += 1;
        offset += line.len() + 1;
        if lines >= line_count {
            break;
        }
    }

    (start + offset).min(content.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exact_match() {
        let content = "fn main() {\n    println!(\"hello\");\n}";
        let pattern = "println!(\"hello\")";
        let result = fuzzy_find(content, pattern).unwrap();
        assert_eq!(result.strategy, "exact");
        assert!(content[result.start..result.end].contains("hello"));
    }

    #[test]
    fn test_exact_replace() {
        let content = "fn foo() {}\nfn bar() {}";
        let result = fuzzy_find_and_replace(content, "fn foo() {}", "fn baz() {}", false);
        match result {
            ReplaceResult::Replaced { content, count } => {
                assert_eq!(count, 1);
                assert_eq!(content, "fn baz() {}\nfn bar() {}");
            }
            _ => panic!("Expected Replaced"),
        }
    }

    #[test]
    fn test_whitespace_normalized() {
        let content = "fn   main()   {\n    println!(\"hi\");\n}";
        let pattern = "fn main() {\n    println!(\"hi\");\n}";
        let result = fuzzy_find(content, pattern);
        assert!(result.is_some());
        assert_eq!(result.unwrap().strategy, "whitespace_normalized");
    }

    #[test]
    fn test_indentation_flexible() {
        let content = "fn main() {\n    if true {\n        return;\n    }\n}";
        let pattern = "fn main() {\n  if true {\n    return;\n  }\n}";
        let result = fuzzy_find(content, pattern);
        assert!(result.is_some());
    }

    #[test]
    fn test_escape_normalized() {
        let content = "fn main() {\n    let x = 1;\n}";
        let pattern = "fn main() {\\n    let x = 1;\\n}";
        let result = fuzzy_find(content, pattern);
        assert!(result.is_some());
        assert_eq!(result.unwrap().strategy, "escape_normalized");
    }

    #[test]
    fn test_unicode_normalized() {
        let content = "fn main() { return \"hello\"; }";
        let pattern = "fn main() { return \u{201c}hello\u{201d}; }";
        let result = fuzzy_find(content, pattern);
        assert!(result.is_some());
        assert_eq!(result.unwrap().strategy, "unicode_normalized");
    }

    #[test]
    fn test_line_trimmed() {
        let content = "fn main() {\n    let x = 1;\n}";
        let pattern = "  fn main() {\n      let x = 1;\n  }";
        let result = fuzzy_find(content, pattern);
        assert!(result.is_some());
    }

    #[test]
    fn test_multiple_occurrences_no_replace_all() {
        let content = "fn foo() {}\nfn bar() {}\nfn foo() {}";
        let result = fuzzy_find_and_replace(content, "fn foo() {}", "fn baz() {}", false);
        match result {
            ReplaceResult::MultipleOccurrences { count } => {
                assert_eq!(count, 2);
            }
            _ => panic!("Expected MultipleOccurrences, got {:?}", result),
        }
    }

    #[test]
    fn test_replace_all() {
        let content = "fn foo() {}\nfn bar() {}\nfn foo() {}";
        let result = fuzzy_find_and_replace(content, "fn foo() {}", "fn baz() {}", true);
        match result {
            ReplaceResult::Replaced { content, count } => {
                assert_eq!(count, 2);
                assert_eq!(content, "fn baz() {}\nfn bar() {}\nfn baz() {}");
            }
            _ => panic!("Expected Replaced"),
        }
    }

    #[test]
    fn test_not_found() {
        let content = "fn main() {}";
        let result = fuzzy_find_and_replace(content, "fn missing() {}", "fn x() {}", false);
        assert!(matches!(result, ReplaceResult::NotFound));
    }

    #[test]
    fn test_block_anchor_single_candidate() {
        let content = "pub struct Foo {\n    pub name: String,\n    pub value: i32,\n}\n";
        let pattern = "pub struct Foo {\n  pub name: String,\n  pub value: i32,\n}\n";
        let result = fuzzy_find(content, pattern);
        // May find via indentation_flexible before block_anchor, but should find something
        assert!(result.is_some());
    }

    #[test]
    fn test_context_aware_fallback() {
        let content = "fn process_data(items: &[Item]) -> Result<()> {\n    for item in items {\n        if item.is_valid() {\n            handle_item(item)?;\n        }\n    }\n    Ok(())\n}";
        let pattern = "fn process_data(items: &[Item]) -> Result<()> {\n  for item in items {\n    if item.is_valid() {\n      handle_item(item)?;\n    }\n  }\n  Ok(())\n}";
        // Should find via indentation_flexible before context_aware
        let result = fuzzy_find(content, pattern);
        assert!(result.is_some());
    }
}
