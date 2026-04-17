/// Nudge system for memory and skill awareness during agent conversation.
///
/// The agent is periodically nudged to:
/// - Review and consolidate memories (memory nudge)
/// - Create skills from novel/complex tasks (skill nudge)
///
/// Nudges are injected as system prompt additions at configurable intervals.

/// Configuration for the nudge system.
#[derive(Debug, Clone)]
pub struct NudgeConfig {
    /// Turns between memory nudges. 0 = disabled.
    pub memory_nudge_interval: u32,
    /// Tool iterations between skill nudges. 0 = disabled.
    pub skill_nudge_interval: u32,
}

impl Default for NudgeConfig {
    fn default() -> Self {
        Self {
            memory_nudge_interval: 10,
            skill_nudge_interval: 5,
        }
    }
}

/// Tracks nudge state and generates nudge prompts.
#[derive(Debug, Clone)]
pub struct NudgeSystem {
    config: NudgeConfig,
    /// Number of turns since last memory nudge.
    turns_since_memory_nudge: u32,
    /// Number of tool iterations since last skill nudge.
    tool_iterations_since_skill_nudge: u32,
    /// Total tool iterations (for skill nudge tracking).
    total_tool_iterations: u32,
}

impl NudgeSystem {
    pub fn new(config: NudgeConfig) -> Self {
        Self {
            config,
            turns_since_memory_nudge: 0,
            tool_iterations_since_skill_nudge: 0,
            total_tool_iterations: 0,
        }
    }

    /// Build a nudge system from HermesConfig.
    pub fn from_hermes_config(config: &crate::config::HermesConfig) -> Self {
        let mem_cfg = config.memory.clone().unwrap_or_default();
        let nudge_config = NudgeConfig {
            memory_nudge_interval: mem_cfg.memory_nudge_interval.unwrap_or(10),
            skill_nudge_interval: mem_cfg.skill_nudge_interval.unwrap_or(5),
        };
        Self::new(nudge_config)
    }

    /// Record a new conversation turn.
    ///
    /// Returns a nudge prompt if memory nudge is due.
    pub fn record_turn(&mut self, memory_count: usize) -> Option<String> {
        self.turns_since_memory_nudge += 1;

        if self.config.memory_nudge_interval > 0
            && self.turns_since_memory_nudge >= self.config.memory_nudge_interval
        {
            self.turns_since_memory_nudge = 0;
            Some(self.memory_nudge_prompt(memory_count))
        } else {
            None
        }
    }

    /// Record tool usage iterations.
    ///
    /// Returns a nudge prompt if skill nudge is due.
    pub fn record_tool_iterations(&mut self, tools_used: &[&str]) -> Option<String> {
        self.total_tool_iterations += 1;
        self.tool_iterations_since_skill_nudge += 1;

        if self.config.skill_nudge_interval > 0
            && self.tool_iterations_since_skill_nudge >= self.config.skill_nudge_interval
        {
            self.tool_iterations_since_skill_nudge = 0;
            self.skill_nudge_prompt(tools_used)
        } else {
            None
        }
    }

    /// Reset all nudge counters (e.g., after new session).
    pub fn reset(&mut self) {
        self.turns_since_memory_nudge = 0;
        self.tool_iterations_since_skill_nudge = 0;
        self.total_tool_iterations = 0;
    }

    /// Check if memory nudge is currently due.
    pub fn is_memory_nudge_due(&self) -> bool {
        self.config.memory_nudge_interval > 0
            && self.turns_since_memory_nudge >= self.config.memory_nudge_interval
    }

    /// Check if skill nudge is currently due.
    pub fn is_skill_nudge_due(&self) -> bool {
        self.config.skill_nudge_interval > 0
            && self.tool_iterations_since_skill_nudge >= self.config.skill_nudge_interval
    }

    /// Get turns remaining until next memory nudge.
    pub fn turns_until_memory_nudge(&self) -> u32 {
        self.config.memory_nudge_interval.saturating_sub(self.turns_since_memory_nudge)
    }

    /// Get tool iterations remaining until next skill nudge.
    pub fn iterations_until_skill_nudge(&self) -> u32 {
        self.config.skill_nudge_interval.saturating_sub(self.tool_iterations_since_skill_nudge)
    }

    fn memory_nudge_prompt(&self, memory_count: usize) -> String {
        format!(
            "MEMORY REVIEW: You have {memory_count} stored memories. \
             Consider whether any should be updated, consolidated, or deleted. \
             Create new memories from significant insights in this conversation."
        )
    }

