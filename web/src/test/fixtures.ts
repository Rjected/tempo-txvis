import type { BlockGraph, BlockSummary, TxNode, DepEdge, ScheduleItem, BlockMetrics, BlockUpdateEvent, ChainIdentity } from "../types";

export const mockMetrics: BlockMetrics = {
  txCount: 4,
  edgeCount: 3,
  componentCount: 2,
  totalWeight: 400000,
  criticalPathWeight: 200000,
  speedupUpperBound: 2.0,
  maxParallelism: 2,
  makespan: 250000,
};

export const mockTxNodes: TxNode[] = [
  {
    txHash: "0xaaaaaa1111111111111111111111111111111111111111111111111111111111",
    txIndex: 0,
    from: "0x1111111111111111111111111111111111111111",
    to: "0x2222222222222222222222222222222222222222",
    txType: 2,
    nonce: 0,
    gasUsed: 100000,
  },
  {
    txHash: "0xbbbbbb2222222222222222222222222222222222222222222222222222222222",
    txIndex: 1,
    from: "0x3333333333333333333333333333333333333333",
    to: "0x4444444444444444444444444444444444444444",
    txType: 2,
    nonce: 5,
    gasUsed: 100000,
  },
  {
    txHash: "0xcccccc3333333333333333333333333333333333333333333333333333333333",
    txIndex: 2,
    from: "0x1111111111111111111111111111111111111111",
    to: "0x5555555555555555555555555555555555555555",
    txType: 2,
    nonce: 1,
    gasUsed: 100000,
    tempo: {
      nonceKey: "0x1",
      lane: "payment",
      section: "sub_blocks",
      callCount: 1,
    },
  },
  {
    txHash: "0xdddddd4444444444444444444444444444444444444444444444444444444444",
    txIndex: 3,
    from: "0x6666666666666666666666666666666666666666",
    to: null,
    txType: 0x76,
    nonce: 0,
    gasUsed: 100000,
    tempo: {
      nonceKey: "0x0",
      lane: "general",
      section: "system",
      callCount: 2,
    },
  },
];

export const mockDepEdges: DepEdge[] = [
  {
    fromTx: 0,
    toTx: 1,
    kind: "raw",
    reasons: [
      { type: "storage", address: "0x2222222222222222222222222222222222222222", slot: "0x01" },
    ],
  },
  {
    fromTx: 0,
    toTx: 2,
    kind: "nonce_1d",
    reasons: [
      { type: "nonceSequence", address: "0x1111111111111111111111111111111111111111" },
    ],
  },
  {
    fromTx: 1,
    toTx: 3,
    kind: "waw",
    reasons: [
      { type: "balance", address: "0x4444444444444444444444444444444444444444" },
    ],
  },
];

export const mockSchedule: ScheduleItem[] = [
  { txIndex: 0, start: 0, duration: 100000, lane: 0, isCritical: true },
  { txIndex: 1, start: 100000, duration: 100000, lane: 0, isCritical: true },
  { txIndex: 2, start: 100000, duration: 100000, lane: 1, isCritical: false },
  { txIndex: 3, start: 200000, duration: 100000, lane: 0, isCritical: false },
];

export const mockBlockGraph: BlockGraph = {
  chain: "ethereum",
  blockNumber: 19000000,
  blockHash: "0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890",
  parentHash: "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
  timestamp: 1700000000,
  txNodes: mockTxNodes,
  depEdges: mockDepEdges,
  schedule: mockSchedule,
  metrics: mockMetrics,
};

export const mockBlockSummaries: BlockSummary[] = [
  {
    chain: "ethereum",
    blockNumber: 19000002,
    blockHash: "0x0002000000000000000000000000000000000000000000000000000000000000",
    timestamp: 1700000024,
    metrics: { ...mockMetrics, txCount: 10 },
  },
  {
    chain: "ethereum",
    blockNumber: 19000001,
    blockHash: "0x0001000000000000000000000000000000000000000000000000000000000000",
    timestamp: 1700000012,
    metrics: { ...mockMetrics, txCount: 8 },
  },
  {
    chain: "ethereum",
    blockNumber: 19000000,
    blockHash: "0x0000000000000000000000000000000000000000000000000000000000000000",
    timestamp: 1700000000,
    metrics: mockMetrics,
  },
];

export const mockBlockUpdateEvent: BlockUpdateEvent = {
  blockNumber: 19000003,
  blockHash: "0x0003000000000000000000000000000000000000000000000000000000000000",
  timestamp: 1700000036,
  txCount: 12,
  speedupUpperBound: 2.5,
  criticalPathWeight: 150000,
};

export const mockChainIdentity: ChainIdentity = {
  chainId: 1,
  chainKind: "ethereum",
  clientVersion: "reth/v1.0.0",
};
