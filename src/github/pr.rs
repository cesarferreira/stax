use anyhow::{Context, Result};
use octocrab::params::pulls::Sort;

use super::GitHubClient;
use crate::remote::RemoteInfo;

#[derive(Debug)]
pub struct PrInfo {
    pub number: u64,
    pub state: String,
    pub title: String,
    pub is_draft: bool,
}

impl GitHubClient {
    /// Find existing PR for a branch
    pub async fn find_pr(&self, branch: &str) -> Result<Option<PrInfo>> {
        let prs = self
            .octocrab
            .pulls(&self.owner, &self.repo)
            .list()
            .head(format!("{}:{}", self.owner, branch))
            .sort(Sort::Created)
            .send()
            .await
            .context("Failed to list PRs")?;

        if let Some(pr) = prs.items.first() {
            Ok(Some(PrInfo {
                number: pr.number,
                state: pr.state.as_ref().map(|s| format!("{:?}", s)).unwrap_or_default(),
                title: pr.title.clone().unwrap_or_default(),
                is_draft: pr.draft.unwrap_or(false),
            }))
        } else {
            Ok(None)
        }
    }

    /// Create a new PR
    pub async fn create_pr(
        &self,
        branch: &str,
        base: &str,
        title: &str,
        body: &str,
        draft: bool,
    ) -> Result<PrInfo> {
        let pr = self
            .octocrab
            .pulls(&self.owner, &self.repo)
            .create(title, branch, base)
            .body(body)
            .draft(Some(draft))
            .send()
            .await
            .context("Failed to create PR")?;

        Ok(PrInfo {
            number: pr.number,
            state: pr.state.as_ref().map(|s| format!("{:?}", s)).unwrap_or_default(),
            title: pr.title.clone().unwrap_or_default(),
            is_draft: pr.draft.unwrap_or(false),
        })
    }

    /// Get a PR by number
    pub async fn get_pr(&self, pr_number: u64) -> Result<PrInfo> {
        let pr = self
            .octocrab
            .pulls(&self.owner, &self.repo)
            .get(pr_number)
            .await
            .context("Failed to get PR")?;

        Ok(PrInfo {
            number: pr.number,
            state: pr.state.as_ref().map(|s| format!("{:?}", s)).unwrap_or_default(),
            title: pr.title.clone().unwrap_or_default(),
            is_draft: pr.draft.unwrap_or(false),
        })
    }

    /// Update PR base branch
    pub async fn update_pr_base(&self, pr_number: u64, new_base: &str) -> Result<()> {
        self.octocrab
            .pulls(&self.owner, &self.repo)
            .update(pr_number)
            .base(new_base)
            .send()
            .await
            .context("Failed to update PR base")?;
        Ok(())
    }

    /// Add or update the stack comment on a PR
    pub async fn update_stack_comment(
        &self,
        pr_number: u64,
        stack_comment: &str,
    ) -> Result<()> {
        let comments = self
            .octocrab
            .issues(&self.owner, &self.repo)
            .list_comments(pr_number)
            .send()
            .await
            .context("Failed to list comments")?;

        // Look for existing stax comment
        let marker = "<!-- stax-stack-comment -->";
        let full_comment = format!("{}\n{}", marker, stack_comment);

        for comment in comments.items {
            if comment.body.as_ref().map(|b| b.contains(marker)).unwrap_or(false) {
                // Update existing comment
                self.octocrab
                    .issues(&self.owner, &self.repo)
                    .update_comment(comment.id, &full_comment)
                    .await
                    .context("Failed to update comment")?;
                return Ok(());
            }
        }

        // Create new comment
        self.octocrab
            .issues(&self.owner, &self.repo)
            .create_comment(pr_number, &full_comment)
            .await
            .context("Failed to create comment")?;

        Ok(())
    }

    pub async fn request_reviewers(&self, pr_number: u64, reviewers: &[String]) -> Result<()> {
        if reviewers.is_empty() {
            return Ok(());
        }

        self.octocrab
            .pulls(&self.owner, &self.repo)
            .request_reviews(pr_number, reviewers.to_vec(), Vec::<String>::new())
            .await
            .context("Failed to request reviewers")?;

        Ok(())
    }

    pub async fn add_labels(&self, pr_number: u64, labels: &[String]) -> Result<()> {
        if labels.is_empty() {
            return Ok(());
        }

        self.octocrab
            .issues(&self.owner, &self.repo)
            .add_labels(pr_number, &labels.to_vec())
            .await
            .context("Failed to add labels")?;

        Ok(())
    }

    pub async fn add_assignees(&self, pr_number: u64, assignees: &[String]) -> Result<()> {
        if assignees.is_empty() {
            return Ok(());
        }

        let assignees_refs: Vec<&str> = assignees.iter().map(|s| s.as_str()).collect();
        self.octocrab
            .issues(&self.owner, &self.repo)
            .add_assignees(pr_number, &assignees_refs)
            .await
            .context("Failed to add assignees")?;

        Ok(())
    }
}

/// PR info for stack comment generation
#[derive(Debug, Clone)]
pub struct StackPrInfo {
    pub branch: String,
    pub pr_number: Option<u64>,
    pub pr_title: Option<String>,
}

/// Generate the stack comment body in freephite style
pub fn generate_stack_comment(
    prs: &[StackPrInfo],
    current_pr_number: u64,
    remote: &RemoteInfo,
    trunk: &str,
) -> String {
    let mut lines = vec![
        "Current dependencies on/for this PR:".to_string(),
        "".to_string(),
        format!("- **{}**:", trunk),
    ];

    // Build stack from bottom (trunk-adjacent) to top (leaf)
    // First PR is closest to trunk, last is the leaf
    for (i, pr_info) in prs.iter().enumerate() {
        let is_current = pr_info.pr_number == Some(current_pr_number);
        let pointer = if is_current { " 👈" } else { "" };

        // Use title if available, otherwise format branch name
        let title = pr_info.pr_title.clone().unwrap_or_else(|| {
            pr_info
                .branch
                .split('/')
                .next_back()
                .unwrap_or(&pr_info.branch)
                .replace(['-', '_'], " ")
        });

        let pr_text = match pr_info.pr_number {
            Some(num) => {
                let url = remote.pr_url(num);
                format!(
                    "{} ᛘ [{}]({}){}",
                    remote.provider.pr_label(),
                    title,
                    url,
                    pointer
                )
            }
            None => format!("`{}`{}", pr_info.branch, pointer),
        };

        // Indent based on position in stack
        let indent = if i == 0 { "  - " } else { "    - " };
        lines.push(format!("{}{}", indent, pr_text));
    }

    lines.push("".to_string());
    lines.push("---".to_string());
    lines.push("*This comment was autogenerated by [stax](https://github.com/cesarferreira/stax)*".to_string());

    lines.join("\n")
}
