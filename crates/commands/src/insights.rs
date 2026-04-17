use anyhow::Result;
use async_trait::async_trait;

use crate::{CommandContext, CommandResult, SlashCommand};

/// /insights — Show usage analytics and insights.
pub struct InsightsCommand;

#[async_trait]
impl SlashCommand for InsightsCommand {
    fn name(&self) -> &str {
        "insights"
    }

    fn description(&self) -> &str {
        "Show usage analytics and insights"
    }

    fn category(&self) -> &str {
        "general"
    }

    async fn execute(&self, args: &str, ctx: &CommandContext) -> Result<CommandResult> {
        let days = parse_days_arg(args).unwrap_or(7);

        let c = &ctx.cost;
        let mut lines = vec![
            format!("Usage Insights (last {days} days):"),
            String::new(),
        ];

        // Query session database for historical data
        let hermes_home = h_core::home::hermes_home();
        let sessions_db_path = hermes_home.join("sessions.db");
        let historical_data = if sessions_db_path.exists() {
            match h_core::SessionDB::open(&sessions_db_path) {
                Ok(db) => Self::query_historical(&db, days),
                Err(e) => Some(format!("DB error: {e}")),
            }
        } else {
            None
        };

        // Current session stats
        lines.push("  Current Session:".to_string());
        lines.push(format!("    Messages:   {}", ctx.messages.len()));
        let user_turns = ctx.messages.iter().filter(|m| m.role == h_core::Role::User).count();
        let assistant_turns = ctx.messages.iter().filter(|m| m.role == h_core::Role::Assistant).count();
        let tool_calls: usize = ctx.messages.iter()
            .map(|m| m.tool_calls.as_ref().map(|t| t.len()).unwrap_or(0))
            .sum();
        lines.push(format!("    User turns: {user_turns}"));
        lines.push(format!("    Assistant:  {assistant_turns}"));
        lines.push(format!("    Tool calls: {tool_calls}"));
        lines.push(String::new());

        // Cost tracker stats
        lines.push("  Cost Tracker:".to_string());
        lines.push(format!("    Total tokens: {}", c.total_tokens()));
        lines.push(format!("    Input tokens:  {}", c.input_tokens));
        lines.push(format!("    Output tokens: {}", c.output_tokens));
        lines.push(format!("    Cache read:    {}", c.cache_read_tokens));
        lines.push(format!("    Cache write:   {}", c.cache_write_tokens));
        lines.push(format!("    Est. cost:     ${:.4}", c.estimated_cost_usd));
        lines.push(format!("    API calls:     {}", c.api_call_count));
        lines.push(String::new());

        // Historical session data
        lines.push("  Historical Sessions:".to_string());
        if let Some(ref err) = historical_data {
            lines.push(format!("    {err}"));
        } else {
            lines.push("    No historical session data available.".to_string());
            lines.push("    Past sessions will appear here after database queries are wired up.".to_string());
        }
        lines.push(String::new());

        // Model info
        lines.push(format!("  Model: {}/{}", ctx.model.provider, ctx.model.model));

        // Budget
        if let Some(budget) = ctx.budget_remaining {
            lines.push(format!("  Budget: {budget}% remaining"));
        }

        Ok(CommandResult::Message(lines.join("\n")))
    }
}

impl InsightsCommand {
    fn query_historical(db: &h_core::SessionDB, _days: usize) -> Option<String> {
        // Try to get session summaries
        match db.get_session_summaries(20) {
            Ok(summaries) if !summaries.is_empty() => {
                let total_sessions = summaries.len();
                let total_messages: i64 = summaries.iter().map(|s| s.message_count).sum();
                let total_cost: f64 = summaries.iter()
                    .filter_map(|s| s.estimated_cost_usd)
                    .sum();
                let sources: std::collections::BTreeMap<&str, usize> =
                    summaries.iter().fold(std::collections::BTreeMap::new(), |mut acc, s| {
                        *acc.entry(s.source.as_str()).or_insert(0) += 1;
                        acc
                    });

                let mut out = String::new();
                out.push_str(&format!("    {total_sessions} recent sessions, {total_messages} total messages"));
                out.push_str(&format!(", ${:.2} total cost", total_cost));
                out.push_str(&format!("\n    Sources: {}", sources.iter().map(|(k, v)| format!("{k}: {v}")).collect::<Vec<_>>().join(", ")));
                out.push_str("\n    Recent sessions:");
                for s in summaries.iter().take(5) {
                    let title = s.title.as_deref().unwrap_or("(untitled)");
                    out.push_str(&format!("\n      - {title} ({source}, {count} msgs)",
                        source = s.source, count = s.message_count));
                }
                Some(out)
            }
            Ok(_) => None,
            Err(_) => None,
        }
    }
}

fn parse_days_arg(args: &str) -> Option<usize> {
    let parts: Vec<&str> = args.split_whitespace().collect();
    for i in 0..parts.len() {
        if parts[i] == "--days" || parts[i] == "-d" {
            if let Some(next) = parts.get(i + 1) {
                return next.parse().ok();
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::sync::Notify;

    fn make_ctx() -> CommandContext {
        CommandContext::new(
            Arc::new(h_core::SessionDB::new_in_memory().unwrap()),
            "test".to_string(),
            vec![
                h_core::Message::user("hello"),
                h_core::Message::assistant("hi"),
            ],
            h_core::ModelRef::new(
                h_core::ProviderId::new("anthropic"),
                h_core::ModelId::new("claude-sonnet-4-6"),
            ),
            h_core::CostTracker {
                input_tokens: 5000,
                output_tokens: 3000,
                cache_read_tokens: 2000,
                cache_write_tokens: 1000,
                estimated_cost_usd: 0.1234,
                api_call_count: 5,
                ..h_core::CostTracker::default()
            },
            Some(85),
            false,
            Arc::new(Notify::new()),
            h_core::HermesConfig::default(),
        )
    }

    #[tokio::test]
    async fn test_insights_default_days() {
        let result = InsightsCommand.execute("", &make_ctx()).await.unwrap();
        match result {
            CommandResult::Message(msg) => {
                assert!(msg.contains("last 7 days"));
                assert!(msg.contains("Total tokens:"));
                assert!(msg.contains("Current Session"));
                assert!(msg.contains("Cost Tracker"));
            }
            _ => panic!("Expected Message"),
        }
    }

    #[tokio::test]
    async fn test_insights_custom_days() {
        let result = InsightsCommand
            .execute("--days 30", &make_ctx())
            .await
            .unwrap();
        match result {
            CommandResult::Message(msg) => assert!(msg.contains("last 30 days")),
            _ => panic!("Expected Message"),
        }
    }

    #[tokio::test]
    async fn test_insights_shows_session_info() {
        let result = InsightsCommand.execute("", &make_ctx()).await.unwrap();
        match result {
            CommandResult::Message(msg) => {
                assert!(msg.contains("User turns: 1"));
                assert!(msg.contains("Assistant:  1"));
                assert!(msg.contains("Budget: 85%"));
            }
            _ => panic!("Expected Message"),
        }
    }
}
