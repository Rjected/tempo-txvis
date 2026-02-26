use std::sync::Arc;

use anyhow::Result;
use futures::StreamExt;
use tokio::sync::broadcast;
use tracing::{info, warn};

use txviz_chain::adapter::ChainAdapter;
use txviz_chain::provider::ChainProvider;
use txviz_core::model::{BlockGraph, BlockUpdateEvent};
use txviz_storage::backend::StorageBackend;

pub struct IngestorConfig {
    pub backfill: u64,
    pub recompute: bool,
}

pub async fn run(
    provider: Arc<dyn ChainProvider>,
    adapter: Arc<dyn ChainAdapter>,
    storage: Arc<dyn StorageBackend>,
    broadcast_tx: broadcast::Sender<BlockUpdateEvent>,
    config: IngestorConfig,
) {
    // Phase 1: Backfill
    if config.backfill > 0 {
        info!(backfill = config.backfill, "starting backfill");
        match provider.latest_block_number().await {
            Ok(latest) => {
                let start = latest.saturating_sub(config.backfill - 1);
                for n in start..=latest {
                    if !config.recompute {
                        match storage.get_block_graph(n).await {
                            Ok(Some(_)) => {
                                info!(block = n, "skipping already-stored block");
                                continue;
                            }
                            Err(e) => {
                                warn!(block = n, error = %e, "error checking storage, processing anyway");
                            }
                            _ => {}
                        }
                    }
                    if let Err(e) = process_block(
                        n,
                        &*provider,
                        &*adapter,
                        &*storage,
                        &broadcast_tx,
                    )
                    .await
                    {
                        warn!(block = n, error = %e, "failed to process block during backfill");
                    }
                }
                info!("backfill complete");
            }
            Err(e) => {
                warn!(error = %e, "failed to get latest block number for backfill");
            }
        }
    }

    // Phase 2: Live tracking
    info!("starting live block tracking");
    let mut stream = provider.subscribe_new_blocks();
    while let Some(result) = stream.next().await {
        match result {
            Ok(notification) => {
                let n = notification.number;
                if !config.recompute {
                    match storage.get_block_graph(n).await {
                        Ok(Some(_)) => {
                            continue;
                        }
                        Err(e) => {
                            warn!(block = n, error = %e, "error checking storage");
                        }
                        _ => {}
                    }
                }
                if let Err(e) = process_block(
                    n,
                    &*provider,
                    &*adapter,
                    &*storage,
                    &broadcast_tx,
                )
                .await
                {
                    warn!(block = n, error = %e, "failed to process block");
                }
            }
            Err(e) => {
                warn!(error = %e, "error in block stream");
            }
        }
    }
}

async fn process_block(
    n: u64,
    provider: &dyn ChainProvider,
    adapter: &dyn ChainAdapter,
    storage: &dyn StorageBackend,
    broadcast_tx: &broadcast::Sender<BlockUpdateEvent>,
) -> Result<()> {
    info!(block = n, "processing block");

    // 1. Fetch block envelope
    let block = provider.get_block(n).await?;

    // 2. Fetch traces
    let diff_traces = match provider.trace_block_prestate_diff(n).await {
        Ok(t) => t,
        Err(e) => {
            warn!(block = n, error = %e, "failed to fetch diff traces, skipping trace analysis");
            Vec::new()
        }
    };

    let prestate_traces = match provider.trace_block_prestate(n).await {
        Ok(t) => Some(t),
        Err(e) => {
            warn!(block = n, error = %e, "failed to fetch prestate traces, using conservative mode");
            None
        }
    };

    // 3. Parse tx nodes
    let mut tx_nodes = Vec::with_capacity(block.transactions.len());
    for (i, (tx, receipt)) in block
        .transactions
        .iter()
        .zip(block.receipts.iter())
        .enumerate()
    {
        match adapter.parse_tx_node(tx, receipt, i as u32) {
            Ok(node) => tx_nodes.push(node),
            Err(e) => {
                warn!(block = n, tx_index = i, error = %e, "failed to parse tx node");
            }
        }
    }

    // 4. Extract state access
    let diff_json = serde_json::to_value(&diff_traces)?;
    let prestate_json = prestate_traces
        .as_ref()
        .map(|t| serde_json::to_value(t))
        .transpose()?;
    let state_accesses =
        txviz_core::trace::extract_state_access(&diff_json, prestate_json.as_ref())?;

    // 5. Build dependency edges
    let mut edges = txviz_core::dag::build_dependency_edges(&tx_nodes, &state_accesses);
    edges.extend(adapter.nonce_edges(&tx_nodes));
    edges.extend(adapter.structural_edges(&tx_nodes));
    let edges = txviz_core::dag::deduplicate_edges(edges);

    // 6. Compute schedule + metrics
    let (schedule, crit_info) = txviz_core::schedule::compute(&tx_nodes, &edges)?;
    let metrics = txviz_core::metrics::compute(&tx_nodes, &edges, &schedule, &crit_info);

    // 7. Assemble BlockGraph
    let graph = BlockGraph {
        chain: adapter.chain_kind(),
        block_number: block.number,
        block_hash: block.hash,
        parent_hash: block.parent_hash,
        timestamp: block.timestamp,
        tx_nodes,
        dep_edges: edges,
        schedule,
        metrics,
    };

    // 8. Store
    storage.put_block_graph(&graph).await?;

    // 9. Broadcast SSE
    let event = BlockUpdateEvent {
        block_number: graph.block_number,
        block_hash: graph.block_hash,
        timestamp: graph.timestamp,
        tx_count: graph.metrics.tx_count,
        speedup_upper_bound: graph.metrics.speedup_upper_bound,
        critical_path_weight: graph.metrics.critical_path_weight,
    };
    let _ = broadcast_tx.send(event);

    info!(
        block = n,
        txs = graph.metrics.tx_count,
        edges = graph.metrics.edge_count,
        speedup = format!("{:.2}x", graph.metrics.speedup_upper_bound),
        "block processed"
    );

    Ok(())
}
