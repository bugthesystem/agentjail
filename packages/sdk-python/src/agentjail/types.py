"""Shared types. These mirror the control plane's JSON schemas exactly.

We model each as a plain `TypedDict` so users can dereference fields by
name while still handing raw decoded JSON straight through (no
round-trip through dataclasses). Types with nested records use
`Required`/`NotRequired` only where the server may omit a field.
"""

from __future__ import annotations

from typing import Literal, NotRequired, TypedDict

ServiceId = Literal["openai", "anthropic", "github", "stripe"]
SeccompSpec = Literal["standard", "strict"]
JailKind = Literal["run", "exec", "fork", "stream", "workspace"]
JailStatus = Literal["running", "completed", "error"]


# ---- credentials ------------------------------------------------------


class CredentialRecord(TypedDict):
    service: ServiceId
    added_at: str
    updated_at: str
    fingerprint: str


# ---- sessions ---------------------------------------------------------


class Session(TypedDict):
    id: str
    created_at: str
    expires_at: str | None
    services: list[ServiceId]
    env: dict[str, str]


# ---- audit ------------------------------------------------------------


class AuditRow(TypedDict):
    id: int
    at: str
    session_id: str
    service: str
    method: str
    path: str
    status: int
    reject_reason: str | None
    upstream_ms: int | None


class AuditList(TypedDict):
    rows: list[AuditRow]
    total: int


# ---- exec / run -------------------------------------------------------


class ResourceStats(TypedDict):
    memory_peak_bytes: int
    cpu_usage_usec: int
    io_read_bytes: int
    io_write_bytes: int


class ExecResult(TypedDict):
    stdout: str
    stderr: str
    exit_code: int
    duration_ms: int
    timed_out: bool
    oom_killed: bool
    stats: NotRequired[ResourceStats]


class NetworkSpecNone(TypedDict):
    mode: Literal["none"]


class NetworkSpecLoopback(TypedDict):
    mode: Literal["loopback"]


class NetworkSpecAllowlist(TypedDict):
    mode: Literal["allowlist"]
    domains: list[str]


NetworkSpec = NetworkSpecNone | NetworkSpecLoopback | NetworkSpecAllowlist


class ForkChild(TypedDict):
    code: str
    memory_mb: NotRequired[int]


class ForkMeta(TypedDict):
    clone_ms: int
    files_cloned: int
    files_cow: int
    bytes_cloned: int
    method: str
    was_frozen: bool


class ForkResult(TypedDict):
    parent: ExecResult
    child: ExecResult
    children: list[ExecResult]
    fork: ForkMeta
    forks: list[ForkMeta]


# ---- jails ------------------------------------------------------------


class JailRecord(TypedDict):
    id: int
    kind: JailKind
    started_at: str
    ended_at: str | None
    status: JailStatus
    session_id: str | None
    label: str
    exit_code: int | None
    duration_ms: int | None
    timed_out: bool | None
    oom_killed: bool | None
    memory_peak_bytes: int | None
    cpu_usage_usec: int | None
    io_read_bytes: int | None
    io_write_bytes: int | None
    stdout: str | None
    stderr: str | None
    error: str | None
    parent_id: NotRequired[int | None]


class JailsList(TypedDict):
    rows: list[JailRecord]
    total: int
    limit: int
    offset: int


# ---- workspaces -------------------------------------------------------


class WorkspaceSpec(TypedDict):
    memory_mb: int
    timeout_secs: int
    cpu_percent: int
    max_pids: int
    network_mode: Literal["none", "loopback", "allowlist"]
    network_domains: list[str]
    seccomp: SeccompSpec
    idle_timeout_secs: int


class WorkspaceDomain(TypedDict):
    domain: str
    backend_url: str


class Workspace(TypedDict):
    id: str
    created_at: str
    deleted_at: str | None
    source_dir: str
    output_dir: str
    config: WorkspaceSpec
    git_repo: str | None
    git_ref: str | None
    label: str | None
    domains: list[WorkspaceDomain]
    last_exec_at: str | None
    paused_at: str | None
    auto_snapshot: str | None


class WorkspaceList(TypedDict):
    rows: list[Workspace]
    total: int
    limit: int
    offset: int


class WorkspaceForkResponse(TypedDict):
    """Response from ``workspaces.fork`` — parent + N children + snapshot id."""

    parent: Workspace
    forks: list[Workspace]
    snapshot_id: str


class GitRepoEntry(TypedDict, total=False):
    """One entry in a multi-repo git seed."""

    repo: str
    ref: str
    dir: str


# ---- snapshots --------------------------------------------------------


class SnapshotRecord(TypedDict):
    id: str
    workspace_id: str | None
    name: str | None
    created_at: str
    path: str
    size_bytes: int


class SnapshotList(TypedDict):
    rows: list[SnapshotRecord]
    total: int
    limit: int
    offset: int


# ---- public -----------------------------------------------------------


class PublicStats(TypedDict):
    active_execs: int
    total_execs: int
    sessions: int
    credentials: int


# ---- stream events ----------------------------------------------------


class StreamStarted(TypedDict):
    type: Literal["started"]
    pid: int


class StreamLine(TypedDict):
    type: Literal["stdout", "stderr"]
    line: str


class StreamStats(TypedDict):
    type: Literal["stats"]
    memory_peak_bytes: int
    cpu_usage_usec: int
    io_read_bytes: int
    io_write_bytes: int


class StreamCompleted(TypedDict):
    type: Literal["completed"]
    exit_code: int
    duration_ms: int
    timed_out: bool
    oom_killed: bool
    memory_peak_bytes: int
    cpu_usage_usec: int


class StreamErrorEvent(TypedDict):
    type: Literal["error"]
    message: str


StreamEvent = StreamStarted | StreamLine | StreamStats | StreamCompleted | StreamErrorEvent
