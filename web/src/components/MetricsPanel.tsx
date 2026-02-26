import type { BlockMetrics } from "../types";

interface MetricsPanelProps {
  metrics: BlockMetrics;
}

function MetricCard({ label, value }: { label: string; value: string | number }) {
  return (
    <div className="rounded-lg bg-white p-4 shadow">
      <div className="text-sm text-gray-500">{label}</div>
      <div className="mt-1 text-2xl font-semibold">{value}</div>
    </div>
  );
}

function formatNumber(n: number): string {
  return n.toLocaleString("en-US");
}

export default function MetricsPanel({ metrics }: MetricsPanelProps) {
  return (
    <div className="grid grid-cols-2 gap-4 md:grid-cols-4">
      <MetricCard label="Transactions" value={metrics.txCount} />
      <MetricCard label="Speedup" value={`${metrics.speedupUpperBound.toFixed(2)}x`} />
      <MetricCard label="Critical Path" value={formatNumber(metrics.criticalPathWeight)} />
      <MetricCard label="Max Parallelism" value={metrics.maxParallelism} />
      <MetricCard label="Edges" value={metrics.edgeCount} />
      <MetricCard label="Components" value={metrics.componentCount} />
      <MetricCard label="Total Weight" value={formatNumber(metrics.totalWeight)} />
      <MetricCard label="Makespan" value={formatNumber(metrics.makespan)} />
    </div>
  );
}
