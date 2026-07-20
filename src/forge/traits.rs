use anyhow::Result;
use std::collections::HashMap;

use crate::ci::CheckRunInfo;
use crate::forge::model::*;

/// Forge-neutral seam over the concrete forge clients (GitHub, GitLab, Gitea).
///
/// Every method mirrors the existing inherent method on each concrete client
/// so the trait can be introduced without changing behavior.
#[allow(async_fn_in_trait)]
pub trait Forge {
    async fn find_open_pr_by_head(&self, branch: &str) -> Result<Option<PrInfoWithHead>>;
    async fn find_pr(&self, branch: &str) -> Result<Option<PrInfo>>;
    async fn list_open_prs_by_head(&self) -> Result<HashMap<String, PrInfoWithHead>>;
    async fn list_open_pull_requests(&self, limit: u8) -> Result<Vec<RepoPrListItem>>;
    async fn list_open_issues(&self, limit: u8) -> Result<Vec<RepoIssueListItem>>;
    async fn create_pr(
        &self,
        head: &str,
        base: &str,
        title: &str,
        body: &str,
        is_draft: bool,
    ) -> Result<PrInfo>;
    async fn get_pr(&self, number: u64) -> Result<PrInfo>;
    async fn get_pr_with_head(&self, number: u64) -> Result<PrInfoWithHead>;
    async fn update_pr_base(&self, number: u64, new_base: &str) -> Result<()>;
    async fn set_pr_draft(&self, number: u64, is_draft: bool) -> Result<()>;
    async fn enqueue_pr(&self, number: u64) -> Result<EnqueueResult>;
    async fn update_pr_branch(&self, number: u64) -> Result<()>;
    async fn update_pr_title(&self, number: u64, title: &str) -> Result<()>;
    async fn update_pr_body(&self, number: u64, body: &str) -> Result<()>;
    async fn get_pr_body(&self, number: u64) -> Result<String>;
    async fn update_stack_comment(&self, number: u64, stack_comment: &str) -> Result<()>;
    async fn create_stack_comment(&self, number: u64, stack_comment: &str) -> Result<()>;
    async fn create_issue_comment(&self, number: u64, body: &str) -> Result<()>;
    async fn close_pr(&self, number: u64) -> Result<()>;
    async fn delete_stack_comment(&self, number: u64) -> Result<()>;
    async fn list_all_comments(&self, number: u64) -> Result<Vec<PrComment>>;
    async fn merge_pr(
        &self,
        number: u64,
        method: MergeMethod,
        commit_title: Option<&str>,
        sha: Option<&str>,
    ) -> Result<()>;
    async fn get_pr_merge_status(&self, number: u64) -> Result<PrMergeStatus>;
    async fn get_pr_review_decision(&self, number: u64) -> Result<Option<String>>;
    async fn is_pr_merged(&self, number: u64) -> Result<bool>;
    async fn get_pr_head_sha(&self, number: u64) -> Result<String>;
    async fn fetch_checks(
        &self,
        repo: &crate::git::GitRepo,
        sha: &str,
    ) -> Result<(Option<String>, Vec<CheckRunInfo>)>;
    async fn request_reviewers(&self, number: u64, reviewers: &[String]) -> Result<()>;
    async fn get_requested_reviewers(&self, number: u64) -> Result<Vec<String>>;
    async fn add_labels(&self, number: u64, labels: &[String]) -> Result<()>;
    async fn add_assignees(&self, number: u64, assignees: &[String]) -> Result<()>;
    async fn get_current_user(&self) -> Result<String>;
    async fn get_user_open_prs(&self, username: &str) -> Result<Vec<OpenPrInfo>>;
    async fn get_recent_merged_prs(&self, hours: i64, username: &str) -> Result<Vec<PrActivity>>;
    async fn get_recent_opened_prs(&self, hours: i64, username: &str) -> Result<Vec<PrActivity>>;
    async fn get_reviews_received(&self, hours: i64, username: &str)
    -> Result<Vec<ReviewActivity>>;
    async fn get_reviews_given(&self, hours: i64, username: &str) -> Result<Vec<ReviewActivity>>;
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[derive(Default)]
    pub(crate) struct FakeForge {
        pub(crate) open_pr_by_head: Option<PrInfoWithHead>,
    }

