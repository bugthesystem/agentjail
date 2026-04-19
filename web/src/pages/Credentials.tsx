import { useState, type FormEvent } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useApi } from "../lib/auth";
import { Card, CardHeader, CardBody, Badge, Button, Input, EmptyState, CodeBlock } from "../components/ui";
import { Key, Plus, Trash2, Fingerprint } from "lucide-react";
import type { ServiceId } from "../lib/api";

const services: ServiceId[] = ["openai", "anthropic", "github", "stripe"];

export function CredentialsPage() {
  const api = useApi();
  const qc = useQueryClient();
  const creds = useQuery({ queryKey: ["credentials"], queryFn: () => api.credentials.list() });
  const [showForm, setShowForm] = useState(false);
  const [service, setService] = useState<ServiceId>("openai");
  const [secret, setSecret] = useState("");

  const addMut = useMutation({
    mutationFn: () => api.credentials.put(service, secret),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["credentials"] });
      setShowForm(false);
      setSecret("");
    },
  });

  const delMut = useMutation({
    mutationFn: (svc: ServiceId) => api.credentials.delete(svc),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["credentials"] }),
  });

  const list = creds.data ?? [];

  function handleSubmit(e: FormEvent) {
    e.preventDefault();
    addMut.mutate();
  }

  return (
    <div className="space-y-6 animate-fade-in">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-xl font-semibold tracking-tight">Credentials</h1>
          <p className="text-sm text-text-tertiary mt-1">
            Real API keys. Never sent to any sandbox.
          </p>
        </div>
        <Button onClick={() => setShowForm(true)} size="sm">
          <Plus size={14} /> Add Key
        </Button>
      </div>

      {/* Add form */}
      {showForm && (
        <Card className="animate-slide-up">
          <CardBody>
            <form onSubmit={handleSubmit} className="space-y-4">
              <div className="space-y-1.5">
                <label className="text-sm font-medium text-text-secondary">Service</label>
                <select
                  value={service}
                  onChange={(e) => setService(e.target.value as ServiceId)}
                  className="w-full h-9 px-3 rounded-lg text-sm bg-bg-subtle border border-border hover:border-text-tertiary focus:outline-none focus:ring-2 focus:ring-accent"
                >
                  {services.map((s) => (
                    <option key={s} value={s}>{s}</option>
                  ))}
                </select>
              </div>
              <Input
                label="Secret"
                type="password"
                placeholder="sk-…"
                value={secret}
                onChange={(e) => setSecret(e.target.value)}
                autoComplete="off"
                required
              />
              {addMut.error && (
                <p className="text-sm text-error">{(addMut.error as Error).message}</p>
              )}
              <div className="flex gap-2">
                <Button type="submit" size="sm" disabled={addMut.isPending}>
                  {addMut.isPending ? "Saving…" : "Save"}
                </Button>
                <Button type="button" variant="ghost" size="sm" onClick={() => setShowForm(false)}>
                  Cancel
                </Button>
              </div>
            </form>
          </CardBody>
        </Card>
      )}

      {/* List */}
      <Card>
        <CardHeader>
          <div className="flex items-center gap-2">
            <Key size={14} className="text-text-tertiary" />
            <h2 className="text-sm font-medium">Configured Keys</h2>
            <Badge>{list.length}</Badge>
          </div>
        </CardHeader>
        <CardBody className="p-0">
          {list.length === 0 ? (
            <EmptyState
              icon={<Key size={24} />}
              title="No credentials configured"
              description="Add a real API key. It stays on the host — sandboxes only see phantom tokens."
              action={
                <CodeBlock language="typescript">{`await aj.credentials.put({ service: "openai", secret: "sk-..." })`}</CodeBlock>
              }
            />
          ) : (
            list.map((cred) => (
              <div
                key={cred.service}
                className="flex items-center justify-between px-5 py-3.5 border-b border-border-subtle hover:bg-bg-muted/50 transition-colors"
              >
                <div className="flex items-center gap-3">
                  <Badge variant="accent">{cred.service}</Badge>
                  <span className="flex items-center gap-1.5 text-xs text-text-tertiary font-mono">
                    <Fingerprint size={12} />
                    {cred.fingerprint.slice(0, 12)}…
                  </span>
                </div>
                <div className="flex items-center gap-3 text-xs text-text-tertiary">
                  <span>Updated {new Date(cred.updated_at).toLocaleDateString()}</span>
                  <Button
                    variant="danger"
                    size="sm"
                    onClick={() => delMut.mutate(cred.service)}
                    aria-label={`Delete ${cred.service} credential`}
                  >
                    <Trash2 size={14} />
                  </Button>
                </div>
              </div>
            ))
          )}
        </CardBody>
      </Card>
    </div>
  );
}
