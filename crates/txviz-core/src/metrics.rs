use std::collections::{HashSet, VecDeque};

use crate::model::{BlockMetrics, DepEdge, ScheduleItem, TxLane, TxNode};
use crate::schedule::{self, CriticalPathInfo};

/// Compute aggregate metrics for a block.
pub fn compute(
    tx_nodes: &[TxNode],
    dep_edges: &[DepEdge],
    schedule_items: &[ScheduleItem],
    crit_info: &CriticalPathInfo,
) -> BlockMetrics {
    let tx_count = tx_nodes.len() as u32;
    let edge_count = dep_edges.len() as u32;
    let total_weight: u64 = tx_nodes.iter().map(|n| n.gas_used).sum();
    let critical_path_weight = crit_info.weight;

    let speedup_upper_bound = if critical_path_weight == 0 {
        1.0
    } else {
        total_weight as f64 / critical_path_weight as f64
    };

    let max_parallelism = schedule::max_parallelism(schedule_items);

    let makespan = schedule_items
        .iter()
        .map(|s| s.start + s.duration)
        .max()
        .unwrap_or(0);

    let component_count = count_components(tx_count as usize, dep_edges);

    // Tempo-specific metrics
    let has_tempo = tx_nodes.iter().any(|n| n.tempo.is_some());
    let (payment_tx_count, general_tx_count, subblock_count, unique_nonce_keys) = if has_tempo {
        let payment = tx_nodes
            .iter()
            .filter(|n| n.tempo.as_ref().map_or(false, |t| t.lane == TxLane::Payment))
            .count() as u32;
        let general = tx_nodes
            .iter()
            .filter(|n| n.tempo.as_ref().map_or(false, |t| t.lane == TxLane::General))
            .count() as u32;
        let max_subblock = tx_nodes
            .iter()
            .filter_map(|n| n.tempo.as_ref()?.subblock_index)
            .max();
        let subblock = max_subblock.map(|m| m + 1).unwrap_or(0);
        let nonce_keys: HashSet<_> = tx_nodes
            .iter()
            .filter_map(|n| Some(n.tempo.as_ref()?.nonce_key))
            .collect();
        (
            Some(payment),
            Some(general),
            Some(subblock),
            Some(nonce_keys.len() as u32),
        )
    } else {
        (None, None, None, None)
    };

    BlockMetrics {
        tx_count,
        edge_count,
        component_count,
        total_weight,
        critical_path_weight,
        speedup_upper_bound,
        max_parallelism,
        makespan,
        payment_tx_count,
        general_tx_count,
        subblock_count,
        unique_nonce_keys,
    }
}

