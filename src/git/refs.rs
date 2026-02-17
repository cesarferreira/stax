use anyhow::{Context, Result};
use git2::Repository;
use std::process::Command;

const METADATA_REF_PREFIX: &str = "refs/branch-metadata/";
const STAX_TRUNK_REF: &str = "refs/stax/trunk";
const STAX_PREV_BRANCH_REF: &str = "refs/stax/prev-branch";

/// Read metadata JSON for a branch from git refs
pub fn read_metadata(repo: &Repository, branch: &str) -> Result<Option<String>> {
    let ref_name = format!("{}{}", METADATA_REF_PREFIX, branch);

    match repo.find_reference(&ref_name) {
        Ok(reference) => {
            let oid = reference.target().context("Reference has no target")?;
            let blob = repo.find_blob(oid)?;
            let content = std::str::from_utf8(blob.content())?;
            Ok(Some(content.to_string()))
        }
        Err(e) if e.code() == git2::ErrorCode::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Write metadata JSON for a branch to git refs
pub fn write_metadata(repo: &Repository, branch: &str, json: &str) -> Result<()> {
    let workdir = repo
        .workdir()
        .context("Repository has no working directory")?;

    // Create blob with json content
    let mut child = Command::new("git")
        .args(["hash-object", "-w", "--stdin"])
        .current_dir(workdir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()?;

    if let Some(stdin) = child.stdin.as_mut() {
        use std::io::Write;
        stdin.write_all(json.as_bytes())?;
    }

    let output = child.wait_with_output()?;
    let hash = String::from_utf8(output.stdout)?.trim().to_string();

    // Update the ref to point to the blob
    let ref_name = format!("{}{}", METADATA_REF_PREFIX, branch);
    let status = Command::new("git")
        .args(["update-ref", &ref_name, &hash])
        .current_dir(workdir)
        .status()
        .context("Failed to update ref")?;

    if !status.success() {
        anyhow::bail!("Failed to update ref {}", ref_name);
    }

    Ok(())
}

/// Delete metadata ref for a branch
pub fn delete_metadata(repo: &Repository, branch: &str) -> Result<()> {
    let ref_name = format!("{}{}", METADATA_REF_PREFIX, branch);
    let workdir = repo
        .workdir()
        .context("Repository has no working directory")?;

    let status = Command::new("git")
        .args(["update-ref", "-d", &ref_name])
        .current_dir(workdir)
        .status()
        .context("Failed to delete ref")?;

    if !status.success() {
        anyhow::bail!("Failed to delete ref {}", ref_name);
    }

    Ok(())
}

/// List all branches that have metadata
pub fn list_metadata_branches(repo: &Repository) -> Result<Vec<String>> {
    let mut branches = Vec::new();

    for reference in repo.references_glob(&format!("{}*", METADATA_REF_PREFIX))? {
        let reference = reference?;
        if let Some(name) = reference.name() {
            let branch = name.strip_prefix(METADATA_REF_PREFIX).unwrap_or(name);
            branches.push(branch.to_string());
        }
    }

    Ok(branches)
}

/// Check if stax has been initialized in this repo
pub fn is_initialized(repo: &Repository) -> bool {
    repo.find_reference(STAX_TRUNK_REF).is_ok()
}

/// Read the configured trunk branch
pub fn read_trunk(repo: &Repository) -> Result<Option<String>> {
    match repo.find_reference(STAX_TRUNK_REF) {
        Ok(reference) => {
            let oid = reference.target().context("Reference has no target")?;
            let blob = repo.find_blob(oid)?;
            let content = std::str::from_utf8(blob.content())?;
            Ok(Some(content.trim().to_string()))
        }
        Err(e) if e.code() == git2::ErrorCode::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Write the trunk branch setting
pub fn write_trunk(repo: &Repository, trunk: &str) -> Result<()> {
    let workdir = repo
        .workdir()
        .context("Repository has no working directory")?;

    // Create blob with trunk name
    let mut child = Command::new("git")
        .args(["hash-object", "-w", "--stdin"])
        .current_dir(workdir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()?;

    if let Some(stdin) = child.stdin.as_mut() {
        use std::io::Write;
        stdin.write_all(trunk.as_bytes())?;
    }

    let output = child.wait_with_output()?;
    let hash = String::from_utf8(output.stdout)?.trim().to_string();

    // Update the ref
    Command::new("git")
        .args(["update-ref", STAX_TRUNK_REF, &hash])
        .current_dir(workdir)
        .status()
        .context("Failed to update trunk ref")?;

    Ok(())
}

/// Read the previous branch (for `stax prev` command)
pub fn read_prev_branch(repo: &Repository) -> Result<Option<String>> {
    match repo.find_reference(STAX_PREV_BRANCH_REF) {
        Ok(reference) => {
            let oid = reference.target().context("Reference has no target")?;
            let blob = repo.find_blob(oid)?;
            let content = std::str::from_utf8(blob.content())?;
            Ok(Some(content.trim().to_string()))
        }
        Err(e) if e.code() == git2::ErrorCode::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Write the previous branch (for `stax prev` command)
pub fn write_prev_branch(repo: &Repository, branch: &str) -> Result<()> {
    let workdir = repo
        .workdir()
        .context("Repository has no working directory")?;

    // Create blob with branch name
    let mut child = Command::new("git")
        .args(["hash-object", "-w", "--stdin"])
        .current_dir(workdir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()?;

    if let Some(stdin) = child.stdin.as_mut() {
        use std::io::Write;
        stdin.write_all(branch.as_bytes())?;
    }

    let output = child.wait_with_output()?;
    let hash = String::from_utf8(output.stdout)?.trim().to_string();

    // Update the ref
    Command::new("git")
        .args(["update-ref", STAX_PREV_BRANCH_REF, &hash])
        .current_dir(workdir)
        .status()
        .context("Failed to update prev-branch ref")?;

    Ok(())
}
