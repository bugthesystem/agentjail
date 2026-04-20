"""Snapshots — capture & restore workspace output dirs."""

from __future__ import annotations

from typing import Any

from ._http import HttpClient
from .types import SnapshotList, SnapshotRecord, Workspace


class Snapshots:
    def __init__(self, http: HttpClient) -> None:
        self._http = http

    def create(
        self,
        workspace_id: str,
        *,
        name: str | None = None,
    ) -> SnapshotRecord:
        body: dict[str, Any] = {}
        if name is not None:
            body["name"] = name
        return self._http.request(
            "POST", f"/v1/workspaces/{workspace_id}/snapshot", json=body
        )

    def list(
        self,
        *,
        workspace_id: str | None = None,
        limit: int | None = None,
        offset: int | None = None,
    ) -> SnapshotList:
        return self._http.request(
            "GET",
            "/v1/snapshots",
            params={"workspace_id": workspace_id, "limit": limit, "offset": offset},
        )

    def get(self, snapshot_id: str) -> SnapshotRecord:
        return self._http.request("GET", f"/v1/snapshots/{snapshot_id}")

    def delete(self, snapshot_id: str) -> None:
        self._http.request("DELETE", f"/v1/snapshots/{snapshot_id}")

    def create_workspace_from(
        self, snapshot_id: str, *, label: str | None = None
    ) -> Workspace:
        body: dict[str, Any] = {"snapshot_id": snapshot_id}
        if label is not None:
            body["label"] = label
        return self._http.request("POST", "/v1/workspaces/from-snapshot", json=body)
