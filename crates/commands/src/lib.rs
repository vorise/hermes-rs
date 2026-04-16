//! Hermes Commands Crate
//!
//! Slash command implementations for the Hermes agent.

pub mod commands;
pub mod registry;
pub mod skills_hub;

pub use commands::{
    SlashCommand, CommandContext, CommandResult, CommandHelp,
    ConfigChangeMessage, parse_command_input,
};
pub use registry::CommandRegistry;
pub use skills_hub::{SkillsHub, SkillInfo, InstallProgress};