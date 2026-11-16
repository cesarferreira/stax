//! `stax pr browse`: browse the pull requests that concern you, or open one.

use crate::config::Config;
use crate::git::GitRepo;
use crate::github::GitHubClient;
use crate::github::pr_browse::{self, Action, PrRef, Source};
use crate::remote::{ForgeType, TrustedRemoteInfo};
use anyhow::{Result, bail};
use std::collections::HashMap;
use std::io::{IsTerminal, Read};
use std::sync::Mutex;

/// Where the browser runs: the repository and branch of the current folder
/// (if it is a GitHub checkout) and the GitHub it talks to.
pub struct Scope {
    pub here: Option<String>,
    pub branch: Option<String>,
    pub trunk: Option<String>,
    /// The current checkout's remote, once validated as a trusted network
    /// destination. Pull requests that live there (`PrRef::on_trusted`) are
    /// reached through its API URL; everything else goes to github.com.
    pub trusted: Option<TrustedRemote>,
    pub config: crate::config::Config,
    /// Web address of the checkout's host, for links: `https://github.com`.
    pub web_base: String,
    clients: Mutex<HashMap<(String, bool), GitHubClient>>,
}

pub struct TrustedRemote {
    pub repo: String,
    pub host: String,
    pub api_base_url: Option<String>,
}

impl Scope {
    pub fn new(
        here: Option<String>,
        branch: Option<String>,
        trunk: Option<String>,
        trusted: Option<TrustedRemote>,
        config: crate::config::Config,
        web_base: String,
    ) -> Self {
        Scope {
            here,
            branch,
            trunk,
            trusted,
            config,
            web_base,
            clients: Mutex::new(HashMap::new()),
        }
    }

    /// A client for `repo` (`owner/name`). Always built through the
    /// host-scoped token resolver, which refuses a token configured for a
    /// different host: the checkout's validated host for its own pull
    /// requests, github.com for everything else. A pasted link or a
    /// look-alike repository name can never steer the token elsewhere.
    /// Clients are kept, so hovering rows doesn't rebuild one per request.
    pub(crate) fn client_for(&self, repo: &str, on_trusted: bool) -> Result<GitHubClient> {
        let key = (repo.to_ascii_lowercase(), on_trusted);
        if let Some(client) = self.clients.lock().ok().and_then(|c| c.get(&key).cloned()) {
            return Ok(client);
        }
        let (owner, name) = repo
            .split_once('/')
            .ok_or_else(|| anyhow::anyhow!("'{repo}' isn't an owner/name repository"))?;
        let client = match (&self.trusted, on_trusted) {
            (Some(t), true) => GitHubClient::new_for_trusted_remote(
                owner,
                name,
                t.api_base_url.clone(),
                &self.config,
                &t.host,
            )?,
            _ => {
                GitHubClient::new_for_trusted_remote(owner, name, None, &self.config, "github.com")?
            }
        };
        if let Ok(mut clients) = self.clients.lock() {
            clients.insert(key, client.clone());
        }
        Ok(client)
    }

    /// The client for searches: the checkout's host when there is one.
    pub(crate) fn search_client(&self) -> Result<GitHubClient> {
        match &self.trusted {
            Some(t) => self.client_for(&t.repo, true),
            None => self.client_for("_/_", false),
        }
    }
}

/// Where the command runs, read from the current folder if it is a GitHub
/// checkout. Outside one, the list still works (yours, review requested, owned)
/// and a pull request is named in full.
pub(crate) fn load_scope() -> Scope {
    let config = Config::load().unwrap_or_default();
    let github = || "https://github.com".to_owned();
    let Ok(repo) = GitRepo::open() else {
        return Scope::new(None, None, None, None, config, github());
    };
    // An unvalidated remote must not decide where the token goes.
    let Ok(trusted) = TrustedRemoteInfo::from_repo(&repo, &config) else {
        return Scope::new(None, None, None, None, config, github());
    };
    let remote = trusted.remote();
    if remote.forge != ForgeType::GitHub {
        return Scope::new(None, None, None, None, config, github());
    }
    let here = format!("{}/{}", remote.namespace, remote.repo);
    let trusted_remote = TrustedRemote {
        repo: here.clone(),
        host: remote.host.clone(),
        api_base_url: remote.api_base_url.clone(),
    };
    Scope::new(
        Some(here),
        repo.current_branch().ok(),
        repo.trunk_branch().ok(),
        Some(trusted_remote),
        config,
        remote.base_url.trim_end_matches('/').to_owned(),
    )
}

/// Rejects a target that names no pull request before anything else happens.
/// A bare number still needs a repository, which `run` supplies.
pub fn validate_target(target: Option<&str>) -> Result<()> {
    if let Some(target) = target {
        PrRef::parse(target, Some("owner/repo"))?;
    }
    Ok(())
}

