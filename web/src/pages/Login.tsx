import { useState, type FormEvent } from "react";
import { useAuth } from "../lib/auth";
import { Button, Input } from "../components/ui";
import { ArrowRight, Shield, Cpu, Lock, Terminal, Sparkles } from "lucide-react";

export function LoginPage() {
  const { login, isLoading, error } = useAuth();
  const [baseUrl, setBaseUrl] = useState("http://localhost:7070");
  const [apiKey, setApiKey] = useState("");

  async function handleSubmit(e: FormEvent) {
    e.preventDefault();
    try { await login(baseUrl, apiKey); } catch { /* handled */ }
  }

  return (
    <div className="min-h-dvh flex bg-bg">
      {/* Left — brand panel */}
      <div className="hidden lg:flex lg:w-[56%] relative overflow-hidden">
        {/* Background effects */}
        <div className="absolute inset-0">
          {/* Radial gradient center */}
          <div className="absolute top-1/3 left-1/3 w-[600px] h-[600px] rounded-full bg-accent/[0.04] blur-[120px]" />
          <div className="absolute bottom-0 right-0 w-[400px] h-[400px] rounded-full bg-accent/[0.03] blur-[100px]" />
          {/* Subtle grid */}
          <div className="absolute inset-0 opacity-[0.025]" style={{
            backgroundImage: `linear-gradient(rgba(255,255,255,0.5) 1px, transparent 1px),
              linear-gradient(90deg, rgba(255,255,255,0.5) 1px, transparent 1px)`,
            backgroundSize: "80px 80px",
          }} />
        </div>

        {/* Content — vertically centered */}
        <div className="relative z-10 flex flex-col justify-between w-full px-16 py-14">
          {/* Top: brand */}
          <div className="flex items-center gap-3">
            <div className="w-9 h-9 rounded-xl bg-gradient-to-br from-accent via-accent to-accent-dim flex items-center justify-center shadow-lg shadow-accent/25">
              <Shield size={16} className="text-text-inverse" />
            </div>
            <span className="text-lg font-bold tracking-tight">agentjail</span>
          </div>

          {/* Middle: hero */}
          <div className="space-y-8 max-w-lg">
            <div>
              <div className="inline-flex items-center gap-1.5 px-3 py-1 rounded-full border border-accent/20 bg-accent/5 text-accent text-xs font-medium mb-6">
                <Sparkles size={12} />
                Open source sandbox platform
              </div>
              <h1 className="text-[2.75rem] font-extrabold tracking-tight leading-[1.08]">
                Let your agents build.
                <br />
                <span className="bg-gradient-to-r from-accent via-accent-hover to-accent bg-clip-text text-transparent">
                  You stay in control.
                </span>
              </h1>
              <p className="mt-5 text-[15px] text-text-secondary leading-relaxed max-w-md">
                Give AI agents the freedom to execute code, call APIs, and ship —
                without ever risking your credentials.
              </p>
            </div>

            {/* Features */}
            <div className="grid grid-cols-2 gap-3">
              {[
                { icon: Lock, label: "Zero-trust credentials", desc: "Keys never enter sandboxes" },
                { icon: Cpu, label: "Isolated execution", desc: "Linux namespace per session" },
                { icon: Terminal, label: "Live code runs", desc: "JS, Python, Bash in jails" },
                { icon: Shield, label: "Built-in guardrails", desc: "Memory, CPU, timeout limits" },
              ].map(({ icon: Icon, label, desc }) => (
                <div key={label} className="flex gap-3 p-3 rounded-xl border border-border/50 bg-bg-subtle/30">
                  <div className="w-8 h-8 rounded-lg bg-accent/8 flex items-center justify-center flex-shrink-0">
                    <Icon size={14} className="text-accent" />
                  </div>
                  <div>
                    <p className="text-[13px] font-medium text-text">{label}</p>
                    <p className="text-[11px] text-text-tertiary mt-0.5">{desc}</p>
                  </div>
                </div>
              ))}
            </div>

            {/* Code preview */}
            <div className="rounded-2xl border border-border/60 bg-bg/60 backdrop-blur-sm overflow-hidden">
              <div className="flex items-center gap-2 px-4 py-2.5 border-b border-border/40">
                <div className="flex gap-1.5">
                  <div className="w-[9px] h-[9px] rounded-full bg-[#ff5f57]" />
                  <div className="w-[9px] h-[9px] rounded-full bg-[#febc2e]" />
                  <div className="w-[9px] h-[9px] rounded-full bg-[#28c840]" />
                </div>
                <span className="text-[10px] text-text-tertiary font-mono ml-3 tracking-wide">quickstart.ts</span>
              </div>
              <pre className="px-5 py-4 text-[12.5px] font-mono leading-[1.7] text-text-tertiary overflow-x-auto">
<span className="text-text-tertiary/60">{"// your keys stay on the host\n"}</span>
<span className="text-accent/90">{"await"}</span><span className="text-text-secondary">{" aj.credentials.put({\n"}</span>
<span className="text-text-secondary">{"  service: "}</span><span className="text-success/80">{'"openai"'}</span><span className="text-text-secondary">{", secret: "}</span><span className="text-success/80">{'"sk-..."'}</span>
<span className="text-text-secondary">{"\n});\n\n"}</span>
<span className="text-text-tertiary/60">{"// agents only see phantom tokens\n"}</span>
<span className="text-accent/90">{"const"}</span><span className="text-text-secondary">{" result = "}</span><span className="text-accent/90">{"await"}</span><span className="text-text-secondary">{" aj.sessions.exec(\n"}</span>
<span className="text-text-secondary">{"  session.id, { cmd: "}</span><span className="text-success/80">{'"node"'}</span><span className="text-text-secondary">{", args: ["}</span><span className="text-success/80">{'"agent.js"'}</span><span className="text-text-secondary">{"] }\n);"}</span>
              </pre>
            </div>
          </div>

          {/* Bottom */}
          <div className="flex items-center gap-4 text-[11px] text-text-tertiary/60">
            <span>Open source</span>
            <span className="w-1 h-1 rounded-full bg-text-tertiary/30" />
            <span>MIT license</span>
            <span className="w-1 h-1 rounded-full bg-text-tertiary/30" />
            <span>Self-hosted</span>
          </div>
        </div>
      </div>

      {/* Right — form */}
      <div className="flex-1 flex items-center justify-center p-8 border-l border-border/40 bg-gradient-to-b from-bg-subtle/50 to-bg">
        <div className="w-full max-w-[340px] animate-fade-in">
          {/* Mobile brand */}
          <div className="lg:hidden flex items-center gap-2.5 mb-10">
            <div className="w-8 h-8 rounded-lg bg-gradient-to-br from-accent to-accent-dim flex items-center justify-center">
              <Shield size={14} className="text-text-inverse" />
            </div>
            <span className="font-bold tracking-tight">agentjail</span>
          </div>

          <h2 className="text-lg font-semibold tracking-tight">Connect</h2>
          <p className="mt-1.5 text-[13px] text-text-secondary">
            Enter your server URL and API key to get started.
          </p>

          <form onSubmit={handleSubmit} className="mt-7 space-y-5">
            <Input
              label="Server URL"
              placeholder="http://localhost:7070"
              value={baseUrl}
              onChange={(e) => setBaseUrl(e.target.value)}
              autoComplete="off"
              spellCheck={false}
              required
            />
            <Input
              label="API Key"
              type="password"
              placeholder="aj_…"
              value={apiKey}
              onChange={(e) => setApiKey(e.target.value)}
              autoComplete="new-password"
              spellCheck={false}
            />

            {error && (
              <div className="rounded-lg bg-error/6 border border-error/12 px-3.5 py-2.5 text-[13px] text-error/90 animate-slide-up">
                {error}
              </div>
            )}

            <Button type="submit" className="w-full h-10 group" disabled={isLoading}>
              {isLoading ? (
                <span className="flex items-center gap-2">
                  <span className="w-3.5 h-3.5 border-2 border-text-inverse/30 border-t-text-inverse rounded-full animate-spin" />
                  Connecting…
                </span>
              ) : (
                <>
                  Connect
                  <ArrowRight size={14} className="transition-transform duration-200 group-hover:translate-x-0.5" />
                </>
              )}
            </Button>
          </form>

          <p className="mt-6 text-center text-[11px] text-text-tertiary/60">
            No account needed &middot; connects directly to your server
          </p>
        </div>
      </div>
    </div>
  );
}
