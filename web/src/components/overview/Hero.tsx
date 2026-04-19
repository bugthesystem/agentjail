import { Panel } from "../Panel";
import { Pill } from "../Pill";
import { Flow } from "../Flow";

export function Hero({ totalEvents, rate }: { totalEvents: number; rate: number }) {
  return (
    <Panel className="!p-0 overflow-hidden">
      <div className="p-6 pb-4 flex items-start justify-between gap-6">
        <div>
          <div className="text-[10px] uppercase tracking-[0.22em] text-ink-400 mb-1.5">
            Phantom flow
          </div>
          <h1 className="display text-[28px] leading-tight font-semibold text-balance max-w-[520px]">
            Live traffic through the <span className="text-[var(--color-phantom)]">phantom edge</span>
          </h1>
          <p className="mt-1.5 text-sm text-ink-400 max-w-[480px]">
            Every request from a sandbox passes through the proxy, which swaps{" "}
            <span className="mono text-ink-300">phm_…</span> for the real key.
          </p>
        </div>
        <div className="flex flex-col items-end gap-2 shrink-0">
          <Pill tone="phantom" dot>proxy live</Pill>
          <div className="mono text-[11px] text-ink-500 tabular-nums">
            {totalEvents} requests seen
          </div>
        </div>
      </div>
      <div className="px-3 pb-2">
        <Flow rate={rate} />
      </div>
    </Panel>
  );
}
