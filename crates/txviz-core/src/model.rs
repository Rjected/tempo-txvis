use std::collections::HashSet;

use alloy_primitives::{Address, B256, U256};
use serde::{Deserialize, Serialize};

/// Identifies which chain we're connected to.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ChainKind {
    Ethereum,
    Tempo,
}

/// A fully computed dependency graph for a single block.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BlockGraph {
    pub chain: ChainKind,
    pub block_number: u64,
    pub block_hash: B256,
    pub parent_hash: B256,
    pub timestamp: u64,
    pub tx_nodes: Vec<TxNode>,
    pub dep_edges: Vec<DepEdge>,
    pub schedule: Vec<ScheduleItem>,
    pub metrics: BlockMetrics,
}

/// A single transaction as a node in the dependency graph.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TxNode {
    pub tx_hash: B256,
    pub tx_index: u32,
    pub from: Address,
    pub to: Option<Address>,
    pub tx_type: u8,
    pub nonce: u64,
    pub gas_used: u64,
    /// Tempo-specific metadata. None for Ethereum transactions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tempo: Option<TempoTxMeta>,
}

/// Tempo-specific transaction metadata extracted from type 0x76 transactions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TempoTxMeta {
    /// The 2D nonce key (U256). 0 = sequential, 1..N = parallel lanes, MAX = expiring.
    pub nonce_key: U256,
    /// Transaction lane: Payment (TIP-20 precompile calls) or General.
    pub lane: TxLane,
    /// Which block section this tx belongs to.
    pub section: BlockSection,
    /// Subblock index if this tx is in the SubBlocks section.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subblock_index: Option<u32>,
    /// Fee payer address if sponsored (different from `from`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fee_payer: Option<Address>,
    /// Number of calls in the batched transaction.
    pub call_count: u32,
    /// Fee token address (TIP-20 stablecoin used for gas).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fee_token: Option<Address>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TxLane {
    Payment,
    General,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BlockSection {
    StartOfBlock,
    NonShared,
    SubBlocks,
    GasIncentive,
    System,
    Unknown,
}

/// A dependency edge between two transactions.
/// Edges are deduplicated per (from, to) pair with multiple reasons.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DepEdge {
    /// Source tx index (earlier in block).
    pub from_tx: u32,
    /// Target tx index (later in block).
    pub to_tx: u32,
    /// Primary classification of this dependency.
    pub kind: DepEdgeKind,
    /// All state keys that cause this dependency.
    pub reasons: Vec<DepReason>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum DepEdgeKind {
    /// Fee sponsorship dependency (shared fee payer balance).
    FeeSponsorship,
    /// Same sender + same nonce_key, sequential nonce (Tempo 2D nonce ordering).
    Nonce2d,
    /// Same sender, sequential nonce (Ethereum 1D nonce ordering).
    Nonce1d,
    /// Write-after-write: tx B writes a key that tx A also wrote.
    Waw,
    /// Read-after-write: tx B reads a key that tx A wrote.
    Raw,
}

/// Why a dependency edge exists — the specific state key(s) involved.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum DepReason {
    /// Conflict on a storage slot.
    Storage { address: Address, slot: B256 },
    /// Conflict on an account balance.
    Balance { address: Address },
    /// Conflict on an account nonce.
    Nonce { address: Address },
    /// Conflict on contract code.
    Code { address: Address },
    /// Nonce sequencing (1D or 2D).
    NonceSequence {
        address: Address,
        #[serde(skip_serializing_if = "Option::is_none")]
        nonce_key: Option<U256>,
    },
    /// Fee payer balance dependency.
    FeePayer { payer: Address },
}

/// A state key used internally for dependency computation.
/// Not serialized to API — internal to DAG builder.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum StateKey {
    Storage(Address, B256),
    Balance(Address),
    Nonce(Address),
    Code(Address),
}

/// Per-transaction read and write sets extracted from traces.
#[derive(Debug, Clone, Default)]
pub struct TxStateAccess {
    pub tx_index: u32,
    pub reads: HashSet<StateKey>,
    pub writes: HashSet<StateKey>,
}

/// Scheduled position of a transaction in the parallel execution timeline.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleItem {
    pub tx_index: u32,
    /// Start time in weight units (gas).
    pub start: u64,
    /// Duration in weight units (gas_used).
    pub duration: u64,
    /// Assigned parallel lane (0-indexed).
    pub lane: u32,
    /// Whether this tx is on the critical path.
    pub is_critical: bool,
}

/// Aggregate metrics for a block's dependency graph.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BlockMetrics {
    pub tx_count: u32,
    pub edge_count: u32,
    /// Number of connected components in the undirected projection.
    pub component_count: u32,
    /// Sum of all tx gas_used (total sequential execution cost).
    pub total_weight: u64,
    /// Length of the critical path (longest path weight in DAG).
    pub critical_path_weight: u64,
    /// total_weight / critical_path_weight — theoretical max speedup.
    pub speedup_upper_bound: f64,
    /// Maximum number of txs executing simultaneously in the schedule.
    pub max_parallelism: u32,
    /// Makespan = finish time of last scheduled tx.
    pub makespan: u64,
    // --- Tempo-specific (None for Ethereum) ---
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payment_tx_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub general_tx_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subblock_count: Option<u32>,
    /// Number of distinct nonce_keys used (Tempo 2D nonce parallelism).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unique_nonce_keys: Option<u32>,
}

/// Summary for block list views (lightweight, no full graph).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BlockSummary {
    pub chain: ChainKind,
    pub block_number: u64,
    pub block_hash: B256,
    pub timestamp: u64,
    pub metrics: BlockMetrics,
}

/// SSE event sent to frontend when a new block is processed.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockUpdateEvent {
    pub block_number: u64,
    pub block_hash: B256,
    pub timestamp: u64,
    pub tx_count: u32,
    pub speedup_upper_bound: f64,
    pub critical_path_weight: u64,
}

/// Range query for listing blocks.
#[derive(Debug, Clone)]
pub struct BlockRange {
    pub from: Option<u64>,
    pub to: Option<u64>,
    pub limit: u32,
}
