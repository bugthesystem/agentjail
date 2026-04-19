import { useQuery } from "@tanstack/react-query";
import { useMemo, useState } from "react";
import { useApi } from "../lib/auth";
import type { JailStatus } from "../lib/api";
import { Panel, PanelHeader } from "../components/Panel";
import { Pill } from "../components/Pill";
import { JailsList } from "../components/jails/JailsList";
import { JailDetail } from "../components/jails/JailDetail";

const FILTERS: { id: JailStatus | "all"; label: string }[] = [
  { id: "all",       label: "all"       },
  { id: "running",   label: "running"   },
  { id: "completed", label: "completed" },
  { id: "error",     label: "error"     },
];

export function Jails() {
  const api = useApi();
  const [filter, setFilter] = useState<JailStatus | "all">("all");
  const [selected, setSelected] = useState<number | null>(null);

  const { data } = useQuery({
    queryKey: ["jails", 200, filter],
    queryFn: () =>
      api.jails.list({
        limit: 200,
        status: filter === "all" ? undefined : filter,
      }),
    refetchInterval: 2000,
  });

  const rows = data?.rows ?? [];

  const { data: detail } = useQuery({
    queryKey: ["jail", selected],
    queryFn: () => api.jails.get(selected!),
    enabled: selected !== null,
    refetchInterval: (q) =>
      q.state.data?.status === "running" ? 1000 : false,
  });

  const counts = useMemo(() => {
    const base = { all: data?.total ?? 0, running: 0, completed: 0, error: 0 };
    for (const r of rows) base[r.status] += 1;
    return base;
  }, [rows, data]);

  return (
    <div className="grid gap-4" style={{ gridTemplateColumns: "minmax(0,1fr) 560px" }}>
      <Panel padded={false}>
        <div className="px-5 py-3 flex items-center justify-between">
          <PanelHeader
            eyebrow="Jails"
            title="Jail run ledger"
            className="!mb-0"
          />
          <div className="flex items-center gap-2">
            <Pill tone="phantom" dot>live</Pill>
            <span className="text-[11px] mono text-ink-500">
              {data?.total ?? 0} total
            </span>
          </div>
        </div>
        <div className="hairline" />
        <div className="px-5 py-2 flex items-center gap-1">
          {FILTERS.map((f) => (
            <button
              key={f.id}
              onClick={() => setFilter(f.id)}
              className={`h-7 px-3 rounded-full text-[11px] mono transition-colors ${
                filter === f.id
                  ? "bg-ink-100 text-ink-950"
                  : "text-ink-400 hover:text-ink-200"
              }`}
            >
              {f.label} · {counts[f.id as keyof typeof counts]}
            </button>
          ))}
        </div>
        <div className="hairline" />
        <div className="max-h-[calc(100vh-220px)] overflow-y-auto">
          <JailsList
            rows={rows}
            selected={selected ?? undefined}
            onSelect={setSelected}
          />
        </div>
      </Panel>

      <div className="space-y-4">
        <JailDetail rec={detail ?? null} />
      </div>
    </div>
  );
}
