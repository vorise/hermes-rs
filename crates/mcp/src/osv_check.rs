use serde::{Deserialize, Serialize};

const OSV_ENDPOINT: &str = "https://api.osv.dev/v1/query";
const OSV_TIMEOUT_SECS: u64 = 10;

/// Detected ecosystem from a package manager command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ecosystem {
    Npm,
    PyPi,
}

/// Parsed package identifier.
#[derive(Debug, Clone)]
pub struct PackageRef {
    pub name: String,
    pub version: Option<String>,
    pub ecosystem: Ecosystem,
}

/// OSV malware check result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MalwareCheckResult {
    /// Whether the package is clean (no MAL-* advisories).
    pub clean: bool,
    /// Error message if malware was found.
    pub error: Option<String>,
    /// List of MAL-* advisory IDs found.
    pub malware_advisories: Vec<String>,
}

/// Detect ecosystem from a command name.
fn detect_ecosystem(command: &str) -> Option<Ecosystem> {
    match command {
        "npx" | "npx.cmd" => Some(Ecosystem::Npm),
        "uvx" | "uvx.cmd" | "pipx" => Some(Ecosystem::PyPi),
        _ => None,
    }
}

/// Parse package name and version from command args.
///
/// Handles:
/// - `["package-name"]` → ("package-name", None)
/// - `["package-name@1.2.3"]` → ("package-name", Some("1.2.3"))
/// - `["--", "package-name"]` → ("package-name", None) (npx separator)
fn parse_package(args: &[String]) -> Option<(String, Option<String>)> {
    // Skip leading flags like "--"
    let pkg = args
        .iter()
        .find(|a| !a.starts_with('-') && !a.is_empty())?;

    if let Some(at_pos) = pkg.find('@') {
        // Only treat as version if there's something after @ and it's not the first char
        if at_pos > 0 && at_pos < pkg.len() - 1 {
            let name = pkg[..at_pos].to_string();
            let version = pkg[at_pos + 1..].to_string();
            Some((name, Some(version)))
        } else {
            Some((pkg.to_string(), None))
        }
    } else {
        Some((pkg.to_string(), None))
    }
}

/// Check a package command for malware advisories via OSV.
///
/// Returns `MalwareCheckResult` with `clean: false` if MAL-* advisories found,
/// `clean: true` if the package is clean or unknown.
/// Network errors, timeouts, and parse failures all return `clean: true` (fail-open).
pub async fn check_package_for_malware(
    command: &str,
    args: &[String],
) -> MalwareCheckResult {
    let ecosystem = match detect_ecosystem(command) {
        Some(e) => e,
        None => {
            // Unknown ecosystem — skip check, allow
            return MalwareCheckResult {
                clean: true,
                error: None,
                malware_advisories: Vec::new(),
            };
        }
    };

    let (name, version) = match parse_package(args) {
        Some((n, v)) => (n, v),
        None => {
            // No package name found — allow
            return MalwareCheckResult {
                clean: true,
                error: None,
                malware_advisories: Vec::new(),
            };
        }
    };

    let package_ref = PackageRef {
        name: name.clone(),
        version: version.clone(),
        ecosystem: ecosystem.clone(),
    };

    match query_osv(package_ref).await {
        Ok(result) => result,
        Err(e) => {
            // Fail-open: network errors, timeouts → allow
            tracing::debug!("OSV check failed (allowing {name}): {e}");
            MalwareCheckResult {
                clean: true,
                error: None,
                malware_advisories: Vec::new(),
            }
        }
    }
}

#[derive(Deserialize)]
struct OsvResponse {
    #[serde(default)]
    vulns: Option<Vec<OsvVuln>>,
}

#[derive(Deserialize)]
struct OsvVuln {
    id: String,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    aliases: Option<Vec<String>>,
}

