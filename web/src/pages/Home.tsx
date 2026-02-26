import { useState, useEffect, useRef } from "react";
import { Link } from "react-router-dom";
import { getBlock, subscribeLive } from "../api/client";
import type { BlockGraph, BlockUpdateEvent } from "../types";
import MetricsPanel from "../components/MetricsPanel";
import ChainBadge from "../components/ChainBadge";

export default function Home() {
  const [block, setBlock] = useState<BlockGraph | null>(null);
  const [loading, setLoading] = useState(true);
  const [connected, setConnected] = useState(false);
  const esRef = useRef<EventSource | null>(null);

  useEffect(() => {
    let cancelled = false;

    async function loadLatest() {
      setLoading(true);
      const data = await getBlock("latest");
      if (!cancelled && data) {
        setBlock(data);
      }
      if (!cancelled) setLoading(false);
    }

    loadLatest();

    const es = subscribeLive(async (event: BlockUpdateEvent) => {
      const data = await getBlock(event.blockNumber);
      if (!cancelled && data) {
        setBlock(data);
      }
    });
    esRef.current = es;
    setConnected(true);

    return () => {
      cancelled = true;
      es.close();
    };
  }, []);

  if (loading && !block) {
    return (
      <div className="flex min-h-screen items-center justify-center">
        <div className="text-gray-400">Loading latest block…</div>
      </div>
    );
  }

  if (!block) {
    return (
      <div className="flex min-h-screen items-center justify-center">
        <div className="text-gray-400">No blocks available</div>
      </div>
    );
  }

  return (
    <div className="mx-auto max-w-5xl p-6">
      <div className="mb-6 flex items-center justify-between">
        <div className="flex items-center gap-3">
          <h1 className="text-2xl font-bold">txviz</h1>
          <ChainBadge chain={block.chain} />
          <div
            data-testid="live-indicator"
            className="flex items-center gap-1 text-sm text-gray-500"
          >
            <span
              className={`inline-block h-2 w-2 rounded-full ${
                connected ? "bg-green-500" : "bg-gray-400"
              }`}
            />
            {connected ? "Live" : "Disconnected"}
          </div>
        </div>
        <Link
          to="/blocks"
          className="text-sm text-blue-600 hover:underline"
        >
          All Blocks →
        </Link>
      </div>

      <div className="mb-6 rounded-lg bg-white p-6 shadow">
        <div className="mb-4 flex items-center justify-between">
          <h2 className="text-lg font-semibold">
            Block {block.blockNumber.toLocaleString()}
          </h2>
          <Link
            to={`/block/${block.blockNumber}`}
            className="text-sm text-blue-600 hover:underline"
          >
            View Details →
          </Link>
        </div>
        <div className="mb-2 text-sm text-gray-500">
          Hash: {block.blockHash.slice(0, 18)}…
        </div>
        <div className="mb-4 text-sm text-gray-500">
          Timestamp: {new Date(block.timestamp * 1000).toLocaleString()}
        </div>
        <MetricsPanel metrics={block.metrics} />
      </div>
    </div>
  );
}
