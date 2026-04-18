pub mod approval;
pub mod builtin_tools;
pub mod code_execution;
pub mod file_tools;
pub mod fuzzy_match;
pub mod interrupt;
pub mod patch_parser;
pub mod process_registry;
mod tool;
pub mod terminal;
pub mod tirith;
pub mod send_message;
pub mod web_tools;
pub mod browser_camofox;

pub use interrupt::{InterruptGuard, check_interrupt};

pub use tool::*;
pub use file_tools::{ReadFileTool, WriteFileTool, PatchTool, SearchFilesTool, GrepTool};
pub use terminal::TerminalTool;
pub use web_tools::{WebSearchTool, WebExtractTool};
pub use builtin_tools::{
    MemoryTool, TodoTool, SessionSearchTool,
    VisionTool, ImageGenTool, TtsTool, TranscriptionTool, DelegateTool,
    HomeAssistantTool, CronJobTool, MixtureOfAgentsTool,
    SkillsTool, SkillsHubTool,
    BrowserTool, ClarifyTool, VoiceTool,
};
pub use code_execution::CodeExecutionTool;
pub use send_message::SendMessageTool;

/// Create all built-in tools as a vector.
pub fn create_all_tools() -> Vec<std::sync::Arc<dyn Tool>> {
    vec![
        std::sync::Arc::new(TerminalTool::new()),
        std::sync::Arc::new(ReadFileTool),
        std::sync::Arc::new(WriteFileTool),
        std::sync::Arc::new(PatchTool),
        std::sync::Arc::new(SearchFilesTool),
        std::sync::Arc::new(GrepTool),
        std::sync::Arc::new(WebSearchTool::new()),
        std::sync::Arc::new(WebExtractTool::new()),
        std::sync::Arc::new(MemoryTool::new()),
        std::sync::Arc::new(TodoTool::new()),
        std::sync::Arc::new(SessionSearchTool::default_path()),
        std::sync::Arc::new(VisionTool::new()),
        std::sync::Arc::new(ImageGenTool::new()),
        std::sync::Arc::new(TtsTool::new()),
        std::sync::Arc::new(TranscriptionTool::new()),
        std::sync::Arc::new(DelegateTool::new()),
        std::sync::Arc::new(HomeAssistantTool::new()),
        std::sync::Arc::new(CronJobTool::new()),
        std::sync::Arc::new(MixtureOfAgentsTool::new()),
        std::sync::Arc::new(SkillsTool::new()),
        std::sync::Arc::new(SkillsHubTool::new()),
        std::sync::Arc::new(BrowserTool::new()),
        std::sync::Arc::new(CodeExecutionTool::new()),
        std::sync::Arc::new(ClarifyTool),
        std::sync::Arc::new(VoiceTool::new()),
        std::sync::Arc::new(SendMessageTool::new()),
    ]
}
