use std::fmt;

/// Operation types in a V4A patch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationType {
    Add,
    Update,
    Delete,
    Move,
}

impl fmt::Display for OperationType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OperationType::Add => write!(f, "add"),
            OperationType::Update => write!(f, "update"),
            OperationType::Delete => write!(f, "delete"),
            OperationType::Move => write!(f, "move"),
        }
    }
}

/// A single line within a hunk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HunkLine {
    pub prefix: char, // ' ', '-', or '+'
    pub content: String,
}

/// A hunk of changes within a file operation.
#[derive(Debug, Clone, Default)]
pub struct Hunk {
    pub context_hint: Option<String>,
    pub lines: Vec<HunkLine>,
}

/// A single patch operation targeting one file.
#[derive(Debug, Clone)]
pub struct PatchOperation {
    pub operation: OperationType,
    pub file_path: String,
    pub new_path: Option<String>, // For MOVE operations
    pub hunks: Vec<Hunk>,
}

/// Error during patch parsing.
#[derive(Debug, thiserror::Error)]
pub enum PatchError {
    #[error("empty file path in {operation} operation")]
    EmptyFilePath { operation: String },
    #[error("UPDATE operation at line {line} has no hunks")]
    UpdateNoHunks { line: usize },
    #[error("MOVE operation at line {line} has no destination path")]
    MoveNoDestination { line: usize },
    #[error("parse error at line {line}: {message}")]
    ParseError { line: usize, message: String },
}

/// Parse a V4A patch string into a list of operations.
///
/// V4A format:
/// ```text
/// *** Begin Patch
/// *** Update File: path/to/file.py
/// @@ optional context hint @@
///  context line
/// -removed line
/// +added line
/// *** Add File: path/to/new.py
/// +new file content
/// *** Delete File: path/to/old.py
/// *** Move File: old/path.py -> new/path.py
/// *** End Patch
/// ```
pub fn parse_patch(input: &str) -> Result<Vec<PatchOperation>, PatchError> {
    let lines: Vec<&str> = input.lines().collect();
    let mut ops = Vec::new();
    let mut i = 0;

    // Skip to "*** Begin Patch" (accepts "***Begin Patch" without space)
    while i < lines.len() {
        let trimmed = lines[i].trim();
        if trimmed == "*** Begin Patch" || trimmed == "***Begin Patch" {
            i += 1;
            break;
        }
        i += 1;
    }

    let mut current_op: Option<PatchOperation> = None;
    let mut current_hunk: Option<Hunk> = None;

    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();

        // End of patch
        if trimmed == "*** End Patch" || trimmed == "***End Patch" {
            // Flush current hunk and operation
            if let Some(hunk) = current_hunk.take() {
                if let Some(op) = current_op.as_mut() {
                    op.hunks.push(hunk);
                }
            }
            if let Some(op) = current_op.take() {
                ops.push(op);
            }
            break;
        }

        // File operation markers
        if let Some(op_type) = parse_operation_marker(trimmed) {
            // Flush previous operation
            if let Some(hunk) = current_hunk.take() {
                if let Some(op) = current_op.as_mut() {
                    op.hunks.push(hunk);
                }
            }
            if let Some(op) = current_op.take() {
                ops.push(op);
            }

            match op_type {
                OpMarker::Add(file_path) => {
                    validate_file_path(&file_path, i, OperationType::Add)?;
                    current_op = Some(PatchOperation {
                        operation: OperationType::Add,
                        file_path,
                        new_path: None,
                        hunks: Vec::new(),
                    });
                    // ADD creates a default hunk to collect content
                    current_hunk = Some(Hunk::default());
                }
                OpMarker::Update(file_path) => {
                    validate_file_path(&file_path, i, OperationType::Update)?;
                    current_op = Some(PatchOperation {
                        operation: OperationType::Update,
                        file_path,
                        new_path: None,
                        hunks: Vec::new(),
                    });
                    // No initial hunk — hunks start with @@ or hunk content
                    current_hunk = None;
                }
                OpMarker::Delete(file_path) => {
                    validate_file_path(&file_path, i, OperationType::Delete)?;
                    current_op = Some(PatchOperation {
                        operation: OperationType::Delete,
                        file_path,
                        new_path: None,
                        hunks: Vec::new(),
                    });
                    // DELETE has no hunks
                    current_hunk = None;
                }
                OpMarker::Move(src, dst) => {
                    validate_file_path(&src, i, OperationType::Move)?;
                    if dst.trim().is_empty() {
                        return Err(PatchError::MoveNoDestination { line: i + 1 });
                    }
                    current_op = Some(PatchOperation {
                        operation: OperationType::Move,
                        file_path: src,
                        new_path: Some(dst.trim().to_string()),
                        hunks: Vec::new(),
                    });
                    current_hunk = None;
                }
            }
            i += 1;
            continue;
        }

        // Context hint: @@ hint @@
        if let Some(hint) = parse_context_hint(trimmed) {
            // Start a new hunk with this context hint
            if let Some(hunk) = current_hunk.take() {
                if let Some(op) = current_op.as_mut() {
                    op.hunks.push(hunk);
                }
            }
            current_hunk = Some(Hunk {
                context_hint: Some(hint),
                lines: Vec::new(),
            });
            i += 1;
            continue;
        }

        // Hunk lines: +, -, \, space prefix, or implicit context
        if let Some(op) = &current_op {
            if matches!(op.operation, OperationType::Update | OperationType::Add) {
                if let Some(hunk_line) = parse_hunk_line(line) {
                    let hunk = current_hunk.get_or_insert_with(Hunk::default);
                    hunk.lines.push(hunk_line);
                }
            }
        }

        i += 1;
    }

    // Validate: UPDATE must have hunks
    for (idx, op) in ops.iter().enumerate() {
        if op.operation == OperationType::Update && op.hunks.is_empty() {
            return Err(PatchError::UpdateNoHunks { line: idx + 1 });
        }
    }

    Ok(ops)
}

