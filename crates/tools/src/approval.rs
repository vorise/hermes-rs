use regex::Regex;

/// Heuristic detection of dangerous terminal commands.
pub fn is_destructive_command(cmd: &str) -> bool {
    let patterns = [
        r"^\s*rm\s+(-[a-zA-Z]*[rf][a-zA-Z]*\s+)*",
        r"^\s*rmdir\s+",
        r"^\s*mv\s+.*\s+/dev/null",
        r"^\s*sed\s+(-[a-zA-Z]*i[a-zA-Z]*\s+)",
        r"^\s*truncate\s+",
        r"^\s*dd\s+",
        r"^\s*shred\s+",
        r"^\s*git\s+reset\s+--hard",
        r"^\s*git\s+clean\s+(-[a-zA-Z]*[dfx][a-zA-Z]*\s+)*",
        r"^\s*git\s+checkout\s+.*--force",
        r">\s*/",
        r">\s*~",
        r"^\s*chmod\s+777\s+/",
        r"^\s*mkfs",
        r"^\s*fdisk",
        r"^\s*:\s*\{",
    ];

    for pattern in &patterns {
        if let Ok(re) = Regex::new(pattern) {
            if re.is_match(cmd) {
                return true;
            }
        }
    }

    false
}

/// Classify a command's risk level.
#[derive(Debug, PartialEq, Eq)]
pub enum CommandRisk {
    Safe,
    Warning,
    Dangerous,
}

pub fn classify_command(cmd: &str) -> CommandRisk {
    if is_destructive_command(cmd) {
        return CommandRisk::Dangerous;
    }

    let warning_patterns = [
        r"^\s*sudo\s+",
        r"^\s*rm\s+",
        r"^\s*mv\s+",
        r"^\s*cp\s+.*\s+/tmp",
        r"^\s*kill\s+",
        r"^\s*pkill\s+",
        r"^\s*killall\s+",
    ];

    for pattern in &warning_patterns {
        if let Ok(re) = Regex::new(pattern) {
            if re.is_match(cmd) {
                return CommandRisk::Warning;
            }
        }
    }

    CommandRisk::Safe
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_destructive_rm() {
        assert!(is_destructive_command("rm -rf /tmp/test"));
        assert!(is_destructive_command("rm -r /tmp/test"));
    }

    #[test]
    fn test_destructive_git_reset() {
        assert!(is_destructive_command("git reset --hard HEAD"));
    }

    #[test]
    fn test_safe_command() {
        assert!(!is_destructive_command("ls -la"));
        assert!(!is_destructive_command("cat file.txt"));
        assert!(!is_destructive_command("echo hello"));
    }

    #[test]
    fn test_classify_dangerous() {
        assert_eq!(classify_command("rm -rf /tmp"), CommandRisk::Dangerous);
    }

    #[test]
    fn test_classify_warning() {
        assert_eq!(classify_command("sudo apt update"), CommandRisk::Warning);
    }

    #[test]
    fn test_classify_safe() {
        assert_eq!(classify_command("ls -la"), CommandRisk::Safe);
    }
}
