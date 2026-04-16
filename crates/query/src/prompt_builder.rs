//! System Prompt Builder
//!
//! Assembles the system prompt from multiple components.

use h_core::HermesConfig;

/// Default agent identity.
const DEFAULT_AGENT_IDENTITY: &str = "You are Hermes, an AI assistant with access to tools.";

/// Build the complete system prompt.
///
/// Components (matching Python codebase):
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
pub fn build_system_prompt(config: &HermesConfig) -> String {
    let mut components: Vec<String> = Vec::new();

    // 1. Agent identity
    components.push(DEFAULT_AGENT_IDENTITY.to_string());

    // 2. Personality
    if let Some(personality) = &config.personality {
        components.push(personality.clone());
    }

    // 3. Platform hints (OS, shell, etc.)
    components.push(build_platform_hints());

    // 4. Tool usage guidance
    components.push(build_tool_guidance().to_string());

    // 5. Memory guidance (if memory is enabled)
    if let Some(memory) = &config.memory {
        if memory.enabled {
            components.push(build_memory_guidance().to_string());
        }
    }

    // 6. Session search guidance
    components.push(build_session_search_guidance().to_string());

    // 7. Skills guidance
    components.push(build_skills_guidance().to_string());

    // 8. Context files (could load AGENTS.md, .cursorrules)
    // Placeholder - would need file I/O

    // 9. Environment hints
    components.push(build_environment_hints());

    // 10. Skills system prompt
    // Placeholder - would need skills system

    // Join all components
    components
        .iter()
        .filter(|s| !s.is_empty())
        .cloned()
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Build platform-specific hints.
fn build_platform_hints() -> String {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;

    format!(
        "Platform: {} ({} architecture).\nWorking directory: {}",
        os,
        arch,
        std::env::current_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "unknown".to_string())
    )
}

/// Build tool usage guidance.
fn build_tool_guidance() -> &'static str {
    "You have access to tools for reading files, writing files, executing code, \
     searching the web, and more. Use tools when appropriate to complete tasks. \
     Always verify tool results before proceeding."
}

/// Build memory guidance.
fn build_memory_guidance() -> &'static str {
    "You have a memory tool that can store and retrieve information across sessions. \
     Use it to remember important details about the user and their preferences."
}

/// Build session search guidance.
fn build_session_search_guidance() -> &'static str {
    "You can search previous conversation sessions to find relevant context. \
     Use session_search when you need to recall information from past interactions."
}

/// Build skills guidance.
fn build_skills_guidance() -> &'static str {
    "Skills are predefined workflows that you can invoke. Use skills_view to list \
     available skills and skill_manage to run or manage them."
}

/// Build environment hints.
fn build_environment_hints() -> String {
    let shell = std::env::var("SHELL")
        .ok()
        .map(|s| format!("Shell: {}", s))
        .unwrap_or_default();

    let editor = std::env::var("EDITOR")
        .ok()
        .map(|e| format!("Editor: {}", e))
        .unwrap_or_default();

    let mut hints = Vec::new();
    if !shell.is_empty() {
        hints.push(shell);
    }
    if !editor.is_empty() {
        hints.push(editor);
    }

    hints.join("\n")
}

/// Build prompt for specific toolset.
pub fn build_toolset_prompt(toolset: &str) -> &'static str {
    match toolset {
        "file" => "File tools: read_file, write_file, patch, search_files. \
                   Use these to read, modify, and search files.",
        "terminal" => "Terminal tool: execute commands safely. \
                       Destructive commands require user approval.",
        "web" => "Web tools: web_search, web_extract. \
                  Use these to search and fetch web content.",
        "memory" => "Memory tool: store and recall information across sessions.",
        "delegate" => "Delegate tool: spawn sub-agents for complex tasks.",
        _ => "",
    }
}

/// Cache key for prompt caching.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct PromptCacheKey {
    /// Config hash.
    pub config_hash: u64,
    /// Personality hash.
    pub personality_hash: u64,
    /// Tool definitions hash.
    pub tools_hash: u64,
}

impl PromptCacheKey {
    /// Create a new cache key.
    pub fn new(config: &HermesConfig, tools: &[h_core::ToolDefinition]) -> Self {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();

        // Hash personality
        config.personality.hash(&mut hasher);
        let personality_hash = hasher.finish();

        // Hash config basics
        config.model.hash(&mut hasher);
        config.provider.hash(&mut hasher);
        let config_hash = hasher.finish();

        // Hash tools
        hasher = DefaultHasher::new();
        for tool in tools {
            tool.function.name.hash(&mut hasher);
        }
        let tools_hash = hasher.finish();

        Self {
            config_hash,
            personality_hash,
            tools_hash,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_system_prompt() {
        let config = HermesConfig::default();
        let prompt = build_system_prompt(&config);

        assert!(prompt.contains("Hermes"));
        assert!(prompt.contains("Platform"));
    }

    #[test]
    fn test_build_platform_hints() {
        let hints = build_platform_hints();
        assert!(hints.contains("Platform"));
    }

    #[test]
    fn test_build_tool_guidance() {
        let guidance = build_tool_guidance();
        assert!(guidance.contains("tools"));
    }

    #[test]
    fn test_build_toolset_prompt() {
        assert!(build_toolset_prompt("file").contains("read_file"));
        assert!(build_toolset_prompt("terminal").contains("commands"));
        assert!(build_toolset_prompt("unknown").is_empty());
    }

    #[test]
    fn test_prompt_cache_key() {
        let config = HermesConfig::default();
        let tools = Vec::new();
        let key1 = PromptCacheKey::new(&config, &tools);
        let key2 = PromptCacheKey::new(&config, &tools);
        assert_eq!(key1, key2);
    }
}