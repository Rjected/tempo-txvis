import type { ChainKind } from "../types";

interface ChainBadgeProps {
  chain: ChainKind;
}

export default function ChainBadge({ chain }: ChainBadgeProps) {
  const isEthereum = chain === "ethereum";
  return (
    <span
      className={`inline-flex items-center rounded-full px-2.5 py-0.5 text-xs font-medium ${
        isEthereum
          ? "bg-blue-100 text-blue-800"
          : "bg-purple-100 text-purple-800"
      }`}
    >
      {isEthereum ? "Ethereum" : "Tempo"}
    </span>
  );
}
