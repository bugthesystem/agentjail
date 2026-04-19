import { useState, type FormEvent } from "react";
import { useMutation } from "@tanstack/react-query";
import { useApi } from "../lib/auth";
import { Card, CardHeader, CardBody, Badge, Button, EmptyState } from "../components/ui";
import { Terminal, Play, Clock, Cpu } from "lucide-react";
import type { ExecResult } from "../lib/api";

type Language = "javascript" | "python" | "bash";

export function RunsPage() {
  const api = useApi();
  const [code, setCode] = useState('console.log("hello from agentjail");');
  const [language, setLanguage] = useState<Language>("javascript");
  const [history, setHistory] = useState<Array<{ code: string; lang: Language; result: ExecResult }>>([]);

  const runMut = useMutation({
    mutationFn: () => api.runs.create(code, language, 30),
    onSuccess: (result) => {
      setHistory((prev) => [{ code, lang: language, result }, ...prev].slice(0, 20));
    },
  });

  function handleSubmit(e: FormEvent) {
    e.preventDefault();
    runMut.mutate();
  }

  return (
    <div className="space-y-6 animate-fade-in">
      <div>
        <h1 className="text-xl font-semibold tracking-tight">Runs</h1>
        <p className="text-sm text-text-tertiary mt-1">
          Execute code in a fresh Linux jail. One-shot, no session needed.
        </p>
      </div>

      {/* Editor */}
      <Card>
        <CardHeader>
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-2">
              <Terminal size={14} className="text-accent" />
              <h2 className="text-sm font-medium">Code</h2>
            </div>
            <div className="flex items-center gap-2">
              {(["javascript", "python", "bash"] as const).map((lang) => (
                <button
                  key={lang}
                  onClick={() => setLanguage(lang)}
                  className={`px-2.5 py-1 rounded-md text-xs font-medium transition-colors ${
                    language === lang
                      ? "bg-accent text-text-inverse"
                      : "text-text-tertiary hover:text-text hover:bg-bg-emphasis"
                  }`}
                >
                  {lang}
                </button>
              ))}
            </div>
          </div>
        </CardHeader>
        <CardBody className="p-0">
          <form onSubmit={handleSubmit}>
            <textarea
              value={code}
              onChange={(e) => setCode(e.target.value)}
              className="w-full min-h-[160px] p-4 bg-bg font-mono text-sm text-text-secondary resize-y border-none outline-none"
              placeholder="Write your code…"
              spellCheck={false}
            />
            <div className="flex items-center justify-between px-4 py-3 border-t border-border">
              {runMut.error && (
                <p className="text-sm text-error">{(runMut.error as Error).message}</p>
              )}
              <div className="ml-auto">
                <Button type="submit" size="sm" disabled={runMut.isPending || !code.trim()}>
                  {runMut.isPending ? "Running…" : <><Play size={14} /> Run</>}
                </Button>
              </div>
            </div>
          </form>
        </CardBody>
      </Card>

      {/* Results */}
      {history.length === 0 && !runMut.data ? (
        <EmptyState
          icon={<Play size={24} />}
          title="No runs yet"
          description="Write some code above and hit Run to execute it in a sandbox."
        />
      ) : (
        <div className="space-y-3">
          {history.map((entry, i) => (
            <RunResult key={i} {...entry} />
          ))}
        </div>
      )}
    </div>
  );
}

function RunResult({ lang, result }: { code: string; lang: string; result: ExecResult }) {
  const ok = result.exit_code === 0;
  return (
    <Card className="animate-slide-up">
      <CardHeader>
        <div className="flex items-center gap-2">
          <Badge variant={ok ? "success" : "error"}>exit {result.exit_code}</Badge>
          <Badge>{lang}</Badge>
          <span className="flex items-center gap-1 text-xs text-text-tertiary ml-auto">
            <Clock size={12} /> {result.duration_ms}ms
          </span>
          {result.stats && (
            <span className="flex items-center gap-1 text-xs text-text-tertiary">
              <Cpu size={12} /> {(result.stats.memory_peak_bytes / 1024 / 1024).toFixed(1)}&nbsp;MB
            </span>
          )}
          {result.timed_out && <Badge variant="warning">timeout</Badge>}
          {result.oom_killed && <Badge variant="error">OOM</Badge>}
        </div>
      </CardHeader>
      <CardBody className="p-0">
        {result.stdout && (
          <pre className="px-4 py-3 font-mono text-sm text-text-secondary border-b border-border-subtle whitespace-pre-wrap">
            {result.stdout}
          </pre>
        )}
        {result.stderr && (
          <pre className="px-4 py-3 font-mono text-sm text-error/80 whitespace-pre-wrap">
            {result.stderr}
          </pre>
        )}
      </CardBody>
    </Card>
  );
}
