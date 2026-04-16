//! Toolset definitions for Hermes Agent.
//!
//! Toolsets are groups of related tools that can be enabled/disabled together.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Toolset identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Toolset {
    /// Core tools always available
    Core,
    /// File I/O tools
    Files,
    /// Terminal/shell tools
    Terminal,
    /// Web tools (search, extract)
    Web,
    /// Browser automation tools
    Browser,
    /// Code execution tools
    CodeExecution,
    /// Delegation/subagent tools
    Delegation,
    /// MCP tools
    Mcp,
    /// Skills tools
    Skills,
    /// Memory tools
    Memory,
    /// Session search tools
    SessionSearch,
    /// Text-to-speech tools
    Tts,
    /// Vision/image analysis tools
    Vision,
    /// Home Assistant tools
    HomeAssistant,
    /// Todo/list tools
    Todo,
    /// Image generation tools
    ImageGeneration,
    /// Cron job tools
    Cronjob,
    /// Transcription tools
    Transcription,
    /// Voice mode tools
    Voice,
}

impl fmt::Display for Toolset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Toolset::Core => "core",
            Toolset::Files => "files",
            Toolset::Terminal => "terminal",
            Toolset::Web => "web",
            Toolset::Browser => "browser",
            Toolset::CodeExecution => "code_execution",
            Toolset::Delegation => "delegation",
            Toolset::Mcp => "mcp",
            Toolset::Skills => "skills",
            Toolset::Memory => "memory",
            Toolset::SessionSearch => "session_search",
            Toolset::Tts => "tts",
            Toolset::Vision => "vision",
            Toolset::HomeAssistant => "homeassistant",
            Toolset::Todo => "todo",
            Toolset::ImageGeneration => "image_generation",
            Toolset::Cronjob => "cronjob",
            Toolset::Transcription => "transcription",
            Toolset::Voice => "voice",
        };
        write!(f, "{}", s)
    }
}

impl std::str::FromStr for Toolset {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().replace('-', "_").as_str() {
            "core" => Ok(Toolset::Core),
            "files" => Ok(Toolset::Files),
            "terminal" => Ok(Toolset::Terminal),
            "web" => Ok(Toolset::Web),
            "browser" => Ok(Toolset::Browser),
            "code_execution" | "codeexecution" => Ok(Toolset::CodeExecution),
            "delegation" => Ok(Toolset::Delegation),
            "mcp" => Ok(Toolset::Mcp),
            "skills" => Ok(Toolset::Skills),
            "memory" => Ok(Toolset::Memory),
            "session_search" | "sessionsearch" => Ok(Toolset::SessionSearch),
            "tts" => Ok(Toolset::Tts),
            "vision" => Ok(Toolset::Vision),
            "homeassistant" | "home_assistant" => Ok(Toolset::HomeAssistant),
            "todo" => Ok(Toolset::Todo),
            "image_generation" | "imagegeneration" => Ok(Toolset::ImageGeneration),
            "cronjob" => Ok(Toolset::Cronjob),
            "transcription" => Ok(Toolset::Transcription),
            "voice" => Ok(Toolset::Voice),
            _ => Err(format!("Unknown toolset: {}", s)),
        }
    }
}

/// Core tools that are always enabled.
pub const HERMES_CORE_TOOLS: &[Toolset] = &[
    Toolset::Core,
    Toolset::Files,
    Toolset::Terminal,
];

/// All available toolsets.
pub const ALL_TOOLSETS: &[Toolset] = &[
    Toolset::Core,
    Toolset::Files,
    Toolset::Terminal,
    Toolset::Web,
    Toolset::Browser,
    Toolset::CodeExecution,
    Toolset::Delegation,
    Toolset::Mcp,
    Toolset::Skills,
    Toolset::Memory,
    Toolset::SessionSearch,
    Toolset::Tts,
    Toolset::Vision,
    Toolset::HomeAssistant,
    Toolset::Todo,
    Toolset::ImageGeneration,
    Toolset::Cronjob,
    Toolset::Transcription,
    Toolset::Voice,
];

/// Get all toolset names as strings.
pub fn toolset_names() -> Vec<String> {
    ALL_TOOLSETS.iter().map(|t| t.to_string()).collect()
}

/// Parse a toolset name string.
pub fn parse_toolset(name: &str) -> Option<Toolset> {
    name.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_toolset_display() {
        assert_eq!(Toolset::Files.to_string(), "files");
        assert_eq!(Toolset::CodeExecution.to_string(), "code_execution");
    }

    #[test]
    fn test_toolset_from_str() {
        assert_eq!("files".parse::<Toolset>().unwrap(), Toolset::Files);
        assert_eq!("code_execution".parse::<Toolset>().unwrap(), Toolset::CodeExecution);
        assert_eq!("code-execution".parse::<Toolset>().unwrap(), Toolset::CodeExecution);
    }

    #[test]
    fn test_all_toolsets() {
        assert!(!ALL_TOOLSETS.is_empty());
        assert!(ALL_TOOLSETS.contains(&Toolset::Core));
    }

    #[test]
    fn test_core_tools() {
        assert!(HERMES_CORE_TOOLS.contains(&Toolset::Core));
        assert!(HERMES_CORE_TOOLS.contains(&Toolset::Files));
        assert!(HERMES_CORE_TOOLS.contains(&Toolset::Terminal));
    }
}