enum OpMarker {
    Add(String),
    Update(String),
    Delete(String),
    Move(String, String),
}

fn parse_operation_marker(line: &str) -> Option<OpMarker> {
    let trimmed = line.trim();

    if let Some(rest) = trimmed.strip_prefix("*** Add File:") {
        return Some(OpMarker::Add(rest.trim().to_string()));
    }
    if let Some(rest) = trimmed.strip_prefix("***Add File:") {
        return Some(OpMarker::Add(rest.trim().to_string()));
    }
    if let Some(rest) = trimmed.strip_prefix("*** Update File:") {
        return Some(OpMarker::Update(rest.trim().to_string()));
    }
    if let Some(rest) = trimmed.strip_prefix("***Update File:") {
        return Some(OpMarker::Update(rest.trim().to_string()));
    }
    if let Some(rest) = trimmed.strip_prefix("*** Delete File:") {
        return Some(OpMarker::Delete(rest.trim().to_string()));
    }
    if let Some(rest) = trimmed.strip_prefix("***Delete File:") {
        return Some(OpMarker::Delete(rest.trim().to_string()));
    }
    if let Some(rest) = trimmed.strip_prefix("*** Move File:") {
        let rest = rest.trim();
        if let Some(pos) = rest.find("->") {
            let src = rest[..pos].trim().to_string();
            let dst = rest[pos + 2..].trim().to_string();
            return Some(OpMarker::Move(src, dst));
        }
        // No -> found, treat as move with empty dest (will error in validation)
        return Some(OpMarker::Move(rest.to_string(), String::new()));
    }
    if let Some(rest) = trimmed.strip_prefix("***Move File:") {
        let rest = rest.trim();
        if let Some(pos) = rest.find("->") {
            let src = rest[..pos].trim().to_string();
            let dst = rest[pos + 2..].trim().to_string();
            return Some(OpMarker::Move(src, dst));
        }
        return Some(OpMarker::Move(rest.to_string(), String::new()));
    }

    None
}

fn parse_context_hint(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.starts_with("@@") && trimmed.ends_with("@@") && trimmed.len() > 4 {
        let hint = trimmed[2..trimmed.len() - 2].trim();
        if !hint.is_empty() {
            return Some(hint.to_string());
        }
    }
    None
}

fn parse_hunk_line(line: &str) -> Option<HunkLine> {
    // Skip \ No newline at end of file markers
    if line.starts_with('\\') {
        return None;
    }

    if let Some(content) = line.strip_prefix('+') {
        Some(HunkLine {
            prefix: '+',
            content: content.to_string(),
        })
    } else if let Some(content) = line.strip_prefix('-') {
        Some(HunkLine {
            prefix: '-',
            content: content.to_string(),
        })
    } else if let Some(content) = line.strip_prefix(' ') {
        Some(HunkLine {
            prefix: ' ',
            content: content.to_string(),
        })
    } else if !line.is_empty() {
        // Lines without prefix treated as context (implicit space)
        Some(HunkLine {
            prefix: ' ',
            content: line.to_string(),
        })
    } else {
        None
    }
}

fn validate_file_path(
    path: &str,
    _line: usize,
    operation: OperationType,
) -> Result<(), PatchError> {
    if path.trim().is_empty() {
        Err(PatchError::EmptyFilePath {
            operation: operation.to_string(),
        })
    } else {
        Ok(())
    }
}

