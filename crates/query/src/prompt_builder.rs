/// System prompt builder that assembles the system prompt from components.
///
/// Order matches the Python codebase:
/// 1. Agent identity
/// 2. Personality (from config or SOUL.md)
/// 3. Platform hints
/// 4. Tool usage guidance
/// 5. Memory guidance
/// 6. Session search guidance
/// 7. Skills guidance
/// 8. Context files (AGENTS.md, .cursorrules)
/// 9. Environment hints
/// 10. Skills system prompt

/// Default agent identity — the core system prompt.
pub const DEFAULT_AGENT_IDENTITY: &str = "You are Hermes, an AI assistant and coding companion built by Nous Research. \
You are helpful, concise, and accurate. You write clean, correct code and explain your reasoning. \
You use tools to accomplish tasks and follow instructions carefully.";

/// Platform-specific hints for different entry points.
pub const PLATFORM_HINTES_CLI: &str = "You are running in a terminal CLI environment. \
Use the terminal tool to execute commands. File tools are available for reading and writing files.";

/// Memory system guidance injected into the system prompt.
pub const MEMORY_GUIDANCE: &str = "You have a persistent memory system available via the memory tool. \
Use it to remember important information across sessions. Save key learnings, user preferences, and \
important context that would be useful in future conversations.";

/// Session search guidance.
pub const SESSION_SEARCH_GUIDE: &str = "You can search past conversations using the session_search tool. \
Use this to find relevant information from previous sessions when it would help answer the user's question.";

/// Skills system guidance.
pub const SKILLS_GUIDANCE: &str = "Skills are procedural knowledge files that define how to perform complex tasks. \
Use the skills_list and skill_view tools to discover and read skills. Follow skill instructions carefully.";

/// Tool use enforcement guidance for models that need it.
pub const TOOL_USE_ENFORCEMENT: &str = "You have access to tools. Use them when they would help accomplish the task. \
Don't describe what you're going to do — just do it by calling the appropriate tool. \
Use multiple tool calls in parallel when they are independent of each other.";

/// Builder for constructing the system prompt.
pub struct PromptBuilder {
    identity: String,
    personality: Option<String>,
    platform_hints: Vec<String>,
    tool_guidance: Option<String>,
    memory_guidance: Option<String>,
    session_search_guidance: Option<String>,
    skills_guidance: Option<String>,
    context_files: Vec<String>,
    env_hints: Vec<String>,
    skills_system: Option<String>,
}

impl Default for PromptBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl PromptBuilder {
    pub fn new() -> Self {
        Self {
            identity: DEFAULT_AGENT_IDENTITY.to_string(),
            personality: None,
            platform_hints: vec![],
            tool_guidance: None,
            memory_guidance: None,
            session_search_guidance: None,
            skills_guidance: None,
            context_files: vec![],
            env_hints: vec![],
            skills_system: None,
        }
    }

    pub fn identity(mut self, identity: &str) -> Self {
        self.identity = identity.to_string();
        self
    }

    pub fn personality(mut self, personality: &str) -> Self {
        self.personality = Some(personality.to_string());
        self
    }

    pub fn platform_hint(mut self, hint: &str) -> Self {
        self.platform_hints.push(hint.to_string());
        self
    }

    pub fn tool_guidance(mut self, guidance: &str) -> Self {
        self.tool_guidance = Some(guidance.to_string());
        self
    }

    pub fn memory_guidance(mut self) -> Self {
        self.memory_guidance = Some(MEMORY_GUIDANCE.to_string());
        self
    }

    pub fn session_search_guidance(mut self) -> Self {
        self.session_search_guidance = Some(SESSION_SEARCH_GUIDE.to_string());
        self
    }

    pub fn skills_guidance(mut self) -> Self {
        self.skills_guidance = Some(SKILLS_GUIDANCE.to_string());
        self
    }

    pub fn context_file(mut self, content: &str) -> Self {
        self.context_files.push(content.to_string());
        self
    }

    pub fn env_hint(mut self, hint: &str) -> Self {
        self.env_hints.push(hint.to_string());
        self
    }

    pub fn skills_system(mut self, content: &str) -> Self {
        self.skills_system = Some(content.to_string());
        self
    }

    /// Build the assembled system prompt.
    pub fn build(&self) -> String {
        let mut sections: Vec<&str> = Vec::new();

        sections.push(&self.identity);

        if let Some(ref p) = self.personality {
            sections.push(p);
        }

        for hint in &self.platform_hints {
            sections.push(hint);
        }

        if let Some(ref g) = self.tool_guidance {
            sections.push(g);
        }

        if let Some(ref g) = self.memory_guidance {
            sections.push(g);
        }

        if let Some(ref g) = self.session_search_guidance {
            sections.push(g);
        }

        if let Some(ref g) = self.skills_guidance {
            sections.push(g);
        }

        for file in &self.context_files {
            sections.push(file);
        }

        for hint in &self.env_hints {
            sections.push(hint);
        }

        if let Some(ref s) = self.skills_system {
            sections.push(s);
        }

        sections.join("\n\n---\n\n")
    }

    /// Build with all default guidance enabled (CLI mode).
    pub fn build_cli() -> String {
        Self::new()
            .platform_hint(PLATFORM_HINTES_CLI)
            .tool_guidance(TOOL_USE_ENFORCEMENT)
            .memory_guidance()
            .session_search_guidance()
            .skills_guidance()
            .build()
    }
}

/// Load personality from SOUL.md if it exists.
pub fn load_soul(path: &std::path::Path) -> Option<String> {
    if path.exists() {
        std::fs::read_to_string(path).ok()
    } else {
        None
    }
}

/// Load context files from the current working directory.
pub fn load_context_files(working_dir: &std::path::Path) -> Vec<String> {
    let mut files = Vec::new();

    // Check for AGENTS.md
    let agents_md = working_dir.join("AGENTS.md");
    if agents_md.exists() {
        if let Ok(content) = std::fs::read_to_string(&agents_md) {
            files.push(format!("## AGENTS.md\n\n{content}"));
        }
    }

    // Check for .cursorrules
    let cursorrules = working_dir.join(".cursorrules");
    if cursorrules.exists() {
        if let Ok(content) = std::fs::read_to_string(&cursorrules) {
            files.push(format!("## .cursorrules\n\n{content}"));
        }
    }

    files
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_prompt_builder() {
        let builder = PromptBuilder::new();
        let prompt = builder.build();
        assert!(prompt.contains("Hermes"));
        assert!(prompt.contains("AI assistant"));
    }

    #[test]
    fn test_prompt_with_sections() {
        let prompt = PromptBuilder::build_cli();
        assert!(prompt.contains("Hermes"));
        assert!(prompt.contains("terminal CLI"));
        assert!(prompt.contains("memory system"));
        assert!(prompt.contains("session_search"));
        assert!(prompt.contains("Skills"));
    }

    #[test]
    fn test_prompt_builder_chaining() {
        let prompt = PromptBuilder::new()
            .identity("Custom identity")
            .personality("Friendly and helpful")
            .platform_hint("CLI mode")
            .build();

        assert!(prompt.contains("Custom identity"));
        assert!(prompt.contains("Friendly and helpful"));
        assert!(prompt.contains("CLI mode"));
    }

    #[test]
    fn test_soul_loading_missing() {
        let result = load_soul(std::path::Path::new("/nonexistent/SOUL.md"));
        assert!(result.is_none());
    }
}
