use anyhow::Result;
use async_trait::async_trait;

use crate::{CommandContext, CommandResult, SlashCommand};

/// /export — Export the current conversation.
pub struct ExportCommand;

#[async_trait]
impl SlashCommand for ExportCommand {
    fn name(&self) -> &str {
        "export"
    }

    fn description(&self) -> &str {
        "Export the current conversation"
    }

    fn category(&self) -> &str {
        "session"
    }

    async fn execute(&self, args: &str, ctx: &CommandContext) -> Result<CommandResult> {
        let format = args.trim();
        match format {
            "" | "text" => export_text(ctx),
            "json" => export_json(ctx),
            "markdown" | "md" => export_markdown(ctx),
            other => Ok(CommandResult::Message(format!(
                "Unknown format: {other}. Use /export [text|json|markdown]"
            ))),
        }
    }
}

fn export_text(ctx: &CommandContext) -> Result<CommandResult> {
    let mut lines = vec![format!("Session: {}", ctx.session_id), "".to_string()];
    for msg in &ctx.messages {
        let role = format_role(&msg.role);
        let content = msg
            .content
            .as_ref()
            .and_then(|c| c.as_text())
            .unwrap_or("");
        lines.push(format!("{role}: {content}"));
        if let Some(tool_calls) = &msg.tool_calls {
            for tc in tool_calls {
                lines.push(format!(
                    "  [tool_call] {}({})",
                    tc.function.name,
                    tc.function.arguments
                ));
            }
        }
    }
    Ok(CommandResult::Message(lines.join("\n")))
}

fn export_json(ctx: &CommandContext) -> Result<CommandResult> {
    let json = serde_json::to_string_pretty(&ctx.messages).unwrap_or_else(|e| {
        format!("Error serializing messages: {e}")
    });
    Ok(CommandResult::Message(json))
}

fn export_markdown(ctx: &CommandContext) -> Result<CommandResult> {
    let mut lines = vec![format!("# Session: {}", ctx.session_id), "".to_string()];
    for msg in &ctx.messages {
        let role = format_role(&msg.role);
        let content = msg
            .content
            .as_ref()
            .and_then(|c| c.as_text())
            .unwrap_or("");
        lines.push(format!("## {role}"));
        lines.push(String::new());
        lines.push(content.to_string());
        lines.push(String::new());
    }
    Ok(CommandResult::Message(lines.join("\n")))
}

fn format_role(role: &h_core::Role) -> String {
    match role {
        h_core::Role::System => "System".to_string(),
        h_core::Role::User => "User".to_string(),
        h_core::Role::Assistant => "Assistant".to_string(),
        h_core::Role::Tool => "Tool".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::sync::Notify;

    fn make_ctx() -> CommandContext {
        let msgs = vec![
            h_core::Message::user("hello"),
            h_core::Message::assistant("hi there"),
        ];
        CommandContext::new(
            Arc::new(h_core::SessionDB::new_in_memory().unwrap()),
            "sess-123".to_string(),
            msgs,
            h_core::ModelRef::new(
                h_core::ProviderId::new("test"),
                h_core::ModelId::new("test"),
            ),
            h_core::CostTracker::default(),
            Some(90),
            false,
            Arc::new(Notify::new()),
            h_core::HermesConfig::default(),
        )
    }

    #[tokio::test]
    async fn test_export_text() {
        let result = ExportCommand.execute("", &make_ctx()).await.unwrap();
        match result {
            CommandResult::Message(msg) => {
                assert!(msg.contains("User: hello"));
                assert!(msg.contains("Assistant: hi there"));
            }
            _ => panic!("Expected Message"),
        }
    }

    #[tokio::test]
    async fn test_export_json() {
        let result = ExportCommand.execute("json", &make_ctx()).await.unwrap();
        match result {
            CommandResult::Message(msg) => {
                assert!(msg.contains("hello"));
                assert!(msg.contains("assistant"));
            }
            _ => panic!("Expected Message"),
        }
    }

    #[tokio::test]
    async fn test_export_markdown() {
        let result = ExportCommand.execute("markdown", &make_ctx()).await.unwrap();
        match result {
            CommandResult::Message(msg) => {
                assert!(msg.contains("# Session"));
                assert!(msg.contains("## User"));
                assert!(msg.contains("hello"));
            }
            _ => panic!("Expected Message"),
        }
    }
}