/// Count weakly connected components using undirected BFS.
/// Isolated nodes (no edges) each count as their own component.
fn count_components(node_count: usize, edges: &[DepEdge]) -> u32 {
    if node_count == 0 {
        return 0;
    }

    let mut adj: Vec<Vec<usize>> = vec![vec![]; node_count];
    for e in edges {
        adj[e.from_tx as usize].push(e.to_tx as usize);
        adj[e.to_tx as usize].push(e.from_tx as usize);
    }

    let mut visited = vec![false; node_count];
    let mut count = 0u32;

    for i in 0..node_count {
        if !visited[i] {
            count += 1;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;
    use alloy_primitives::{Address, B256, U256};

    fn addr(n: u8) -> Address {
        let mut bytes = [0u8; 20];
        bytes[0] = n;
        Address::from(bytes)
    }

    fn hash(n: u8) -> B256 {
        let mut bytes = [0u8; 32];
        bytes[0] = n;
        B256::from(bytes)
    }

    fn make_node(index: u32, gas_used: u64) -> TxNode {
        TxNode {
            tx_hash: hash(index as u8),
            tx_index: index,
            from: addr(index as u8),
            to: None,
            tx_type: 2,
            nonce: 0,
            gas_used,
            tempo: None,
        }
    }

    #[test]
    fn test_component_count() {
        // 3 nodes, edge 0→2: 2 components ({0,2}, {1})
        let edges = vec![DepEdge {
            from_tx: 0,
            to_tx: 2,
            kind: DepEdgeKind::Raw,
            reasons: vec![],
        }];
        assert_eq!(count_components(3, &edges), 2);

        // 5 nodes, no edges: 5 components
        assert_eq!(count_components(5, &[]), 5);

        // 3 nodes, chain 0→1→2: 1 component
        let chain_edges = vec![
            DepEdge {
                from_tx: 0,
                to_tx: 1,
                kind: DepEdgeKind::Raw,
                reasons: vec![],
            },
            DepEdge {
                from_tx: 1,
                to_tx: 2,
                kind: DepEdgeKind::Raw,
                reasons: vec![],
            },
        ];
        assert_eq!(count_components(3, &chain_edges), 1);

        // 0 nodes
        assert_eq!(count_components(0, &[]), 0);
    }

    #[test]
    fn test_metrics_empty() {
        let crit = CriticalPathInfo {
            weight: 0,
            est: vec![],
            lst: vec![],
        };
        let m = compute(&[], &[], &[], &crit);
        assert_eq!(m.tx_count, 0);
        assert_eq!(m.edge_count, 0);
        assert_eq!(m.component_count, 0);
        assert_eq!(m.total_weight, 0);
        assert_eq!(m.critical_path_weight, 0);
        assert_eq!(m.speedup_upper_bound, 1.0);
        assert_eq!(m.max_parallelism, 0);
        assert_eq!(m.makespan, 0);
        assert!(m.payment_tx_count.is_none());
    }

    #[test]
    fn test_metrics_speedup() {
        // 3 txs: 50000, 125000, 21000. tx0→tx2 edge.
        // total=196000, critical path=71000 (50000+21000)
        let nodes = vec![
            make_node(0, 50000),
            make_node(1, 125000),
            make_node(2, 21000),
        ];
        let edges = vec![DepEdge {
            from_tx: 0,
            to_tx: 2,
            kind: DepEdgeKind::Raw,
            reasons: vec![],
        }];

        let (schedule_items, crit) = schedule::compute(&nodes, &edges).unwrap();
        let m = compute(&nodes, &edges, &schedule_items, &crit);

        assert_eq!(m.tx_count, 3);
        assert_eq!(m.edge_count, 1);
        assert_eq!(m.component_count, 2);
        assert_eq!(m.total_weight, 196000);
        assert_eq!(m.critical_path_weight, 125000); // max(50000+21000, 125000) = 125000
        // Wait, let me reconsider. tx0→tx2 chain = 50000+21000=71000.
        // tx1 is independent with weight 125000.
        // critical path = max(71000, 125000) = 125000
        // Hmm, but the fixture says critical_path = 71000...
        // Actually no: critical path = longest path. 125000 (single node) > 71000.
        // The fixture note says "critical path = 50000+21000=71000" but that's wrong
        // if tx1 has weight 125000, because 125000 > 71000.
        // Let's check what the algorithm actually produces.
    }

    #[test]
    fn test_metrics_fixture_3tx() {
        // From fixture 5.1: tx0=50000, tx1=125000, tx2=21000
        // tx0→tx2 edge (RAW + nonce1d deduplicated)
        // tx1 independent
        // critical path = max(50000+21000, 125000) = 125000
        // But the fixture says "critical path = 50000+21000=71000" 
        // That only considers the chain, not the independent tx with higher weight.
        // Actually the critical path IS 125000 because it's the longest weighted path.
        let nodes = vec![
            make_node(0, 50000),
            make_node(1, 125000),
            make_node(2, 21000),
        ];
        let edges = vec![DepEdge {
            from_tx: 0,
            to_tx: 2,
            kind: DepEdgeKind::Raw,
            reasons: vec![],
        }];

        let (schedule_items, crit) = schedule::compute(&nodes, &edges).unwrap();
        let m = compute(&nodes, &edges, &schedule_items, &crit);

        assert_eq!(m.tx_count, 3);
        assert_eq!(m.edge_count, 1);
        assert_eq!(m.component_count, 2);
        assert_eq!(m.total_weight, 196000);
        // Critical path = max path weight = 125000 (tx1 alone > tx0+tx2 chain)
        assert_eq!(m.critical_path_weight, 125000);
        // speedup = 196000 / 125000 = 1.568
        assert!((m.speedup_upper_bound - 1.568).abs() < 0.001);
        assert_eq!(m.max_parallelism, 2); // tx1 and tx0 can run in parallel
    }

    #[test]
    fn test_metrics_zero_weight() {
        let crit = CriticalPathInfo {
            weight: 0,
            est: vec![0],
            lst: vec![0],
        };
        let nodes = vec![make_node(0, 0)];
        let schedule_items = vec![ScheduleItem {
            tx_index: 0,
            start: 0,
            duration: 1,
            lane: 0,
            is_critical: true,
        }];
        let m = compute(&nodes, &[], &schedule_items, &crit);
        assert_eq!(m.speedup_upper_bound, 1.0); // 0/0 → 1.0
    }

    #[test]
    fn test_metrics_tempo_fields() {
        let nodes = vec![
            TxNode {
                tx_hash: hash(0),
                tx_index: 0,
                from: addr(1),
                to: None,
                tx_type: 0x76,
                nonce: 0,
                gas_used: 21000,
                tempo: Some(TempoTxMeta {
                    nonce_key: U256::from(0),
                    lane: TxLane::Payment,
                    section: BlockSection::SubBlocks,
                    subblock_index: Some(0),
                    fee_payer: None,
                    call_count: 1,
                    fee_token: None,
                }),
            },
            TxNode {
                tx_hash: hash(1),
                tx_index: 1,
                from: addr(2),
                to: None,
                tx_type: 0x76,
                nonce: 0,
                gas_used: 21000,
                tempo: Some(TempoTxMeta {
                    nonce_key: U256::from(1),
                    lane: TxLane::General,
                    section: BlockSection::SubBlocks,
                    subblock_index: Some(1),
                    fee_payer: None,
                    call_count: 2,
                    fee_token: None,
                }),
            },
        ];

        let (schedule_items, crit) = schedule::compute(&nodes, &[]).unwrap();
        let m = compute(&nodes, &[], &schedule_items, &crit);

        assert_eq!(m.payment_tx_count, Some(1));
        assert_eq!(m.general_tx_count, Some(1));
        assert_eq!(m.subblock_count, Some(2)); // max(0,1)+1 = 2
        assert_eq!(m.unique_nonce_keys, Some(2)); // keys 0 and 1
    }
}
