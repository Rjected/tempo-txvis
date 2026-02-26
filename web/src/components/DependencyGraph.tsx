import { useState, useCallback, useMemo } from "react";
import type { BlockGraph, DepEdgeKind } from "../types";

interface DependencyGraphProps {
  graph: BlockGraph;
  onSelectTx: (txIndex: number) => void;
  selectedTx?: number;
}

const EDGE_COLORS: Record<DepEdgeKind, string> = {
  raw: "#ef4444",
  waw: "#f97316",
  nonce_1d: "#6b7280",
  nonce_2d: "#8b5cf6",
  fee_sponsorship: "#22c55e",
};

const EDGE_LABELS: Record<DepEdgeKind, string> = {
  raw: "RAW",
  waw: "WAW",
  nonce_1d: "Nonce 1D",
  nonce_2d: "Nonce 2D",
  fee_sponsorship: "Fee Sponsorship",
};

const ALL_EDGE_KINDS: DepEdgeKind[] = ["raw", "waw", "nonce_1d", "nonce_2d", "fee_sponsorship"];

function getNodeColor(graph: BlockGraph, txIndex: number, selectedTx?: number): string {
  if (selectedTx === txIndex) return "#f59e0b";

  const schedule = graph.schedule.find((s) => s.txIndex === txIndex);
  if (schedule?.isCritical) return "#ef4444";

  const tx = graph.txNodes.find((t) => t.txIndex === txIndex);
  if (tx?.tempo) {
    if (tx.tempo.section === "system") return "#6b7280";
    if (tx.tempo.lane === "payment") return "#22c55e";
    return "#4a9eff";
  }

  return "#4a9eff";
}

function shortHash(hash: string): string {
  return hash.slice(0, 6);
}

export default function DependencyGraph({ graph, onSelectTx, selectedTx }: DependencyGraphProps) {
  const [enabledKinds, setEnabledKinds] = useState<Set<DepEdgeKind>>(
    () => new Set(ALL_EDGE_KINDS),
  );
  const [tooltip, setTooltip] = useState<{ x: number; y: number; text: string } | null>(null);

  const toggleKind = useCallback((kind: DepEdgeKind) => {
    setEnabledKinds((prev) => {
      const next = new Set(prev);
      if (next.has(kind)) {
        next.delete(kind);
      } else {
        next.add(kind);
      }
      return next;
    });
  }, []);

  const nodePositions = useMemo(() => {
    const positions = new Map<number, { x: number; y: number }>();
    const levels = new Map<number, number>();

    for (const tx of graph.txNodes) {
      levels.set(tx.txIndex, 0);
    }
    // Multi-pass to propagate levels correctly
    for (let pass = 0; pass < graph.txNodes.length; pass++) {
      for (const edge of graph.depEdges) {
        const fromLevel = levels.get(edge.fromTx) ?? 0;
        const currentToLevel = levels.get(edge.toTx) ?? 0;
        if (fromLevel + 1 > currentToLevel) {
          levels.set(edge.toTx, fromLevel + 1);
        }
      }
    }

    const byLevel = new Map<number, number[]>();
    for (const [txIndex, level] of levels) {
      if (!byLevel.has(level)) byLevel.set(level, []);
      byLevel.get(level)!.push(txIndex);
    }

    const levelCount = Math.max(...levels.values(), 0) + 1;
    for (const [level, txIndices] of byLevel) {
      txIndices.sort((a, b) => a - b);
      const count = txIndices.length;
      txIndices.forEach((txIndex, i) => {
        positions.set(txIndex, {
          x: 100 + (i * 600) / Math.max(count - 1, 1),
          y: 50 + (level * 350) / Math.max(levelCount - 1, 1),
        });
      });
    }

    return positions;
  }, [graph]);

  return (
    <div className="relative">
      {/* Filter controls */}
      <div className="mb-3 flex flex-wrap gap-3">
        {ALL_EDGE_KINDS.map((kind) => (
          <label key={kind} className="flex items-center gap-1 text-sm">
            <input
              type="checkbox"
              checked={enabledKinds.has(kind)}
              onChange={() => toggleKind(kind)}
              aria-label={EDGE_LABELS[kind]}
            />
            <span
              className="inline-block h-3 w-3 rounded-full"
              style={{ backgroundColor: EDGE_COLORS[kind] }}
            />
            {EDGE_LABELS[kind]}
          </label>
        ))}
      </div>

      {/* Graph area */}
      <svg viewBox="0 0 800 500" className="w-full rounded-lg border bg-gray-50">
        {/* Edges */}
        {graph.depEdges.map((edge, i) => {
          const from = nodePositions.get(edge.fromTx);
          const to = nodePositions.get(edge.toTx);
          if (!from || !to) return null;
          const isHidden = !enabledKinds.has(edge.kind);

          return (
            <line
              key={`edge-${i}`}
              data-testid={`graph-edge-${i}`}
              data-edge-kind={edge.kind}
              className={isHidden ? "hidden" : ""}
              x1={from.x}
              y1={from.y}
              x2={to.x}
              y2={to.y}
              stroke={EDGE_COLORS[edge.kind]}
              strokeWidth={2}
              strokeOpacity={0.7}
              onMouseEnter={(e) => {
                const reasons = edge.reasons
                  .map((r) => `${r.type}${r.address ? ` (${r.address.slice(0, 10)}…)` : ""}`)
                  .join(", ");
                setTooltip({
                  x: e.clientX,
                  y: e.clientY,
                  text: `${EDGE_LABELS[edge.kind]}: ${reasons}`,
                });
              }}
              onMouseLeave={() => setTooltip(null)}
            />
          );
        })}

        {/* Nodes */}
        {graph.txNodes.map((tx) => {
          const pos = nodePositions.get(tx.txIndex);
          if (!pos) return null;
          const color = getNodeColor(graph, tx.txIndex, selectedTx);

          return (
            <g
              key={tx.txIndex}
              data-testid={`graph-node-${tx.txIndex}`}
              className={`cursor-pointer ${selectedTx === tx.txIndex ? "selected" : ""}`}
              onClick={() => onSelectTx(tx.txIndex)}
            >
              <circle
                cx={pos.x}
                cy={pos.y}
                r={20}
                fill={color}
                stroke={selectedTx === tx.txIndex ? "#f59e0b" : "#fff"}
                strokeWidth={2}
              />
              <text
                x={pos.x}
                y={pos.y - 2}
                textAnchor="middle"
                fill="#fff"
                fontSize={10}
                fontWeight="bold"
              >
                {tx.txIndex}
              </text>
              <text
                x={pos.x}
                y={pos.y + 9}
                textAnchor="middle"
                fill="#fff"
                fontSize={7}
              >
                0x{shortHash(tx.txHash).slice(2)}
              </text>
            </g>
          );
        })}
      </svg>

      {/* Tooltip */}
      {tooltip && (
        <div
          className="pointer-events-none fixed z-50 rounded bg-gray-900 px-2 py-1 text-xs text-white shadow"
          style={{ left: tooltip.x + 10, top: tooltip.y + 10 }}
        >
          {tooltip.text}
        </div>
      )}
    </div>
  );
}
