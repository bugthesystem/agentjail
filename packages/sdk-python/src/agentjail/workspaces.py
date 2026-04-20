"""Workspaces — persistent multi-exec mount trees."""

from __future__ import annotations

from typing import Any

from ._http import HttpClient
from .types import ExecResult, Workspace, WorkspaceDomain, WorkspaceList


class Workspaces:
    def __init__(self, http: HttpClient) -> None:
        self._http = http

    def create(
        self,
        *,
        git: dict[str, str] | None = None,
        label: str | None = None,
        memory_mb: int | None = None,
        timeout_secs: int | None = None,
        idle_timeout_secs: int | None = None,
        domains: list[WorkspaceDomain] | None = None,
        network: Any = None,
        seccomp: str | None = None,
        cpu_percent: int | None = None,
        max_pids: int | None = None,
    ) -> Workspace:
        body: dict[str, Any] = {}
        if git is not None:
            body["git"] = git
        if label is not None:
            body["label"] = label
        if memory_mb is not None:
            body["memory_mb"] = memory_mb
        if timeout_secs is not None:
            body["timeout_secs"] = timeout_secs
        if idle_timeout_secs is not None:
            body["idle_timeout_secs"] = idle_timeout_secs
        if domains is not None:
            body["domains"] = domains
        if network is not None:
            body["network"] = network
        if seccomp is not None:
            body["seccomp"] = seccomp
        if cpu_percent is not None:
            body["cpu_percent"] = cpu_percent
        if max_pids is not None:
            body["max_pids"] = max_pids
        return self._http.request("POST", "/v1/workspaces", json=body)

    def list(
        self, *, limit: int | None = None, offset: int | None = None
    ) -> WorkspaceList:
        return self._http.request(
            "GET", "/v1/workspaces", params={"limit": limit, "offset": offset}
        )

    def get(self, workspace_id: str) -> Workspace:
        return self._http.request("GET", f"/v1/workspaces/{workspace_id}")

    def delete(self, workspace_id: str) -> None:
        self._http.request("DELETE", f"/v1/workspaces/{workspace_id}")

    def exec(
        self,
        workspace_id: str,
        *,
        cmd: str,
        args: list[str] | None = None,
        timeout_secs: int | None = None,
        memory_mb: int | None = None,
        env: list[tuple[str, str]] | None = None,
    ) -> ExecResult:
        body: dict[str, Any] = {"cmd": cmd}
        if args is not None:
            body["args"] = args
        if timeout_secs is not None:
            body["timeout_secs"] = timeout_secs
        if memory_mb is not None:
            body["memory_mb"] = memory_mb
        if env is not None:
            body["env"] = [list(pair) for pair in env]
        return self._http.request("POST", f"/v1/workspaces/{workspace_id}/exec", json=body)
