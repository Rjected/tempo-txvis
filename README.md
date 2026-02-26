# tempo-txviz

Transaction dependency graph visualizer for Ethereum and Tempo blocks. Shows which transactions can run in parallel, computes optimal schedules, and highlights Tempo-specific parallelism features (2D nonces, subblocks, payment lanes).

## Prerequisites

- Rust toolchain (stable)
- Node.js ≥ 18
- Access to an Ethereum or Tempo node with `debug_traceBlockByNumber` enabled

## Build

```bash
# Install frontend dependencies and build
cd web && npm install && npm run build && cd ..

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
cd web && npm run dev
```

## Tests

```bash
# All Rust tests
cargo test --workspace

# Frontend tests
cd web && npx vitest run
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
