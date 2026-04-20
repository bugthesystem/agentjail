//! Live-fork — `POST /v1/runs/fork`.
//!
//! Spawn a parent, COW-clone the output mid-run, then spawn one or more
//! children against the forked state. Children run in parallel.
//!
//! The request accepts either legacy `child_code` (single child) or
//! `children: [{code,...}]` (N children). The response mirrors the shape:
//! single-child calls get `child` + `fork`; multi-child calls get
//! `children` + `forks`. Legacy fields stay populated for the first child
//! so existing SDKs keep working.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};

use crate::error::{CtlError, Result};
use crate::jails::JailKind;
use crate::sampler;

use super::AppState;
use super::exec::{
    ExecOptions, ExecResponse, GitSpec, default_language, jail_config, language_runtime,
    output_to_response,
};
use super::exec_git::git_clone;
use super::exec_monitor::run_monitored;

#[derive(Debug, Deserialize)]
pub(crate) struct ForkRequest {
    parent_code: String,
    #[serde(default)]
    child_code: Option<String>,
    #[serde(default)]
    children: Vec<ForkChild>,
    #[serde(default = "default_language")]
    language: String,
    /// How long the parent runs before we freeze + fork. Default 1500ms.
    #[serde(default)]
    fork_after_ms: Option<u64>,
    timeout_secs: Option<u64>,
    memory_mb: Option<u64>,
    #[serde(flatten)]
    options: ExecOptions,
    #[serde(default)]
    git: Option<GitSpec>,
}

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct ForkChild {
    code: String,
    #[serde(default)]
    memory_mb: Option<u64>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ForkResponse {
    parent: ExecResponse,
    /// First child (back-compat). Identical to `children[0]`.
    child: ExecResponse,
    /// All children in invocation order.
    children: Vec<ExecResponse>,
    /// ForkMeta for the first child (back-compat).
    fork: ForkMeta,
    /// Per-child ForkMeta.
    forks: Vec<ForkMeta>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ForkMeta {
    clone_ms: u64,
    files_cloned: u64,
    files_cow: u64,
    bytes_cloned: u64,
    method: String,
    was_frozen: bool,
}

impl From<agentjail::ForkInfo> for ForkMeta {
    fn from(f: agentjail::ForkInfo) -> Self {
        Self {
            clone_ms:     u64::try_from(f.clone_duration.as_millis()).unwrap_or(u64::MAX),
            files_cloned: f.files_cloned,
            files_cow:    f.files_cow,
            bytes_cloned: f.bytes_cloned,
            method:       format!("{:?}", f.clone_method).to_lowercase(),
            was_frozen:   f.was_frozen,
        }
    }
}

pub(crate) async fn create_fork_run(
    State(state): State<AppState>,
    Json(req): Json<ForkRequest>,
) -> Result<(StatusCode, Json<ForkResponse>)> {
    let exec_cfg = state.exec_config.as_ref()
        .ok_or_else(|| CtlError::Internal("exec not enabled".into()))?;

    // Normalize: either `children: [...]` or fall back to the legacy
    // single `child_code`. At least one child is required.
    let mut children: Vec<ForkChild> = req.children.clone();
    if children.is_empty() {
        let Some(c) = req.child_code.clone() else {
            return Err(CtlError::BadRequest("children or child_code is required".into()));
        };
        children.push(ForkChild { code: c, memory_mb: None });
    }
    if children.len() > 16 {
        return Err(CtlError::BadRequest("at most 16 children per fork".into()));
    }
    if req.parent_code.len() > 1024 * 1024
        || children.iter().any(|c| c.code.len() > 1024 * 1024)
    {
        return Err(CtlError::BadRequest("code exceeds 1 MB".into()));
    }
    let timeout    = req.timeout_secs.unwrap_or(exec_cfg.default_timeout_secs).clamp(1, 3600);
    let memory     = req.memory_mb.unwrap_or(exec_cfg.default_memory_mb).min(8192);
    let fork_after = req.fork_after_ms.unwrap_or(1500).min(30_000);

    let _permit = state.exec_semaphore.try_acquire()
        .map_err(|_| CtlError::BadRequest("too many concurrent executions".into()))?;
    let _guard = state.exec_metrics.start();

    let (filename, cmd) = language_runtime(&req.language)?;

    let source_dir = tempfile::tempdir().map_err(CtlError::Io)?;
    let parent_out = tempfile::tempdir().map_err(CtlError::Io)?;

    if let Some(g) = &req.git { git_clone(g, source_dir.path()).await?; }

    // Write parent + each child file into the same source dir so the forked
    // jails all see them via /workspace.
    std::fs::write(source_dir.path().join(filename), &req.parent_code).map_err(CtlError::Io)?;
    for (i, c) in children.iter().enumerate() {
        let name = format!("child_{i}_{filename}");
        std::fs::write(source_dir.path().join(&name), &c.code).map_err(CtlError::Io)?;
    }

    let run_env = vec![("PATH".into(), "/usr/local/bin:/usr/bin:/bin".into())];
    let parent_config = jail_config(
        source_dir.path(), parent_out.path(), memory, timeout, run_env.clone(), &req.options,
        /* source_rw */ false,
    )?;

    let parent_jail   = agentjail::Jail::new(parent_config)?;
    let parent_rec    = state.jails.start(JailKind::Fork, format!("{} · parent", req.language), None, None).await;
    let parent_handle = parent_jail.spawn(cmd, &[&format!("/workspace/{filename}")])?;

    let parent_sampler = parent_handle.cgroup_path().map(|p| {
        let jails = state.jails.clone();
        sampler::spawn(p, std::time::Duration::from_millis(500), move |s| {
            let jails = jails.clone();
            tokio::spawn(async move { jails.sample_stats(parent_rec, &s).await; });
        })
    });

    // Let the parent do its setup (write checkpoint, etc.) before we fork.
    tokio::time::sleep(std::time::Duration::from_millis(fork_after)).await;

    // Fork once per child — each gets its own COW clone of the parent output.
    // We keep each child's output tempdir alive for the duration of the run.
    let mut child_outs: Vec<tempfile::TempDir> = Vec::with_capacity(children.len());
    let mut child_jails: Vec<agentjail::Jail>  = Vec::with_capacity(children.len());
    let mut forks_meta: Vec<ForkMeta>          = Vec::with_capacity(children.len());
    for _ in &children {
        let out = tempfile::tempdir().map_err(CtlError::Io)?;
        let (jail, info) = parent_jail.live_fork(Some(&parent_handle), out.path())?;
        child_outs.push(out);
        child_jails.push(jail);
        forks_meta.push(info.into());
    }

    // Record child rows (each linked to parent_rec).
    let mut child_recs: Vec<i64> = Vec::with_capacity(children.len());
    for i in 0..children.len() {
        let label = if children.len() == 1 {
            format!("{} · child", req.language)
        } else {
            format!("{} · child {}", req.language, i)
        };
        let id = state.jails.start(JailKind::Fork, label, None, Some(parent_rec)).await;
        child_recs.push(id);
    }

    // Run all children in parallel + parent's wait.
    let child_futures: Vec<_> = (0..children.len()).map(|i| {
        let jails = state.jails.clone();
        let rec   = child_recs[i];
        let jail  = &child_jails[i];
        let name  = format!("/workspace/child_{i}_{filename}");
        async move {
            let args = [name.as_str()];
            run_monitored(&jails, rec, jail, cmd, &args).await
        }
    }).collect();

    let (parent_res, child_ress) = tokio::join!(
        parent_handle.wait(),
        futures::future::join_all(child_futures),
    );
    if let Some(h) = parent_sampler { h.abort(); }

    // Collect results and record outcomes.
    let parent_output = match parent_res {
        Ok(r)  => { state.jails.finish(parent_rec, &r).await; r }
        Err(e) => { state.jails.error(parent_rec, e.to_string()).await; return Err(e.into()); }
    };
    let mut child_outputs: Vec<agentjail::Output> = Vec::with_capacity(child_ress.len());
    for (i, r) in child_ress.into_iter().enumerate() {
        match r {
            Ok(o)  => { state.jails.finish(child_recs[i], &o).await; child_outputs.push(o); }
            Err(e) => { state.jails.error(child_recs[i], e.to_string()).await; return Err(e.into()); }
        }
    }

    drop(source_dir);
    drop(parent_out);
    drop(child_outs);

    tracing::info!(
        language    = %req.language,
        parent_exit = parent_output.exit_code,
        children    = child_outputs.len(),
        "fork run completed"
    );

    let all: Vec<ExecResponse> = child_outputs.into_iter().map(output_to_response).collect();
    let first_child = all[0].clone();
    let first_fork  = forks_meta[0].clone();

    Ok((
        StatusCode::CREATED,
        Json(ForkResponse {
            parent:   output_to_response(parent_output),
            child:    first_child,
            children: all,
            fork:     first_fork,
            forks:    forks_meta,
        }),
    ))
}
