import { useState, type FormEvent } from "react";
import { useAuth } from "../lib/auth";
import { Button, Input } from "../components/ui";
import { Zap, ArrowRight, Terminal } from "lucide-react";

export function LoginPage() {
  const { login, isLoading, error } = useAuth();
  const [baseUrl, setBaseUrl] = useState("http://localhost:7070");
  const [apiKey, setApiKey] = useState("");

  async function handleSubmit(e: FormEvent) {
    e.preventDefault();
    try {
      await login(baseUrl, apiKey);
    } catch {
      // error state handled by auth context
    }
  }

  return (
    <div className="min-h-dvh flex">
      {/* Left — branding */}
      <div className="hidden lg:flex lg:w-1/2 bg-bg-subtle border-r border-border flex-col justify-between p-12">
        <div>
          <div className="flex items-center gap-3">
            <div className="w-10 h-10 rounded-xl bg-accent flex items-center justify-center">
              <Zap size={20} className="text-text-inverse" />
            </div>
            <div>
              <h1 className="text-xl font-bold tracking-tight">agentjail</h1>
              <p className="text-xs text-text-tertiary">secure sandbox platform</p>
            </div>
          </div>
        </div>

        <div className="space-y-8">
          <div className="space-y-4">
            <p className="text-sm text-text-secondary leading-relaxed max-w-md">
              Run untrusted AI-agent code in isolated Linux namespaces.
              Real API keys never enter the sandbox &mdash; only phantom tokens
              that die when the session ends.
            </p>
          </div>

          <div className="rounded-xl border border-border bg-bg p-4 font-mono text-sm space-y-2">
            <div className="flex items-center gap-2 text-text-tertiary">
              <Terminal size={14} />
              <span className="text-2xs uppercase tracking-wider">Quick start</span>
            </div>
            <pre className="text-text-secondary">
{`const aj = new Agentjail({ apiKey });

const session = await aj.sessions.create({
  services: ["openai"],
});

// Real key never enters the jail
const result = await aj.sessions.exec(
  session.id,
  { cmd: "node", args: ["agent.js"] },
);`}
            </pre>
          </div>
        </div>

        <p className="text-2xs text-text-tertiary">
          Open source &middot; MIT license &middot; Self-hosted
        </p>
      </div>

      {/* Right — form */}
      <div className="flex-1 flex items-center justify-center p-8">
        <div className="w-full max-w-sm space-y-8">
          <div className="lg:hidden flex items-center gap-3 mb-8">
            <div className="w-8 h-8 rounded-lg bg-accent flex items-center justify-center">
              <Zap size={16} className="text-text-inverse" />
            </div>
            <span className="font-semibold tracking-tight">agentjail</span>
          </div>

          <div>
            <h2 className="text-lg font-semibold">Connect to control plane</h2>
            <p className="mt-1 text-sm text-text-tertiary">
              Enter the URL of your agentjail server and API key.
            </p>
          </div>

          <form onSubmit={handleSubmit} className="space-y-4">
            <Input
              label="Server URL"
              placeholder="http://localhost:7070"
              value={baseUrl}
              onChange={(e) => setBaseUrl(e.target.value)}
              autoComplete="url"
              spellCheck={false}
              required
            />
            <Input
              label="API Key"
              type="password"
              placeholder="aj_…"
              value={apiKey}
              onChange={(e) => setApiKey(e.target.value)}
              autoComplete="off"
              spellCheck={false}
            />

            {error && (
              <div className="rounded-lg bg-error/10 border border-error/20 px-3 py-2 text-sm text-error animate-fade-in">
                {error}
              </div>
            )}

            <Button type="submit" className="w-full" disabled={isLoading}>
              {isLoading ? (
                "Connecting…"
              ) : (
                <>
                  Connect <ArrowRight size={14} />
                </>
              )}
            </Button>
          </form>

          <p className="text-center text-2xs text-text-tertiary">
            No account needed &middot; connects directly to your server
          </p>
        </div>
      </div>
    </div>
  );
}
