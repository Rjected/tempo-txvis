import { useState, useEffect, useCallback } from "react";
import { Link } from "react-router-dom";
import { listBlocks } from "../api/client";
import type { BlockSummary } from "../types";

const PAGE_SIZE = 20;

export default function BlockList() {
  const [blocks, setBlocks] = useState<BlockSummary[]>([]);
  const [loading, setLoading] = useState(true);
  const [page, setPage] = useState(0);

  const load = useCallback(async (pageNum: number) => {
    setLoading(true);
    const result = await listBlocks({
      limit: PAGE_SIZE,
      ...(pageNum > 0 ? { to: blocks[blocks.length - 1]?.blockNumber } : {}),
    });
    setBlocks(result.blocks);
    setLoading(false);
  }, [blocks]);

  useEffect(() => {
    load(0);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const handleNext = () => {
    const nextPage = page + 1;
    setPage(nextPage);
    load(nextPage);
  };

  const handlePrev = () => {
    if (page > 0) {
      const prevPage = page - 1;
      setPage(prevPage);
      load(prevPage);
    }
  };

  return (
    <div className="mx-auto max-w-5xl p-6">
      <div className="mb-6 flex items-center justify-between">
        <h1 className="text-2xl font-bold">Blocks</h1>
        <Link to="/" className="text-sm text-blue-600 hover:underline">
          ← Home
        </Link>
      </div>

      <div className="overflow-hidden rounded-lg bg-white shadow">
        <table className="min-w-full divide-y divide-gray-200">
          <thead className="bg-gray-50">
            <tr>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase text-gray-500">
                Block
              </th>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase text-gray-500">
                Timestamp
              </th>
              <th className="px-4 py-3 text-right text-xs font-medium uppercase text-gray-500">
                Txs
              </th>
              <th className="px-4 py-3 text-right text-xs font-medium uppercase text-gray-500">
                Speedup
              </th>
              <th className="px-4 py-3 text-right text-xs font-medium uppercase text-gray-500">
                Critical Path
              </th>
            </tr>
          </thead>
          <tbody className="divide-y divide-gray-200">
            {blocks.map((block) => (
              <tr key={block.blockNumber} className="hover:bg-gray-50">
                <td className="px-4 py-3">
                  <Link
                    to={`/block/${block.blockNumber}`}
                    className="font-medium text-blue-600 hover:underline"
                  >
                    {block.blockNumber.toLocaleString()}
                  </Link>
                </td>
                <td className="px-4 py-3 text-sm text-gray-500">
                  {new Date(block.timestamp * 1000).toLocaleString()}
                </td>
                <td className="px-4 py-3 text-right text-sm">
                  {block.metrics.txCount}
                </td>
                <td className="px-4 py-3 text-right text-sm">
                  {block.metrics.speedupUpperBound.toFixed(2)}x
                </td>
                <td className="px-4 py-3 text-right text-sm">
                  {block.metrics.criticalPathWeight.toLocaleString()}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      {loading && (
        <div className="mt-4 text-center text-gray-400">Loading…</div>
      )}

      <div className="mt-4 flex justify-between">
        <button
          onClick={handlePrev}
          disabled={page === 0}
          className="rounded bg-gray-200 px-4 py-2 text-sm hover:bg-gray-300 disabled:opacity-50"
        >
          ← Previous
        </button>
        <button
          onClick={handleNext}
          className="rounded bg-gray-200 px-4 py-2 text-sm hover:bg-gray-300"
        >
          Next →
        </button>
      </div>
    </div>
  );
}
