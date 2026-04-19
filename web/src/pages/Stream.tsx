import { useQuery } from "@tanstack/react-query";
import { useMemo, useState } from "react";
import { useApi } from "../lib/auth";
import { Panel, PanelHeader } from "../components/Panel";
import { Pill } from "../components/Pill";
import { AuditList } from "../components/AuditList";

const FILTERS = ["all", "200", "4xx", "5xx", "blocked"] as const;
type Filter = (typeof FILTERS)[number];

export function Stream() {
  const api = useApi();
  const [filter, setFilter] = useState<Filter>("all");

  const { data } = useQuery({
    queryKey: ["audit", 200],
    queryFn: () => api.audit.recent(200),
    refetchInterval: 1500,
  });

  const filtered = useMemo(() => {
    const rows = data?.rows ?? [];
    switch (filter) {
      case "200":     return rows.filter((r) => r.status >= 200 && r.status < 300);
      case "4xx":     return rows.filter((r) => r.status >= 400 && r.status < 500);
      case "5xx":     return rows.filter((r) => r.status >= 500);
      case "blocked": return rows.filter((r) => r.reject_reason);
      default:        return rows;
    }
  }, [data, filter]);

  const counts = useMemo(() => {
    const r = data?.rows ?? [];
    return {
      all:     r.length,
      "200":   r.filter((x) => x.status >= 200 && x.status < 300).length,
      "4xx":   r.filter((x) => x.status >= 400 && x.status < 500).length,
      "5xx":   r.filter((x) => x.status >= 500).length,
      blocked: r.filter((x) => x.reject_reason).length,
    };
  }, [data]);

  return (
    <Panel padded={false}>
      <div className="px-5 py-4 flex items-center justify-between">
        <PanelHeader
          eyebrow="Audit"
          title="Live request stream"
          className="!mb-0"
        />
        <div className="flex items-center gap-2">
          <Pill tone="phantom" dot>live</Pill>
          <span className="text-[11px] mono text-ink-500">
            {data?.total ?? 0} total · window 200
          </span>
        </div>
      </div>
      <div className="hairline" />
      <div className="px-5 py-3 flex items-center gap-1">
        {FILTERS.map((f) => (
          <button
            key={f}
            onClick={() => setFilter(f)}
            className={`h-7 px-3 rounded-full text-[11px] mono transition-colors ${
              filter === f
                ? "bg-ink-100 text-ink-950"
                : "text-ink-400 hover:text-ink-200"
            }`}
          >
            {f} · {counts[f]}
          </button>
        ))}
      </div>
      <div className="hairline" />
      <div className="max-h-[calc(100vh-260px)] overflow-y-auto">
        <AuditList rows={filtered} />
      </div>
    </Panel>
  );
}
