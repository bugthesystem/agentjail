#!/usr/bin/env bash
#
# End-to-end smoke test for persistent workspaces + named snapshots.
#
# Requires the control plane running + reachable at $CTL_URL (default
# http://localhost:7000) with $AGENTJAIL_API_KEY set to a valid key
# (unset in open dev mode).
#
# Flow:
#   1. Create a workspace (no git).
#   2. Exec: write a file into /workspace (output dir inside jail).
#   3. Snapshot the workspace.
#   4. Exec: rm the file.
#   5. Verify it's gone.
#   6. Restore the snapshot into a fresh workspace.
#   7. Verify the file is back.
#   8. Clean up both workspaces + the snapshot.
#
# All assertions are `jq` one-liners; the script exits non-zero on the
# first failure.

set -euo pipefail

CTL_URL="${CTL_URL:-http://localhost:7000}"
API_KEY="${AGENTJAIL_API_KEY:-}"

need() { command -v "$1" >/dev/null 2>&1 || { echo "missing: $1"; exit 2; }; }
need curl
need jq

AUTH=()
if [ -n "$API_KEY" ]; then
  AUTH=(-H "authorization: Bearer $API_KEY")
fi

call() {
  # call METHOD PATH [BODY] -> prints JSON to stdout
  local method="$1" path="$2" body="${3:-}"
  if [ -n "$body" ]; then
    curl -sS -f "${AUTH[@]}" -H 'content-type: application/json' \
      -X "$method" "$CTL_URL$path" -d "$body"
  else
    curl -sS -f "${AUTH[@]}" -X "$method" "$CTL_URL$path"
  fi
}

assert_eq() {
  local label="$1" got="$2" want="$3"
  if [ "$got" != "$want" ]; then
    echo "FAIL: $label: got $got, want $want"
    exit 1
  fi
}

echo "▸ Creating workspace"
WS=$(call POST /v1/workspaces '{"memory_mb":256,"timeout_secs":30}')
WS_ID=$(jq -r .id <<<"$WS")
echo "  id: $WS_ID"

echo "▸ Exec: write file"
WRITE=$(call POST "/v1/workspaces/$WS_ID/exec" '{"cmd":"/bin/sh","args":["-c","echo baseline > /workspace/marker.txt; cat /workspace/marker.txt"]}')
assert_eq "exec exit_code" "$(jq -r .exit_code <<<"$WRITE")" "0"
assert_eq "exec stdout"     "$(jq -r .stdout    <<<"$WRITE")" "baseline"

echo "▸ Snapshot workspace"
SNAP=$(call POST "/v1/workspaces/$WS_ID/snapshot" '{"name":"baseline"}')
SNAP_ID=$(jq -r .id <<<"$SNAP")
SNAP_SIZE=$(jq -r .size_bytes <<<"$SNAP")
echo "  id: $SNAP_ID ($SNAP_SIZE bytes)"
if [ "$SNAP_SIZE" = "0" ]; then
  echo "FAIL: snapshot size is 0"
  exit 1
fi

echo "▸ Exec: remove file"
RM=$(call POST "/v1/workspaces/$WS_ID/exec" '{"cmd":"/bin/sh","args":["-c","rm /workspace/marker.txt; ls /workspace"]}')
assert_eq "rm exit_code" "$(jq -r .exit_code <<<"$RM")" "0"

echo "▸ Verify file is gone"
LS=$(call POST "/v1/workspaces/$WS_ID/exec" '{"cmd":"/bin/sh","args":["-c","ls /workspace | wc -l"]}')
assert_eq "ls count" "$(jq -r '.stdout | tonumber' <<<"$LS")" "0"

echo "▸ Restore snapshot into a new workspace"
WS2=$(call POST /v1/workspaces/from-snapshot "$(jq -c --arg id "$SNAP_ID" '{snapshot_id: $id}' <<<'{}')")
WS2_ID=$(jq -r .id <<<"$WS2")
echo "  new id: $WS2_ID"

echo "▸ Verify file is back in new workspace"
CAT=$(call POST "/v1/workspaces/$WS2_ID/exec" '{"cmd":"/bin/sh","args":["-c","cat /workspace/marker.txt"]}')
assert_eq "restored stdout" "$(jq -r .stdout <<<"$CAT")" "baseline"

echo "▸ Cleanup"
call DELETE "/v1/workspaces/$WS_ID"  >/dev/null
call DELETE "/v1/workspaces/$WS2_ID" >/dev/null
call DELETE "/v1/snapshots/$SNAP_ID" >/dev/null

echo "✓ workspaces + snapshots E2E passed"