    fn skill_nudge_prompt(&self, tools_used: &[&str]) -> Option<String> {
        if tools_used.is_empty() {
            return None;
        }

        let tool_list = tools_used.iter().take(5).cloned().collect::<Vec<_>>().join(", ");
        Some(format!(
            "SKILL CREATION OPPORTUNITY: You've used tools ({tool_list}) across \
             {} tool iteration(s). If you've completed a novel or complex task, \
             consider creating a skill to capture the procedure for future reuse.",
            self.total_tool_iterations
        ))
    }

    /// Get nudge status summary for display.
    pub fn status_summary(&self) -> Vec<String> {
        let mut lines = Vec::new();

        if self.config.memory_nudge_interval > 0 {
            lines.push(format!(
                "  Memory nudge: every {} turns ({} until next)",
                self.config.memory_nudge_interval,
                self.turns_until_memory_nudge()
            ));
        } else {
            lines.push("  Memory nudge: disabled".to_string());
        }

        if self.config.skill_nudge_interval > 0 {
            lines.push(format!(
                "  Skill nudge: every {} tool iterations ({} until next)",
                self.config.skill_nudge_interval,
                self.iterations_until_skill_nudge()
            ));
        } else {
            lines.push("  Skill nudge: disabled".to_string());
        }

        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_nudge_fires_at_interval() {
        let mut nudge = NudgeSystem::new(NudgeConfig {
            memory_nudge_interval: 3,
            skill_nudge_interval: 0,
        });

        assert!(nudge.record_turn(5).is_none());
        assert!(nudge.record_turn(5).is_none());
        let prompt = nudge.record_turn(5);
        assert!(prompt.is_some());
        assert!(prompt.unwrap().contains("MEMORY REVIEW"));
    }

    #[test]
    fn test_skill_nudge_fires_at_interval() {
        let mut nudge = NudgeSystem::new(NudgeConfig {
            memory_nudge_interval: 0,
            skill_nudge_interval: 2,
        });

        assert!(nudge.record_tool_iterations(&["read_file"]).is_none());
        let prompt = nudge.record_tool_iterations(&["read_file", "terminal"]);
        assert!(prompt.is_some());
        assert!(prompt.unwrap().contains("SKILL CREATION"));
    }

    #[test]
    fn test_disabled_nudges_never_fire() {
        let mut nudge = NudgeSystem::new(NudgeConfig {
            memory_nudge_interval: 0,
            skill_nudge_interval: 0,
        });

        for _ in 0..100 {
            assert!(nudge.record_turn(10).is_none());
            assert!(nudge.record_tool_iterations(&["tool"]).is_none());
        }
    }

    #[test]
    fn test_reset_clears_counters() {
        let mut nudge = NudgeSystem::new(NudgeConfig {
            memory_nudge_interval: 3,
            skill_nudge_interval: 2,
        });

        nudge.record_turn(5);
        nudge.record_turn(5);
        nudge.record_tool_iterations(&["tool"]);

        nudge.reset();

        assert!(!nudge.is_memory_nudge_due());
        assert!(!nudge.is_skill_nudge_due());
        assert_eq!(nudge.turns_until_memory_nudge(), 3);
        assert_eq!(nudge.iterations_until_skill_nudge(), 2);
    }

    #[test]
    fn test_status_summary() {
        let nudge = NudgeSystem::new(NudgeConfig {
            memory_nudge_interval: 10,
            skill_nudge_interval: 5,
        });

        let summary = nudge.status_summary();
        assert_eq!(summary.len(), 2);
        assert!(summary[0].contains("10 turns"));
        assert!(summary[1].contains("5 tool iterations"));
    }

    #[test]
    fn test_skill_nudge_empty_tools_returns_none() {
        let mut nudge = NudgeSystem::new(NudgeConfig {
            memory_nudge_interval: 0,
            skill_nudge_interval: 1,
        });

        let prompt = nudge.record_tool_iterations(&[]);
        assert!(prompt.is_none());
    }

    #[test]
    fn test_from_hermes_config() {
        use crate::config::{HermesConfig, MemoryConfig};
        let config = HermesConfig {
            memory: Some(MemoryConfig {
                enabled: Some(true),
                honcho_url: None,
                memory_nudge_interval: Some(7),
                skill_nudge_interval: Some(3),
            }),
            ..HermesConfig::default()
        };

        let nudge = NudgeSystem::from_hermes_config(&config);
        assert_eq!(nudge.config.memory_nudge_interval, 7);
        assert_eq!(nudge.config.skill_nudge_interval, 3);
    }
}
