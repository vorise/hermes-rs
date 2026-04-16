use anyhow::Result;
use async_trait::async_trait;
use crate::{CommandContext, CommandResult, SlashCommand};
use h_mcp::McpClient;

/// Slash command: `/mcp` — manage MCP servers.
pub struct McpCommand;

#[async_trait]
impl SlashCommand for McpCommand {
    fn name(&self) -> &str {
        "mcp"
    }

    fn description(&self) -> &str {
        "Manage MCP servers (list, add, remove, connect, disconnect, tools)"
    }

    fn category(&self) -> &str {
        "integration"
    }

    async fn execute(&self, args: &str, _ctx: &CommandContext) -> Result<CommandResult> {
        let parts: Vec<&str> = args.split_whitespace().collect();

        match parts.first().copied() {
            Some("list") => cmd_list(),
            Some("add") => cmd_add(&parts[1..]),
            Some("remove") => cmd_remove(&parts[1..]),
            Some("status") => cmd_status(),
            Some("connect") => cmd_connect(&parts[1..]),
            Some("disconnect") => cmd_disconnect(&parts[1..]),
            Some("tools") => cmd_tools(),
            Some(sub) => Ok(CommandResult::Message(format!(
                "Unknown mcp subcommand: {sub}. Use /mcp help for usage."
            ))),
            None => cmd_list(),
        }
    }
}

fn cmd_list() -> Result<CommandResult> {
    let config = h_mcp::McpConfig::from_default().ok();
    match config {
        Some(cfg) if !cfg.is_empty() => {
            let mut output = String::from("## Configured MCP Servers\n\n");
            for (name, entry) in cfg.all_servers() {
                let status = if entry.enabled { "enabled" } else { "disabled" };
                let transport_desc = match &entry.transport {
                    h_mcp::config::TransportConfig::Stdio { command, args, .. } => {
                        let args_str = args.join(" ");
                        if args_str.is_empty() {
                            format!("stdio: {command}")
                        } else {
                            format!("stdio: {command} {args_str}")
                        }
                    }
                    h_mcp::config::TransportConfig::Sse { url } => {
                        format!("sse: {url}")
                    }
                };
                output.push_str(&format!("- **{name}** ({status}) — {transport_desc}\n"));
            }
            Ok(CommandResult::Message(output))
        }
        _ => Ok(CommandResult::Message(
            "No MCP servers configured. Use `/mcp add <name> <command> [args...]`.".to_string(),
        )),
    }
}

fn cmd_add(args: &[&str]) -> Result<CommandResult> {
    if args.is_empty() {
        return Ok(CommandResult::Message(
            "Usage: /mcp add <name> <command> [args...]".to_string(),
        ));
    }

    let name = args[0];
    let command = args.get(1).copied().unwrap_or("");
    let cmd_args: Vec<String> = args[2..].iter().map(|s| s.to_string()).collect();

    if command.is_empty() {
        return Ok(CommandResult::Message(
            "Usage: /mcp add <name> <command> [args...]".to_string(),
        ));
    }

    let entry = h_mcp::McpServerEntry {
        name: name.to_string(),
        enabled: true,
        transport: h_mcp::config::TransportConfig::Stdio {
            command: command.to_string(),
            args: cmd_args,
            env: None,
        },
        settings: Default::default(),
    };

    let mut config = h_mcp::McpConfig::from_default().unwrap_or_default();
    config.add_server(name, entry);
    let config_path = h_mcp::McpConfig::default_path();
    if let Some(parent) = config_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    config.save(&config_path)?;

    Ok(CommandResult::Message(format!(
        "MCP server '{name}' added to config. Connect with `/mcp connect {name}`."
    )))
}

fn cmd_remove(args: &[&str]) -> Result<CommandResult> {
    if args.is_empty() {
        return Ok(CommandResult::Message(
            "Usage: /mcp remove <name>".to_string(),
        ));
    }

    let name = args[0];
    let mut config = h_mcp::McpConfig::from_default().unwrap_or_default();
    if config.remove_server(name).is_some() {
        let config_path = h_mcp::McpConfig::default_path();
        config.save(&config_path)?;
        Ok(CommandResult::Message(format!(
            "MCP server '{name}' removed."
        )))
    } else {
        Ok(CommandResult::Message(format!(
            "MCP server '{name}' not found in config."
        )))
    }
}

