import { useState, useEffect } from "react";
import { useParams, useNavigate } from "react-router-dom";
import { getBlock } from "../api/client";
import type { BlockGraph } from "../types";
import DependencyGraph from "../components/DependencyGraph";
import ScheduleGantt from "../components/ScheduleGantt";
import MetricsPanel from "../components/MetricsPanel";
import TxInspector from "../components/TxInspector";
import BlockNav from "../components/BlockNav";
import ChainBadge from "../components/ChainBadge";

type Tab = "graph" | "gantt";

export default function BlockDetail() {
  const { number } = useParams<{ number: string }>();
  const navigate = useNavigate();
  const [block, setBlock] = useState<BlockGraph | null>(null);
  const [loading, setLoading] = useState(true);
  const [activeTab, setActiveTab] = useState<Tab>("graph");
  const [selectedTx, setSelectedTx] = useState<number | null>(null);

  useEffect(() => {
    if (!number) return;
    let cancelled = false;

    async function load() {
      setLoading(true);
      setSelectedTx(null);
      const data = await getBlock(number!);
      if (!cancelled) {
        setBlock(data);
        setLoading(false);
      }
    }

    load();
    return () => {
      cancelled = true;
    };
  }, [number]);

  const handleNavigate = (blockNumber: number) => {
    navigate(`/block/${blockNumber}`);
  };

  if (loading && !block) {
    return (
      <div className="flex min-h-screen items-center justify-center">
        <div className="text-gray-400">Loading block…</div>
      </div>
    );
  }

  if (!block) {
    return (
      <div className="flex min-h-screen items-center justify-center">
        <div className="text-gray-400">Block not found</div>
      </div>
    );
  }

  return (
    <div className="mx-auto max-w-7xl p-6">
      {/* Header */}
      <div className="mb-6 flex items-center justify-between">
        <div className="flex items-center gap-3">
          <h1 className="text-2xl font-bold">
            Block {block.blockNumber.toLocaleString()}
          </h1>
          <ChainBadge chain={block.chain} />
        </div>
        <BlockNav blockNumber={block.blockNumber} onNavigate={handleNavigate} />
      </div>

      {/* Metrics */}
      <div className="mb-6">
        <MetricsPanel metrics={block.metrics} />
      </div>

      {/* Tabs */}
      <div className="mb-4 flex border-b">
        <button
          role="tab"
          aria-selected={activeTab === "graph"}
          onClick={() => setActiveTab("graph")}
          className={`px-4 py-2 text-sm font-medium ${
            activeTab === "graph"
              ? "border-b-2 border-blue-600 text-blue-600"
              : "text-gray-500 hover:text-gray-700"
          }`}
        >
          Graph
        </button>
        <button
          role="tab"
          aria-selected={activeTab === "gantt"}
          onClick={() => setActiveTab("gantt")}
          className={`px-4 py-2 text-sm font-medium ${
            activeTab === "gantt"
              ? "border-b-2 border-blue-600 text-blue-600"
              : "text-gray-500 hover:text-gray-700"
          }`}
        >
          Gantt
        </button>
      </div>

      {/* Content */}
      <div className="flex gap-4">
        <div className="flex-1">
          {activeTab === "graph" && (
            <DependencyGraph
              graph={block}
              onSelectTx={setSelectedTx}
              selectedTx={selectedTx ?? undefined}
            />
          )}
          {activeTab === "gantt" && (
            <ScheduleGantt
              schedule={block.schedule}
              txNodes={block.txNodes}
            />
          )}
        </div>

        {selectedTx !== null && (
          <TxInspector
            txNodes={block.txNodes}
            depEdges={block.depEdges}
            selectedTx={selectedTx}
          />
        )}
      </div>
    </div>
  );
}