pub fn run(target: Option<String>) -> Result<()> {
    let scope = load_scope();
    let initial = target
        .map(|target| PrRef::parse(&target, scope.here.as_deref()))
        .transpose()?;
    crate::tui::pr_browse::run(scope, initial)
}

// ---------------------------------------------------------------------------
// Non-interactive commands, for scripts and agents
// ---------------------------------------------------------------------------

impl Scope {
    /// The pull request's address on the host it lives on.
    pub(crate) fn pr_url(&self, pr: &PrRef) -> String {
        let base = if pr.on_trusted {
            self.web_base.as_str()
        } else {
            "https://github.com"
        };
        format!("{base}/{}/pull/{}", pr.repo, pr.number)
    }
}

/// The pull request a command acts on: the named one, or else the open pull
/// request of the checked-out branch.
fn resolve(rt: &tokio::runtime::Runtime, scope: &Scope, target: Option<&str>) -> Result<PrRef> {
    if let Some(target) = target {
        return PrRef::parse(target, scope.here.as_deref());
    }
    let (Some(repo), Some(branch)) = (&scope.here, &scope.branch) else {
        bail!(
            "Name a pull request (12, owner/repo#12 or a link): this folder isn't a GitHub \
             checkout on a branch."
        );
    };
    let source = Source::Branch {
        repo: repo.clone(),
        branch: branch.clone(),
    };
    let client = scope.search_client()?;
    let found = rt.block_on(pr_browse::search(&client, &source, "", true))?;
    match found.into_iter().next() {
        Some(entry) => Ok(entry.pr),
        None => bail!(
            "No open pull request for branch '{branch}'. Name one, or create it with `stax submit`."
        ),
    }
}

/// Runs `work` with the pull request's client on a fresh runtime.
fn with_pr<T>(
    target: Option<&str>,
    work: impl FnOnce(&tokio::runtime::Runtime, &Scope, &PrRef, &GitHubClient) -> Result<T>,
) -> Result<T> {
    let scope = load_scope();
    let rt = tokio::runtime::Runtime::new()?;
    // Clients build their transport on the current runtime.
    let _enter = rt.enter();
    let pr = resolve(&rt, &scope, target)?;
    let client = scope.client_for(&pr.repo, pr.on_trusted)?;
    work(&rt, &scope, &pr, &client)
}

/// `stax pr show`: the pull request with its checks, reviews and conversation.
pub fn show(target: Option<String>, json: bool) -> Result<()> {
    with_pr(target.as_deref(), |rt, scope, pr, client| {
        let detail = rt.block_on(pr_browse::fetch_detail(client, pr))?;
        let url = scope.pr_url(pr);
        if json {
            println!(
                "{}",
                serde_json::to_string_pretty(&pr_browse::detail_json(&detail, &url))?
            );
        } else {
            print!("{}", pr_browse::detail_text(&detail, &url));
        }
        Ok(())
    })
}

/// `stax pr diff`: the pull request's unified diff.
pub fn diff(target: Option<String>) -> Result<()> {
    with_pr(target.as_deref(), |rt, _, pr, client| {
        let patch = rt.block_on(pr_browse::fetch_patch(client, pr))?;
        print!("{patch}");
        Ok(())
    })
}

/// The comment text: `-m`, or standard input when `-m -` or no `-m` is given
/// and input is piped. Read before anything touches the network.
fn read_message(message: Option<String>) -> Result<String> {
    let text = match message.as_deref() {
        Some("-") => read_stdin()?,
        Some(text) => text.to_owned(),
        None if !std::io::stdin().is_terminal() => read_stdin()?,
        None => bail!("Give the comment with -m \"text\", or pipe it in on standard input."),
    };
    let text = text.trim().to_owned();
    if text.is_empty() {
        bail!("The comment is empty. Give it with -m \"text\", or pipe it in on standard input.");
    }
    Ok(text)
}

fn read_stdin() -> Result<String> {
    let mut text = String::new();
    std::io::stdin().read_to_string(&mut text)?;
    Ok(text)
}

/// `stax pr comment`: adds a comment to the conversation.
pub fn comment(target: Option<String>, message: Option<String>) -> Result<()> {
    let body = read_message(message)?;
    with_pr(target.as_deref(), |rt, scope, pr, client| {
        rt.block_on(pr_browse::perform(client, pr, &Action::Comment(body), ""))?;
        println!("Commented on {}\n{}", pr.label(), scope.pr_url(pr));
        Ok(())
    })
}

/// `stax pr approve`: approves the pull request.
pub fn approve(target: Option<String>) -> Result<()> {
    with_pr(target.as_deref(), |rt, scope, pr, client| {
        rt.block_on(pr_browse::perform(client, pr, &Action::Approve, ""))?;
        println!("Approved {}\n{}", pr.label(), scope.pr_url(pr));
        Ok(())
    })
}