    impl Forge for FakeForge {
        async fn find_open_pr_by_head(&self, _branch: &str) -> Result<Option<PrInfoWithHead>> {
            Ok(self.open_pr_by_head.clone())
        }
        async fn find_pr(&self, _branch: &str) -> Result<Option<PrInfo>> {
            Ok(Some(PrInfo {
                number: 7,
                state: "OPEN".to_string(),
                is_draft: false,
                base: "main".to_string(),
            }))
        }
        async fn list_open_prs_by_head(&self) -> Result<HashMap<String, PrInfoWithHead>> {
            Ok(HashMap::new())
        }
        async fn list_open_pull_requests(&self, _limit: u8) -> Result<Vec<RepoPrListItem>> {
            anyhow::bail!("unused in fake")
        }
        async fn list_open_issues(&self, _limit: u8) -> Result<Vec<RepoIssueListItem>> {
            anyhow::bail!("unused in fake")
        }
        async fn create_pr(
            &self,
            _head: &str,
            _base: &str,
            _title: &str,
            _body: &str,
            _is_draft: bool,
        ) -> Result<PrInfo> {
            anyhow::bail!("unused in fake")
        }
        async fn get_pr(&self, _number: u64) -> Result<PrInfo> {
            anyhow::bail!("unused in fake")
        }
        async fn get_pr_with_head(&self, _number: u64) -> Result<PrInfoWithHead> {
            anyhow::bail!("unused in fake")
        }
        async fn update_pr_base(&self, _number: u64, _new_base: &str) -> Result<()> {
            anyhow::bail!("unused in fake")
        }
        async fn set_pr_draft(&self, _number: u64, _is_draft: bool) -> Result<()> {
            anyhow::bail!("unused in fake")
        }
        async fn enqueue_pr(&self, _number: u64) -> Result<EnqueueResult> {
            anyhow::bail!("unused in fake")
        }
        async fn update_pr_branch(&self, _number: u64) -> Result<()> {
            anyhow::bail!("unused in fake")
        }
        async fn update_pr_title(&self, _number: u64, _title: &str) -> Result<()> {
            anyhow::bail!("unused in fake")
        }
        async fn update_pr_body(&self, _number: u64, _body: &str) -> Result<()> {
            anyhow::bail!("unused in fake")
        }
        async fn get_pr_body(&self, _number: u64) -> Result<String> {
            anyhow::bail!("unused in fake")
        }
        async fn update_stack_comment(&self, _number: u64, _stack_comment: &str) -> Result<()> {
            anyhow::bail!("unused in fake")
        }
        async fn create_stack_comment(&self, _number: u64, _stack_comment: &str) -> Result<()> {
            anyhow::bail!("unused in fake")
        }
        async fn create_issue_comment(&self, _number: u64, _body: &str) -> Result<()> {
            anyhow::bail!("unused in fake")
        }
        async fn close_pr(&self, _number: u64) -> Result<()> {
            anyhow::bail!("unused in fake")
        }
        async fn delete_stack_comment(&self, _number: u64) -> Result<()> {
            anyhow::bail!("unused in fake")
        }
        async fn list_all_comments(&self, _number: u64) -> Result<Vec<PrComment>> {
            anyhow::bail!("unused in fake")
        }
        async fn merge_pr(
            &self,
            _number: u64,
            _method: MergeMethod,
            _commit_title: Option<&str>,
            _sha: Option<&str>,
        ) -> Result<()> {
            anyhow::bail!("unused in fake")
        }
        async fn get_pr_merge_status(&self, _number: u64) -> Result<PrMergeStatus> {
            anyhow::bail!("unused in fake")
        }
        async fn get_pr_review_decision(&self, _number: u64) -> Result<Option<String>> {
            anyhow::bail!("unused in fake")
        }
        async fn is_pr_merged(&self, _number: u64) -> Result<bool> {
            anyhow::bail!("unused in fake")
        }
        async fn get_pr_head_sha(&self, _number: u64) -> Result<String> {
            anyhow::bail!("unused in fake")
        }
        async fn fetch_checks(
            &self,
            _repo: &crate::git::GitRepo,
            _sha: &str,
        ) -> Result<(Option<String>, Vec<CheckRunInfo>)> {
            anyhow::bail!("unused in fake")
        }
        async fn request_reviewers(&self, _number: u64, _reviewers: &[String]) -> Result<()> {
            anyhow::bail!("unused in fake")
        }
        async fn get_requested_reviewers(&self, _number: u64) -> Result<Vec<String>> {
            anyhow::bail!("unused in fake")
        }
        async fn add_labels(&self, _number: u64, _labels: &[String]) -> Result<()> {
            anyhow::bail!("unused in fake")
        }
        async fn add_assignees(&self, _number: u64, _assignees: &[String]) -> Result<()> {
            anyhow::bail!("unused in fake")
        }
        async fn get_current_user(&self) -> Result<String> {
            Ok("fake-user".to_string())
        }
        async fn get_user_open_prs(&self, _username: &str) -> Result<Vec<OpenPrInfo>> {
            anyhow::bail!("unused in fake")
        }
        async fn get_recent_merged_prs(
            &self,
            _hours: i64,
            _username: &str,
        ) -> Result<Vec<PrActivity>> {
            anyhow::bail!("unused in fake")
        }
        async fn get_recent_opened_prs(
            &self,
            _hours: i64,
            _username: &str,
        ) -> Result<Vec<PrActivity>> {
            anyhow::bail!("unused in fake")
        }
        async fn get_reviews_received(
            &self,
            _hours: i64,
            _username: &str,
        ) -> Result<Vec<ReviewActivity>> {
            anyhow::bail!("unused in fake")
        }
        async fn get_reviews_given(
            &self,
            _hours: i64,
            _username: &str,
        ) -> Result<Vec<ReviewActivity>> {
            anyhow::bail!("unused in fake")
        }
    }

    async fn probe<F: Forge>(f: &F) -> Result<()> {
        let user = f.get_current_user().await?;
        assert_eq!(user, "fake-user");
        let pr = f.find_pr("feature").await?;
        assert_eq!(pr.map(|p| p.number), Some(7));
        Ok(())
    }

    #[test]
    fn fake_forge_satisfies_seam() {
        let fake = FakeForge::default();
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(probe(&fake))
            .unwrap();
    }
}
