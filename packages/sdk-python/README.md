# agentjail (Python)

Python client for the [agentjail] control plane. Mirrors the
[@agentjail/sdk][node] TypeScript package.

```python
from agentjail import Agentjail

aj = Agentjail(base_url="http://localhost:7000", api_key="aj_local_...")

aj.credentials.put(service="openai", secret="sk-real")

session = aj.sessions.create(services=["openai"], ttl_secs=600)
# Hand `session["env"]` to whatever sandbox you use — the sandbox
# sees only phantom tokens + base URLs pointing at the proxy.
```

## Install

```bash
pip install agentjail
```

Python ≥ 3.10. Depends on [httpx].

## API

- `credentials.list()` / `put(service, secret)` / `delete(service)`
- `sessions.create(services=, ttl_secs=, scopes=)` / `list()` / `get(id)` / `close(id)`
- `sessions.exec(id, cmd=, args=, ...)`
- `runs.create(code=, language=, ...)` / `fork(parent_code=, child_code | children=)` / `stream(...)`
- `workspaces.create(...)` / `list()` / `get(id)` / `delete(id)` / `exec(id, cmd=, args=)`
- `snapshots.create(workspace_id, name=)` / `list(...)` / `get(id)` / `delete(id)` / `create_workspace_from(snapshot_id, label=)`
- `audit.recent(limit=)`
- `jails.list(...)` / `jails.get(id)`
- `public.health()` / `public.stats()` — no auth

### Persistent workspaces + snapshots

```python
ws = aj.workspaces.create(
    git={"repo": "https://github.com/my-org/app", "ref": "main"},
    label="ci",
    idle_timeout_secs=60,
)

aj.workspaces.exec(ws["id"], cmd="bun", args=["install"])
baseline = aj.snapshots.create(ws["id"], name="deps-ready")

lint = aj.workspaces.exec(ws["id"], cmd="bun", args=["run", "lint"])
if lint["exit_code"] != 0:
    clean = aj.snapshots.create_workspace_from(baseline["id"], label="recovered")
    # …retry against clean["id"]
```

### Streaming

```python
for ev in aj.runs.stream(code="print(i for i in range(5))", language="python"):
    if ev["type"] == "stdout":
        print(ev["line"])
```

### Inbound hostname routing

Workspaces can declare `domains`; the server’s gateway listener
forwards matching `Host:` traffic to the caller-supplied `backend_url`.

```python
aj.workspaces.create(
    label="preview",
    domains=[{"domain": "review-42.preview.local", "backend_url": "http://127.0.0.1:3000"}],
)
```

See the [gateway docs](https://github.com/bugthesystem/agentjail) for
the on-server listener setup.

## Context-manager

```python
with Agentjail(base_url=..., api_key=...) as aj:
    aj.public.health()
```

## License

MIT.

[agentjail]: https://github.com/bugthesystem/agentjail
[node]: https://www.npmjs.com/package/@agentjail/sdk
[httpx]: https://www.python-httpx.org/
