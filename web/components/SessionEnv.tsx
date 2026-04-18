import { CodeBlock } from "./CodeBlock";
import { CopyButton } from "./CopyButton";

/** Renders the env map as dotenv-style lines with a copy button. */
export function SessionEnv({ env }: { env: Record<string, string> }) {
  const lines = Object.entries(env)
    .map(([k, v]) => `${k}=${v}`)
    .join("\n");
  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-center justify-between">
        <span className="text-[11px] uppercase tracking-wider text-muted">
          Environment
        </span>
        <CopyButton value={lines} label="Copy env" />
      </div>
      <CodeBlock code={lines} />
      <p className="text-[11px] text-muted">
        These are phantom tokens. Worthless off the proxy; revoked when the
        session is closed.
      </p>
    </div>
  );
}
