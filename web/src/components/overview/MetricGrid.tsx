import { Metric } from "../Metric";
import { useSeries } from "../../lib/useSeries";

interface MetricGridProps {
  sessions: number;
  active: number;
  totalExecs: number;
  proxyEvents: number;
}

export function MetricGrid({ sessions, active, totalExecs, proxyEvents }: MetricGridProps) {
  const sessionSeries = useSeries(sessions);
  const activeSeries  = useSeries(active);
  const proxySeries   = useSeries(proxyEvents);
  const totalSeries   = useSeries(totalExecs);

  return (
    <div className="grid grid-cols-4 gap-3">
      <Metric label="Sessions"     value={sessions}    tone="phantom" series={sessionSeries} />
      <Metric label="Active execs" value={active}      tone="flare"   series={activeSeries} delta={`${totalExecs} total`} />
      <Metric label="Proxy events" value={proxyEvents} tone="iris"    series={proxySeries} />
      <Metric label="Total execs"  value={totalExecs}  tone="phantom" series={totalSeries} />
    </div>
  );
}
