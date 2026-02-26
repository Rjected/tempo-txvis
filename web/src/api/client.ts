import type { BlockGraph, BlockSummary, BlockUpdateEvent, ChainIdentity } from "../types";

const BASE = "";

export async function getChain(): Promise<ChainIdentity> {
  const res = await fetch(`${BASE}/api/chain`);
  if (!res.ok) throw new Error(`Failed to fetch chain: ${res.status}`);
  return res.json();
}

export async function getBlock(n: number | string): Promise<BlockGraph | null> {
  const res = await fetch(`${BASE}/api/block/${n}`);
  if (res.status === 404) return null;
  if (!res.ok) throw new Error(`Failed to fetch block: ${res.status}`);
  return res.json();
}

export async function getBlockByHash(hash: string): Promise<BlockGraph | null> {
  const res = await fetch(`${BASE}/api/block/hash/${hash}`);
  if (res.status === 404) return null;
  if (!res.ok) throw new Error(`Failed to fetch block: ${res.status}`);
  return res.json();
}

export async function listBlocks(opts?: {
  from?: number;
  to?: number;
  limit?: number;
}): Promise<{ blocks: BlockSummary[] }> {
  const params = new URLSearchParams();
  if (opts?.from !== undefined) params.set("from", String(opts.from));
  if (opts?.to !== undefined) params.set("to", String(opts.to));
  if (opts?.limit !== undefined) params.set("limit", String(opts.limit));
  const qs = params.toString();
  const res = await fetch(`${BASE}/api/blocks${qs ? `?${qs}` : ""}`);
  if (!res.ok) throw new Error(`Failed to list blocks: ${res.status}`);
  return res.json();
}

export function subscribeLive(onBlock: (e: BlockUpdateEvent) => void): EventSource {
  const es = new EventSource(`${BASE}/api/live`);
  es.addEventListener("block", (event) => {
    const data: BlockUpdateEvent = JSON.parse((event as MessageEvent).data);
    onBlock(data);
  });
  return es;
}
