use serde::{Deserialize, Serialize};

/// File context tracked by the IDE.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileContext {
    /// Absolute file path.
    pub path: String,
    /// File content (may be full file or a subset).
    pub content: String,
    /// Whether the file is open in the IDE.
    pub is_open: bool,
}

/// Text selection in the IDE.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Selection {
    /// Selected file path.
    pub file_path: String,
    /// Selected text content.
    pub text: String,
    /// Start line (1-based).
    pub start_line: usize,
    /// End line (1-based).
    pub end_line: usize,
}

impl Selection {
    pub fn is_empty(&self) -> bool {
        self.text.trim().is_empty()
    }
}

/// Context snapshot sent with messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextSnapshot {
    /// Open files with context.
    pub files: Vec<FileContext>,
    /// Current selection, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selection: Option<Selection>,
    /// Current git branch.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_branch: Option<String>,
}

impl ContextSnapshot {
    pub fn new() -> Self {
        Self {
            files: Vec::new(),
            selection: None,
            git_branch: None,
        }
    }

    /// Format the context as a text block for injection into prompts.
    pub fn format_prompt_context(&self) -> String {
        let mut parts = Vec::new();

        if !self.files.is_empty() {
            let mut file_section = String::from("## Context Files\n");
            for file in &self.files {
                file_section.push_str(&format!("\n### {}\n", file.path));
                file_section.push_str(&file.content);
            }
            parts.push(file_section);
        }

        if let Some(sel) = &self.selection {
            parts.push(format!(
                "## Selected Text\n**File:** {} (lines {}-{})\n```\n{}\n```",
                sel.file_path, sel.start_line, sel.end_line, sel.text
            ));
        }

        if let Some(branch) = &self.git_branch {
            parts.push(format!("## Git\nCurrent branch: `{branch}`"));
        }

        parts.join("\n\n")
    }
}

impl Default for ContextSnapshot {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_selection_is_empty() {
        let sel = Selection {
            file_path: "/test.rs".to_string(),
            text: "   ".to_string(),
            start_line: 1,
            end_line: 1,
        };
        assert!(sel.is_empty());

        let sel2 = Selection {
            file_path: "/test.rs".to_string(),
            text: "fn main()".to_string(),
            start_line: 1,
            end_line: 1,
        };
        assert!(!sel2.is_empty());
    }

    #[test]
    fn test_context_snapshot_format_empty() {
        let snapshot = ContextSnapshot::new();
        let formatted = snapshot.format_prompt_context();
        assert!(formatted.is_empty());
    }

    #[test]
    fn test_context_snapshot_format_with_files() {
        let mut snapshot = ContextSnapshot::new();
        snapshot.files.push(FileContext {
            path: "/src/main.rs".to_string(),
            content: "fn main() {}".to_string(),
            is_open: true,
        });
        let formatted = snapshot.format_prompt_context();
        assert!(formatted.contains("## Context Files"));
        assert!(formatted.contains("/src/main.rs"));
    }

    #[test]
    fn test_context_snapshot_format_with_selection() {
        let mut snapshot = ContextSnapshot::new();
        snapshot.selection = Some(Selection {
            file_path: "/src/main.rs".to_string(),
            text: "fn main()".to_string(),
            start_line: 1,
            end_line: 1,
        });
        let formatted = snapshot.format_prompt_context();
        assert!(formatted.contains("## Selected Text"));
        assert!(formatted.contains("fn main()"));
    }
}
