use std::collections::HashSet;

use crate::skills::Skill;

/// Safety check result for a skill.
#[derive(Debug, Clone)]
pub struct SkillCheck {
    /// Skill being checked.
    pub skill_id: String,
    /// Whether the skill passed all checks.
    pub passed: bool,
    /// Warning messages (non-blocking advisories).
    pub warnings: Vec<String>,
    /// Blocking error messages (prevent execution).
    pub errors: Vec<String>,
}

impl SkillCheck {
    fn new(skill_id: &str) -> Self {
        Self {
            skill_id: skill_id.to_string(),
            passed: true,
            warnings: Vec::new(),
            errors: Vec::new(),
        }
    }

    fn warn(&mut self, msg: &str) {
        self.warnings.push(msg.to_string());
    }

    fn error(&mut self, msg: &str) {
        self.errors.push(msg.to_string());
        self.passed = false;
    }
}

/// Patterns that indicate a skill may be attempting to execute shell commands.
const DANGEROUS_PATTERNS: &[&str] = &[
    "subprocess",
    "os.system",
    "os.popen",
    "`curl ",
    "`wget ",
    "exec(",
    "eval(",
    "import subprocess",
    "import os",
    "import sys",
    "ctypes",
    "dlopen",
];

/// Validate a skill for safety before enabling or installing it.
///
/// This checks the skill content for potentially dangerous patterns
/// and validates metadata consistency.
pub fn validate_skill_safety(skill: &Skill) -> SkillCheck {
    let mut check = SkillCheck::new(&skill.id);

    // Content must not be empty
    if skill.content.is_empty() {
        check.error("Skill content is empty");
        return check;
    }

    // Check for dangerous patterns
    let content_lower = skill.content.to_lowercase();
    for pattern in DANGEROUS_PATTERNS {
        if content_lower.contains(pattern) {
            check.warn(&format!(
                "Skill contains potentially dangerous pattern: '{pattern}'"
            ));
        }
    }

    // Check content size (warn if very large)
    let content_len = skill.content.len();
    if content_len > 50_000 {
        check.warn(&format!(
            "Skill content is very large ({content_len} bytes). Large skills may impact token usage."
        ));
    }

    // Check for recursive skill references (skills that try to install other skills)
    if content_lower.contains("/skills install") || content_lower.contains("skill_install") {
        check.warn("Skill references installing other skills — this could create dependency chains");
    }

    // Check for API key patterns (skills shouldn't hardcode credentials)
    if content_lower.contains("sk-ant-") || content_lower.contains("sk-") || content_lower.contains("api_key") {
        check.error("Skill appears to contain API keys or credentials");
    }

    // Check for file system paths that look suspicious
    if content_lower.contains("/etc/passwd") || content_lower.contains("/etc/shadow") {
        check.error("Skill references sensitive system files");
    }

    // Check for network exfiltration patterns
    if content_lower.contains("curl -d") || content_lower.contains("curl --data") || content_lower.contains("requests.post") {
        check.warn("Skill may attempt to send data to external endpoints");
    }

    check
}

/// Check for conflicting skills (skills with overlapping names or descriptions).
pub fn detect_skill_conflicts(skills: &[&Skill]) -> Vec<(String, String, String)> {
    let mut conflicts = Vec::new();
    let mut seen_names: HashSet<String> = HashSet::new();
    let mut seen_descs: HashSet<String> = HashSet::new();

    for skill in skills {
        let name_key = skill.name.to_lowercase();
        let desc_key = skill.description.to_lowercase();

        if seen_names.contains(&name_key) {
            conflicts.push((
                skill.id.clone(),
                "name".to_string(),
                format!("Duplicate skill name: '{}'", skill.name),
            ));
        }
        seen_names.insert(name_key);

        // Check for identical descriptions (potential copy-paste or duplicates)
        if !desc_key.is_empty() && seen_descs.contains(&desc_key) {
            conflicts.push((
                skill.id.clone(),
                "description".to_string(),
                format!("Duplicate skill description for: '{}'", skill.name),
            ));
        }
        seen_descs.insert(desc_key);
    }

    conflicts
}

/// Run all safety checks on a batch of skills.
///
/// Returns a list of checks, one per skill, plus any cross-skill conflicts.
pub fn run_skill_checks(skills: &[&Skill]) -> (Vec<SkillCheck>, Vec<(String, String, String)>) {
    let checks: Vec<SkillCheck> = skills.iter().map(|s| validate_skill_safety(s)).collect();
    let conflicts = detect_skill_conflicts(skills);
    (checks, conflicts)
}

