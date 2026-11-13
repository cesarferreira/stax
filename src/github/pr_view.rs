//! Data layer for `stax pr view`: the open pull requests that concern you, one
//! pull request read in full (checks, reviews, conversation), and what you can
//! do to it (merge, close, reopen, mark ready, approve, comment).
//!
//! Everything here is presentation-neutral. The parsers take raw GraphQL JSON
//! so they can be tested without the network; the fetchers go through the
//! repo-bound [`GitHubClient`], so auth, proxy and API-base handling are the
//! same as every other stax command.

use super::client::GitHubClient;
use crate::forge::MergeMethod;
use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use std::collections::HashSet;

/// One pull request: the repository (`owner/name`) and its number.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PrRef {
    pub repo: String,
    pub number: u64,
    /// Whether this lives on the checkout's own (validated) remote rather than\n    /// on github.com. Decides which API and token reach it.
    pub on_trusted: bool,
}

impl PrRef {
    /// Reads `12`, `#12`, `owner/repo#12` or a pull request link. A bare
    /// number belongs to `here`, the repository of the current folder.
    pub fn parse(target: &str, here: Option<&str>) -> Result<Self> {
        let target = target.trim();
        let bad = || {
            anyhow::anyhow!(
                "'{target}' isn't a pull request. Use a number (12), owner/repo#12, or a link \
                 like https://github.com/owner/repo/pull/12."
            )
        };

        if let Some(rest) = ["https://", "http://"]
            .iter()
            .find_map(|scheme| target.strip_prefix(scheme))
        {
            return parse_pull_url(rest).ok_or_else(bad);
        }

        if let Some((repo, number)) = target.split_once('#')
            && !repo.is_empty()
        {
            let number: u64 = number.parse().map_err(|_| bad())?;
            let mut parts = repo.split('/');
            let valid = matches!(
                (parts.next(), parts.next(), parts.next()),
                (Some(owner), Some(name), None) if !owner.is_empty() && !name.is_empty()
            );
            return if valid && number > 0 {
                Ok(PrRef {
                    on_trusted: here.is_some_and(|h| h.eq_ignore_ascii_case(repo)),
                    repo: repo.to_owned(),
                    number,
                })
            } else {
                Err(bad())
            };
        }

        let number: u64 = target.trim_start_matches('#').parse().map_err(|_| bad())?;
        if number == 0 {
            return Err(bad());
        }
        let repo = here.ok_or_else(|| {
            anyhow::anyhow!(
                "#{number} needs a repository, and this folder isn't a GitHub repository. \
                 Use owner/repo#{number} or the pull request's link."
            )
        })?;
        Ok(PrRef {
            on_trusted: true,
            repo: repo.to_owned(),
            number,
        })
    }

    pub fn label(&self) -> String {
        format!("{}#{}", self.repo, self.number)
    }

    pub fn owner_and_name(&self) -> (&str, &str) {
        self.repo.split_once('/').unwrap_or(("", &self.repo))
    }
}

/// `host/owner/repo/pull/N[/...]` (scheme already stripped) on the github.com
/// web host.
fn parse_pull_url(rest: &str) -> Option<PrRef> {
    let rest = rest
        .split(['?', '#'])
        .next()
        .unwrap_or_default()
        .trim_end_matches('/');
    let mut parts = rest.split('/');
    let host = parts.next()?;
    if !host.eq_ignore_ascii_case("github.com") && !host.eq_ignore_ascii_case("www.github.com") {
        return None;
    }
    let (owner, name, kind, number) = (parts.next()?, parts.next()?, parts.next()?, parts.next()?);
    if owner.is_empty() || name.is_empty() || kind != "pull" {
        return None;
    }
    let number: u64 = number.parse().ok().filter(|n| *n > 0)?;
    Some(PrRef {
        on_trusted: false,
        repo: format!("{owner}/{name}"),
        number,
    })
}

/// A row in the list of pull requests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub pr: PrRef,
    pub title: String,
    pub author: String,
    pub updated: String,
    pub draft: bool,
}

/// Pull requests listed under one heading.
#[derive(Debug, Clone)]
pub struct Group {
    pub title: String,
    pub entries: Vec<Entry>,
}

/// Where one heading of the list gets its pull requests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    Branch { repo: String, branch: String },
    Repo(String),
    Yours,
    Reviewing,
    Owned,
}

impl Source {
    pub fn title(&self) -> String {
        match self {
            Source::Branch { .. } => "This branch".to_owned(),
            Source::Repo(repo) => repo.clone(),
            Source::Yours => "Yours".to_owned(),
            Source::Reviewing => "Review requested".to_owned(),
            Source::Owned => "In your repositories".to_owned(),
        }
    }

    /// The GitHub search query for this heading. `viewer` is the signed-in
    /// login, used for the repositories you own.
    pub fn query(&self, viewer: &str) -> String {
        let filter = match self {
            Source::Branch { repo, branch } => format!("repo:{repo} head:{branch}"),
            Source::Repo(repo) => format!("repo:{repo}"),
            Source::Yours => "author:@me".to_owned(),
            Source::Reviewing => "review-requested:@me".to_owned(),
            Source::Owned => format!("user:{viewer}"),
        };
        format!("is:pr is:open archived:false {filter}")
    }
}

