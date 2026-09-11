use super::OperationWarning;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BranchNameContext {
    pub format: Option<String>,
    pub prefix: Option<String>,
    pub legacy_date: bool,
    pub date_format: String,
    pub replacement: String,
    /// Lowercase the generated branch name. Only affects the branch name —
    /// callers keep the original text for commit messages.
    pub lowercase: bool,
    pub user: Option<String>,
    pub date: chrono::NaiveDate,
}

impl BranchNameContext {
    pub(crate) fn literal() -> Self {
        Self {
            format: None,
            prefix: None,
            legacy_date: false,
            date_format: "%Y-%m-%d".into(),
            replacement: "-".into(),
            lowercase: true,
            user: None,
            date: chrono::NaiveDate::from_ymd_opt(1970, 1, 1).unwrap(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BranchNameResult {
    pub name: String,
    pub warnings: Vec<OperationWarning>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BranchNameError {
    Empty,
    MissingMessagePlaceholder { format: String },
    InvalidRef { candidate: String },
}

pub(crate) fn format_branch_name(
    input: &str,
    context: &BranchNameContext,
) -> Result<BranchNameResult, BranchNameError> {
    let message = sanitize_segment(input, context);
    let mut candidate = if let Some(format) = &context.format {
        if !format.contains("{message}") {
            return Err(BranchNameError::MissingMessagePlaceholder {
                format: format.clone(),
            });
        }
        apply_format_template(format, &message, context)
    } else {
        let mut name = message;
        if context.legacy_date {
            name = format!(
                "{}{}{}",
                context.date.format("%Y-%m-%d"),
                replacement_char(&context.replacement),
                name
            );
        }
        apply_prefix(name, context.prefix.as_deref())
    };

    if context.lowercase {
        candidate = candidate.to_lowercase();
    }

    if candidate.is_empty() {
        return Err(BranchNameError::Empty);
    }
    if !git2::Reference::is_valid_name(&format!("refs/heads/{candidate}")) {
        return Err(BranchNameError::InvalidRef { candidate });
    }

    let warnings = if candidate == input {
        Vec::new()
    } else {
        vec![OperationWarning::BranchNameNormalized {
            original: input.to_string(),
            normalized: std::mem::take(&mut candidate),
        }]
    };
    let name = match warnings.first() {
        Some(OperationWarning::BranchNameNormalized { normalized, .. }) => normalized.clone(),
        _ => candidate,
    };
    Ok(BranchNameResult { name, warnings })
}

fn apply_format_template(template: &str, message: &str, context: &BranchNameContext) -> String {
    let mut result = template.replace("{message}", message);
    if result.contains("{date}") {
        result = result.replace(
            "{date}",
            &context.date.format(&context.date_format).to_string(),
        );
    }
    let user = context
        .user
        .as_deref()
        .map(|user| sanitize_segment(user, context))
        .unwrap_or_default();
    if result.contains("{user}") {
        result = result.replace("{user}", &user);
    }
    while result.contains("//") {
        result = result.replace("//", "/");
    }
    result = result.trim_matches('/').to_string();
    // Dedupe a duplicated leading user segment: when the message already begins
    // with the user prefix (e.g. `{user}/{message}` + message "cesar/foo"), the
    // substitution above yields "cesar/cesar/foo". Collapse the leading
    // "<user>/<user>/" back to "<user>/" so the `{user}` template stays
    // idempotent, matching how `apply_prefix` refuses to double an explicit
    // prefix.
    if !user.is_empty()
        && let Some(rest) = result.strip_prefix(&format!("{user}/{user}/"))
    {
        result = format!("{user}/{rest}");
    }
    apply_prefix(result, context.prefix.as_deref())
}

fn apply_prefix(mut name: String, prefix: Option<&str>) -> String {
    let Some(prefix) = prefix.and_then(non_empty_trimmed) else {
        return name;
    };
    let prefix = normalize_prefix_override(prefix);
    if !name.starts_with(&prefix) {
        name = format!("{prefix}{name}");
    }
    name
}

/// Sanitize a segment and apply the configured casing. Lowercasing the segments
/// (not just the assembled name) keeps the `{user}` dedupe below comparable.
fn sanitize_segment(segment: &str, context: &BranchNameContext) -> String {
    let sanitized = sanitize_branch_segment(segment, &context.replacement);
    if context.lowercase {
        sanitized.to_lowercase()
    } else {
        sanitized
    }
}

fn sanitize_branch_segment(segment: &str, replacement: &str) -> String {
    let replacement = replacement_char(replacement);
    let mut result = String::with_capacity(segment.len());
    let mut previous_was_replacement = false;
    for character in segment.chars() {
        let next = if character.is_alphanumeric()
            || character == '-'
            || character == '_'
            || character == '/'
        {
            character
        } else {
            replacement
        };
        if next == replacement {
            if previous_was_replacement {
                continue;
            }
            previous_was_replacement = true;
        } else {
            previous_was_replacement = false;
        }
        result.push(next);
    }
    result
        .trim_start_matches(replacement)
        .trim_end_matches(replacement)
        .to_string()
}

fn replacement_char(replacement: &str) -> char {
    replacement.chars().next().unwrap_or('-')
}

fn non_empty_trimmed(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

fn normalize_prefix_override(prefix: &str) -> String {
    if prefix.ends_with('/') || prefix.ends_with('-') || prefix.ends_with('_') {
        prefix.to_string()
    } else {
        format!("{prefix}/")
    }
}

#[cfg(test)]
mod tests {
    use super::{BranchNameContext, BranchNameError, format_branch_name};
    use crate::application::OperationWarning;

    #[test]
    fn format_branch_name_is_pure_and_returns_normalization_warning() {
        let context = BranchNameContext {
            format: Some("{user}/{message}".into()),
            prefix: None,
            legacy_date: false,
            date_format: "%Y-%m-%d".into(),
            replacement: "-".into(),
            lowercase: true,
            user: Some("César Ferreira".into()),
            date: chrono::NaiveDate::from_ymd_opt(2026, 7, 12).unwrap(),
        };
        let result = format_branch_name("  Fix GUI!  ", &context).unwrap();
        assert_eq!(result.name, "césar-ferreira/fix-gui");
        assert_eq!(
            result.warnings,
            vec![OperationWarning::BranchNameNormalized {
                original: "  Fix GUI!  ".into(),
                normalized: "césar-ferreira/fix-gui".into(),
            }]
        );
    }

    #[test]
    fn format_branch_name_preserves_case_when_lowercase_is_disabled() {
        let context = BranchNameContext {
            format: Some("{user}/{message}".into()),
            prefix: None,
            legacy_date: false,
            date_format: "%Y-%m-%d".into(),
            replacement: "-".into(),
            lowercase: false,
            user: Some("César Ferreira".into()),
            date: chrono::NaiveDate::from_ymd_opt(2026, 7, 12).unwrap(),
        };
        let result = format_branch_name("  Fix GUI!  ", &context).unwrap();
        assert_eq!(result.name, "César-Ferreira/Fix-GUI");
    }

    #[test]
    fn format_branch_name_lowercases_prefix_and_template_literals() {
        let context = BranchNameContext {
            format: None,
            prefix: Some("Cesar/Feature".into()),
            legacy_date: false,
            date_format: "%Y-%m-%d".into(),
            replacement: "-".into(),
            lowercase: true,
            user: None,
            date: chrono::NaiveDate::from_ymd_opt(2026, 7, 12).unwrap(),
        };
        let result = format_branch_name("Bump Fastlane Version", &context).unwrap();
        assert_eq!(result.name, "cesar/feature/bump-fastlane-version");
    }

    #[test]
    fn format_branch_name_dedupes_leading_user_segment_across_case() {
        let context = BranchNameContext {
            format: Some("{user}/{message}".into()),
            prefix: None,
            legacy_date: false,
            date_format: "%Y-%m-%d".into(),
            replacement: "-".into(),
            lowercase: true,
            user: Some("Cesar".into()),
            date: chrono::NaiveDate::from_ymd_opt(2026, 7, 12).unwrap(),
        };
        let result = format_branch_name("cesar/Fix Thing", &context).unwrap();
        assert_eq!(result.name, "cesar/fix-thing");
    }

    #[test]
    fn format_branch_name_dedupes_leading_user_segment() {
        let context = BranchNameContext {
            format: Some("{user}/{message}".into()),
            prefix: None,
            legacy_date: false,
            date_format: "%Y-%m-%d".into(),
            replacement: "-".into(),
            lowercase: true,
            user: Some("cesar".into()),
            date: chrono::NaiveDate::from_ymd_opt(2026, 7, 12).unwrap(),
        };
        // Message already leads with the user prefix — must not double it.
        let result = format_branch_name("cesar/fix-thing", &context).unwrap();
        assert_eq!(result.name, "cesar/fix-thing");
        assert!(result.warnings.is_empty());
        // A bare message still gets the single user prefix.
        let bare = format_branch_name("fix-thing", &context).unwrap();
        assert_eq!(bare.name, "cesar/fix-thing");
        // A message segment that merely starts with the user text (not the
        // full `user/` segment) is left untouched.
        let similar = format_branch_name("cesarine", &context).unwrap();
        assert_eq!(similar.name, "cesar/cesarine");
    }

    #[test]
    fn format_branch_name_rejects_an_empty_normalized_ref() {
        let context = BranchNameContext::literal();
        let error = format_branch_name("!!!", &context).unwrap_err();
        assert_eq!(error, BranchNameError::Empty);
    }

    #[test]
    fn format_branch_name_rejects_an_invalid_git_ref() {
        let error = format_branch_name("///", &BranchNameContext::literal()).unwrap_err();
        assert_eq!(
            error,
            BranchNameError::InvalidRef {
                candidate: "///".into()
            }
        );
    }

    #[test]
    fn format_branch_name_preserves_unicode_alphanumeric_characters() {
        let result = format_branch_name("ação-日本語", &BranchNameContext::literal()).unwrap();
        assert_eq!(result.name, "ação-日本語");
    }
}
