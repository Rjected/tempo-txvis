# tempo-txviz: Transaction Dependency Visualization

## Implementation Plan (TDD / Validation-First)

> **Goal**: Build a tool that visualizes transaction dependency graphs for Ethereum and Tempo blocks.
> Shows which transactions can run in parallel, computes optimal schedules, and highlights
> Tempo-specific parallelism features (2D nonces, subblocks, payment lanes).
>
> **Phase 1**: Local. Rust backend (axum) + React frontend. Runs alongside a reth/Tempo node on a dev box.
> All features delivered here. No Docker.
>
> **Phase 2**: Production. Swap infra only — Cloudflare Workers + KV + R2. Zero changes to core logic.

---

## Table of Contents

1. [Architecture Overview](#1-architecture-overview)
2. [Workspace Structure](#2-workspace-structure)
3. [Data Model](#3-data-model)
4. [Component Specifications](#4-component-specifications)
5. [Test Fixtures](#5-test-fixtures)
6. [Algorithm Specifications](#6-algorithm-specifications)
7. [API Contract](#7-api-contract)
8. [Execution Plan](#8-execution-plan)
9. [Dependency & Crate Manifest](#9-dependency--crate-manifest)
10. [CLI Specification](#10-cli-specification)

---

## 1. Architecture Overview

```
              ┌────────────────────┐
              │   web (React UI)   │
              │  Cytoscape.js DAG  │
              │  visx SVG Gantt    │
              └────────┬───────────┘
                       │ HTTP / SSE
          ┌────────────▼───────────┐      ┌──────────────────────┐
          │  txviz-api (routes)    │◄────►│ txviz-storage (trait) │
          │  GET /api/block/:n     │      │  impl: SQLite + FS   │
          │  GET /api/blocks       │      └──────────────────────┘
          │  GET /api/live (SSE)   │
          └────────────┬───────────┘
                       │
          ┌────────────▼───────────┐
          │  txviz-core            │
          │  • trace parsing       │
          │  • DAG construction    │
          │  • schedule compute    │
          │  • metrics compute     │
          └────────────────────────┘
                       ▲
          ┌────────────┴───────────┐
          │  txviz-chain           │
          │  • JSON-RPC client     │
          │  • block polling/sub   │
          │  • trace fetching      │
          │  • chain detection     │
          │  • Tempo tx decoding   │
          └────────────────────────┘
                       ▲
          ┌────────────┴───────────┐
          │  txviz-server (bin)    │
          │  • wires everything    │
          │  • embeds static UI    │
          │  • CLI (clap)          │
          └────────────────────────┘
```

**Key rule**: `txviz-core` is pure computation — zero IO, zero async. Everything is synchronous
functions that take data in and return data out. This makes it trivially testable and portable.

---

## 2. Workspace Structure

```
tempo-txviz/
├── Cargo.toml                    # workspace root
├── PLAN.md                       # this file
├── crates/
│   ├── txviz-core/               # Pure logic: models, trace parsing, DAG, schedule, metrics
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── model.rs          # BlockGraph, TxNode, DepEdge, BlockMetrics, ScheduleItem
│   │       ├── trace.rs          # Parse prestateTracer JSON → per-tx read/write sets
│   │       ├── dag.rs            # Build dependency DAG from read/write sets + chain metadata
│   │       ├── schedule.rs       # Critical path, greedy list scheduling, max parallelism
│   │       └── metrics.rs        # Compute BlockMetrics from DAG + schedule
│   │
│   ├── txviz-chain/              # RPC client, block streaming, chain detection
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── provider.rs       # ChainProvider trait + RPC implementation
│   │       ├── adapter.rs        # ChainAdapter trait + Ethereum/Tempo impls
│   │       ├── types.rs          # BlockEnvelope, RpcTransaction, ChainIdentity
│   │       ├── tempo.rs          # Tempo 0x76 tx decoding, TempoTxMeta extraction
│   │       └── stream.rs         # Block subscription (WS) / polling fallback
│   │
│   ├── txviz-storage/            # Storage abstraction + SQLite/FS implementation
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── backend.rs        # StorageBackend trait
│   │       └── sqlite.rs         # SQLite + filesystem JSON blob impl
│   │
│   ├── txviz-api/                # Axum route handlers (injected dependencies)
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── routes.rs         # All route handlers
│   │       ├── state.rs          # AppState (shared across handlers)
│   │       └── sse.rs            # SSE endpoint + broadcast
│   │
│   └── txviz-server/             # Binary: CLI + wiring + embedded frontend
│       ├── Cargo.toml
│       └── src/
│           ├── main.rs           # CLI parsing, component wiring, server start
│           └── ingestor.rs       # Background task: poll blocks → compute → store → broadcast
│
└── web/                          # React frontend
    ├── package.json
    ├── tsconfig.json
    ├── vite.config.ts
    ├── index.html
    └── src/
        ├── main.tsx
        ├── App.tsx
        ├── api/
        │   └── client.ts         # Typed API client + SSE subscription
        ├── types/
        │   └── index.ts          # BlockGraph, TxNode, DepEdge, etc. (mirrors Rust model)
        ├── pages/
        │   ├── Home.tsx           # Latest block + live indicator
        │   ├── BlockList.tsx      # Paginated block list with metrics
        │   └── BlockDetail.tsx    # Graph + Gantt tabs + tx inspector
        └── components/
            ├── DependencyGraph.tsx # Cytoscape.js interactive DAG
            ├── ScheduleGantt.tsx   # visx SVG Gantt chart
            ├── MetricsPanel.tsx    # Key metrics cards
            ├── TxInspector.tsx     # Side panel: tx details + dependency reasons
            ├── ChainBadge.tsx      # Ethereum/Tempo indicator
            └── BlockNav.tsx        # Block number input + prev/next arrows
```

---

## 3. Data Model

### 3.1 Rust Types (`txviz-core/src/model.rs`)

All types derive `Debug, Clone, Serialize, Deserialize, PartialEq`.

```rust
use alloy_primitives::{Address, B256, U256};

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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum DepEdgeKind {
    /// Read-after-write: tx B reads a key that tx A wrote.
    Raw,
    /// Write-after-write: tx B writes a key that tx A also wrote.
    Waw,
    /// Same sender, sequential nonce (Ethereum 1D nonce ordering).
    Nonce1d,
    /// Same sender + same nonce_key, sequential nonce (Tempo 2D nonce ordering).
    Nonce2d,
    /// Fee sponsorship dependency (shared fee payer balance).
    FeeSponsorship,
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
    NonceSequence { address: Address, nonce_key: Option<U256> },
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
```

### 3.2 TypeScript Types (`web/src/types/index.ts`)

These mirror the Rust types exactly (camelCase JSON serialization).

```typescript
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
```

---

## 4. Component Specifications

### 4.1 `txviz-core::trace` — Prestate Trace Parsing

**Input**: Raw JSON from `debug_traceBlockByNumber` with `prestateTracer` config.

**Output**: `Vec<TxStateAccess>` — per-transaction read and write sets.

**Two-pass approach** (for full accuracy):

| Pass | Tracer Config | Purpose |
|------|--------------|---------|
| 1 | `prestateTracer`, `diffMode: true` | Extract WRITE sets (what changed) |
| 2 | `prestateTracer`, `diffMode: false` | Extract TOUCHED sets (everything accessed) |

**Read set** = `touched_set - write_set` for each tx.

If only one pass is available (diffMode: true only), use conservative approximation:
treat everything in `pre` as a read, everything differing between `pre` and `post` as a write.

**Extraction rules from diff-mode trace (Pass 1)**:

For each tx result `{ pre, post }`:

| Condition | Classification |
|-----------|---------------|
| `addr` in `post` with field that differs from `pre` | **Write** to that field |
| `addr` in `pre` but NOT in `post` | **Write** (selfdestruct/deletion) |
| `addr` in `post` but NOT in `pre` | **Write** (account creation) |
| `slot` in `post[addr].storage` | **Write** to `Storage(addr, slot)` |
| `slot` in `pre[addr].storage` but NOT in `post[addr].storage` | **Write** (slot cleared) |

**Extraction rules from prestate-mode trace (Pass 2)**:

For each tx result (map of addr → account state):
- Every `addr` present → touched `Balance(addr)`, `Nonce(addr)`
- If `code` present → touched `Code(addr)`
- Every `slot` in `storage` → touched `Storage(addr, slot)`

**Read set** = touched keys from Pass 2 minus write keys from Pass 1.

**JSON format handling**: Accept both wrapper formats:
- `[{ "txHash": "0x...", "result": { "pre": {...}, "post": {...} } }, ...]`
- `[{ "pre": {...}, "post": {...} }, ...]`

**Edge cases to handle**:
- Empty trace result `[]` → empty `Vec<TxStateAccess>`
- `"error"` field present on a tx trace → skip that tx, log warning
- `nonce` field as JSON number OR hex string (geth variance)
- Missing `balance`/`nonce`/`code` fields (treat as not accessed for that field)

### 4.2 `txviz-core::dag` — DAG Construction

**Input**: `Vec<TxNode>`, `Vec<TxStateAccess>`, `ChainKind`

**Output**: `Vec<DepEdge>` (deduplicated, one edge per tx pair)

**Algorithm**:

```
Initialize:
  last_writer: HashMap<StateKey, u32>     // key → tx_index of last writer
  edges: HashMap<(u32, u32), DepEdge>     // (from, to) → deduplicated edge

For each tx i in block order (0..N):
  // --- State dependency edges ---
  For each key in writes[i]:
    if let Some(prev) = last_writer[key]:
      add_or_merge_edge(prev, i, Waw, reason_from(key))

  For each key in reads[i]:
    if let Some(prev) = last_writer[key]:
      add_or_merge_edge(prev, i, Raw, reason_from(key))

  For each key in writes[i]:
    last_writer[key] = i

  // --- Nonce sequencing edges ---
  (See below per chain kind)

  // --- Fee sponsorship edges (Tempo only) ---
  (See below)

Return edges.values() sorted by (from_tx, to_tx)
```

**Edge deduplication** (`add_or_merge_edge`):
```
fn add_or_merge_edge(from, to, kind, reason):
  key = (from, to)
  if edges contains key:
    existing = edges[key]
    existing.reasons.push(reason)  // deduplicate reasons too
    // kind priority: Raw > Waw > Nonce (use strongest)
    existing.kind = max(existing.kind, kind)
  else:
    edges[key] = DepEdge { from, to, kind, reasons: [reason] }
```

**Nonce sequencing edges**:

*Ethereum (1D)*:
```
Group txs by sender address.
For each group, sort by nonce ascending.
For consecutive pairs (i, j): add edge i→j, kind=Nonce1d
```

*Tempo (2D)*:
```
Group txs by (sender, nonce_key).
For each group, sort by nonce ascending.
For consecutive pairs (i, j): add edge i→j, kind=Nonce2d
Expiring nonce (nonce_key = U256::MAX): no nonce sequencing edge.
```

**Fee sponsorship edges (Tempo)**:
```
Track last_tx_by_fee_payer: HashMap<Address, u32>
For each tx with fee_payer != sender:
  if let Some(prev) = last_tx_by_fee_payer[fee_payer]:
    add edge prev→i, kind=FeeSponsorship
  last_tx_by_fee_payer[fee_payer] = i
```

### 4.3 `txviz-core::schedule` — Critical Path + List Scheduling

**Input**: `Vec<TxNode>` (for weights), `Vec<DepEdge>` (DAG structure)

**Output**: `(Vec<ScheduleItem>, CriticalPathInfo)`

**Weight**: `w[i] = tx_nodes[i].gas_used`. If `gas_used == 0`, use `1` as fallback.

#### 4.3.1 Topological Sort

Use Kahn's algorithm (BFS-based). If the graph has a cycle, return an error
(should never happen for a valid block, but defend against bad data).

#### 4.3.2 Critical Path (Longest Path via DP)

```
topo = topological_sort(dag)

// Forward pass: earliest finish time
est[v] = 0 for all v   // earliest start time
for v in topo:
  for each edge v → s:
    est[s] = max(est[s], est[v] + w[v])

// eft[v] = est[v] + w[v]
// critical_path_weight = max over all v of eft[v]

// Backward pass: latest start time (for identifying critical nodes)
lst[v] = critical_path_weight - w[v] for all v
for v in reverse(topo):
  for each edge v → s:
    lst[v] = min(lst[v], lst[s] - w[v])

// A node is critical if est[v] == lst[v]
// (zero slack)
```

#### 4.3.3 Greedy List Scheduling (Unlimited Lanes)

Use unlimited lanes (we want to show maximum theoretical parallelism).

```
Compute rank[v] = longest path from v to any sink (for priority):
  rank[v] = w[v] for all v
  for v in reverse(topo):
    for each edge v → s:
      rank[v] = max(rank[v], w[v] + rank[s])

ready = max-heap ordered by rank[v]
push all sources (indegree 0) into ready

lane_free: Vec<u64>  // dynamic, grows as needed

while ready not empty:
  v = pop_max(ready)
  dep_ready = max(finish[p] for p in predecessors(v)), or 0 if none

  // Find lane with earliest availability >= dep_ready
  best_lane = None
  best_start = u64::MAX
  for (lane_idx, free_at) in lane_free:
    s = max(free_at, dep_ready)
    if s < best_start:
      best_start = s
      best_lane = lane_idx

  // If no existing lane works well, or all lanes are busy past dep_ready, add new lane
  if best_lane is None or best_start > dep_ready:
    // Check if a new lane would be better
    if dep_ready < best_start:
      best_lane = lane_free.len()
      lane_free.push(0)
      best_start = dep_ready

  schedule[v] = ScheduleItem {
    tx_index: v,
    start: best_start,
    duration: w[v],
    lane: best_lane,
    is_critical: (est[v] == lst[v]),
  }
  lane_free[best_lane] = best_start + w[v]

  for each successor s of v:
    decrement pending_predecessors[s]
    if pending_predecessors[s] == 0:
      push s into ready

makespan = max over all v of (start[v] + duration[v])
```

#### 4.3.4 Max Parallelism (Sweep Line)

```
events = []
for each scheduled item:
  events.push((start, +1))
  events.push((start + duration, -1))

sort events by (time asc, delta asc)  // -1 before +1 at same time

current = 0
max_parallel = 0
for (time, delta) in events:
  current += delta
  max_parallel = max(max_parallel, current)
```

### 4.4 `txviz-core::metrics` — Compute BlockMetrics

**Input**: `Vec<TxNode>`, `Vec<DepEdge>`, `Vec<ScheduleItem>`, `CriticalPathInfo`

**Output**: `BlockMetrics`

```
tx_count = tx_nodes.len()
edge_count = dep_edges.len()
total_weight = sum of tx_nodes[i].gas_used
critical_path_weight = from CriticalPathInfo
speedup_upper_bound = total_weight as f64 / critical_path_weight as f64
  (handle division by zero: if critical_path_weight == 0, speedup = 1.0)
max_parallelism = from sweep line
makespan = from schedule

component_count = number of weakly connected components
  (build undirected graph from dep_edges, BFS/DFS to count components;
   isolated nodes = their own component)

// Tempo-specific
if any tx has tempo metadata:
  payment_tx_count = count where lane == Payment
  general_tx_count = count where lane == General
  subblock_count = max(subblock_index) + 1 across all txs, or 0
  unique_nonce_keys = count distinct nonce_key values
```

### 4.5 `txviz-chain` — RPC Client + Chain Detection

#### 4.5.1 `ChainProvider` trait

```rust
#[async_trait]
pub trait ChainProvider: Send + Sync {
    /// Detect chain identity (kind, chain_id, client version).
    async fn chain_identity(&self) -> Result<ChainIdentity>;

    /// Get the latest block number.
    async fn latest_block_number(&self) -> Result<u64>;

    /// Fetch a full block by number (header + tx list + receipts).
    async fn get_block(&self, number: u64) -> Result<BlockEnvelope>;

    /// Fetch prestate diff traces for all txs in a block.
    /// Returns one PrestateDiffTrace per transaction, in block order.
    async fn trace_block_prestate_diff(&self, number: u64) -> Result<Vec<PrestateDiffTrace>>;

    /// Fetch prestate (non-diff) traces for read set extraction.
    /// Returns one PrestateTrace per transaction, in block order.
    async fn trace_block_prestate(&self, number: u64) -> Result<Vec<PrestateTrace>>;

    /// Subscribe to new block headers. Returns a stream.
    /// Falls back to polling if WS is not available.
    fn subscribe_new_blocks(&self) -> BoxStream<'static, Result<NewBlockNotification>>;
}
```

#### 4.5.2 Chain Detection

```
1. Call eth_chainId → chain_id
2. Match against known Tempo chain IDs:
   - 42431 (Moderato testnet)
   - (add others as they launch)
3. If no match, call web3_clientVersion → check for "tempo" substring
4. If still no match, classify as Ethereum
5. CLI --force-chain overrides all detection
```

#### 4.5.3 `ChainAdapter` trait

```rust
pub trait ChainAdapter: Send + Sync {
    fn chain_kind(&self) -> ChainKind;

    /// Parse chain-specific tx metadata from RPC transaction object.
    fn parse_tx_node(&self, tx: &RpcTransaction, receipt: &RpcReceipt) -> Result<TxNode>;

    /// Generate extra dependency edges beyond state-diff analysis.
    /// This includes nonce sequencing edges and Tempo-specific edges.
    fn nonce_edges(&self, nodes: &[TxNode]) -> Vec<DepEdge>;

    /// Generate Tempo-specific structural edges (fee sponsorship, etc.).
    fn structural_edges(&self, nodes: &[TxNode]) -> Vec<DepEdge>;
}
```

#### 4.5.4 Tempo Transaction Decoding

For Tempo blocks, transactions with `type == 0x76` need special handling:

**Strategy**: Parse from `eth_getBlockByNumber` response. The Tempo RPC node
extends the standard transaction JSON with Tempo-specific fields:

```json
{
  "type": "0x76",
  "from": "0x...",
  "nonce": "0x1",
  "nonceKey": "0x0",
  "calls": [
    { "to": "0x...", "value": "0x0", "input": "0x..." },
    { "to": "0x...", "value": "0x100", "input": "0x..." }
  ],
  "feeToken": "0x20c0000000000000000000000000000000000000",
  "validBefore": "0x...",
  "validAfter": "0x...",
  "gasLimit": "0x...",
  "maxFeePerGas": "0x...",
  "maxPriorityFeePerGas": "0x..."
}
```

**If fields are missing**: fall back to raw tx bytes via `eth_getRawTransactionByHash`
and RLP-decode the 0x76 envelope. But try JSON fields first (simpler, more likely to work).

**Section assignment**: Derive from block body ordering:
- System tx to `address(0x0)` with zero-signature → `System`
- Tx with `nonce_key` prefix `0x5b` (high byte) → `SubBlocks`
- Otherwise → `NonShared` (unless further classification is available)

**Lane detection**: Check if `to` address (or any call target) has prefix `0x20C0`:
- If yes → `Payment`
- Otherwise → `General`

### 4.6 `txviz-storage` — Storage Backend

#### 4.6.1 `StorageBackend` trait

```rust
#[async_trait]
pub trait StorageBackend: Send + Sync {
    async fn put_block_graph(&self, graph: &BlockGraph) -> Result<()>;
    async fn get_block_graph(&self, number: u64) -> Result<Option<BlockGraph>>;
    async fn get_block_graph_by_hash(&self, hash: B256) -> Result<Option<BlockGraph>>;
    async fn list_blocks(&self, range: &BlockRange) -> Result<Vec<BlockSummary>>;
    async fn latest_block_number(&self) -> Result<Option<u64>>;
    async fn delete_blocks_before(&self, number: u64) -> Result<u64>;
}
```

#### 4.6.2 SQLite Schema

```sql
CREATE TABLE IF NOT EXISTS blocks (
    block_number  INTEGER PRIMARY KEY,
    block_hash    TEXT NOT NULL UNIQUE,
    parent_hash   TEXT NOT NULL,
    chain         TEXT NOT NULL,  -- 'ethereum' or 'tempo'
    timestamp     INTEGER NOT NULL,
    metrics_json  TEXT NOT NULL,  -- serialized BlockMetrics
    graph_path    TEXT NOT NULL,  -- relative path to JSON blob
    created_at    INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE INDEX IF NOT EXISTS idx_blocks_hash ON blocks(block_hash);
CREATE INDEX IF NOT EXISTS idx_blocks_timestamp ON blocks(timestamp);
```

**Graph blob storage**: `{data_dir}/graphs/{block_hash}.json.gz` (gzip compressed).

`put_block_graph`:
1. Serialize `BlockGraph` to JSON
2. Gzip compress
3. Write to `{data_dir}/graphs/{block_hash}.json.gz`
4. INSERT OR REPLACE into `blocks` table with `graph_path` = relative path

`get_block_graph`:
1. SELECT from `blocks` by number
2. Read + decompress file at `graph_path`
3. Deserialize `BlockGraph`

### 4.7 `txviz-api` — HTTP Route Handlers

#### 4.7.1 AppState

```rust
pub struct AppState {
    pub storage: Arc<dyn StorageBackend>,
    pub chain_identity: ChainIdentity,
    pub live_tx: broadcast::Sender<BlockUpdateEvent>,
}
```

#### 4.7.2 Routes

See [Section 7: API Contract](#7-api-contract) for full request/response specs.

#### 4.7.3 SSE Implementation

```rust
async fn live_handler(State(state): State<Arc<AppState>>) -> Sse<impl Stream<Item = ...>> {
    let rx = state.live_tx.subscribe();
    let stream = BroadcastStream::new(rx)
        .filter_map(|result| {
            result.ok().map(|event| {
                Event::default()
                    .event("block")
                    .json_data(&event)
                    .unwrap()
            })
        });

    // Prepend a heartbeat and set keep-alive
    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("heartbeat")
    )
}
```

### 4.8 `txviz-server` — Binary + Ingestor

#### 4.8.1 Startup Sequence

```
1. Parse CLI args (clap)
2. Initialize tracing/logging
3. Create data_dir if needed
4. Initialize SQLite (run migrations)
5. Connect to RPC (HTTP + optional WS)
6. Detect chain identity
7. Create broadcast channel for SSE
8. Start ingestor background task
9. Build axum router (API routes + static file serving)
10. Bind and serve
```

#### 4.8.2 Ingestor Background Task

```
async fn ingestor_loop(provider, storage, adapter, broadcast_tx, config):
  // Phase 1: Backfill
  latest = provider.latest_block_number()
  start = latest - config.backfill
  for n in start..=latest:
    if storage.get_block_graph(n).is_some():
      continue  // already computed
    process_block(n, provider, storage, adapter, broadcast_tx)

  // Phase 2: Live tracking
  let stream = provider.subscribe_new_blocks()
  for notification in stream:
    process_block(notification.number, provider, storage, adapter, broadcast_tx)

async fn process_block(n, provider, storage, adapter, broadcast_tx):
  // 1. Fetch block
  block = provider.get_block(n)

  // 2. Fetch traces (both passes)
  diff_traces = provider.trace_block_prestate_diff(n)
  prestate_traces = provider.trace_block_prestate(n)

  // 3. Parse tx nodes
  tx_nodes = block.transactions.iter()
    .zip(block.receipts.iter())
    .map(|(tx, receipt)| adapter.parse_tx_node(tx, receipt))
    .collect()

  // 4. Extract state access
  state_accesses = trace::extract_state_access(&diff_traces, &prestate_traces)

  // 5. Build DAG
  mut edges = dag::build_dependency_edges(&tx_nodes, &state_accesses)
  edges.extend(adapter.nonce_edges(&tx_nodes))
  edges.extend(adapter.structural_edges(&tx_nodes))
  edges = dag::deduplicate_edges(edges)

  // 6. Compute schedule + metrics
  (schedule, crit_info) = schedule::compute(&tx_nodes, &edges)
  metrics = metrics::compute(&tx_nodes, &edges, &schedule, &crit_info)

  // 7. Assemble BlockGraph
  graph = BlockGraph { block metadata, tx_nodes, edges, schedule, metrics }

  // 8. Store
  storage.put_block_graph(&graph)

  // 9. Broadcast to SSE subscribers
  broadcast_tx.send(BlockUpdateEvent::from(&graph))
```

---

## 5. Test Fixtures

### 5.1 Prestate Diff Trace — Simple (3 txs, Ethereum)

This fixture models:
- **tx0**: Token transfer — sender balance decreases, recipient balance increases, contract storage slot changes
- **tx1**: Contract creation — new account appears, no pre-state
- **tx2**: Simple ETH transfer — only balance changes

Dependencies expected:
- tx0 → tx2 (WAW on sender balance, if same sender; or independent if different senders)
- tx0, tx1 independent (different state keys)

```json
{
  "fixture_name": "simple_3tx_ethereum",
  "chain": "ethereum",
  "block_number": 19000000,
  "block_hash": "0x0000000000000000000000000000000000000000000000000000000000000001",
  "parent_hash": "0x0000000000000000000000000000000000000000000000000000000000000000",
  "timestamp": 1700000000,

  "transactions": [
    {
      "hash": "0xaaaa000000000000000000000000000000000000000000000000000000000000",
      "from": "0x1111111111111111111111111111111111111111",
      "to": "0xcccccccccccccccccccccccccccccccccccccccc",
      "type": "0x02",
      "nonce": "0x0a",
      "gas_used": "0xc350",
      "value": "0x0"
    },
    {
      "hash": "0xbbbb000000000000000000000000000000000000000000000000000000000000",
      "from": "0x2222222222222222222222222222222222222222",
      "to": null,
      "type": "0x02",
      "nonce": "0x00",
      "gas_used": "0x1e848",
      "value": "0x0"
    },
    {
      "hash": "0xcccc000000000000000000000000000000000000000000000000000000000000",
      "from": "0x1111111111111111111111111111111111111111",
      "to": "0x3333333333333333333333333333333333333333",
      "type": "0x02",
      "nonce": "0x0b",
      "gas_used": "0x5208",
      "value": "0xde0b6b3a7640000"
    }
  ],

  "diff_traces": [
    {
      "txHash": "0xaaaa000000000000000000000000000000000000000000000000000000000000",
      "result": {
        "pre": {
          "0x1111111111111111111111111111111111111111": {
            "balance": "0x1000000000000000000",
            "nonce": 10
          },
          "0xcccccccccccccccccccccccccccccccccccccccc": {
            "storage": {
              "0x0000000000000000000000000000000000000000000000000000000000000001": "0x00000000000000000000000000000000000000000000000000000000000003e8"
            }
          }
        },
        "post": {
          "0x1111111111111111111111111111111111111111": {
            "balance": "0x0f00000000000000000",
            "nonce": 11
          },
          "0xcccccccccccccccccccccccccccccccccccccccc": {
            "storage": {
              "0x0000000000000000000000000000000000000000000000000000000000000001": "0x00000000000000000000000000000000000000000000000000000000000002d0"
            }
          }
        }
      }
    },
    {
      "txHash": "0xbbbb000000000000000000000000000000000000000000000000000000000000",
      "result": {
        "pre": {
          "0x2222222222222222222222222222222222222222": {
            "balance": "0x2000000000000000000",
            "nonce": 0
          }
        },
        "post": {
          "0x2222222222222222222222222222222222222222": {
            "balance": "0x1f00000000000000000",
            "nonce": 1
          },
          "0x4444444444444444444444444444444444444444": {
            "balance": "0x0",
            "nonce": 1,
            "code": "0x6080604052",
            "storage": {
              "0x0000000000000000000000000000000000000000000000000000000000000000": "0x0000000000000000000000000000000000000000000000000000000000000001"
            }
          }
        }
      }
    },
    {
      "txHash": "0xcccc000000000000000000000000000000000000000000000000000000000000",
      "result": {
        "pre": {
          "0x1111111111111111111111111111111111111111": {
            "balance": "0x0f00000000000000000",
            "nonce": 11
          },
          "0x3333333333333333333333333333333333333333": {
            "balance": "0x0500000000000000000"
          }
        },
        "post": {
          "0x1111111111111111111111111111111111111111": {
            "balance": "0x0e00000000000000000",
            "nonce": 12
          },
          "0x3333333333333333333333333333333333333333": {
            "balance": "0x0600000000000000000"
          }
        }
      }
    }
  ],

  "expected_edges": [
    {
      "from_tx": 0,
      "to_tx": 2,
      "kind": "raw",
      "reason_summary": "tx2 reads balance/nonce of 0x1111 which tx0 wrote"
    },
    {
      "from_tx": 0,
      "to_tx": 2,
      "kind": "nonce_1d",
      "reason_summary": "same sender 0x1111, nonce 10 → 11"
    }
  ],
  "expected_edge_note": "tx0→tx2 should be deduplicated into a single edge with multiple reasons. tx1 is independent of both.",

  "expected_metrics": {
    "tx_count": 3,
    "component_count": 2,
    "speedup_note": "tx0+tx2 are chained (sequential), tx1 is independent. With weights 50000, 125000, 21000: critical path = 50000+21000=71000, total=196000, speedup≈2.76"
  }
}
```

### 5.2 Empty Block Fixture

```json
{
  "fixture_name": "empty_block",
  "block_number": 19000001,
  "transactions": [],
  "diff_traces": [],
  "expected_metrics": {
    "tx_count": 0,
    "edge_count": 0,
    "component_count": 0,
    "total_weight": 0,
    "critical_path_weight": 0,
    "speedup_upper_bound": 1.0,
    "max_parallelism": 0,
    "makespan": 0
  }
}
```

### 5.3 Fully Independent Block (Max Parallelism)

5 transactions, all from different senders, touching different state. Zero edges.

```json
{
  "fixture_name": "fully_independent",
  "block_number": 19000002,
  "transaction_count": 5,
  "expected_metrics": {
    "tx_count": 5,
    "edge_count": 0,
    "component_count": 5,
    "speedup_note": "All independent. Critical path = max single tx weight. If all equal weight W, speedup = 5.0"
  }
}
```

### 5.4 Fully Sequential Block (No Parallelism)

5 transactions, each depending on the previous (same sender, same storage slot).

```json
{
  "fixture_name": "fully_sequential",
  "block_number": 19000003,
  "transaction_count": 5,
  "expected_metrics": {
    "tx_count": 5,
    "edge_count": 4,
    "component_count": 1,
    "speedup_upper_bound": 1.0,
    "max_parallelism": 1
  }
}
```

### 5.5 Tempo Block with 2D Nonces

```json
{
  "fixture_name": "tempo_2d_nonce",
  "chain": "tempo",
  "block_number": 69000,
  "transactions": [
    { "from": "0xAAAA...", "nonce_key": "0x0", "nonce": 5, "type": "0x76", "note": "sequential lane" },
    { "from": "0xAAAA...", "nonce_key": "0x1", "nonce": 0, "type": "0x76", "note": "parallel lane 1" },
    { "from": "0xAAAA...", "nonce_key": "0x2", "nonce": 0, "type": "0x76", "note": "parallel lane 2" },
    { "from": "0xAAAA...", "nonce_key": "0x0", "nonce": 6, "type": "0x76", "note": "sequential lane" }
  ],
  "expected_edges": [
    { "from_tx": 0, "to_tx": 3, "kind": "nonce_2d", "note": "same sender, nonce_key=0, nonce 5→6" }
  ],
  "expected_note": "tx1 and tx2 have different nonce_keys, so NO nonce edges between them. They are fully independent from each other. Only tx0→tx3 has a nonce edge."
}
```

### 5.6 Tempo Block with Fee Sponsorship

```json
{
  "fixture_name": "tempo_fee_sponsorship",
  "chain": "tempo",
  "block_number": 69001,
  "transactions": [
    { "from": "0xUSER1", "fee_payer": "0xSPONSOR", "note": "sponsored tx" },
    { "from": "0xUSER2", "fee_payer": "0xSPONSOR", "note": "also sponsored by same payer" },
    { "from": "0xUSER3", "fee_payer": null, "note": "self-paying" }
  ],
  "expected_edges": [
    { "from_tx": 0, "to_tx": 1, "kind": "fee_sponsorship", "note": "shared fee payer 0xSPONSOR" }
  ],
  "expected_note": "tx2 is independent — different fee payer (self). tx0→tx1 linked through shared sponsor balance dependency."
}
```

---

## 6. Algorithm Specifications

> Fully specified in [Section 4.3](#43-txviz-coreschedule--critical-path--list-scheduling).
> Additional implementation notes:

### 6.1 Connected Components (Undirected)

Build an undirected adjacency list from `dep_edges`. Run BFS/DFS from each unvisited node.
Isolated nodes (no edges) each count as their own component.

```rust
fn count_components(node_count: usize, edges: &[DepEdge]) -> u32 {
    let mut adj: Vec<Vec<usize>> = vec![vec![]; node_count];
    for e in edges {
        adj[e.from_tx as usize].push(e.to_tx as usize);
        adj[e.to_tx as usize].push(e.from_tx as usize);
    }

    let mut visited = vec![false; node_count];
    let mut count = 0;
    for i in 0..node_count {
        if !visited[i] {
            count += 1;
            // BFS from i
            let mut queue = VecDeque::new();
            queue.push_back(i);
            visited[i] = true;
            while let Some(v) = queue.pop_front() {
                for &u in &adj[v] {
                    if !visited[u] {
                        visited[u] = true;
                        queue.push_back(u);
                    }
                }
            }
        }
    }
    count
}
```

### 6.2 petgraph Usage

Use `petgraph::graph::DiGraph<u32, DepEdgeKind>` internally:
- Node weights = tx_index
- Edge weights = DepEdgeKind
- Use `petgraph::algo::toposort` for topological ordering
- Use `petgraph::algo::connected_components` on `Graph::from_edges` (undirected) for component count

Keep petgraph as an internal implementation detail of `txviz-core`. The public API
takes/returns `Vec<DepEdge>` — never expose petgraph types.

---

## 7. API Contract

### 7.1 `GET /api/chain`

**Response** `200 OK`:
```json
{
  "chainId": 1,
  "chainKind": "ethereum",
  "clientVersion": "reth/v1.0.0"
}
```

### 7.2 `GET /api/blocks?from={n}&to={m}&limit={l}`

All params optional. Default: last 50 blocks.

**Response** `200 OK`:
```json
{
  "blocks": [
    {
      "chain": "ethereum",
      "blockNumber": 19000000,
      "blockHash": "0x...",
      "timestamp": 1700000000,
      "metrics": { ... }
    }
  ]
}
```

### 7.3 `GET /api/block/:number`

**Response** `200 OK`: Full `BlockGraph` JSON.

**Response** `404`: `{ "error": "Block not found" }`

### 7.4 `GET /api/block/hash/:hash`

Same as above but by hash.

### 7.5 `GET /api/live`

**Response**: `text/event-stream`

```
event: block
data: {"blockNumber":19000000,"blockHash":"0x...","timestamp":1700000000,"txCount":150,"speedupUpperBound":3.2,"criticalPathWeight":450000}

: heartbeat

event: block
data: {"blockNumber":19000001,...}
```

**Event name**: `block`
**Keep-alive**: comment heartbeat every 15 seconds.

### 7.6 `GET /` (and all non-`/api` paths)

Serve embedded static frontend files. Fallback to `index.html` for client-side routing.

### 7.7 Error Response Format

All errors use:
```json
{
  "error": "Human-readable message"
}
```
With appropriate HTTP status codes (400, 404, 500, 502).

---

## 8. Execution Plan

### 8.0 Prerequisites (must be done first, before any parallel work)

**Task 0: Workspace Scaffold** (~15 min)

Set up the Cargo workspace with all crate skeletons and the React app shell.
No logic — just `Cargo.toml` files, `lib.rs` stubs, `package.json`, etc.

This unblocks everything else.

Deliverables:
- [ ] Root `Cargo.toml` with workspace members
- [ ] Each crate's `Cargo.toml` with dependencies (see [Section 9](#9-dependency--crate-manifest))
- [ ] Each crate's `src/lib.rs` (or `main.rs`) with module declarations
- [ ] `txviz-core/src/model.rs` with all types from [Section 3.1](#31-rust-types-txviz-coresrcmodelrs)
- [ ] `web/package.json` with dependencies
- [ ] `web/src/types/index.ts` with all types from [Section 3.2](#32-typescript-types-websrctypesindexts)
- [ ] `cargo check` passes (empty impls / `todo!()` are fine)
- [ ] `cd web && npm install` succeeds

**Verification**: `cargo check --workspace` and `cd web && npx tsc --noEmit` both pass.

---

### 8.1 Parallel Work Streams (after Task 0)

Once the scaffold exists, these five streams can proceed **simultaneously**.
Each stream follows TDD: write tests first, run them (they fail), implement, run until green.

```
                    ┌──────────────┐
                    │  Task 0:     │
                    │  Scaffold    │
                    └──────┬───────┘
                           │
            ┌──────────────┼──────────────┬──────────────┬──────────────┐
            ▼              ▼              ▼              ▼              ▼
    ┌───────────┐  ┌───────────┐  ┌───────────┐  ┌───────────┐  ┌───────────┐
    │ Stream A  │  │ Stream B  │  │ Stream C  │  │ Stream D  │  │ Stream E  │
    │ txviz-    │  │ txviz-    │  │ txviz-    │  │ txviz-api │  │ web/      │
    │ core      │  │ chain     │  │ storage   │  │ (routes)  │  │ frontend  │
    └─────┬─────┘  └─────┬─────┘  └─────┬─────┘  └─────┬─────┘  └─────┬─────┘
          │              │              │              │              │
          └──────────────┴──────────────┴──────┬───────┴──────────────┘
                                               ▼
                                       ┌───────────────┐
                                       │  Task 6:      │
                                       │  Integration  │
                                       │  (txviz-      │
                                       │   server)     │
                                       └───────────────┘
```

---

#### Stream A: `txviz-core` (Pure Logic)

**No dependencies** on other crates (except `txviz-core::model` from scaffold).
All tests use hardcoded fixture data — no IO, no mocks.

##### Task A1: Trace Parsing (`txviz-core::trace`)

Write tests FIRST using the fixtures from [Section 5](#5-test-fixtures).

Tests to write:
```rust
#[cfg(test)]
mod tests {
    // Test: parse diff trace with 3 txs (fixture 5.1)
    // → verify write sets: tx0 writes Balance(0x1111), Nonce(0x1111), Storage(0xcccc, slot1)
    // → verify tx1 writes Balance(0x2222), Nonce(0x2222), Code(0x4444), Storage(0x4444, slot0), Balance(0x4444), Nonce(0x4444)
    // → verify tx2 writes Balance(0x1111), Nonce(0x1111), Balance(0x3333)
    fn test_parse_diff_traces_simple_3tx() { ... }

    // Test: empty trace input → empty output
    fn test_parse_diff_traces_empty() { ... }

    // Test: trace entry with "error" field → skip that tx
    fn test_parse_diff_traces_with_error() { ... }

    // Test: nonce as JSON number vs hex string
    fn test_parse_nonce_formats() { ... }

    // Test: selfdestruct (addr in pre, not in post)
    fn test_parse_selfdestruct() { ... }

    // Test: contract creation (addr in post, not in pre)
    fn test_parse_contract_creation() { ... }

    // Test: accept both wrapper formats (with/without txHash wrapper)
    fn test_parse_both_json_formats() { ... }

    // Test: parse prestate (non-diff) traces for read set
    fn test_parse_prestate_traces() { ... }

    // Test: combine diff + prestate → correct read/write sets
    fn test_combine_diff_and_prestate() { ... }
}
```

Run: `cargo test -p txviz-core trace`

##### Task A2: DAG Construction (`txviz-core::dag`)

Tests to write:
```rust
#[cfg(test)]
mod tests {
    // Test: 3-tx fixture → expected edges (tx0→tx2 RAW on balance, deduplicated)
    fn test_build_edges_simple_3tx() { ... }

    // Test: fully independent txs → zero edges
    fn test_build_edges_independent() { ... }

    // Test: fully sequential (same sender, same slot) → chain of edges
    fn test_build_edges_sequential() { ... }

    // Test: edge deduplication — same (from,to) pair with multiple reasons
    fn test_edge_deduplication() { ... }

    // Test: nonce edges (Ethereum 1D) — same sender, consecutive nonces
    fn test_nonce_edges_1d() { ... }

    // Test: nonce edges (Tempo 2D) — same sender, different nonce_keys = independent
    fn test_nonce_edges_2d_independent() { ... }

    // Test: nonce edges (Tempo 2D) — same sender, same nonce_key = linked
    fn test_nonce_edges_2d_linked() { ... }

    // Test: fee sponsorship edges — shared fee payer creates dependency
    fn test_fee_sponsorship_edges() { ... }

    // Test: empty input → empty edges
    fn test_build_edges_empty() { ... }

    // Test: single tx → no edges
    fn test_build_edges_single_tx() { ... }
}
```

Run: `cargo test -p txviz-core dag`

##### Task A3: Schedule + Metrics (`txviz-core::schedule`, `txviz-core::metrics`)

Tests to write:
```rust
#[cfg(test)]
mod tests {
    // Test: topological sort on simple DAG
    fn test_toposort_simple() { ... }

    // Test: topological sort on disconnected graph
    fn test_toposort_disconnected() { ... }

    // Test: critical path — chain of 3 txs with weights 100, 200, 300 → crit = 600
    fn test_critical_path_chain() { ... }

    // Test: critical path — diamond DAG (A→B, A→C, B→D, C→D)
    fn test_critical_path_diamond() { ... }

    // Test: critical path — no edges → crit = max single weight
    fn test_critical_path_independent() { ... }

    // Test: schedule — chain → all in lane 0, sequential starts
    fn test_schedule_chain() { ... }

    // Test: schedule — independent → parallel lanes, all start at 0
    fn test_schedule_independent() { ... }

    // Test: schedule — diamond DAG → correct start times
    fn test_schedule_diamond() { ... }

    // Test: schedule — critical path nodes marked correctly
    fn test_schedule_critical_marking() { ... }

    // Test: max parallelism — independent txs → count equals tx count
    fn test_max_parallelism_independent() { ... }

    // Test: max parallelism — chain → 1
    fn test_max_parallelism_chain() { ... }

    // Test: component count — various topologies
    fn test_component_count() { ... }

    // Test: metrics — empty block
    fn test_metrics_empty() { ... }

    // Test: metrics — speedup_upper_bound = total_weight / critical_path_weight
    fn test_metrics_speedup() { ... }

    // Test: metrics — Tempo-specific fields populated
    fn test_metrics_tempo_fields() { ... }

    // Test: metrics — division by zero (critical_path = 0) → speedup = 1.0
    fn test_metrics_zero_weight() { ... }
}
```

Run: `cargo test -p txviz-core schedule metrics`

---

#### Stream B: `txviz-chain` (RPC + Chain Adapters)

Tests use mock RPC responses (no real node needed).

##### Task B1: Chain Detection

Tests:
```rust
#[cfg(test)]
mod tests {
    // Test: chain_id 1 → Ethereum
    fn test_detect_ethereum_mainnet() { ... }

    // Test: chain_id 42431 → Tempo (Moderato)
    fn test_detect_tempo_moderato() { ... }

    // Test: unknown chain_id + client version contains "tempo" → Tempo
    fn test_detect_tempo_from_client_version() { ... }

    // Test: unknown chain_id + unknown client → Ethereum fallback
    fn test_detect_unknown_defaults_ethereum() { ... }

    // Test: force_chain override
    fn test_force_chain_override() { ... }
}
```

##### Task B2: Ethereum Chain Adapter

Tests:
```rust
#[cfg(test)]
mod tests {
    // Test: parse standard EIP-1559 tx JSON → TxNode
    fn test_parse_ethereum_tx() { ... }

    // Test: parse legacy tx JSON → TxNode
    fn test_parse_legacy_tx() { ... }

    // Test: nonce_edges for 3 txs from same sender → chain
    fn test_ethereum_nonce_edges() { ... }

    // Test: nonce_edges for txs from different senders → independent
    fn test_ethereum_nonce_edges_different_senders() { ... }

    // Test: structural_edges returns empty (no Tempo features)
    fn test_ethereum_structural_edges_empty() { ... }
}
```

##### Task B3: Tempo Chain Adapter

Tests:
```rust
#[cfg(test)]
mod tests {
    // Test: parse 0x76 tx JSON with all Tempo fields → TxNode with TempoTxMeta
    fn test_parse_tempo_tx_full() { ... }

    // Test: parse 0x76 tx with missing optional fields → graceful defaults
    fn test_parse_tempo_tx_partial() { ... }

    // Test: lane detection — call to 0x20C0... address → Payment
    fn test_tempo_lane_detection_payment() { ... }

    // Test: lane detection — call to regular address → General
    fn test_tempo_lane_detection_general() { ... }

    // Test: section detection — nonce_key prefix 0x5b → SubBlocks
    fn test_tempo_section_subblock() { ... }

    // Test: section detection — system tx to address(0) → System
    fn test_tempo_section_system() { ... }

    // Test: nonce_edges 2D — same sender, different nonce_keys → independent
    fn test_tempo_nonce_edges_parallel() { ... }

    // Test: nonce_edges 2D — same sender, same nonce_key → chained
    fn test_tempo_nonce_edges_sequential() { ... }

    // Test: nonce_edges — expiring nonce (MAX key) → no nonce edge
    fn test_tempo_expiring_nonce_no_edge() { ... }

    // Test: structural_edges — fee sponsorship
    fn test_tempo_fee_sponsorship_edges() { ... }
}
```

##### Task B4: RPC Client

Tests (using `wiremock` or similar for mock HTTP):
```rust
#[cfg(test)]
mod tests {
    // Test: trace_block_prestate_diff sends correct JSON-RPC request
    fn test_rpc_trace_request_format() { ... }

    // Test: get_block parses full block response correctly
    fn test_rpc_get_block() { ... }

    // Test: timeout handling
    fn test_rpc_timeout() { ... }

    // Test: RPC error response → proper error propagation
    fn test_rpc_error_handling() { ... }
}
```

Run: `cargo test -p txviz-chain`

---

#### Stream C: `txviz-storage` (SQLite + FS)

Tests use in-memory SQLite (`":memory:"`) and `tempdir`.

##### Task C1: Storage Backend Implementation

Tests:
```rust
#[cfg(test)]
mod tests {
    // Test: put + get roundtrip
    fn test_put_get_roundtrip() { ... }

    // Test: get nonexistent block → None
    fn test_get_nonexistent() { ... }

    // Test: get by hash
    fn test_get_by_hash() { ... }

    // Test: list_blocks with range
    fn test_list_blocks_range() { ... }

    // Test: list_blocks with limit
    fn test_list_blocks_limit() { ... }

    // Test: list_blocks returns newest first
    fn test_list_blocks_order() { ... }

    // Test: latest_block_number
    fn test_latest_block_number() { ... }

    // Test: latest_block_number empty → None
    fn test_latest_block_number_empty() { ... }

    // Test: put overwrites existing block (idempotent)
    fn test_put_overwrite() { ... }

    // Test: delete_blocks_before
    fn test_delete_old_blocks() { ... }

    // Test: graph JSON file is created and readable
    fn test_graph_file_created() { ... }

    // Test: graph file is gzip compressed
    fn test_graph_file_compressed() { ... }
}
```

Run: `cargo test -p txviz-storage`

---

#### Stream D: `txviz-api` (Route Handlers)

Tests use mock `StorageBackend` (in-memory HashMap). No real HTTP server needed
for unit tests — use `axum::test_helpers` or construct handlers directly.

##### Task D1: Route Handlers

Tests:
```rust
#[cfg(test)]
mod tests {
    // Test: GET /api/chain → returns chain identity
    fn test_chain_endpoint() { ... }

    // Test: GET /api/block/19000000 → returns BlockGraph JSON
    fn test_get_block_by_number() { ... }

    // Test: GET /api/block/99999999 → 404
    fn test_get_block_not_found() { ... }

    // Test: GET /api/block/hash/0x... → returns BlockGraph
    fn test_get_block_by_hash() { ... }

    // Test: GET /api/blocks → returns list of BlockSummary
    fn test_list_blocks() { ... }

    // Test: GET /api/blocks?from=100&to=200 → filtered list
    fn test_list_blocks_with_range() { ... }

    // Test: GET /api/blocks?limit=10 → respects limit
    fn test_list_blocks_with_limit() { ... }

    // Test: CORS headers present
    fn test_cors_headers() { ... }
}
```

##### Task D2: SSE Endpoint

Tests:
```rust
#[cfg(test)]
mod tests {
    // Test: SSE endpoint returns correct content-type
    fn test_sse_content_type() { ... }

    // Test: broadcast event appears in SSE stream
    fn test_sse_receives_broadcast() { ... }

    // Test: multiple subscribers receive same event
    fn test_sse_multiple_subscribers() { ... }
}
```

Run: `cargo test -p txviz-api`

---

#### Stream E: `web/` (React Frontend)

Can be developed entirely against fixture JSON data (no running backend needed).
Use Vite + React + TypeScript.

##### Task E1: Types + API Client

- [ ] `web/src/types/index.ts` — all TypeScript types (from scaffold)
- [ ] `web/src/api/client.ts` — typed fetch functions:
  - `getChain(): Promise<ChainIdentity>`
  - `getBlock(n: number): Promise<BlockGraph>`
  - `getBlockByHash(h: string): Promise<BlockGraph>`
  - `listBlocks(opts?): Promise<{ blocks: BlockSummary[] }>`
  - `subscribeLive(onBlock: (e: BlockUpdateEvent) => void): EventSource`

Tests (vitest):
```typescript
// Test: API client constructs correct URLs
// Test: API client handles 404 → null
// Test: SSE subscription creates EventSource with correct URL
```

##### Task E2: DependencyGraph Component (Cytoscape.js)

- [ ] Install: `cytoscape`, `react-cytoscapejs`, `cytoscape-dagre`
- [ ] Component takes `BlockGraph` as prop
- [ ] Nodes: one per `TxNode`, labeled with tx index + short hash
- [ ] Edges: one per `DepEdge`, colored by kind
- [ ] Layout: dagre (top-to-bottom DAG)
- [ ] Click node → emit `onSelectTx(txIndex)` event
- [ ] Hover edge → show reasons tooltip
- [ ] Filter controls: toggle edge kinds on/off

Color scheme:
| Element | Color |
|---------|-------|
| Node (default) | `#4a9eff` |
| Node (payment lane) | `#22c55e` |
| Node (general lane) | `#4a9eff` |
| Node (system) | `#6b7280` |
| Node (selected) | `#f59e0b` |
| Node (critical path) | `#ef4444` |
| Edge RAW | `#ef4444` (red) |
| Edge WAW | `#f97316` (orange) |
| Edge Nonce1D | `#6b7280` (gray) |
| Edge Nonce2D | `#8b5cf6` (purple) |
| Edge FeeSponsorship | `#22c55e` (green) |

Tests (vitest + testing-library):
```typescript
// Test: renders correct number of nodes
// Test: renders correct number of edges
// Test: node click fires onSelectTx
// Test: filter toggle hides/shows edge types
```

##### Task E3: ScheduleGantt Component (visx)

- [ ] Install: `@visx/group`, `@visx/scale`, `@visx/shape`, `@visx/axis`, `@visx/tooltip`
- [ ] Component takes `schedule: ScheduleItem[]`, `txNodes: TxNode[]`
- [ ] X-axis: time (weight units / gas)
- [ ] Y-axis: lanes
- [ ] Each bar = one tx, width = duration, positioned at (start, lane)
- [ ] Critical path bars highlighted in red
- [ ] Hover bar → tooltip with tx details

Tests:
```typescript
// Test: renders correct number of bars
// Test: bar widths proportional to duration
// Test: critical path bars have highlight class
// Test: hover shows tooltip
```

##### Task E4: Pages

**Home.tsx**:
- Show latest block summary
- Live indicator (green dot when SSE connected)
- Auto-update when new block arrives
- Link to block detail

**BlockList.tsx**:
- Paginated list of blocks
- Each row: block number, timestamp, tx count, speedup, critical path
- Click row → navigate to detail

**BlockDetail.tsx**:
- Tabs: Graph | Gantt
- MetricsPanel at top
- TxInspector side panel (opens on node click)
- Block navigation (prev/next arrows, number input)

Tests:
```typescript
// Test: Home shows latest block
// Test: Home updates on SSE event
// Test: BlockList renders rows
// Test: BlockList pagination
// Test: BlockDetail loads graph data
// Test: BlockDetail tab switching
// Test: TxInspector shows details for selected tx
```

Run: `cd web && npx vitest run`

---

### 8.2 Integration (after all streams complete)

#### Task 6: `txviz-server` — Wire Everything Together

This is the ONLY sequential task. It composes all the pieces.

##### Task 6.1: Ingestor

```rust
#[cfg(test)]
mod tests {
    // Test: process_block produces correct BlockGraph for fixture block
    fn test_process_block_end_to_end() { ... }

    // Test: backfill processes N blocks on startup
    fn test_backfill() { ... }

    // Test: skips already-stored blocks
    fn test_skip_existing() { ... }

    // Test: handles trace fetch error gracefully (logs, continues)
    fn test_trace_error_resilience() { ... }
}
```

##### Task 6.2: CLI + Main

```rust
#[cfg(test)]
mod tests {
    // Test: CLI parses all flags correctly
    fn test_cli_parsing() { ... }

    // Test: default values are sane
    fn test_cli_defaults() { ... }
}
```

##### Task 6.3: Static File Embedding

- [ ] Build frontend: `cd web && npm run build`
- [ ] Embed `web/dist/` using `rust-embed`
- [ ] Serve at `/` with SPA fallback (non-`/api` paths → `index.html`)
- [ ] Test: `GET /` returns HTML
- [ ] Test: `GET /assets/index-xxx.js` returns JS with correct content-type

##### Task 6.4: End-to-End Smoke Test

Manual (requires a running node):
```bash
# Start with a local reth node
cargo run -p txviz-server -- \
  --rpc-http http://127.0.0.1:8545 \
  --backfill 5 \
  --bind 127.0.0.1:8080

# Verify:
curl http://localhost:8080/api/chain           # → chain identity
curl http://localhost:8080/api/blocks           # → list of blocks
curl http://localhost:8080/api/block/latest      # → latest block graph
curl http://localhost:8080/                     # → HTML frontend
# Open browser to http://localhost:8080 → visual verification
```

---

### 8.3 Execution Summary

```
Time →
─────────────────────────────────────────────────────────────────

Task 0 (scaffold)     ████ (15 min)
                         │
                         ├── Stream A (core)    ██████████████ (A1→A2→A3)
                         ├── Stream B (chain)   ██████████████ (B1→B2→B3→B4)
                         ├── Stream C (storage) ████████ (C1)
                         ├── Stream D (api)     ████████ (D1→D2)
                         └── Stream E (web)     ██████████████ (E1→E2→E3→E4)
                                                              │
                                                              ▼
                                                Task 6 (integration) ████████
```

**Parallelism**: 5 streams run simultaneously.
**Critical path**: Stream A or Stream B (whichever takes longer) → Task 6.

---

## 9. Dependency & Crate Manifest

### 9.1 Root `Cargo.toml`

```toml
[workspace]
members = [
    "crates/txviz-core",
    "crates/txviz-chain",
    "crates/txviz-storage",
    "crates/txviz-api",
    "crates/txviz-server",
]
resolver = "2"

[workspace.dependencies]
# Shared across crates
alloy-primitives = { version = "0.8", features = ["serde"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
anyhow = "1"
thiserror = "2"
tokio = { version = "1", features = ["full"] }
tracing = "0.1"
async-trait = "0.1"
futures = "0.3"
```

### 9.2 `txviz-core/Cargo.toml`

```toml
[package]
name = "txviz-core"
version = "0.1.0"
edition = "2021"

[dependencies]
alloy-primitives.workspace = true
serde.workspace = true
serde_json.workspace = true
anyhow.workspace = true
thiserror.workspace = true
petgraph = "0.7"
indexmap = { version = "2", features = ["serde"] }

[dev-dependencies]
pretty_assertions = "1"
```

### 9.3 `txviz-chain/Cargo.toml`

```toml
[package]
name = "txviz-chain"
version = "0.1.0"
edition = "2021"

[dependencies]
txviz-core = { path = "../txviz-core" }
alloy-primitives.workspace = true
serde.workspace = true
serde_json.workspace = true
anyhow.workspace = true
thiserror.workspace = true
tokio.workspace = true
tracing.workspace = true
async-trait.workspace = true
futures.workspace = true
reqwest = { version = "0.12", features = ["json"] }
tokio-tungstenite = { version = "0.24", features = ["native-tls"] }
url = "2"

[dev-dependencies]
wiremock = "0.6"
tokio = { workspace = true, features = ["test-util"] }
```

### 9.4 `txviz-storage/Cargo.toml`

```toml
[package]
name = "txviz-storage"
version = "0.1.0"
edition = "2021"

[dependencies]
txviz-core = { path = "../txviz-core" }
alloy-primitives.workspace = true
serde.workspace = true
serde_json.workspace = true
anyhow.workspace = true
thiserror.workspace = true
tokio.workspace = true
tracing.workspace = true
async-trait.workspace = true
sqlx = { version = "0.8", features = ["runtime-tokio-rustls", "sqlite"] }
flate2 = "1"

[dev-dependencies]
tempfile = "3"
tokio = { workspace = true, features = ["test-util"] }
```

### 9.5 `txviz-api/Cargo.toml`

```toml
[package]
name = "txviz-api"
version = "0.1.0"
edition = "2021"

[dependencies]
txviz-core = { path = "../txviz-core" }
txviz-storage = { path = "../txviz-storage" }
alloy-primitives.workspace = true
serde.workspace = true
serde_json.workspace = true
anyhow.workspace = true
tokio.workspace = true
tracing.workspace = true
async-trait.workspace = true
axum = { version = "0.8", features = ["macros"] }
tower-http = { version = "0.6", features = ["cors", "trace", "fs"] }
tokio-stream = "0.1"

[dev-dependencies]
axum-test = "16"
tokio = { workspace = true, features = ["test-util"] }
```

### 9.6 `txviz-server/Cargo.toml`

```toml
[package]
name = "txviz-server"
version = "0.1.0"
edition = "2021"

[dependencies]
txviz-core = { path = "../txviz-core" }
txviz-chain = { path = "../txviz-chain" }
txviz-storage = { path = "../txviz-storage" }
txviz-api = { path = "../txviz-api" }
alloy-primitives.workspace = true
serde.workspace = true
serde_json.workspace = true
anyhow.workspace = true
tokio.workspace = true
tracing.workspace = true
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
clap = { version = "4", features = ["derive"] }
rust-embed = "8"
axum = { version = "0.8", features = ["macros"] }
tower-http = { version = "0.6", features = ["cors", "trace", "fs"] }
mime_guess = "2"
```

### 9.7 `web/package.json`

```json
{
  "name": "txviz-web",
  "private": true,
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "tsc -b && vite build",
    "preview": "vite preview",
    "test": "vitest run",
    "typecheck": "tsc --noEmit"
  },
  "dependencies": {
    "react": "^19",
    "react-dom": "^19",
    "react-router-dom": "^7",
    "cytoscape": "^3.30",
    "cytoscape-dagre": "^2.5",
    "react-cytoscapejs": "^2.0",
    "@visx/group": "^3",
    "@visx/scale": "^3",
    "@visx/shape": "^3",
    "@visx/axis": "^3",
    "@visx/tooltip": "^3"
  },
  "devDependencies": {
    "@types/react": "^19",
    "@types/react-dom": "^19",
    "@types/cytoscape": "^3",
    "@vitejs/plugin-react": "^4",
    "typescript": "^5.7",
    "vite": "^6",
    "vitest": "^3",
    "@testing-library/react": "^16",
    "jsdom": "^25",
    "tailwindcss": "^4"
  }
}
```

---

## 10. CLI Specification

### 10.1 Full CLI Help

```
txviz-server — Transaction dependency graph visualizer

Usage: txviz-server [OPTIONS] --rpc-http <URL>

Options:
  RPC Connection:
    --rpc-http <URL>          HTTP JSON-RPC endpoint (required)
    --rpc-ws <URL>            WebSocket JSON-RPC endpoint (enables subscriptions)
    --rpc-timeout <DURATION>  RPC request timeout [default: 30s]

  Operation:
    --start-block <SPEC>      Starting block: "latest" or block number [default: latest]
    --backfill <N>            Number of historical blocks to process on startup [default: 0]
    --poll-interval <DURATION> Polling interval for new blocks [default: 2s]
    --recompute               Force recomputation of existing blocks

  Storage:
    --data-dir <PATH>         Data directory for SQLite + graph files [default: ./.txviz]

  Server:
    --bind <ADDR>             Listen address [default: 127.0.0.1:8080]
    --cors-origin <ORIGIN>    CORS allowed origin [default: *]
    --ui-proxy <URL>          Proxy non-API requests to this URL (for Vite dev server)

  Chain:
    --force-chain <KIND>      Override chain detection [possible values: ethereum, tempo]

  Logging:
    --log-level <LEVEL>       Log level [default: info] [possible values: trace, debug, info, warn, error]
    --log-json                Output logs as JSON

  -h, --help                  Print help
  -V, --version               Print version
```

### 10.2 Examples

```bash
# Minimal: connect to local reth, serve on default port
txviz-server --rpc-http http://127.0.0.1:8545

# With backfill and WebSocket subscription
txviz-server --rpc-http http://127.0.0.1:8545 --rpc-ws ws://127.0.0.1:8546 --backfill 100

# Tempo node on custom port
txviz-server --rpc-http http://127.0.0.1:9545 --force-chain tempo --bind 0.0.0.0:3000

# Development mode with Vite proxy
txviz-server --rpc-http http://127.0.0.1:8545 --ui-proxy http://localhost:5173

# Recompute all stored blocks (e.g., after algorithm changes)
txviz-server --rpc-http http://127.0.0.1:8545 --recompute
```

### 10.3 Startup Banner

```
╔══════════════════════════════════════════════╗
║  txviz-server v0.1.0                         ║
║  Transaction Dependency Visualizer           ║
╠══════════════════════════════════════════════╣
║  Chain:    Tempo (Moderato, chain_id=42431)  ║
║  RPC:      http://127.0.0.1:8545            ║
║  UI:       http://127.0.0.1:8080            ║
║  Data:     /home/dcline/.txviz              ║
║  Backfill: 100 blocks                       ║
╚══════════════════════════════════════════════╝
```

---

## Appendix A: Phase 2 Delta (Production Deployment)

Phase 2 only changes the **runtime layer**. No changes to `txviz-core`, `txviz-chain`, or `txviz-api`.

| Phase 1 (Local) | Phase 2 (Production) |
|---|---|
| `txviz-server` (axum binary) | `txviz-worker` (Cloudflare Worker) |
| SQLite index | Workers KV |
| Filesystem JSON.gz blobs | R2 objects |
| `tokio::broadcast` → SSE | Durable Object → SSE |
| Internal poll loop | Cron Trigger |
| `rust-embed` static files | Workers Sites / Pages |

The `StorageBackend` trait abstraction makes this a mechanical port:
- `put_block_graph` → KV put (index) + R2 put (blob)
- `get_block_graph` → KV get (index) + R2 get (blob)
- `list_blocks` → KV list with prefix

---

## Appendix B: Key Design Decisions

| Decision | Rationale |
|---|---|
| **Rust backend** | Same language as reth/Tempo. No impedance mismatch parsing traces. Fast. |
| **petgraph internal only** | Don't leak graph library types into API. Serialize to `Vec<DepEdge>`. |
| **Two-pass tracing** | diffMode alone doesn't give reads. Second pass (prestate mode) gives full touched set. |
| **Edge deduplication** | One edge per (from, to) pair with multiple reasons. Cleaner graphs. |
| **Unlimited lanes in schedule** | Shows theoretical max parallelism. Real hardware limits are a separate concern. |
| **gas_used as weight** | Best available proxy for execution time. Consistent across chains. |
| **SQLite + FS** | Simple, no external services, fast for local use. Maps cleanly to KV+R2. |
| **SSE not WebSocket** | One-way server→client is sufficient. SSE is simpler, browser-native. |
| **Embedded static files** | One binary = one command. No separate file server needed. |
| **Conservative read sets** | If unsure whether something was read, include it. May add edges but preserves correctness. |

---

## Appendix C: Edge Case Handling

| Edge Case | Handling |
|---|---|
| **Empty block** | Valid BlockGraph with 0 nodes, 0 edges, metrics all zero, speedup = 1.0 |
| **Single tx block** | 1 node, 0 edges, speedup = 1.0, max_parallelism = 1 |
| **Reverted tx** | Still include in graph. Nonce still increments, gas fees still paid. State writes may be absent from diff trace — handled naturally (fewer write edges). |
| **Selfdestruct** | Address in pre but not post → writes to Balance, Code, all Storage of that address |
| **Contract creation** | Address in post but not pre → writes to Balance, Nonce, Code, Storage |
| **Trace error for one tx** | Log warning, skip that tx's state access analysis. Still include it as a node with nonce edges only. |
| **RPC timeout** | Retry once, then skip block with error log. Ingestor continues to next block. |
| **Block reorg** | If block hash changes for a number we already stored, overwrite. Log warning. |
| **Very large block (1000+ txs)** | May produce dense graph. Frontend should handle: limit node labels by zoom level, collapse isolated nodes. Backend: bounded edge generation (lastWriter map keeps it O(n * keys)). |
| **Zero gas_used** | Use weight = 1 as fallback. Prevents division by zero in schedule. |