/// The list's headings, in order: this branch's pull request, this
/// repository's, yours, those waiting on your review, then the rest in
/// repositories you own. A default branch has no pull request of its own.
pub fn sources(here: Option<&str>, branch: Option<&str>, trunk: Option<&str>) -> Vec<Source> {
    let mut out = Vec::new();
    if let Some(repo) = here {
        if let Some(branch) = branch
            .filter(|b| !matches!(*b, "main" | "master"))
            .filter(|b| Some(*b) != trunk)
        {
            out.push(Source::Branch {
                repo: repo.to_owned(),
                branch: branch.to_owned(),
            });
        }
        out.push(Source::Repo(repo.to_owned()));
    }
    out.extend([Source::Yours, Source::Reviewing, Source::Owned]);
    out
}

/// Keeps each pull request under the first heading it appears in, newest
/// first within a heading.
pub fn dedupe(groups: Vec<Group>) -> Vec<Group> {
    let mut seen = HashSet::new();
    groups
        .into_iter()
        .map(|mut group| {
            group.entries.retain(|entry| seen.insert(entry.pr.clone()));
            group
                .entries
                .sort_by_key(|entry| std::cmp::Reverse(parse_time(&entry.updated)));
            group
        })
        .collect()
}

/// Whether `entry` matches every word of `filter` in its title, repository,
/// number or author, ignoring case.
pub fn matches_filter(entry: &Entry, filter: &str) -> bool {
    let haystack = format!(
        "{} {} {} {}",
        entry.title, entry.pr.repo, entry.pr.number, entry.author
    )
    .to_lowercase();
    filter
        .split_whitespace()
        .all(|word| haystack.contains(&word.trim_start_matches('#').to_lowercase()))
}

pub fn parse_time(text: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|t| t.with_timezone(&Utc))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Open,
    Draft,
    Merged,
    Closed,
}

impl State {
    pub fn label(self) -> &'static str {
        match self {
            State::Open => "Open",
            State::Draft => "Draft",
            State::Merged => "Merged",
            State::Closed => "Closed",
        }
    }
}

pub fn method_label(method: MergeMethod) -> &'static str {
    match method {
        MergeMethod::Squash => "Squash and merge",
        MergeMethod::Merge => "Merge commit",
        MergeMethod::Rebase => "Rebase and merge",
    }
}

