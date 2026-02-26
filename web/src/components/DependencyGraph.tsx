import { useState, useCallback, useRef, useEffect, useMemo } from "react";
import CytoscapeComponent from "react-cytoscapejs";
import cytoscape from "cytoscape";
import dagre from "cytoscape-dagre";
import type { BlockGraph, DepEdgeKind } from "../types";

cytoscape.use(dagre);

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

function getNodeColor(graph: BlockGraph, txIndex: number): string {
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
  return hash.slice(2, 6);
}

export default function DependencyGraph({ graph, onSelectTx, selectedTx }: DependencyGraphProps) {
  const [enabledKinds, setEnabledKinds] = useState<Set<DepEdgeKind>>(
    () => new Set(ALL_EDGE_KINDS),
  );
  const [tooltip, setTooltip] = useState<{ x: number; y: number; text: string } | null>(null);
  const cyRef = useRef<cytoscape.Core | null>(null);

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

  // Build Cytoscape elements from graph data
  const elements = useMemo(() => {
    const nodes = graph.txNodes.map((tx) => ({
      data: {
        id: `tx-${tx.txIndex}`,
        label: `${tx.txIndex}`,
        hashLabel: `0x${shortHash(tx.txHash)}`,
        txIndex: tx.txIndex,
        color: getNodeColor(graph, tx.txIndex),
      },
    }));

    const edges = graph.depEdges.map((edge, i) => ({
      data: {
        id: `edge-${i}`,
        source: `tx-${edge.fromTx}`,
        target: `tx-${edge.toTx}`,
        kind: edge.kind,
        color: EDGE_COLORS[edge.kind],
        tooltipText: `${EDGE_LABELS[edge.kind]}: ${edge.reasons
          .map((r) => `${r.type}${r.address ? ` (${r.address.slice(0, 10)}…)` : ""}`)
          .join(", ")}`,
      },
    }));

    return [...nodes, ...edges];
  }, [graph]);

  // Apply edge visibility when filters change
  useEffect(() => {
    const cy = cyRef.current;
    if (!cy) return;
    cy.edges().forEach((edge) => {
      const kind = edge.data("kind") as DepEdgeKind;
      if (enabledKinds.has(kind)) {
        edge.style("display", "element");
      } else {
        edge.style("display", "none");
      }
    });
  }, [enabledKinds]);

  // Apply selected node highlight
  useEffect(() => {
    const cy = cyRef.current;
    if (!cy) return;
    cy.nodes().forEach((node) => {
      const txIndex = node.data("txIndex");
      if (txIndex === selectedTx) {
        node.style({
          "background-color": "#f59e0b",
          "border-color": "#f59e0b",
          "border-width": 3,
        });
      } else {
        node.style({
          "background-color": node.data("color"),
          "border-color": "#fff",
          "border-width": 2,
        });
      }
    });
  }, [selectedTx]);

  const handleCyInit = useCallback(
    (cy: cytoscape.Core) => {
      cyRef.current = cy;

      // Node click → select tx
      cy.on("tap", "node", (evt) => {
        const txIndex = evt.target.data("txIndex") as number;
        onSelectTx(txIndex);
      });

      // Edge hover → tooltip
      cy.on("mouseover", "edge", (evt) => {
        const edge = evt.target;
        const pos = evt.renderedPosition || evt.position;
        setTooltip({
          x: pos.x,
          y: pos.y,
          text: edge.data("tooltipText"),
        });
      });
      cy.on("mouseout", "edge", () => {
        setTooltip(null);
      });

      // Double click → fit to view
      cy.on("dbltap", (evt) => {
        if (evt.target === cy) {
          cy.fit(undefined, 40);
        }
      });

      // Run dagre layout
      cy.layout({
        name: "dagre",
        rankDir: "TB",
        nodeSep: 30,
        rankSep: 50,
        padding: 30,
        animate: false,
      } as unknown as cytoscape.LayoutOptions).run();

      cy.fit(undefined, 40);
    },
    [onSelectTx],
  );

  const stylesheet: cytoscape.StylesheetStyle[] = [
    {
      selector: "node",
      style: {
        "background-color": "data(color)",
        label: "data(label)",
        "text-valign": "center",
        "text-halign": "center",
        color: "#fff",
        "font-size": "10px",
        "font-weight": "bold",
        width: 36,
        height: 36,
        "border-width": 2,
        "border-color": "#fff",
        "text-outline-width": 0,
      },
    },
    {
      selector: "edge",
      style: {
        width: 2,
        "line-color": "data(color)",
        "target-arrow-color": "data(color)",
        "target-arrow-shape": "triangle",
        "curve-style": "bezier",
        opacity: 0.7,
        "arrow-scale": 0.8,
      },
    },
  ];

  return (
    <div className="relative">
      {/* Filter controls */}
      <div className="mb-3 flex flex-wrap items-center gap-3">
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
        <button
          type="button"
          className="ml-auto rounded bg-gray-200 px-2 py-1 text-xs text-gray-600 hover:bg-gray-300"
          onClick={() => cyRef.current?.fit(undefined, 40)}
        >
          Fit to View
        </button>
      </div>

      {/* Graph area */}
      <div className="rounded-lg border bg-gray-50" style={{ height: 600 }}>
        <CytoscapeComponent
          elements={elements}
          stylesheet={stylesheet}
          style={{ width: "100%", height: "100%" }}
          cy={handleCyInit}
          minZoom={0.1}
          maxZoom={3}
          boxSelectionEnabled={false}
          userPanningEnabled={true}
          userZoomingEnabled={true}
        />
      </div>

      {/* Tooltip */}
      {tooltip && (
        <div
          className="pointer-events-none absolute z-50 rounded bg-gray-900 px-2 py-1 text-xs text-white shadow"
          style={{ left: tooltip.x + 10, top: tooltip.y + 50 }}
        >
          {tooltip.text}
        </div>
      )}
    </div>
  );
}
