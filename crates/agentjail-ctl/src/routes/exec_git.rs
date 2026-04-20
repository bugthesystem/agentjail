//! Host-side `git clone` for seeding a jail's source directory.
//!
//! Extracted from [`super::exec`] because the workflow is entirely
//! host-side (runs before the jail locks down the filesystem) and has
//! its own validation surface: URL scheme, length caps, subdir safety,
//! per-clone 60 s timeout.

use super::exec::GitSpec;
use crate::error::{CtlError, Result};

/// Shallow-clone one or more repos into the jail's source directory.
/// Runs on the host, *before* the jail locks down the filesystem.
///
/// - Single-repo form (`{ repo, ref? }`): contents land at the root of
///   `dst` (back-compat with earlier versions).
/// - Multi-repo form (`{ repos: [{ repo, ref?, dir? }] }`): each repo
///   clones into its own subdirectory under `dst`. Default subdir
///   name is the repo basename (with any `.git` suffix stripped).
///
/// Security: every URL must be `https://` (no ssh/git/file), URL ≤ 512
/// bytes, ref ≤ 200, git runs with `--depth=1` and a 60 s hard timeout.
pub(super) async fn git_clone(spec: &GitSpec, dst: &std::path::Path) -> Result<()> {
    match (spec.repo.as_deref(), spec.repos.is_empty()) {
        (Some(_), false) => {
            return Err(CtlError::BadRequest(
                "git: set either `repo` or `repos`, not both".into(),
            ));
        }
        (None, true) => {
            return Err(CtlError::BadRequest(
                "git: provide `repo` or `repos`".into(),
            ));
        }
        _ => {}
    }

    if let Some(repo) = &spec.repo {
        clone_one(repo, spec.git_ref.as_deref(), dst).await?;
    } else {
        for entry in &spec.repos {
            let subdir = entry
                .dir
                .clone()
                .unwrap_or_else(|| default_repo_subdir(&entry.repo));
            validate_subdir(&subdir)?;
            let sub = dst.join(&subdir);
            std::fs::create_dir_all(&sub).map_err(CtlError::Io)?;
            clone_one(&entry.repo, entry.git_ref.as_deref(), &sub).await?;
        }
    }
    Ok(())
}

/// Shallow-clone a single repo into `dst`, flattening the clone dir so
/// `dst` ends up at the repo root.
async fn clone_one(repo: &str, git_ref: Option<&str>, dst: &std::path::Path) -> Result<()> {
    if !repo.starts_with("https://") || repo.len() > 512 {
        return Err(CtlError::BadRequest(
            "git.repo must be https:// (max 512 bytes)".into(),
        ));
    }
    if let Some(r) = git_ref
        && (r.len() > 200 || r.chars().any(|c| c.is_control()))
    {
        return Err(CtlError::BadRequest("git.ref invalid".into()));
    }

    // git clone refuses to clone into a non-empty dir, so stage into a
    // subpath and flatten afterwards.
    let target = dst.join("__repo");
    std::fs::create_dir_all(&target).map_err(CtlError::Io)?;

    let mut cmd = tokio::process::Command::new("git");
    cmd.arg("clone").arg("--depth=1").arg("--single-branch");
    if let Some(r) = git_ref {
        cmd.arg("--branch").arg(r);
    }
    cmd.arg(repo).arg(&target);
    cmd.kill_on_drop(true);

    let out = tokio::time::timeout(std::time::Duration::from_secs(60), cmd.output())
        .await
        .map_err(|_| CtlError::BadRequest("git clone timed out (60s)".into()))?
        .map_err(CtlError::Io)?;

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let tail: String = stderr.lines().rev().take(3).collect::<Vec<_>>().join(" · ");
        return Err(CtlError::BadRequest(format!("git clone failed: {tail}")));
    }

    for entry in std::fs::read_dir(&target).map_err(CtlError::Io)? {
        let e = entry.map_err(CtlError::Io)?;
        let from = e.path();
        let to = dst.join(e.file_name());
        std::fs::rename(&from, &to).map_err(CtlError::Io)?;
    }
    let _ = std::fs::remove_dir_all(&target);
    Ok(())
}

fn default_repo_subdir(url: &str) -> String {
    url.rsplit('/')
        .next()
        .unwrap_or(url)
        .trim_end_matches(".git")
        .trim()
        .to_string()
}

fn validate_subdir(name: &str) -> Result<()> {
    if name.is_empty() || name == "." || name == ".." {
        return Err(CtlError::BadRequest(format!("git.repos.dir invalid: {name:?}")));
    }
    if name.contains('/') || name.contains('\\') || name.starts_with('.') {
        return Err(CtlError::BadRequest(format!(
            "git.repos.dir must not contain slashes or leading `.`: {name:?}"
        )));
    }
    Ok(())
}
