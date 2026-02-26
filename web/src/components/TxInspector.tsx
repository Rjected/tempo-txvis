import type { TxNode, DepEdge } from "../types";

interface TxInspectorProps {
  txNodes: TxNode[];
  depEdges: DepEdge[];
  selectedTx: number | null;
}

function shortHash(hash: string): string {
  return hash.slice(0, 10) + "…";
}

function shortAddr(addr: string): string {
  return addr.slice(0, 6) + "…" + addr.slice(-4);
}

function formatNumber(n: number): string {
  return n.toLocaleString("en-US");
}

const KIND_LABELS: Record<string, string> = {
  raw: "RAW",
  waw: "WAW",
  nonce_1d: "Nonce 1D",
  nonce_2d: "Nonce 2D",
  fee_sponsorship: "Fee Sponsorship",
};

export default function TxInspector({ txNodes, depEdges, selectedTx }: TxInspectorProps) {
  if (selectedTx === null) return null;

  const tx = txNodes.find((t) => t.txIndex === selectedTx);
  if (!tx) return null;

  const incomingEdges = depEdges.filter((e) => e.toTx === selectedTx);
  const outgoingEdges = depEdges.filter((e) => e.fromTx === selectedTx);

  return (
    <div data-testid="tx-inspector" className="w-80 overflow-y-auto rounded-lg bg-white p-4 shadow-lg">
      <h3 className="mb-3 text-lg font-semibold">Tx {tx.txIndex}</h3>

      <div className="space-y-2 text-sm">
        <div>
          <span className="text-gray-500">Hash:</span>{" "}
          <span className="font-mono">{shortHash(tx.txHash)}</span>
        </div>
        <div>
          <span className="text-gray-500">From:</span>{" "}
          <span className="font-mono">{shortAddr(tx.from)}</span>
        </div>
        {tx.to && (
          <div>
            <span className="text-gray-500">To:</span>{" "}
            <span className="font-mono">{shortAddr(tx.to)}</span>
          </div>
        )}
        <div>
          <span className="text-gray-500">Gas Used:</span>{" "}
          {formatNumber(tx.gasUsed)}
        </div>
        <div>
          <span className="text-gray-500">Nonce:</span> {tx.nonce}
        </div>
        <div>
          <span className="text-gray-500">Type:</span> {tx.txType}
        </div>

        {tx.tempo && (
          <div className="mt-3 rounded border border-purple-200 bg-purple-50 p-2">
            <div className="mb-1 font-medium text-purple-800">Tempo Metadata</div>
            <div>
              <span className="text-gray-500">Lane:</span> {tx.tempo.lane === "payment" ? "Payment" : "General"}
            </div>
            <div>
              <span className="text-gray-500">Section:</span> {tx.tempo.section}
            </div>
            <div>
              <span className="text-gray-500">Nonce Key:</span>{" "}
              <span className="font-mono">{tx.tempo.nonceKey}</span>
            </div>
            <div>
              <span className="text-gray-500">Calls:</span> {tx.tempo.callCount}
            </div>
            {tx.tempo.feePayer && (
              <div>
                <span className="text-gray-500">Fee Payer:</span>{" "}
                <span className="font-mono">{shortAddr(tx.tempo.feePayer)}</span>
              </div>
            )}
          </div>
        )}

        {incomingEdges.length > 0 && (
          <div className="mt-3">
            <div className="font-medium">Depends on:</div>
            {incomingEdges.map((e, i) => (
              <div key={i} className="ml-2 text-xs">
                Tx {e.fromTx} → <span className="font-medium">{KIND_LABELS[e.kind] ?? e.kind}</span>
                {e.reasons.map((r, j) => (
                  <span key={j} className="ml-1 text-gray-400">
                    ({r.type}{r.address ? ` ${shortAddr(r.address)}` : ""})
                  </span>
                ))}
              </div>
            ))}
          </div>
        )}

        {outgoingEdges.length > 0 && (
          <div className="mt-3">
            <div className="font-medium">Blocks:</div>
            {outgoingEdges.map((e, i) => (
              <div key={i} className="ml-2 text-xs">
                → Tx {e.toTx}: <span className="font-medium">{KIND_LABELS[e.kind] ?? e.kind}</span>
                {e.reasons.map((r, j) => (
                  <span key={j} className="ml-1 text-gray-400">
                    ({r.type}{r.address ? ` ${shortAddr(r.address)}` : ""})
                  </span>
                ))}
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
