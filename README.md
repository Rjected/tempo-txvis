# tempo-txviz

Transaction dependency graph visualizer for Ethereum and Tempo blocks. Shows which transactions can run in parallel, computes optimal schedules, and highlights Tempo-specific parallelism features (2D nonces, subblocks, payment lanes).

## Prerequisites

- Rust toolchain (stable)
- [Bun](https://bun.sh) ≥ 1.0
- Access to an Ethereum or Tempo node with the required RPC namespaces enabled

### Required RPC Namespaces

The node must expose these namespaces on the HTTP endpoint:

| Namespace | Methods Used | Notes |
|-----------|-------------|-------|
| **eth** | `eth_chainId`, `eth_blockNumber`, `eth_getBlockByNumber`, `eth_getTransactionReceipt` | Standard, usually enabled by default |
| **web3** | `web3_clientVersion` | Used for chain detection |
| **debug** | `debug_traceBlockByNumber` | **Required** — uses `prestateTracer` with `diffMode`. Often disabled by default. |

#### reth

```bash
reth node --http --http.api eth,web3,debug
```

#### Tempo

```bash
tempo --http --http.api eth,web3,debug
```

#### Geth

```bash
geth --http --http.api eth,web3,debug
```

> **Note:** The `debug` namespace is compute-heavy and typically disabled on public RPC providers. You need your own node.

## Build

```bash
# Install frontend dependencies and build
cd web && bun install && bun run build && cd ..

# Build the server (embeds the frontend)
cargo build -p txviz-server --release
```

## Run

```bash
# Minimal: connect to a local node
cargo run -p txviz-server --release -- --rpc-http http://127.0.0.1:8545

# With backfill (process last 100 blocks on startup)
cargo run -p txviz-server --release -- \
  --rpc-http http://127.0.0.1:8545 \
  --backfill 100

# With WebSocket subscription for live blocks
cargo run -p txviz-server --release -- \
  --rpc-http http://127.0.0.1:8545 \
  --rpc-ws ws://127.0.0.1:8546 \
  --backfill 50

# Tempo node
cargo run -p txviz-server --release -- \
  --rpc-http http://127.0.0.1:9545 \
  --force-chain tempo \
  --backfill 20

# Custom bind address and data directory
cargo run -p txviz-server --release -- \
  --rpc-http http://127.0.0.1:8545 \
  --bind 0.0.0.0:3000 \
  --data-dir /tmp/txviz-data
```

Then open http://127.0.0.1:8080 (or your custom `--bind` address).

## Development

```bash
# Run the backend (proxies UI requests to Vite dev server)
cargo run -p txviz-server -- \
  --rpc-http http://127.0.0.1:8545 \
  --ui-proxy http://localhost:5173

# In another terminal, start the Vite dev server
cd web && bun run dev
```

## Tests

```bash
# All Rust tests
cargo test --workspace

# Frontend tests
cd web && bun run test
```

## CLI Reference

```
txviz-server [OPTIONS] --rpc-http <URL>

Options:
  --rpc-http <URL>            HTTP JSON-RPC endpoint (required)
  --rpc-ws <URL>              WebSocket endpoint (enables subscriptions)
  --rpc-timeout <DURATION>    RPC request timeout [default: 30s]
  --start-block <SPEC>        "latest" or block number [default: latest]
  --backfill <N>              Historical blocks to process [default: 0]
  --poll-interval <DURATION>  Polling interval [default: 2s]
  --recompute                 Force recomputation of existing blocks
  --data-dir <PATH>           Data directory [default: ./.txviz]
  --bind <ADDR>               Listen address [default: 127.0.0.1:8080]
  --cors-origin <ORIGIN>      CORS allowed origin [default: *]
  --ui-proxy <URL>            Proxy to Vite dev server
  --force-chain <KIND>        Override detection [ethereum, tempo]
  --log-level <LEVEL>         Log level [default: info]
  --log-json                  JSON log output
```
