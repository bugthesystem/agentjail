import { DocPage, Section, Inline, Hint } from "../../components/docs/DocPage";
import { Code } from "../../components/docs/Code";

export function Snapshots() {
  return (
    <DocPage
      eyebrow="Concepts"
      title="Named snapshots"
      lead={
        <>
          Capture a workspace&rsquo;s <Inline>output</Inline> dir at a moment in
          time and restore it into a fresh workspace later. Safe to call
          mid-exec: the engine freezes the running jail&rsquo;s cgroup around
          the copy so the capture is consistent.
        </>
      }
    >
      <Section id="create" title="Capture a snapshot">
        <p>
          Optional <Inline>name</Inline> is purely for humans. The returned{" "}
          <Inline>size_bytes</Inline> is what was copied (symlinks are skipped
          for safety).
        </p>
        <Code lang="ts">{`const snap = await aj.snapshots.create(ws.id, { name: "post-install" });
console.log(snap.id, snap.size_bytes, "bytes");`}</Code>
      </Section>

      <Section id="mid-run" title="Mid-run snapshots">
        <p>
          If an exec is currently running against the workspace, the server
          looks up its cgroup and freezes it for the duration of the copy —
          sub-ms on cgroup v2. On systems without cgroup freeze, it logs a
          warning and falls back to a plain read.
        </p>
        <Code lang="text">{`POST /v1/workspaces/wrk_abc/snapshot
  1. look up workspace cgroup_path (if exec in flight)
  2. freeze_cgroup(path)   ·   <1ms
  3. Snapshot::create(output_dir, snapshots/<id>/)
  4. thaw_cgroup(path)      ·   <1ms
  5. INSERT into snapshots (...)`}</Code>
        <Hint title="No extra contention" tone="phantom">
          Snapshots do <em>not</em> take the per-workspace exec mutex — they
          run alongside an in-flight exec, relying on freeze-during-copy for
          coherence. Two snapshots of the same workspace serialize at the
          filesystem layer only.
        </Hint>
      </Section>

      <Section id="restore" title="Restore into a new workspace">
        <p>
          Snapshots don&rsquo;t mutate their parent. Use them to branch a new
          workspace from a known-good point:
        </p>
        <Code lang="ts">{`const lint = await aj.workspaces.exec(ws.id, {
  cmd: "bun", args: ["run", "lint"],
});
if (lint.exit_code !== 0) {
  // Roll back to the post-install state in a fresh workspace.
  const clean = await aj.snapshots.createWorkspaceFrom(snap.id, {
    label: "recovered",
  });
  // retry against \`clean.id\`
}`}</Code>
        <p>
          The new workspace inherits the parent&rsquo;s jail config (memory,
          network policy, seccomp, limits) when the parent is still alive.
          When the parent has been deleted, sensible defaults apply and the
          snapshot stays useful anyway.
        </p>
      </Section>

      <Section id="lifecycle" title="Listing + deleting">
        <Code lang="ts">{`const page = await aj.snapshots.list({ workspaceId: ws.id, limit: 50 });
for (const s of page.rows) {
  console.log(s.id, s.name, s.size_bytes);
}

await aj.snapshots.delete(snap.id);`}</Code>
        <p>
          Hard-deleting a workspace sets each snapshot&rsquo;s{" "}
          <Inline>workspace_id</Inline> to <Inline>null</Inline>, not the
          snapshot itself. Manage retention with{" "}
          <Inline>snapshots.delete(id)</Inline> or run the server with{" "}
          <Inline>AGENTJAIL_SNAPSHOT_MAX_COUNT</Inline> /{" "}
          <Inline>AGENTJAIL_SNAPSHOT_MAX_AGE_SECS</Inline> set.
        </p>
      </Section>
    </DocPage>
  );
}
