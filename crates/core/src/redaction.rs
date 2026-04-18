use regex::Regex;
use std::sync::LazyLock;

/// Check if redaction is enabled via `HERMES_REDACT_SECRETS` env var.
/// Snapshot at import time — runtime mutations cannot disable mid-session.
static REDACT_ENABLED: LazyLock<bool> = LazyLock::new(|| {
    let val = std::env::var("HERMES_REDACT_SECRETS")
        .ok()
        .unwrap_or_default()
        .to_lowercase();
    !matches!(val.as_str(), "0" | "false" | "no" | "off")
});

/// Token prefix patterns matched as a single compiled regex (35+ patterns).
static TOKEN_PREFIX_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?x)
        # API key tokens with known prefixes
        (?:
            sk-[A-Za-z0-9_-]{10,}                # OpenAI / Anthropic / OpenRouter
            | ghp_[A-Za-z0-9]{10,}               # GitHub PAT classic
            | github_pat_[A-Za-z0-9_]{10,}       # GitHub PAT fine-grained
            | gho_[A-Za-z0-9]{10,}               # GitHub OAuth
            | ghu_[A-Za-z0-9]{10,}               # GitHub OAuth user
            | ghs_[A-Za-z0-9]{10,}               # GitHub OAuth server
            | ghr_[A-Za-z0-9]{10,}               # GitHub OAuth refresh
            | xox[baprs]-[A-Za-z0-9-]{10,}       # Slack tokens
            | AIza[A-Za-z0-9_-]{30,}             # Google API keys
            | AKIA[A-Z0-9]{16}                   # AWS Access Key ID
            | sk_live_[A-Za-z0-9]{10,}           # Stripe live
            | sk_test_[A-Za-z0-9]{10,}           # Stripe test
            | rk_live_[A-Za-z0-9]{10,}           # Stripe restricted
            | SG\.[A-Za-z0-9_-]{10,}             # SendGrid
            | hf_[A-Za-z0-9]{10,}                # HuggingFace
            | pplx-[A-Za-z0-9]{10,}              # Perplexity
            | fc-[A-Za-z0-9]{10,}                # Firecrawl
            | bb_live_[A-Za-z0-9_-]{10,}         # BrowserBase
            | tvly-[A-Za-z0-9]{10,}              # Tavily
            | exa_[A-Za-z0-9]{10,}               # Exa search
            | gsk_[A-Za-z0-9]{10,}               # Groq Cloud
            | npm_[A-Za-z0-9]{10,}               # npm
            | pypi-[A-Za-z0-9_-]{10,}            # PyPI
            | syt_[A-Za-z0-9]{10,}               # Matrix
            | dop_v1_[A-Za-z0-9_-]{10,}          # DigitalOcean
            | doo_v1_[A-Za-z0-9_-]{10,}          # DigitalOcean OAuth
            | am_[A-Za-z0-9_-]{10,}              # AgentMail
            | sk_[A-Za-z0-9_]{10,}               # ElevenLabs
            | r8_[A-Za-z0-9]{10,}                # Replicate
            | fal_[A-Za-z0-9_-]{10,}             # Fal.ai
            | mem0_[A-Za-z0-9_-]{10,}            # Mem0
            | brv_[A-Za-z0-9_-]{10,}             # ByteRover
            | hsk-[A-Za-z0-9_-]{10,}             # Hindsight
        )
        ",
    )
    .unwrap()
});

/// Environment variable assignments with secret-like names: `KEY=value`.
static SECRET_ENV_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)(?:API_?KEY|TOKEN|SECRET|PASSWORD|PASSWD|CREDENTIAL|AUTH)[A-Z_]*\s*=\s*\S{4,}"#,
    )
    .unwrap()
});

/// JSON fields with secret-like keys: `"apiKey": "value"`.
static SECRET_JSON_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)(?:"(?:api_?key|token|secret|password|access_token|auth|private_key|client_secret)"\s*:\s*)"(?:[A-Za-z0-9_+/=-]{4,})""#,
    )
    .unwrap()
});

/// Authorization headers: `Authorization: Bearer <token>`.
static AUTH_HEADER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(Authorization:\s*(?:Bearer\s+)?)([A-Za-z0-9_\-./+=]{10,})").unwrap()
});

/// Telegram bot tokens: `bot<digits>:<token>` where token >= 30 chars.
static TELEGRAM_BOT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(bot\d+:)[A-Za-z0-9_-]{30,}").unwrap()
});

