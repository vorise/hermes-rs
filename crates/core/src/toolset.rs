use serde::{Deserialize, Serialize};

/// Known toolsets in Hermes. Each toolset groups related tools.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Toolset {
    /// Terminal execution tools
    Terminal,
    /// File I/O tools
    FileIo,
    /// Web search and extraction
    Web,
    /// Browser automation
    Browser,
    /// Code execution sandbox
    CodeExecution,
    /// Subagent delegation
    Delegation,
    /// Skills management
    Skills,
    /// Memory operations
    Memory,
    /// Session search
    SessionSearch,
    /// Text-to-speech
    Tts,
    /// Vision/image analysis
    Vision,
    /// Home Assistant integration
    HomeAssistant,
    /// Todo management
    Todo,
    /// Image generation
    ImageGen,
    /// Cron job management
    CronJob,
    /// Transcription
    Transcription,
    /// Voice input
    Voice,
    /// MCP client tools
    Mcp,
}

impl Toolset {
    pub fn as_str(&self) -> &str {
        match self {
            Toolset::Terminal => "terminal",
            Toolset::FileIo => "file_io",
            Toolset::Web => "web",
            Toolset::Browser => "browser",
            Toolset::CodeExecution => "code_execution",
            Toolset::Delegation => "delegation",
            Toolset::Skills => "skills",
            Toolset::Memory => "memory",
            Toolset::SessionSearch => "session_search",
            Toolset::Tts => "tts",
            Toolset::Vision => "vision",
            Toolset::HomeAssistant => "home_assistant",
            Toolset::Todo => "todo",
            Toolset::ImageGen => "image_gen",
            Toolset::CronJob => "cron_job",
            Toolset::Transcription => "transcription",
            Toolset::Voice => "voice",
            Toolset::Mcp => "mcp",
        }
    }
}

impl std::fmt::Display for Toolset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for Toolset {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "terminal" => Ok(Toolset::Terminal),
            "file_io" => Ok(Toolset::FileIo),
            "web" => Ok(Toolset::Web),
            "browser" => Ok(Toolset::Browser),
            "code_execution" => Ok(Toolset::CodeExecution),
            "delegation" => Ok(Toolset::Delegation),
            "skills" => Ok(Toolset::Skills),
            "memory" => Ok(Toolset::Memory),
            "session_search" => Ok(Toolset::SessionSearch),
            "tts" => Ok(Toolset::Tts),
            "vision" => Ok(Toolset::Vision),
            "home_assistant" => Ok(Toolset::HomeAssistant),
            "todo" => Ok(Toolset::Todo),
            "image_gen" => Ok(Toolset::ImageGen),
            "cron_job" => Ok(Toolset::CronJob),
            "transcription" => Ok(Toolset::Transcription),
            "voice" => Ok(Toolset::Voice),
            "mcp" => Ok(Toolset::Mcp),
            _ => Err(format!("unknown toolset: {s}")),
        }
    }
}