fn cmd_status() -> Result<CommandResult> {
    let config = h_mcp::McpConfig::from_default().ok();

    let mut output = String::from("## MCP Server Status\n\n");
    output.push_str("| Server | Configured | Connected |\n");
    output.push_str("|--------|------------|-----------|\n");

    let server_names: Vec<String> = match &config {
        Some(cfg) => cfg.all_servers().iter().map(|(n, _)| n.to_string()).collect(),
        None => Vec::new(),
    };

    for name in &server_names {
        output.push_str(&format!("| {name} | Yes | No |\n"));
    }

    if server_names.is_empty() {
        output.push_str("\nNo servers configured.\n");
    }

    Ok(CommandResult::Message(output))
}

fn cmd_connect(args: &[&str]) -> Result<CommandResult> {
    if args.is_empty() {
        return Ok(CommandResult::Message(
            "Usage: /mcp connect <name>".to_string(),
        ));
    }

    let name = args[0];
    let config = h_mcp::McpConfig::from_default().ok();

    let _entry = config
        .and_then(|c| c.get_server(name).cloned())
        .ok_or_else(|| anyhow::anyhow!("MCP server '{name}' not found in config"))?;

    Ok(CommandResult::Message(format!(
        "Connecting to MCP server '{name}'... (runtime connection not yet wired into query loop)"
    )))
}

fn cmd_disconnect(args: &[&str]) -> Result<CommandResult> {
    if args.is_empty() {
        return Ok(CommandResult::Message(
            "Usage: /mcp disconnect <name>".to_string(),
        ));
    }

    let name = args[0];
    Ok(CommandResult::Message(format!(
        "Disconnecting from MCP server '{name}'..."
    )))
}

fn cmd_tools() -> Result<CommandResult> {
    let client = McpClient::new();
    let tools = client.list_tools();

    if tools.is_empty() {
        return Ok(CommandResult::Message(
            "No MCP tools available. Connect to a server first with `/mcp connect <name>`."
                .to_string(),
        ));
    }

    let mut output = String::from("## Available MCP Tools\n\n");
    for tool in tools {
        output.push_str(&format!(
            "- **{name}** ({server}): {desc}\n",
            name = tool.name,
            server = tool.server_name,
            desc = tool.description,
        ));
    }
    Ok(CommandResult::Message(output))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mcp_command_name() {
        assert_eq!(McpCommand.name(), "mcp");
    }

    #[test]
    fn test_mcp_command_category() {
        assert_eq!(McpCommand.category(), "integration");
    }

    #[tokio::test]
    async fn test_mcp_no_args_shows_list() {
        let result = McpCommand.execute("", &dummy_context()).await.unwrap();
        match result {
            CommandResult::Message(msg) => {
                assert!(msg.contains("MCP") || msg.contains("configured"));
            }
            _ => panic!("Expected Message variant"),
        }
    }

    #[tokio::test]
    async fn test_mcp_add_requires_name_and_command() {
        let result = McpCommand.execute("add", &dummy_context()).await.unwrap();
        match result {
            CommandResult::Message(msg) => {
                assert!(msg.contains("Usage"));
            }
            _ => panic!("Expected Message variant"),
        }
    }

    #[tokio::test]
    async fn test_mcp_remove_requires_name() {
        let result = McpCommand.execute("remove", &dummy_context()).await.unwrap();
        match result {
            CommandResult::Message(msg) => {
                assert!(msg.contains("Usage"));
            }
            _ => panic!("Expected Message variant"),
        }
    }

    #[tokio::test]
    async fn test_mcp_tools_empty() {
        let result = McpCommand.execute("tools", &dummy_context()).await.unwrap();
        match result {
            CommandResult::Message(msg) => {
                assert!(msg.contains("No MCP tools available"));
            }
            _ => panic!("Expected Message variant"),
        }
    }

    fn dummy_context() -> CommandContext {
        use std::sync::Arc;
        use tokio::sync::Notify;
        let db = h_core::SessionDB::new_in_memory().unwrap();
        CommandContext {
            session_db: Arc::new(db),
            session_id: "test".to_string(),
            messages: Vec::new(),
            model: h_core::ModelRef::new(h_core::ProviderId::new("test"), h_core::ModelId::new("test")),
            cost: h_core::CostTracker::default(),
            budget_remaining: Some(10),
            is_processing: false,
            interrupt_notify: Arc::new(Notify::new()),
            hermes_config: h_core::HermesConfig::default(),
        }
    }
}