/// Extract content from ADD operation hunks (lines prefixed with '+').
pub fn extract_add_content(ops: &[PatchOperation]) -> Vec<(String, String)> {
    ops.iter()
        .filter(|op| op.operation == OperationType::Add)
        .map(|op| {
            let content: String = op
                .hunks
                .iter()
                .flat_map(|h| h.lines.iter())
                .filter(|hl| hl.prefix == '+')
                .map(|hl| hl.content.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            (op.file_path.clone(), content)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_empty_patch() {
        let input = "*** Begin Patch\n*** End Patch";
        let ops = parse_patch(input).unwrap();
        assert!(ops.is_empty());
    }

    #[test]
    fn test_parse_add_file() {
        let input = "\
*** Begin Patch
*** Add File: hello.py
+print(\"hello\")
+print(\"world\")
*** End Patch";
        let ops = parse_patch(input).unwrap();
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].operation, OperationType::Add);
        assert_eq!(ops[0].file_path, "hello.py");
        assert_eq!(ops[0].hunks[0].lines.len(), 2);
    }

    #[test]
    fn test_parse_update_file() {
        let input = "\
*** Begin Patch
*** Update File: config.py
@@ old config @@
-OLD_VALUE = 1
+NEW_VALUE = 2
*** End Patch";
        let ops = parse_patch(input).unwrap();
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].operation, OperationType::Update);
        assert_eq!(ops[0].file_path, "config.py");
        assert!(ops[0].hunks[0].context_hint.as_deref() == Some("old config"));
    }

    #[test]
    fn test_parse_delete_file() {
        let input = "\
*** Begin Patch
*** Delete File: old.py
*** End Patch";
        let ops = parse_patch(input).unwrap();
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].operation, OperationType::Delete);
        assert!(ops[0].hunks.is_empty());
    }

    #[test]
    fn test_parse_move_file() {
        let input = "\
*** Begin Patch
*** Move File: old/path.py -> new/path.py
*** End Patch";
        let ops = parse_patch(input).unwrap();
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].operation, OperationType::Move);
        assert_eq!(ops[0].file_path, "old/path.py");
        assert_eq!(ops[0].new_path.as_deref(), Some("new/path.py"));
    }

    #[test]
    fn test_parse_multi_file_patch() {
        let input = "\
*** Begin Patch
*** Add File: a.py
+line1
*** Update File: b.py
-old
+new
*** Delete File: c.py
*** End Patch";
        let ops = parse_patch(input).unwrap();
        assert_eq!(ops.len(), 3);
        assert_eq!(ops[0].operation, OperationType::Add);
        assert_eq!(ops[1].operation, OperationType::Update);
        assert_eq!(ops[2].operation, OperationType::Delete);
    }

    #[test]
    fn test_parse_update_no_hunks_error() {
        let input = "*** Begin Patch\n*** Update File: test.py\n*** End Patch";
        let err = parse_patch(input).unwrap_err();
        assert!(matches!(err, PatchError::UpdateNoHunks { .. }));
    }

    #[test]
    fn test_parse_move_no_destination_error() {
        let input = "*** Begin Patch\n*** Move File: src.py ->\n*** End Patch";
        let err = parse_patch(input).unwrap_err();
        assert!(matches!(err, PatchError::MoveNoDestination { .. }));
    }

    #[test]
    fn test_parse_without_space_marker() {
        let input = "\
***Begin Patch
***Add File: test.py
+content
***End Patch";
        let ops = parse_patch(input).unwrap();
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].file_path, "test.py");
    }

    #[test]
    fn test_parse_implicit_context_lines() {
        let input = "\
*** Begin Patch
*** Update File: test.py
 context line with space prefix
no prefix line
*** End Patch";
        let ops = parse_patch(input).unwrap();
        assert_eq!(ops[0].hunks[0].lines.len(), 2);
        assert_eq!(ops[0].hunks[0].lines[0].prefix, ' ');
        assert_eq!(ops[0].hunks[0].lines[1].prefix, ' ');
    }

    #[test]
    fn test_extract_add_content() {
        let input = "\
*** Begin Patch
*** Add File: new.py
+line1
+line2
+line3
*** End Patch";
        let ops = parse_patch(input).unwrap();
        let files = extract_add_content(&ops);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].0, "new.py");
        assert_eq!(files[0].1, "line1\nline2\nline3");
    }

    #[test]
    fn test_parse_no_newline_marker_skipped() {
        let input = "\
*** Begin Patch
*** Add File: test.py
+hello
\\ No newline at end of file
*** End Patch";
        let ops = parse_patch(input).unwrap();
        assert_eq!(ops[0].hunks[0].lines.len(), 1);
        assert_eq!(ops[0].hunks[0].lines[0].content, "hello");
    }
}
