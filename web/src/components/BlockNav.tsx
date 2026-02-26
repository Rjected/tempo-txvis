import { useState } from "react";

interface BlockNavProps {
  blockNumber: number;
  onNavigate: (blockNumber: number) => void;
}

export default function BlockNav({ blockNumber, onNavigate }: BlockNavProps) {
  const [inputValue, setInputValue] = useState(String(blockNumber));

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    const num = parseInt(inputValue, 10);
    if (!isNaN(num)) {
      onNavigate(num);
    }
  };

  return (
    <div className="flex items-center gap-2">
      <button
        aria-label="Previous block"
        onClick={() => onNavigate(blockNumber - 1)}
        className="rounded bg-gray-200 px-3 py-1 text-sm hover:bg-gray-300"
      >
        ←
      </button>
      <form onSubmit={handleSubmit} className="flex items-center">
        <input
          type="number"
          value={inputValue}
          onChange={(e) => setInputValue(e.target.value)}
          className="w-32 rounded border px-2 py-1 text-center text-sm"
        />
      </form>
      <button
        aria-label="Next block"
        onClick={() => onNavigate(blockNumber + 1)}
        className="rounded bg-gray-200 px-3 py-1 text-sm hover:bg-gray-300"
      >
        →
      </button>
    </div>
  );
}
