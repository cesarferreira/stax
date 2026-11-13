//! `stax pr browse`: browse the pull requests that concern you, or open one.

use crate::config::Config;
use crate::git::GitRepo;
use crate::github::pr_browse::PrRef;
use crate::remote::{ForgeType, TrustedRemoteInfo};
use crate::tui::pr_browse::{Scope, TrustedRemote};
use anyhow::Result;

/// Where the command runs, read from the current folder if it is a GitHub
/// checkout. Outside one, the list still works (yours, review requested, owned)
/// and a pull request is named in full.
fn load_scope() -> Scope {
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
