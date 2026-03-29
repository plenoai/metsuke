use rmcp::model::ErrorData;

/// Validate a GitHub owner or repo name.
/// Allows only alphanumeric, hyphens, underscores, and dots.
/// Rejects path traversal attempts.
pub fn validate_github_name(value: &str, field: &str) -> Result<(), ErrorData> {
    if value.is_empty() || value.len() > 100 {
        return Err(ErrorData::invalid_params(
            format!("{field} must be 1-100 characters"),
            None,
        ));
    }
    if !value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        return Err(ErrorData::invalid_params(
            format!("{field} contains invalid characters (allowed: a-z, 0-9, -, _, .)"),
            None,
        ));
    }
    if value.starts_with('.') || value.contains("..") {
        return Err(ErrorData::invalid_params(
            format!("{field} must not start with '.' or contain '..'"),
            None,
        ));
    }
    Ok(())
}

/// Validate that a policy name is a known preset (not a file path).
const KNOWN_PRESETS: &[&str] = &[
    "default", "oss", "aiops", "soc1", "soc2", "slsa-l1", "slsa-l2", "slsa-l3", "slsa-l4",
];

pub fn validate_policy(value: &str) -> Result<(), ErrorData> {
    if !KNOWN_PRESETS.contains(&value) {
        return Err(ErrorData::invalid_params(
            format!(
                "Unknown policy: {value}. Available: {}",
                KNOWN_PRESETS.join(", ")
            ),
            None,
        ));
    }
    Ok(())
}

/// Validate a git reference (branch, tag, SHA).
/// Rejects git metacharacters per `git check-ref-format` rules.
pub fn validate_git_ref(value: &str) -> Result<(), ErrorData> {
    if value.is_empty() || value.len() > 256 {
        return Err(ErrorData::invalid_params(
            "reference must be 1-256 characters".to_string(),
            None,
        ));
    }
    if value.contains("..") || value.contains('\0') {
        return Err(ErrorData::invalid_params(
            "reference contains invalid characters".to_string(),
            None,
        ));
    }
    // Reject git revision operators and metacharacters
    if value
        .chars()
        .any(|c| matches!(c, ' ' | '~' | '^' | ':' | '?' | '*' | '[' | '\\'))
    {
        return Err(ErrorData::invalid_params(
            "reference contains git metacharacters (space, ~, ^, :, ?, *, [, \\)".to_string(),
            None,
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_github_names() {
        assert!(validate_github_name("octocat", "owner").is_ok());
        assert!(validate_github_name("my-repo_v2", "repo").is_ok());
        assert!(validate_github_name("a.b", "repo").is_ok());
    }

    #[test]
    fn rejects_path_traversal() {
        assert!(validate_github_name("../../etc", "owner").is_err());
        assert!(validate_github_name("..secret", "owner").is_err());
        assert!(validate_github_name(".hidden", "owner").is_err());
    }

    #[test]
    fn rejects_special_chars() {
        assert!(validate_github_name("owner/repo", "owner").is_err());
        assert!(validate_github_name("owner repo", "owner").is_err());
        assert!(validate_github_name("", "owner").is_err());
    }

    #[test]
    fn valid_policies() {
        assert!(validate_policy("default").is_ok());
        assert!(validate_policy("soc2").is_ok());
        assert!(validate_policy("slsa-l4").is_ok());
    }

    #[test]
    fn rejects_file_paths_as_policy() {
        assert!(validate_policy("../../../../etc/passwd").is_err());
        assert!(validate_policy("/tmp/evil.rego").is_err());
        assert!(validate_policy("custom").is_err());
    }

    #[test]
    fn valid_git_refs() {
        assert!(validate_git_ref("HEAD").is_ok());
        assert!(validate_git_ref("main").is_ok());
        assert!(validate_git_ref("v1.0.0").is_ok());
    }

    #[test]
    fn rejects_bad_refs() {
        assert!(validate_git_ref("").is_err());
        assert!(validate_git_ref("a..b").is_err());
    }

    #[test]
    fn rejects_git_metacharacters() {
        assert!(validate_git_ref("main branch").is_err(), "space");
        assert!(validate_git_ref("HEAD~1").is_err(), "tilde");
        assert!(validate_git_ref("HEAD^2").is_err(), "caret");
        assert!(validate_git_ref("refs:hack").is_err(), "colon");
        assert!(validate_git_ref("ref?glob").is_err(), "question mark");
        assert!(validate_git_ref("ref*glob").is_err(), "asterisk");
        assert!(validate_git_ref("ref[0]").is_err(), "bracket");
        assert!(validate_git_ref("ref\\path").is_err(), "backslash");
    }

    #[test]
    fn github_name_boundary_length() {
        let name_100 = "a".repeat(100);
        assert!(validate_github_name(&name_100, "repo").is_ok());
        let name_101 = "a".repeat(101);
        assert!(validate_github_name(&name_101, "repo").is_err());
    }

    #[test]
    fn git_ref_boundary_length() {
        let ref_256 = "a".repeat(256);
        assert!(validate_git_ref(&ref_256).is_ok());
        let ref_257 = "a".repeat(257);
        assert!(validate_git_ref(&ref_257).is_err());
    }
}