/// Check if a skill's content references tool calls that may not be available.
///
/// Returns a list of tool names mentioned by the skill that aren't in the available set.
pub fn check_skill_tool_requirements(skill: &Skill, available_tools: &HashSet<&str>) -> Vec<String> {
    let mut missing = Vec::new();
    // Look for tool call patterns like `use_tool("name")`, `call: name`, or backtick-enclosed tool names
    let content_lower = skill.content.to_lowercase();

    // Common tool name patterns
    let known_hermes_tools = [
        "read_file", "write_file", "patch", "search_files", "grep",
        "terminal", "web_search", "web_extract", "memory", "todo",
        "session_search", "delegate", "skills_list", "skill_view",
        "tts", "vision", "transcribe", "home_assistant",
        "image_gen", "cron", "mixture",
    ];

    for tool_name in &known_hermes_tools {
        if content_lower.contains(tool_name) && !available_tools.contains(tool_name) {
            missing.push(tool_name.to_string());
        }
    }

    missing
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_skill(id: &str, content: &str) -> Skill {
        Skill {
            id: id.to_string(),
            name: id.to_string(),
            description: format!("Skill {id}"),
            version: "1.0.0".to_string(),
            author: "test".to_string(),
            content: content.to_string(),
            path: Default::default(),
            enabled: true,
        }
    }

    #[test]
    fn test_safe_skill_passes() {
        let skill = make_skill("github-auth", "This skill helps you authenticate with GitHub.\n\nUse the `gh` CLI when available.");
        let check = validate_skill_safety(&skill);
        assert!(check.passed);
        assert!(check.warnings.is_empty());
        assert!(check.errors.is_empty());
    }

    #[test]
    fn test_dangerous_pattern_warned() {
        let skill = make_skill("risky", "To run this, use: subprocess.run(['ls'])");
        let check = validate_skill_safety(&skill);
        assert!(check.passed); // warnings don't block
        assert!(check.warnings.iter().any(|w| w.contains("subprocess")));
    }

    #[test]
    fn test_api_key_in_skill_blocked() {
        let skill = make_skill("leaky", "Use this API key: sk-ant-abc123 to make calls.");
        let check = validate_skill_safety(&skill);
        assert!(!check.passed);
        assert!(check.errors.iter().any(|e| e.contains("API keys")));
    }

    #[test]
    fn test_sensitive_file_blocked() {
        let skill = make_skill("sensitive", "Read /etc/passwd to get user list.");
        let check = validate_skill_safety(&skill);
        assert!(!check.passed);
    }

    #[test]
    fn test_empty_skill_blocked() {
        let skill = make_skill("empty", "");
        let check = validate_skill_safety(&skill);
        assert!(!check.passed);
        assert!(check.errors.iter().any(|e| e.contains("empty")));
    }

    #[test]
    fn test_large_skill_warned() {
        let content = "a".repeat(60_000);
        let skill = make_skill("large", &content);
        let check = validate_skill_safety(&skill);
        assert!(check.passed);
        assert!(check.warnings.iter().any(|w| w.contains("very large")));
    }

    #[test]
    fn test_skill_conflicts_duplicate_name() {
        let s1 = make_skill("github-auth", "Auth skill v1");
        let s2 = Skill {
            id: "github-auth-v2".to_string(),
            name: "github-auth".to_string(), // Same name
            description: "Auth skill v2".to_string(),
            version: "2.0.0".to_string(),
            author: "test".to_string(),
            content: "Auth skill v2".to_string(),
            path: Default::default(),
            enabled: true,
        };
        let conflicts = detect_skill_conflicts(&[&s1, &s2]);
        assert_eq!(conflicts.len(), 1);
        assert!(conflicts[0].2.contains("Duplicate skill name"));
    }

    #[test]
    fn test_skill_tool_requirements() {
        let skill = make_skill("deploy", "Use read_file to check the config, then use terminal to deploy.");
        let available: HashSet<&str> = ["read_file", "terminal", "web_search"].into_iter().collect();
        let missing = check_skill_tool_requirements(&skill, &available);
        assert!(missing.is_empty()); // Both tools are available
    }

    #[test]
    fn test_skill_tool_requirements_missing() {
        let skill = make_skill("deploy", "Use read_file and vision to inspect the project.");
        let available: HashSet<&str> = ["read_file"].into_iter().collect();
        let missing = check_skill_tool_requirements(&skill, &available);
        assert!(missing.contains(&"vision".to_string()));
    }

    #[test]
    fn test_run_batch_checks() {
        let s1 = make_skill("safe", "Do something helpful.");
        let s2 = make_skill("risky", "Use subprocess to run commands.");
        let s3 = make_skill("bad", "Key: sk-ant-secret");
        let (checks, _conflicts) = run_skill_checks(&[&s1, &s2, &s3]);
        assert_eq!(checks.len(), 3);
        assert!(checks[0].passed);
        assert!(checks[1].passed); // warning only
        assert!(!checks[2].passed); // error
    }
}
