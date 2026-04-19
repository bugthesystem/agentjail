import { useState, type FormEvent } from "react";
import { useAuth } from "../lib/auth";
import { Button, Input } from "../components/ui";
import { ArrowRight, Shield, Cpu, Lock, Terminal } from "lucide-react";

export function LoginPage() {
  const { login, isLoading, error } = useAuth();
  const [baseUrl, setBaseUrl] = useState("http://localhost:7070");
  const [apiKey, setApiKey] = useState("");

  async function handleSubmit(e: FormEvent) {
    e.preventDefault();
    try { await login(baseUrl, apiKey); } catch { /* handled by context */ }
  }

  return (
    <div className="min-h-dvh flex noise">
      {/* Left — immersive brand */}
      <div className="hidden lg:flex lg:w-[55%] relative overflow-hidden flex-col justify-between p-12">
        {/* Gradient orbs */}
        <div className="absolute -top-32 -left-32 w-96 h-96 rounded-full bg-accent/5 blur-3xl animate-glow" />
        <div className="absolute bottom-20 right-10 w-64 h-64 rounded-full bg-accent/3 blur-3xl animate-glow" style={{ animationDelay: "1.5s" }} />

        {/* Grid lines */}
        <div className="absolute inset-0 opacity-[0.02]" style={{
          backgroundImage: "linear-gradient(var(--color-text) 1px, transparent 1px), linear-gradient(90deg, var(--color-text) 1px, transparent 1px)",
          backgroundSize: "60px 60px",
        }} />

        {/* Content */}
        <div className="relative z-10">
          <div className="flex items-center gap-3">
            <div className="w-10 h-10 rounded-xl bg-gradient-to-br from-accent to-accent-dim flex items-center justify-center shadow-lg shadow-accent/20">
              <Shield size={18} className="text-text-inverse" />
            </div>
            <h1 className="text-2xl font-bold tracking-tight">agentjail</h1>
          </div>
        </div>

        <div className="relative z-10 space-y-10">
          <div className="max-w-lg">
            <h2 className="text-4xl font-bold tracking-tight leading-[1.1]">
              Let your agents build.
              <br />
              <span className="text-accent">You stay in control.</span>
            </h2>
            <p className="mt-4 text-text-secondary leading-relaxed max-w-md">
              Give AI agents the freedom to execute code, call APIs, and ship —
              without ever risking your credentials. Every session is isolated.
              Every secret is protected.
            </p>
          </div>

          {/* Feature pills */}
          <div className="flex flex-wrap gap-2">
            {[
              { icon: Lock, text: "Zero-trust credentials" },
              { icon: Cpu, text: "Isolated sandboxes" },
              { icon: Terminal, text: "Live code execution" },
              { icon: Shield, text: "Built-in guardrails" },
            ].map(({ icon: Icon, text }) => (
              <div key={text} className="flex items-center gap-2 px-3 py-1.5 rounded-full border border-border bg-bg-subtle/50 text-sm text-text-secondary">
                <Icon size={13} className="text-accent" />
                {text}
              </div>
            ))}
          </div>

          {/* Code preview */}
          <div className="max-w-lg rounded-xl border border-border bg-bg/80 backdrop-blur-sm overflow-hidden card-glow">
            <div className="flex items-center gap-2 px-4 py-2.5 border-b border-border">
              <div className="flex gap-1.5">
                <div className="w-2.5 h-2.5 rounded-full bg-error/50" />
                <div className="w-2.5 h-2.5 rounded-full bg-warning/50" />
                <div className="w-2.5 h-2.5 rounded-full bg-success/50" />
              </div>
              <span className="text-2xs text-text-tertiary font-mono ml-2">quickstart.ts</span>
            </div>
            <pre className="p-4 text-[13px] font-mono leading-relaxed text-text-secondary overflow-x-auto">
<span className="text-text-tertiary">{"// credentials stay on the host\n"}</span>
<span className="text-accent">{"await"}</span>{" aj.credentials.put({\n  service: "}
<span className="text-success">{"\"openai\""}</span>{",\n  secret: "}
<span className="text-success">{"\"sk-...\""}</span>{"\n});\n\n"}
<span className="text-text-tertiary">{"// sandbox only sees phantom tokens\n"}</span>
<span className="text-accent">{"const"}</span>{" result = "}
<span className="text-accent">{"await"}</span>{" aj.sessions.exec(\n  session.id,\n  { cmd: "}
<span className="text-success">{"\"node\""}</span>{", args: ["}
<span className="text-success">{"\"agent.js\""}</span>{"] },\n);"}
            </pre>
          </div>
        </div>

        <div className="relative z-10 flex items-center gap-4 text-2xs text-text-tertiary">
          <span>Open source</span>
          <span className="w-px h-3 bg-border" />
          <span>MIT license</span>
          <span className="w-px h-3 bg-border" />
          <span>Self-hosted</span>
        </div>
      </div>

      {/* Right — login form */}
      <div className="flex-1 flex items-center justify-center p-8 bg-bg-subtle/30">
        <div className="w-full max-w-sm space-y-8 animate-fade-in">
          {/* Mobile brand */}
          <div className="lg:hidden flex items-center gap-3 mb-4">
            <div className="w-9 h-9 rounded-xl bg-gradient-to-br from-accent to-accent-dim flex items-center justify-center">
              <Shield size={16} className="text-text-inverse" />
            </div>
            <span className="font-bold text-lg tracking-tight">agentjail</span>
          </div>

          <div>
            <h2 className="text-xl font-semibold tracking-tight">Connect</h2>
            <p className="mt-1.5 text-sm text-text-secondary">
              Enter your server URL and API key to get started.
            </p>
          </div>

          <form onSubmit={handleSubmit} className="space-y-5">
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
              <div className="rounded-lg bg-error/8 border border-error/15 px-3 py-2.5 text-sm text-error animate-slide-up">
                {error}
              </div>
            )}

            <Button type="submit" className="w-full group" disabled={isLoading}>
              {isLoading ? (
                <span className="flex items-center gap-2">
                  <span className="w-3.5 h-3.5 border-2 border-text-inverse/30 border-t-text-inverse rounded-full animate-spin" />
                  Connecting…
                </span>
              ) : (
                <>
                  Connect
                  <ArrowRight size={14} className="transition-transform group-hover:translate-x-0.5" />
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