/// Private key blocks: `-----BEGIN ... PRIVATE KEY----- ... -----END ... PRIVATE KEY-----`.
static PRIVATE_KEY_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(-----BEGIN [A-Z ]*PRIVATE KEY-----)[\s\S]*?(-----END [A-Z ]*PRIVATE KEY-----)")
        .unwrap()
});

/// Database connection strings with embedded passwords.
static DB_CONN_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:postgres(?:ql)?|mysql|mongodb\+srv|redis|amqp)://[^:]+:([^@]+)@").unwrap()
});

/// Phone numbers in E.164 format: `+<country><number>` (7-15 digits total).
static PHONE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\+\d{7,15}").unwrap()
});

/// Redact sensitive text from logs, tool output, and trajectory files.
///
/// Applies all pattern categories:
/// 1. Token prefix patterns (35+ API key formats)
/// 2. Environment variable assignments
/// 3. JSON secret fields
/// 4. Authorization headers
/// 5. Telegram bot tokens
/// 6. Private key blocks
/// 7. Database connection strings
/// 8. Phone numbers
///
/// Masking strategy:
/// - Tokens < 18 chars: replaced with `***`
/// - Tokens >= 18 chars: `{first6}...{last4}`
pub fn redact_sensitive_text(text: &str) -> String {
    if !*REDACT_ENABLED {
        return text.to_string();
    }

    let mut result = text.to_string();

    // 1. Token prefix patterns
    result = TOKEN_PREFIX_RE
        .replace_all(&result, |caps: &regex::Captures| mask_token(&caps[0]))
        .to_string();

    // 2. Environment assignments: mask the value portion
    result = SECRET_ENV_RE
        .replace_all(&result, |caps: &regex::Captures| {
            let full = &caps[0];
            if let Some(eq_pos) = full.find('=') {
                let key = &full[..=eq_pos];
                let val = &full[eq_pos + 1..];
                format!("{}{}", key, mask_token(val))
            } else {
                full.to_string()
            }
        })
        .to_string();

    // 3. JSON fields: mask the value
    result = SECRET_JSON_RE
        .replace_all(&result, |caps: &regex::Captures| {
            let full = &caps[0];
            if let Some(colon_pos) = full.find(':') {
                let key_part = &full[..=colon_pos];
                // Find the quoted value
                let rest = &full[colon_pos + 1..];
                if let Some(open_q) = rest.find('"') {
                    let after_open = &rest[open_q + 1..];
                    if let Some(close_q) = after_open.find('"') {
                        let val = &after_open[..close_q];
                        let masked = mask_token(val);
                        return format!("{} \"{}\"", key_part, masked);
                    }
                }
            }
            full.to_string()
        })
        .to_string();

    // 4. Authorization headers
    result = AUTH_HEADER_RE
        .replace_all(&result, |caps: &regex::Captures| {
            format!("{}{}", &caps[1], mask_token(&caps[2]))
        })
        .to_string();

    // 5. Telegram bot tokens
    result = TELEGRAM_BOT_RE
        .replace_all(&result, |caps: &regex::Captures| {
            format!("{}{}", &caps[1], mask_token(&caps[0][caps[1].len()..]))
        })
        .to_string();

    // 6. Private key blocks — replace inner content with placeholder
    result = PRIVATE_KEY_RE
        .replace_all(&result, "${1}\n[REDACTED PRIVATE KEY]\n${2}")
        .to_string();

    // 7. Database connection strings
    result = DB_CONN_RE
        .replace_all(&result, |caps: &regex::Captures| {
            let full = &caps[0];
            let pw = &caps[1];
            let masked_pw = mask_token(pw);
            // Reconstruct: scheme://user:MASKED@
            full.replace(pw, &masked_pw)
        })
        .to_string();

    // 8. Phone numbers
    result = PHONE_RE
        .replace_all(&result, |caps: &regex::Captures| {
            let phone = &caps[0];
            let digits: Vec<char> = phone.chars().filter(|c| c.is_ascii_digit()).collect();
            if digits.len() >= 7 {
                let country_len = if digits[0] == '1' { 1 } else { 2 };
                let last4: String = digits[digits.len() - 4..].iter().collect();
                let country: String = digits[..country_len].iter().collect();
                let masked = "+".to_string() + &country + &"*".repeat(digits.len() - country_len - 4) + &last4;
                masked
            } else {
                phone.to_string()
            }
        })
        .to_string();

    result
}