pub fn method_key(method: MergeMethod) -> char {
    match method {
        MergeMethod::Squash => 's',
        MergeMethod::Merge => 'm',
        MergeMethod::Rebase => 'r',
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckStatus {
    Passed,
    Failed,
    Running,
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub name: String,
    pub status: CheckStatus,
    pub url: Option<String>,
}

/// What the checks add up to, in a few words.
pub fn describe_checks(checks: &[Check]) -> String {
    if checks.is_empty() {
        return "No checks".to_owned();
    }
    let total = checks.len();
    let count = |status| checks.iter().filter(|c| c.status == status).count();
    let (failed, running) = (count(CheckStatus::Failed), count(CheckStatus::Running));
    let mut parts = Vec::new();
    if running > 0 {
        parts.push(format!("{running} of {total} running"));
    }
    if failed > 0 {
        parts.push(if parts.is_empty() {
            format!("{failed} of {total} failing")
        } else {
            format!("{failed} failed")
        });
    }
    if parts.is_empty() {
        let passed = count(CheckStatus::Passed);
        return if passed + count(CheckStatus::Skipped) == total {
            "All checks passed".to_owned()
        } else {
            format!("{passed} of {total} passing")
        };
    }
    parts.join(" · ")
}

/// The worst of the checks: failing beats running beats passing.
pub fn checks_status(checks: &[Check]) -> Option<CheckStatus> {
    if checks.is_empty() {
        None
    } else if checks.iter().any(|c| c.status == CheckStatus::Failed) {
        Some(CheckStatus::Failed)
    } else if checks.iter().any(|c| c.status == CheckStatus::Running) {
        Some(CheckStatus::Running)
    } else {
        Some(CheckStatus::Passed)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Requested,
    Approved,
    ChangesRequested,
    Commented,
    Dismissed,
}

impl Verdict {
    fn parse(state: &str) -> Option<Self> {
        match state {
            "APPROVED" => Some(Verdict::Approved),
            "CHANGES_REQUESTED" => Some(Verdict::ChangesRequested),
            "COMMENTED" => Some(Verdict::Commented),
            "DISMISSED" => Some(Verdict::Dismissed),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Verdict::Requested => "review requested",
            Verdict::Approved => "approved",
            Verdict::ChangesRequested => "requested changes",
            Verdict::Commented => "commented",
            Verdict::Dismissed => "dismissed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reviewer {
    pub login: String,
    pub verdict: Verdict,
}

/// Whether the branch can go in, from GitHub's merge state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mergeability {
    Ready,
    Conflicts,
    Blocked,
    Behind,
    Unstable,
    Unknown,
}

impl Mergeability {
    fn parse(merge_state: &str, mergeable: &str) -> Self {
        match merge_state {
            "CLEAN" | "HAS_HOOKS" => Mergeability::Ready,
            "DIRTY" => Mergeability::Conflicts,
            "BLOCKED" => Mergeability::Blocked,
            "BEHIND" => Mergeability::Behind,
            "UNSTABLE" => Mergeability::Unstable,
            _ if mergeable == "CONFLICTING" => Mergeability::Conflicts,
            _ => Mergeability::Unknown,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Mergeability::Ready => "Ready to merge",
            Mergeability::Conflicts => "Has merge conflicts",
            Mergeability::Blocked => "Blocked: required reviews or checks are missing",
            Mergeability::Behind => "Behind the base branch",
            Mergeability::Unstable => "Mergeable, but checks are failing",
            Mergeability::Unknown => "Merge state unknown",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventKind {
    Opened,
    Commit { sha: String, headline: String },
    Comment { body: String },
    Review { verdict: Verdict, body: String },
    Inline { path: String, body: String },
    Merged,
    Closed,
}

/// Something that happened on the pull request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    pub at: String,
    pub actor: String,
    pub kind: EventKind,
}

/// A pull request read in full.
#[derive(Debug, Clone)]
pub struct Detail {
    pub pr: PrRef,
    pub title: String,
    pub body: String,
    pub state: State,
    pub author: String,
    pub updated: String,
    pub base: String,
    pub head: String,
    /// The head commit; a merge is pinned to it so a late push is never merged unseen.
    pub head_sha: String,
    pub additions: u64,
    pub deletions: u64,
    pub labels: Vec<String>,
    pub reviewers: Vec<Reviewer>,
    pub checks: Vec<Check>,
    pub mergeability: Mergeability,
    /// The merge methods the repository allows, in the order offered.
    pub methods: Vec<MergeMethod>,
    /// Whether you may merge, close and reopen (write access or above).
    pub can_write: bool,
    pub timeline: Vec<Event>,
}

/// Something done to a pull request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Merge(MergeMethod),
    Close,
    Reopen,
    Ready,
    Draft,
    Approve,
    Comment(String),
}

impl Action {
    /// What the footer says once it's done.
    pub fn done(&self, pr: &PrRef) -> String {
        let label = format!("#{}", pr.number);
        match self {
            Action::Merge(method) => {
                format!(
                    "{label} merged via {}",
                    method_label(*method).to_lowercase()
                )
            }
            Action::Close => format!("{label} closed"),
            Action::Reopen => format!("{label} reopened"),
            Action::Ready => format!("{label} is ready for review"),
            Action::Draft => format!("{label} is a draft again"),
            Action::Approve => format!("Approved {label}"),
            Action::Comment(_) => format!("Commented on {label}"),
        }
    }
}

// ---------------------------------------------------------------------------
// Fetching
// ---------------------------------------------------------------------------

/// One GraphQL search; the same query serves every heading.
const SEARCH: &str = "query($q: String!) { search(query: $q, type: ISSUE, first: 100) { nodes { \
    ... on PullRequest { number title isDraft updatedAt author { login } \
    repository { nameWithOwner } } } } }";

const DETAIL: &str = r#"
query($owner: String!, $name: String!, $number: Int!) {
  repository(owner: $owner, name: $name) {
    squashMergeAllowed
    mergeCommitAllowed
    rebaseMergeAllowed
    viewerPermission
    pullRequest(number: $number) {
      number title body state isDraft createdAt updatedAt mergedAt closedAt
      baseRefName headRefName headRefOid additions deletions
      mergeable mergeStateStatus
      author { login }
      labels(first: 30) { nodes { name } }
      reviewRequests(first: 30) {
        nodes { requestedReviewer { __typename ... on User { login } ... on Team { name } } }
      }
      latestReviews(first: 50) { nodes { state author { login } } }
      reviews(first: 100) { nodes { state body submittedAt author { login } } }
      comments(first: 100) { nodes { body createdAt author { login } } }
      reviewThreads(first: 100) {
        nodes { comments(first: 30) { nodes { body path createdAt author { login } } } }
      }
      commits(last: 100) {
        nodes {
          commit {
            oid messageHeadline committedDate
            authors(first: 1) { nodes { name user { login } } }
          }
        }
      }
      head: commits(last: 1) {
        nodes {
          commit {
            statusCheckRollup {
              contexts(first: 100) {
                nodes {
                  __typename
                  ... on CheckRun {
                    name status conclusion detailsUrl
                    checkSuite { workflowRun { workflow { name } } }
                  }
                  ... on StatusContext { context state targetUrl }
                }
              }
            }
          }
        }
      }
    }
  }
}
"#;

/// The open pull requests one heading lists, as GitHub ranks them.
pub async fn search(
    client: &GitHubClient,
    source: &Source,
    viewer: &str,
    on_trusted: bool,
) -> Result<Vec<Entry>> {
    client.record_api_call("graphql.pr_view.search");
    let data: Value = client
        .graphql_data(json!({
            "query": SEARCH,
            "variables": { "q": source.query(viewer) },
        }))
        .await
        .context("Failed to search pull requests")
        .map_err(|e| client.enrich_api_error(e))?;
    Ok(parse_search(
        data.pointer("/search/nodes").unwrap_or(&Value::Null),
        on_trusted,
    ))
}

/// Reads a pull request in full: its page, the repository's merge settings,
/// and the comments left on lines of the diff, in one request.
pub async fn fetch_detail(client: &GitHubClient, pr: &PrRef) -> Result<Detail> {
    client.record_api_call("graphql.pr_view.detail");
    let (owner, name) = pr.owner_and_name();
    let data: Value = client
        .graphql_data(json!({
            "query": DETAIL,
            "variables": { "owner": owner, "name": name, "number": pr.number },
        }))
        .await
        .with_context(|| format!("Failed to read {}", pr.label()))
        .map_err(|e| client.enrich_api_error(e))?;
    let repository = data
        .get("repository")
        .filter(|r| !r.is_null())
        .with_context(|| format!("Repository {} was not found", pr.repo))?;
    parse_detail(pr.clone(), repository)
}

/// The pull request's unified diff.
pub async fn fetch_patch(client: &GitHubClient, pr: &PrRef) -> Result<String> {
    client.board_get_pr_diff(pr.number).await
}

/// Does `action` to the pull request. A merge is pinned to `head_sha`, so a
/// push that landed after the page was read is refused instead of merged unseen.
pub async fn perform(
    client: &GitHubClient,
    pr: &PrRef,
    action: &Action,
    head_sha: &str,
) -> Result<()> {
    match action {
        Action::Merge(method) => {
            let sha = (!head_sha.is_empty()).then(|| head_sha.to_owned());
            client.merge_pr(pr.number, *method, None, None, sha).await
        }
        Action::Close => client.close_pr(pr.number).await,
        Action::Reopen => {
            client.record_api_call("pulls.update.state");
            client
                .octocrab
                .pulls(&client.owner, &client.repo)
                .update(pr.number)
                .state(octocrab::params::pulls::State::Open)
                .send()
                .await
                .context("Failed to reopen the pull request")?;
            Ok(())
        }
        Action::Ready => client.set_pr_draft(pr.number, false).await,
        Action::Draft => client.set_pr_draft(pr.number, true).await,
        Action::Approve => {
            client.record_api_call("pulls.reviews.create");
            let route = format!(
                "/repos/{}/{}/pulls/{}/reviews",
                client.owner, client.repo, pr.number
            );
            let _: Value = client
                .octocrab
                .post(route, Some(&json!({ "event": "APPROVE" })))
                .await
                .context("Failed to approve the pull request")?;
            Ok(())
        }
        Action::Comment(body) => {
            if body.trim().is_empty() {
                bail!("A comment can't be empty");
            }
            client.create_issue_comment(pr.number, body).await
        }
    }
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

fn text(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

/// `{ login }` under `key`, or `ghost` for a deleted account.
fn login(value: &Value, key: &str) -> String {
    value
        .get(key)
        .map(|author| text(author, "login"))
        .filter(|login| !login.is_empty())
        .unwrap_or_else(|| "ghost".to_owned())
}

fn number(value: &Value, key: &str) -> u64 {
    value.get(key).and_then(Value::as_u64).unwrap_or_default()
}

/// The `nodes` array of a GraphQL connection under `key`.
fn nodes<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value
        .get(key)
        .and_then(|connection| connection.get("nodes"))
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
}

pub fn parse_search(nodes: &Value, on_trusted: bool) -> Vec<Entry> {
    nodes
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let repo = item.get("repository").map(|r| text(r, "nameWithOwner"))?;
                    (!repo.is_empty() && number(item, "number") > 0).then(|| Entry {
                        pr: PrRef {
                            on_trusted,
                            repo,
                            number: number(item, "number"),
                        },
                        title: text(item, "title"),
                        author: login(item, "author"),
                        updated: text(item, "updatedAt"),
                        draft: item
                            .get("isDraft")
                            .and_then(Value::as_bool)
                            .unwrap_or(false),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Builds a [`Detail`] from the `repository` object of the DETAIL query.
pub fn parse_detail(pr: PrRef, repository: &Value) -> Result<Detail> {
    let page = repository
        .get("pullRequest")
        .filter(|p| !p.is_null())
        .with_context(|| format!("{} was not found", pr.label()))?;

    let state = match (text(page, "state").as_str(), page.get("isDraft")) {
        ("MERGED", _) => State::Merged,
        ("CLOSED", _) => State::Closed,
        (_, Some(Value::Bool(true))) => State::Draft,
        _ => State::Open,
    };

    let mut reviewers: Vec<Reviewer> = Vec::new();
    for review in nodes(page, "latestReviews") {
        if let Some(verdict) = Verdict::parse(&text(review, "state")) {
            reviewers.push(Reviewer {
                login: login(review, "author"),
                verdict,
            });
        }
    }
    for request in nodes(page, "reviewRequests") {
        let Some(reviewer) = request.get("requestedReviewer") else {
            continue;
        };
        // A team asks by name, a person by login.
        let name = Some(text(reviewer, "login"))
            .filter(|l| !l.is_empty())
            .unwrap_or_else(|| text(reviewer, "name"));
        if !name.is_empty() && !reviewers.iter().any(|r| r.login == name) {
            reviewers.push(Reviewer {
                login: name,
                verdict: Verdict::Requested,
            });
        }
    }

    let methods = [
        ("squashMergeAllowed", MergeMethod::Squash),
        ("mergeCommitAllowed", MergeMethod::Merge),
        ("rebaseMergeAllowed", MergeMethod::Rebase),
    ]
    .into_iter()
    .filter(|(key, _)| {
        repository
            .get(key)
            .and_then(Value::as_bool)
            .unwrap_or(false)
    })
    .map(|(_, method)| method)
    .collect();
    let can_write = matches!(
        text(repository, "viewerPermission").as_str(),
        "ADMIN" | "MAINTAIN" | "WRITE"
    );

    let checks = nodes(page, "head")
        .last()
        .and_then(|node| node.pointer("/commit/statusCheckRollup"))
        .map(|rollup| nodes(rollup, "contexts").iter().map(parse_check).collect())
        .unwrap_or_default();

    let author = login(page, "author");
    let created = text(page, "createdAt");
    Ok(Detail {
        title: text(page, "title"),
        body: text(page, "body"),
        state,
        updated: text(page, "updatedAt"),
        base: text(page, "baseRefName"),
        head: text(page, "headRefName"),
        head_sha: text(page, "headRefOid"),
        additions: number(page, "additions"),
        deletions: number(page, "deletions"),
        labels: nodes(page, "labels")
            .iter()
            .map(|label| text(label, "name"))
            .filter(|name| !name.is_empty())
            .collect(),
        reviewers,
        checks,
        mergeability: Mergeability::parse(
            &text(page, "mergeStateStatus"),
            &text(page, "mergeable"),
        ),
        methods,
        can_write,
        timeline: timeline(page, &author, &created, state),
        author,
        pr,
    })
}

fn parse_check(check: &Value) -> Check {
    let is_status_context = text(check, "__typename") == "StatusContext";
    let (name, url) = if is_status_context {
        (text(check, "context"), text(check, "targetUrl"))
    } else {
        let workflow = check
            .pointer("/checkSuite/workflowRun/workflow/name")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let name = text(check, "name");
        let name = if workflow.is_empty() || workflow == name {
            name
        } else {
            format!("{workflow} / {name}")
        };
        (name, text(check, "detailsUrl"))
    };
    let status = if is_status_context {
        match text(check, "state").as_str() {
            "SUCCESS" => CheckStatus::Passed,
            "FAILURE" | "ERROR" => CheckStatus::Failed,
            _ => CheckStatus::Running,
        }
    } else if text(check, "status") != "COMPLETED" {
        CheckStatus::Running
    } else {
        match text(check, "conclusion").as_str() {
            "SUCCESS" | "NEUTRAL" => CheckStatus::Passed,
            "SKIPPED" => CheckStatus::Skipped,
            "ACTION_REQUIRED" => CheckStatus::Running,
            _ => CheckStatus::Failed,
        }
    };
    Check {
        name,
        status,
        url: (!url.is_empty()).then_some(url),
    }
}

fn timeline(page: &Value, author: &str, created: &str, state: State) -> Vec<Event> {
    let mut events = vec![Event {
        at: created.to_owned(),
        actor: author.to_owned(),
        kind: EventKind::Opened,
    }];
    for node in nodes(page, "commits") {
        let Some(commit) = node.get("commit") else {
            continue;
        };
        let actor = nodes(commit, "authors")
            .first()
            .map(|a| {
                a.get("user")
                    .map(|user| text(user, "login"))
                    .filter(|l| !l.is_empty())
                    .unwrap_or_else(|| text(a, "name"))
            })
            .unwrap_or_default();
        let sha = text(commit, "oid");
        events.push(Event {
            at: text(commit, "committedDate"),
            actor,
            kind: EventKind::Commit {
                sha: sha.chars().take(7).collect(),
                headline: text(commit, "messageHeadline"),
            },
        });
    }
    for comment in nodes(page, "comments") {
        events.push(Event {
            at: text(comment, "createdAt"),
            actor: login(comment, "author"),
            kind: EventKind::Comment {
                body: text(comment, "body"),
            },
        });
    }
    for review in nodes(page, "reviews") {
        let Some(verdict) = Verdict::parse(&text(review, "state")) else {
            continue;
        };
        let body = text(review, "body");
        // A bare "commented" review only wraps line comments, which show on their own.
        if verdict == Verdict::Commented && body.trim().is_empty() {
            continue;
        }
        events.push(Event {
            at: text(review, "submittedAt"),
            actor: login(review, "author"),
            kind: EventKind::Review { verdict, body },
        });
    }
    for thread in nodes(page, "reviewThreads") {
        for comment in nodes(thread, "comments") {
            events.push(Event {
                at: text(comment, "createdAt"),
                actor: login(comment, "author"),
                kind: EventKind::Inline {
                    path: text(comment, "path"),
                    body: text(comment, "body"),
                },
            });
        }
    }
    // Stable sort: events that tie keep the order they were added in.
    events.sort_by(|a, b| a.at.cmp(&b.at));
    match state {
        State::Merged => events.push(Event {
            at: text(page, "mergedAt"),
            actor: String::new(),
            kind: EventKind::Merged,
        }),
        State::Closed => events.push(Event {
            at: text(page, "closedAt"),
            actor: String::new(),
            kind: EventKind::Closed,
        }),
        State::Open | State::Draft => {}
    }
    events
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(repo: &str, number: u64, title: &str, updated: &str) -> Entry {
        Entry {
            pr: PrRef {
                on_trusted: false,
                repo: repo.to_owned(),
                number,
            },
            title: title.to_owned(),
            author: "ada".to_owned(),
            updated: updated.to_owned(),
            draft: false,
        }
    }

    #[test]
    fn parses_every_target_shape() {
        let here = Some("acme/widgets");
        let at = |repo: &str, number, on_trusted| PrRef {
            on_trusted,
            repo: repo.to_owned(),
            number,
        };
        // A bare number belongs to the checkout, so it is read from its remote.
        assert_eq!(
            PrRef::parse("12", here).unwrap(),
            at("acme/widgets", 12, true)
        );
        assert_eq!(
            PrRef::parse(" #12 ", here).unwrap(),
            at("acme/widgets", 12, true)
        );
        // owner/repo#N is the checkout's only when it names the checkout's repo.
        assert_eq!(
            PrRef::parse("ACME/widgets#3", here).unwrap(),
            at("ACME/widgets", 3, true)
        );
        assert_eq!(
            PrRef::parse("owner/repo#7", here).unwrap(),
            at("owner/repo", 7, false)
        );
        assert_eq!(
            PrRef::parse("owner/repo#7", None).unwrap(),
            at("owner/repo", 7, false)
        );
        // A link names github.com, never the checkout's host.
        assert_eq!(
            PrRef::parse("https://github.com/acme/widgets/pull/12", here).unwrap(),
            at("acme/widgets", 12, false)
        );
        assert_eq!(
            PrRef::parse("https://github.com/owner/repo/pull/12/files?x=1#diff", None).unwrap(),
            at("owner/repo", 12, false)
        );
    }

    #[test]
    fn rejects_what_is_not_a_pull_request() {
        for target in [
            "",
            "abc",
            "0",
            "owner#3",
            "a/b/c#3",
            "owner/repo#x",
            "https://github.com/owner/repo/issues/3",
            "https://example.com/owner/repo/pull/3",
            "https://github.com/owner/repo/pull/0",
        ] {
            assert!(
                PrRef::parse(target, Some("acme/widgets")).is_err(),
                "{target:?} should be rejected"
            );
        }
        let err = PrRef::parse("12", None).unwrap_err().to_string();
        assert!(err.contains("owner/repo#12"), "{err}");
    }

    #[test]
    fn branch_heading_skips_the_default_branch() {
        let titles = |branch: Option<&str>, trunk: Option<&str>| -> Vec<String> {
            sources(Some("a/b"), branch, trunk)
                .iter()
                .map(Source::title)
                .collect()
        };
        assert_eq!(
            titles(Some("feat/x"), Some("main")),
            [
                "This branch",
                "a/b",
                "Yours",
                "Review requested",
                "In your repositories"
            ]
        );
        assert_eq!(titles(Some("main"), None)[0], "a/b");
        assert_eq!(titles(Some("develop"), Some("develop"))[0], "a/b");
        assert_eq!(
            sources(None, None, None),
            [Source::Yours, Source::Reviewing, Source::Owned]
        );
    }

    #[test]
    fn dedupe_keeps_first_heading_and_sorts_newest_first() {
        let groups = dedupe(vec![
            Group {
                title: "one".into(),
                entries: vec![
                    entry("a/b", 1, "old", "2026-01-01T00:00:00Z"),
                    entry("a/b", 2, "new", "2026-02-01T00:00:00Z"),
                ],
            },
            Group {
                title: "two".into(),
                entries: vec![
                    entry("a/b", 2, "new", "2026-02-01T00:00:00Z"),
                    entry("a/b", 3, "other", "2026-03-01T00:00:00Z"),
                ],
            },
        ]);
        let numbers = |g: &Group| g.entries.iter().map(|e| e.pr.number).collect::<Vec<_>>();
        assert_eq!(numbers(&groups[0]), [2, 1]);
        assert_eq!(numbers(&groups[1]), [3]);
    }

    #[test]
    fn filter_matches_all_words_across_fields() {
        let e = entry("acme/widgets", 42, "Fix flaky retry", "");
        assert!(matches_filter(&e, ""));
        assert!(matches_filter(&e, "flaky RETRY"));
        assert!(matches_filter(&e, "widgets #42"));
        assert!(matches_filter(&e, "ada fix"));
        assert!(!matches_filter(&e, "flaky nope"));
    }

    #[test]
    fn search_skips_non_pull_request_nodes() {
        let nodes = json!([
            {"number": 5, "title": "T", "isDraft": true, "updatedAt": "2026-01-01T00:00:00Z",
             "author": null, "repository": {"nameWithOwner": "a/b"}},
            {},
        ]);
        let entries = parse_search(&nodes, false);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].pr.label(), "a/b#5");
        assert_eq!(entries[0].author, "ghost");
        assert!(entries[0].draft);
    }

    #[test]
    fn checks_are_summarised_by_the_worst_one() {
        let check = |status| Check {
            name: "c".into(),
            status,
            url: None,
        };
        assert_eq!(describe_checks(&[]), "No checks");
        assert_eq!(checks_status(&[]), None);
        let mixed = [
            check(CheckStatus::Passed),
            check(CheckStatus::Running),
            check(CheckStatus::Failed),
        ];
        assert_eq!(describe_checks(&mixed), "1 of 3 running · 1 failed");
        assert_eq!(checks_status(&mixed), Some(CheckStatus::Failed));
        assert_eq!(
            describe_checks(&[check(CheckStatus::Passed), check(CheckStatus::Skipped)]),
            "All checks passed"
        );
    }

    fn fixture() -> Value {
        serde_json::from_str(
            r##"{
            "squashMergeAllowed": true,
            "mergeCommitAllowed": false,
            "rebaseMergeAllowed": true,
            "viewerPermission": "WRITE",
            "pullRequest": {
                "number": 9, "title": "Add thing", "body": "Body", "state": "OPEN",
                "isDraft": true, "createdAt": "2026-01-01T00:00:00Z",
                "updatedAt": "2026-01-03T00:00:00Z",
                "baseRefName": "main", "headRefName": "feat", "headRefOid": "abcdef1234567",
                "additions": 10, "deletions": 2,
                "mergeable": "MERGEABLE", "mergeStateStatus": "BLOCKED",
                "author": {"login": "ada"},
                "labels": {"nodes": [{"name": "bug"}]},
                "reviewRequests": {"nodes": [
                    {"requestedReviewer": {"__typename": "User", "login": "bob"}},
                    {"requestedReviewer": {"__typename": "Team", "name": "core"}},
                    {"requestedReviewer": {"__typename": "User", "login": "carol"}}
                ]},
                "latestReviews": {"nodes": [
                    {"state": "APPROVED", "author": {"login": "carol"}},
                    {"state": "PENDING", "author": {"login": "dave"}}
                ]},
                "reviews": {"nodes": [
                    {"state": "COMMENTED", "body": "  ", "submittedAt": "2026-01-02T00:00:00Z",
                     "author": {"login": "bob"}},
                    {"state": "APPROVED", "body": "lgtm", "submittedAt": "2026-01-02T05:00:00Z",
                     "author": {"login": "carol"}}
                ]},
                "comments": {"nodes": [
                    {"body": "hi", "createdAt": "2026-01-02T01:00:00Z", "author": null}
                ]},
                "reviewThreads": {"nodes": [{"comments": {"nodes": [
                    {"body": "nit", "path": "src/a.rs", "createdAt": "2026-01-02T02:00:00Z",
                     "author": {"login": "bob"}}
                ]}}]},
                "commits": {"nodes": [{"commit": {
                    "oid": "abcdef1234567", "messageHeadline": "wip",
                    "committedDate": "2026-01-01T12:00:00Z",
                    "authors": {"nodes": [{"name": "Ada L", "user": {"login": "ada"}}]}
                }}]},
                "head": {"nodes": [{"commit": {"statusCheckRollup": {"contexts": {"nodes": [
                    {"__typename": "CheckRun", "name": "test", "status": "COMPLETED",
                     "conclusion": "FAILURE", "detailsUrl": "https://ci/1",
                     "checkSuite": {"workflowRun": {"workflow": {"name": "CI"}}}},
                    {"__typename": "CheckRun", "name": "lint", "status": "IN_PROGRESS",
                     "conclusion": null, "detailsUrl": "",
                     "checkSuite": {"workflowRun": {"workflow": {"name": "lint"}}}},
                    {"__typename": "StatusContext", "context": "ci/legacy", "state": "SUCCESS",
                     "targetUrl": null}
                ]}}}}]}
            }
        }"##,
        )
        .unwrap()
    }

    #[test]
    fn detail_is_read_from_a_graphql_page() {
        let pr = PrRef {
            on_trusted: false,
            repo: "a/b".into(),
            number: 9,
        };
        let detail = parse_detail(pr, &fixture()).unwrap();

        assert_eq!(detail.state, State::Draft);
        assert_eq!(detail.mergeability, Mergeability::Blocked);
        assert_eq!(detail.labels, ["bug"]);
        assert_eq!(
            detail.methods,
            [MergeMethod::Squash, MergeMethod::Rebase],
            "only the methods the repository allows"
        );
        assert!(detail.can_write);

        // carol's review wins over her pending request; dave's pending review is dropped.
        let reviewers: Vec<_> = detail
            .reviewers
            .iter()
            .map(|r| (r.login.as_str(), r.verdict))
            .collect();
        assert_eq!(
            reviewers,
            [
                ("carol", Verdict::Approved),
                ("bob", Verdict::Requested),
                ("core", Verdict::Requested)
            ]
        );

        let checks: Vec<_> = detail
            .checks
            .iter()
            .map(|c| (c.name.as_str(), c.status))
            .collect();
        assert_eq!(
            checks,
            [
                ("CI / test", CheckStatus::Failed),
                ("lint", CheckStatus::Running),
                ("ci/legacy", CheckStatus::Passed)
            ]
        );
        assert_eq!(detail.checks[0].url.as_deref(), Some("https://ci/1"));
        assert_eq!(detail.checks[2].url, None);
    }

    #[test]
    fn timeline_is_chronological_and_drops_empty_review_wrappers() {
        let pr = PrRef {
            on_trusted: false,
            repo: "a/b".into(),
            number: 9,
        };
        let detail = parse_detail(pr, &fixture()).unwrap();
        let kinds: Vec<&str> = detail
            .timeline
            .iter()
            .map(|e| match &e.kind {
                EventKind::Opened => "opened",
                EventKind::Commit { .. } => "commit",
                EventKind::Comment { .. } => "comment",
                EventKind::Review { .. } => "review",
                EventKind::Inline { .. } => "inline",
                EventKind::Merged => "merged",
                EventKind::Closed => "closed",
            })
            .collect();
        assert_eq!(kinds, ["opened", "commit", "comment", "inline", "review"]);
        assert_eq!(detail.timeline[2].actor, "ghost");
        match &detail.timeline[1].kind {
            EventKind::Commit { sha, .. } => assert_eq!(sha, "abcdef1"),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn merged_and_closed_end_the_timeline_and_read_only_viewers_cannot_write() {
        let mut repository = fixture();
        repository["viewerPermission"] = json!("READ");
        repository["pullRequest"]["state"] = json!("MERGED");
        repository["pullRequest"]["mergedAt"] = json!("2026-01-02T03:00:00Z");
        let pr = PrRef {
            on_trusted: false,
            repo: "a/b".into(),
            number: 9,
        };
        let detail = parse_detail(pr, &repository).unwrap();
        assert_eq!(detail.state, State::Merged);
        assert!(!detail.can_write);
        let last = detail.timeline.last().unwrap();
        assert_eq!(last.kind, EventKind::Merged);
        assert_eq!(last.at, "2026-01-02T03:00:00Z");
    }

    #[test]
    fn missing_pull_request_is_an_error() {
        let pr = PrRef {
            on_trusted: false,
            repo: "a/b".into(),
            number: 9,
        };
        let err = parse_detail(pr, &json!({"pullRequest": null}))
            .unwrap_err()
            .to_string();
        assert!(err.contains("a/b#9"), "{err}");
    }

    mod api {
        use super::*;
        use octocrab::Octocrab;
        use wiremock::matchers::{body_partial_json, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        async fn client(server: &MockServer) -> GitHubClient {
            let _ = rustls::crypto::ring::default_provider().install_default();
            let octocrab = Octocrab::builder()
                .base_uri(server.uri())
                .unwrap()
                .personal_token("test-token".to_string())
                .build()
                .unwrap();
            GitHubClient::with_octocrab(octocrab, "acme", "widgets")
        }

        fn pr() -> PrRef {
            PrRef {
                on_trusted: false,
                repo: "acme/widgets".into(),
                number: 9,
            }
        }

        #[tokio::test]
        async fn search_sends_the_query_as_a_variable() {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path("/graphql"))
                .and(body_partial_json(json!({
                    "variables": { "q": "is:pr is:open archived:false repo:acme/widgets" }
                })))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "data": { "search": { "nodes": [{
                        "number": 3, "title": "T", "isDraft": false,
                        "updatedAt": "2026-01-01T00:00:00Z", "author": {"login": "ada"},
                        "repository": {"nameWithOwner": "acme/widgets"}
                    }]}}
                })))
                .expect(1)
                .mount(&server)
                .await;

            let entries = search(
                &client(&server).await,
                &Source::Repo("acme/widgets".into()),
                "",
                false,
            )
            .await
            .unwrap();
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0].pr.label(), "acme/widgets#3");
        }

        #[tokio::test]
        async fn detail_passes_owner_name_and_number_as_variables() {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path("/graphql"))
                .and(body_partial_json(json!({
                    "variables": { "owner": "acme", "name": "widgets", "number": 9 }
                })))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "data": { "repository": fixture() }
                })))
                .expect(1)
                .mount(&server)
                .await;

            let detail = fetch_detail(&client(&server).await, &pr()).await.unwrap();
            assert_eq!(detail.title, "Add thing");
            assert_eq!(detail.head_sha, "abcdef1234567");
        }

        #[tokio::test]
        async fn merge_is_pinned_to_the_head_commit_and_method() {
            let server = MockServer::start().await;
            Mock::given(method("PUT"))
                .and(path("/repos/acme/widgets/pulls/9/merge"))
                .and(body_partial_json(json!({
                    "sha": "abcdef1234567", "merge_method": "squash"
                })))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "merged": true, "sha": "x", "message": "ok"
                })))
                .expect(1)
                .mount(&server)
                .await;

            perform(
                &client(&server).await,
                &pr(),
                &Action::Merge(MergeMethod::Squash),
                "abcdef1234567",
            )
            .await
            .unwrap();
        }

        #[tokio::test]
        async fn approve_posts_an_approve_review() {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path("/repos/acme/widgets/pulls/9/reviews"))
                .and(body_partial_json(json!({ "event": "APPROVE" })))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
                .expect(1)
                .mount(&server)
                .await;

            perform(&client(&server).await, &pr(), &Action::Approve, "")
                .await
                .unwrap();
        }

        #[tokio::test]
        async fn empty_comment_is_refused_without_a_request() {
            let server = MockServer::start().await;
            let err = perform(
                &client(&server).await,
                &pr(),
                &Action::Comment("  ".into()),
                "",
            )
            .await
            .unwrap_err();
            assert!(err.to_string().contains("empty"), "{err}");
            assert!(server.received_requests().await.unwrap().is_empty());
        }
    }
}
