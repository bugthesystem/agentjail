"""Read-only snapshot of the running control plane's configuration."""

from __future__ import annotations

from ._http import HttpClient
from .types import SettingsSnapshot


class Settings:
    """`GET /v1/config` — operator-facing server config.

    Safe-to-display fields only (bind addresses, GC policy, provider
    metadata). Credentials never appear here.
    """

    def __init__(self, http: HttpClient) -> None:
        self._http = http

    def get(self) -> SettingsSnapshot:
        """Fetch the current settings snapshot."""
        resp = self._http.request("GET", "/v1/config")
        return resp  # type: ignore[return-value]