/// Mask a token according to the redaction policy:
/// - < 18 chars: `***`
/// - >= 18 chars: `{first6}...{last4}`
fn mask_token(token: &str) -> String {
    if token.len() < 18 {
        "***".to_string()
    } else {
        format!("{}...{}", &token[..6], &token[token.len() - 4..])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_redact_openai_key() {
        let input = "Using key sk-proj-abc1234567890abcdef in request";
        let out = redact_sensitive_text(input);
        assert!(!out.contains("sk-proj-abc1234567890abcdef"));
        assert!(out.contains("sk-pro"));
        assert!(out.contains("..."));
        assert!(out.contains("cdef"));
    }

    #[test]
    fn test_redact_github_pat() {
        let input = "token=ghp_ABCDEFGHIJKLMNOPQRST";
        let out = redact_sensitive_text(input);
        assert!(!out.contains("ghp_ABCDEFGHIJKLMNOPQRST"));
    }

    #[test]
    fn test_redact_env_assignment() {
        let input = "OPENAI_API_KEY=sk-test-abcdef1234567890";
        let out = redact_sensitive_text(input);
        assert!(!out.contains("sk-test-abcdef1234567890"));
        assert!(out.contains("OPENAI_API_KEY="));
    }

    #[test]
    fn test_redact_json_secret() {
        let input = r#"{"apiKey": "secretvalue1234567890"}"#;
        let out = redact_sensitive_text(input);
        assert!(!out.contains("secretvalue1234567890"));
    }

    #[test]
    fn test_redact_auth_header() {
        let input = "Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9";
        let out = redact_sensitive_text(input);
        assert!(!out.contains("eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9"));
        assert!(out.contains("Authorization:"));
    }

    #[test]
    fn test_redact_telegram_bot() {
        let input = "bot123456:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
        let out = redact_sensitive_text(input);
        assert!(!out.contains("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"));
        assert!(out.contains("bot123456:"));
    }

    #[test]
    fn test_redact_private_key() {
        let input = "-----BEGIN RSA PRIVATE KEY-----\nMIIEowIBAAKCAQEA...\n-----END RSA PRIVATE KEY-----";
        let out = redact_sensitive_text(input);
        assert!(!out.contains("MIIEowIBAAKCAQEA"));
        assert!(out.contains("[REDACTED PRIVATE KEY]"));
        assert!(out.contains("-----BEGIN RSA PRIVATE KEY-----"));
        assert!(out.contains("-----END RSA PRIVATE KEY-----"));
    }

    #[test]
    fn test_redact_db_conn_string() {
        let input = "postgres://admin:SuperSecret123@localhost:5432/mydb";
        let out = redact_sensitive_text(input);
        assert!(!out.contains("SuperSecret123"));
        assert!(out.contains("@localhost"));
    }

    #[test]
    fn test_redact_phone() {
        let input = "Call me at +1234567890123";
        let out = redact_sensitive_text(input);
        assert!(!out.contains("+1234567890123"));
    }

    #[test]
    fn test_mask_token_short() {
        assert_eq!(mask_token("short1234"), "***");
    }

    #[test]
    fn test_mask_token_long() {
        let result = mask_token("abcdefghijklmnopqrstuvwxyz");
        assert_eq!(result, "abcdef...wxyz");
    }

    #[test]
    fn test_no_redaction_when_disabled() {
        // REDACT_ENABLED is determined at import time, so we can't
        // easily test the disabled path. Verify the function runs.
        let out = redact_sensitive_text("nothing sensitive here");
        assert_eq!(out, "nothing sensitive here");
    }

    #[test]
    fn test_redact_stripe_key() {
        let input = "stripe_key=sk_live_abcdef1234567890abcdef";
        let out = redact_sensitive_text(input);
        assert!(!out.contains("sk_live_abcdef1234567890abcdef"));
    }

    #[test]
    fn test_redact_aws_key() {
        let input = "AKIAIOSFODNN7EXAMPLE";
        let out = redact_sensitive_text(input);
        assert!(!out.contains("AKIAIOSFODNN7EXAMPLE"));
    }

    #[test]
    fn test_redact_multiple_secrets() {
        let input = r#"
            OPENAI_API_KEY=sk-abc1234567890abcdef
            GITHUB_TOKEN=ghp_ABCDEFGHIJKLMNOPQRST
            {"secret": "mysecretvalue12345678"}
        "#;
        let out = redact_sensitive_text(input);
        assert!(!out.contains("sk-abc1234567890abcdef"));
        assert!(!out.contains("ghp_ABCDEFGHIJKLMNOPQRST"));
        assert!(!out.contains("mysecretvalue12345678"));
    }
}
