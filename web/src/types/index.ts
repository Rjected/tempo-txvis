export type ChainKind = "ethereum" | "tempo";
export type TxLane = "payment" | "general";
export type BlockSection = "start_of_block" | "non_shared" | "sub_blocks" | "gas_incentive" | "system" | "unknown";
export type DepEdgeKind = "raw" | "waw" | "nonce_1d" | "nonce_2d" | "fee_sponsorship";

export interface BlockGraph {
  chain: ChainKind;
  blockNumber: number;
  blockHash: string;
  parentHash: string;
  timestamp: number;
  txNodes: TxNode[];
  depEdges: DepEdge[];
  schedule: ScheduleItem[];
  metrics: BlockMetrics;
}

export interface TxNode {
  txHash: string;
  txIndex: number;
  from: string;
  to: string | null;
  txType: number;
  nonce: number;
  gasUsed: number;
  tempo?: TempoTxMeta;
}

export interface TempoTxMeta {
  nonceKey: string;
  lane: TxLane;
  section: BlockSection;
  subblockIndex?: number;
  feePayer?: string;
  callCount: number;
  feeToken?: string;
}

export interface DepEdge {
  fromTx: number;
  toTx: number;
  kind: DepEdgeKind;
  reasons: DepReason[];
}

export interface DepReason {
  type: "storage" | "balance" | "nonce" | "code" | "nonceSequence" | "feePayer";
  address?: string;
  slot?: string;
  nonceKey?: string;
  payer?: string;
}

export interface ScheduleItem {
  txIndex: number;
  start: number;
  duration: number;
  lane: number;
  isCritical: boolean;
}

export interface BlockMetrics {
  txCount: number;
  edgeCount: number;
  componentCount: number;
  totalWeight: number;
  criticalPathWeight: number;
  speedupUpperBound: number;
  maxParallelism: number;
  makespan: number;
  paymentTxCount?: number;
  generalTxCount?: number;
  subblockCount?: number;
  uniqueNonceKeys?: number;
}

export interface BlockSummary {
  chain: ChainKind;
  blockNumber: number;
  blockHash: string;
  timestamp: number;
  metrics: BlockMetrics;
}

export interface BlockUpdateEvent {
  blockNumber: number;
  blockHash: string;
  timestamp: number;
  txCount: number;
  speedupUpperBound: number;
  criticalPathWeight: number;
}

export interface ChainIdentity {
  chainId: number;
  chainKind: ChainKind;
  clientVersion: string;
}