async fn query_osv(package: PackageRef) -> Result<MalwareCheckResult, String> {
    let pkg_name = package.name.clone();
    let ecosystem_str = match package.ecosystem {
        Ecosystem::Npm => "npm",
        Ecosystem::PyPi => "PyPI",
    };

    let mut body = serde_json::json!({
        "package": {
            "name": package.name,
            "ecosystem": ecosystem_str,
        }
    });

    if let Some(ref version) = package.version {
        body["version"] = serde_json::Value::String(version.clone());
    }

    let client = reqwest::Client::new();
    let resp = client
        .post(OSV_ENDPOINT)
        .json(&body)
        .timeout(std::time::Duration::from_secs(OSV_TIMEOUT_SECS))
        .send()
        .await
        .map_err(|e| format!("OSV request failed: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("OSV returned status: {}", resp.status()));
    }

    let osv_resp: OsvResponse = resp
        .json()
        .await
        .map_err(|e| format!("OSV response parse error: {e}"))?;

    // Filter for MAL-* advisories only (confirmed malware)
    let malware: Vec<OsvVuln> = osv_resp
        .vulns
        .into_iter()
        .flatten()
        .filter(|v| v.id.starts_with("MAL-"))
        .collect();

    if malware.is_empty() {
        return Ok(MalwareCheckResult {
            clean: true,
            error: None,
            malware_advisories: Vec::new(),
        });
    }

    // Format error message (limit to first 3 advisories)
    let ids: Vec<&str> = malware.iter().take(3).map(|v| v.id.as_str()).collect();
    let ids_str = ids.join(", ");

    let summaries: Vec<String> = malware
        .iter()
        .take(3)
        .map(|v| {
            let s = v.summary.as_deref().unwrap_or(&v.id);
            s.chars().take(100).collect::<String>()
        })
        .collect();
    let summaries_str = summaries.join("; ");

    let ecosystem_display = match package.ecosystem {
        Ecosystem::Npm => "npm",
        Ecosystem::PyPi => "PyPI",
    };

    let error = format!(
        "BLOCKED: Package '{pkg_name}' ({ecosystem_display}) has known malware advisories: {ids_str} — {summaries_str}"
    );

    Ok(MalwareCheckResult {
        clean: false,
        error: Some(error),
        malware_advisories: ids.iter().map(|s| s.to_string()).collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_ecosystem() {
        assert_eq!(detect_ecosystem("npx"), Some(Ecosystem::Npm));
        assert_eq!(detect_ecosystem("npx.cmd"), Some(Ecosystem::Npm));
        assert_eq!(detect_ecosystem("uvx"), Some(Ecosystem::PyPi));
        assert_eq!(detect_ecosystem("uvx.cmd"), Some(Ecosystem::PyPi));
        assert_eq!(detect_ecosystem("pipx"), Some(Ecosystem::PyPi));
        assert_eq!(detect_ecosystem("cargo"), None);
    }

    #[test]
    fn test_parse_package_name_only() {
        let result = parse_package(&["some-package".to_string()]);
        assert_eq!(result, Some(("some-package".to_string(), None)));
    }

    #[test]
    fn test_parse_package_with_version() {
        let result = parse_package(&["some-package@1.2.3".to_string()]);
        assert_eq!(
            result,
            Some(("some-package".to_string(), Some("1.2.3".to_string())))
        );
    }

    #[test]
    fn test_parse_package_skip_flags() {
        let result = parse_package(&[
            "--".to_string(),
            "some-package".to_string(),
        ]);
        assert_eq!(result, Some(("some-package".to_string(), None)));
    }

    #[test]
    fn test_parse_package_empty_args() {
        let result = parse_package(&[]);
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_package_at_start_only() {
        let result = parse_package(&["@scope/pkg".to_string()]);
        assert_eq!(result, Some(("@scope/pkg".to_string(), None)));
    }

    #[test]
    fn test_malware_result_helpers() {
        let clean = MalwareCheckResult {
            clean: true,
            error: None,
            malware_advisories: Vec::new(),
        };
        assert!(clean.clean);
        assert!(clean.error.is_none());

        let blocked = MalwareCheckResult {
            clean: false,
            error: Some("BLOCKED".to_string()),
            malware_advisories: vec!["MAL-2024-001".to_string()],
        };
        assert!(!blocked.clean);
        assert!(blocked.error.is_some());
    }
}
