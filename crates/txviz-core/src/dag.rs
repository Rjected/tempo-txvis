use std::collections::HashMap;

use alloy_primitives::U256;

use crate::model::{DepEdge, DepEdgeKind, DepReason, StateKey, TxNode, TxStateAccess};

/// Build state-based dependency edges from read/write sets.
pub fn build_dependency_edges(
    _tx_nodes: &[TxNode],
    state_accesses: &[TxStateAccess],
) -> Vec<DepEdge> {
    let mut last_writer: HashMap<&StateKey, u32> = HashMap::new();
    let mut edges: HashMap<(u32, u32), DepEdge> = HashMap::new();

    for acc in state_accesses {
        let i = acc.tx_index;

        // WAW: for each write, check if there's a previous writer
        for key in &acc.writes {
            if let Some(&prev) = last_writer.get(key) {
                if prev < i {
                    add_or_merge_edge(&mut edges, prev, i, DepEdgeKind::Waw, reason_from(key));
                }
            }
        }

        // RAW: for each read, check if there's a previous writer
        for key in &acc.reads {
            if let Some(&prev) = last_writer.get(key) {
                if prev < i {
                    add_or_merge_edge(&mut edges, prev, i, DepEdgeKind::Raw, reason_from(key));
                }
            }
        }

        // Update last_writer for all writes
        for key in &acc.writes {
            last_writer.insert(key, i);
        }
    }

    let mut result: Vec<DepEdge> = edges.into_values().collect();
    result.sort_by_key(|e| (e.from_tx, e.to_tx));
    result
}

/// Generate nonce sequencing edges for Ethereum (1D nonce).
/// Group txs by sender, sort by nonce, add edges between consecutive pairs.
pub fn nonce_edges_1d(nodes: &[TxNode]) -> Vec<DepEdge> {
    let mut by_sender: HashMap<_, Vec<&TxNode>> = HashMap::new();
    for node in nodes {
        by_sender.entry(node.from).or_default().push(node);
    }

    let mut edges = Vec::new();
    for (_, group) in &mut by_sender {
        if group.len() < 2 {
            continue;
        }
        group.sort_by_key(|n| n.nonce);
        for pair in group.windows(2) {
            let from_node = pair[0];
            let to_node = pair[1];
            edges.push(DepEdge {
                from_tx: from_node.tx_index,
                to_tx: to_node.tx_index,
                kind: DepEdgeKind::Nonce1d,
                reasons: vec![DepReason::NonceSequence {
                    address: from_node.from,
                    nonce_key: None,
                }],
            });
        }
    }

    edges.sort_by_key(|e| (e.from_tx, e.to_tx));
    edges
}

/// Generate nonce sequencing edges for Tempo (2D nonce).
/// Group txs by (sender, nonce_key), sort by nonce, add edges between consecutive pairs.
/// Expiring nonce (nonce_key = U256::MAX): no nonce sequencing edge.
pub fn nonce_edges_2d(nodes: &[TxNode]) -> Vec<DepEdge> {
    let mut by_key: HashMap<(_, U256), Vec<&TxNode>> = HashMap::new();
    for node in nodes {
        if let Some(ref tempo) = node.tempo {
            // Skip expiring nonce (U256::MAX)
            if tempo.nonce_key == U256::MAX {
                continue;
            }
            by_key
                .entry((node.from, tempo.nonce_key))
                .or_default()
                .push(node);
        }
    }

    let mut edges = Vec::new();
    for ((sender, nonce_key), group) in &mut by_key {
        if group.len() < 2 {
            continue;
        }
        group.sort_by_key(|n| n.nonce);
        for pair in group.windows(2) {
            let from_node = pair[0];
            let to_node = pair[1];
            edges.push(DepEdge {
                from_tx: from_node.tx_index,
                to_tx: to_node.tx_index,
                kind: DepEdgeKind::Nonce2d,
                reasons: vec![DepReason::NonceSequence {
                    address: *sender,
                    nonce_key: Some(*nonce_key),
                }],
            });
        }
    }

    edges.sort_by_key(|e| (e.from_tx, e.to_tx));
    edges
}

/// Generate fee sponsorship edges (Tempo only).
/// Track last tx by fee payer. If a tx has fee_payer != sender, link to previous.
pub fn fee_sponsorship_edges(nodes: &[TxNode]) -> Vec<DepEdge> {
    let mut last_by_payer: HashMap<_, u32> = HashMap::new();
    let mut edges = Vec::new();

    for node in nodes {
        if let Some(ref tempo) = node.tempo {
            if let Some(payer) = tempo.fee_payer {
                if payer != node.from {
                    if let Some(prev) = last_by_payer.get(&payer) {
                        edges.push(DepEdge {
                            from_tx: *prev,
                            to_tx: node.tx_index,
                            kind: DepEdgeKind::FeeSponsorship,
                            reasons: vec![DepReason::FeePayer { payer }],
                        });
                    }
                    last_by_payer.insert(payer, node.tx_index);
                }
            }
        }
    }

    edges.sort_by_key(|e| (e.from_tx, e.to_tx));
    edges
}

/// Deduplicate edges: merge multiple edges between same (from, to) pair.
/// Uses strongest kind (Raw > Waw > Nonce1d > Nonce2d > FeeSponsorship) and combines reasons.
pub fn deduplicate_edges(edges: Vec<DepEdge>) -> Vec<DepEdge> {
    let mut merged: HashMap<(u32, u32), DepEdge> = HashMap::new();

    for edge in edges {
        let key = (edge.from_tx, edge.to_tx);
        if let Some(existing) = merged.get_mut(&key) {
            // Use strongest kind
            if edge.kind > existing.kind {
                existing.kind = edge.kind;
            }
            // Add new unique reasons
            for reason in edge.reasons {
                if !existing.reasons.contains(&reason) {
                    existing.reasons.push(reason);
                }
            }
        } else {
            merged.insert(key, edge);
        }
    }

    let mut result: Vec<DepEdge> = merged.into_values().collect();
    result.sort_by_key(|e| (e.from_tx, e.to_tx));
    result
}

fn add_or_merge_edge(
    edges: &mut HashMap<(u32, u32), DepEdge>,
    from: u32,
    to: u32,
    kind: DepEdgeKind,
    reason: DepReason,
) {
    let key = (from, to);
    if let Some(existing) = edges.get_mut(&key) {
        if kind > existing.kind {
            existing.kind = kind;
        }
        if !existing.reasons.contains(&reason) {
            existing.reasons.push(reason);
        }
    } else {
        edges.insert(
            key,
            DepEdge {
                from_tx: from,
                to_tx: to,
                kind,
                reasons: vec![reason],
            },
        );
    }
}

fn reason_from(key: &StateKey) -> DepReason {
    match key {
        StateKey::Storage(addr, slot) => DepReason::Storage {
            address: *addr,
            slot: *slot,
        },
        StateKey::Balance(addr) => DepReason::Balance { address: *addr },
        StateKey::Nonce(addr) => DepReason::Nonce { address: *addr },
        StateKey::Code(addr) => DepReason::Code { address: *addr },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;
    use alloy_primitives::{Address, B256};
    use std::collections::HashSet;

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

    fn make_node(index: u32, from: Address, nonce: u64, gas_used: u64) -> TxNode {
        TxNode {
            tx_hash: hash(index as u8),
            tx_index: index,
            from,
            to: None,
            tx_type: 2,
            nonce,
            gas_used,
            tempo: None,
        }
    }

    fn make_tempo_node(
        index: u32,
        from: Address,
        nonce: u64,
        nonce_key: U256,
        fee_payer: Option<Address>,
    ) -> TxNode {
        TxNode {
            tx_hash: hash(index as u8),
            tx_index: index,
            from,
            to: None,
            tx_type: 0x76,
            nonce,
            gas_used: 21000,
            tempo: Some(TempoTxMeta {
                nonce_key,
                lane: TxLane::General,
                section: BlockSection::NonShared,
                subblock_index: None,
                fee_payer,
                call_count: 1,
                fee_token: None,
            }),
        }
    }

    #[test]
    fn test_build_edges_simple_3tx() {
        // tx0 and tx2 share writes on Balance(sender), Nonce(sender)
        // tx1 writes to different addresses
        let sender = addr(0x11);
        let contract = addr(0xCC);
        let sender2 = addr(0x22);
        let created = addr(0x44);
        let recipient = addr(0x33);

        let slot_1: B256 = "0x0000000000000000000000000000000000000000000000000000000000000001"
            .parse()
            .unwrap();

        let nodes = vec![
            make_node(0, sender, 10, 50000),
            make_node(1, sender2, 0, 125000),
            make_node(2, sender, 11, 21000),
        ];

        let accesses = vec![
            TxStateAccess {
                tx_index: 0,
                reads: HashSet::new(),
                writes: [
                    StateKey::Balance(sender),
                    StateKey::Nonce(sender),
                    StateKey::Storage(contract, slot_1),
                ]
                .into(),
            },
            TxStateAccess {
                tx_index: 1,
                reads: HashSet::new(),
                writes: [
                    StateKey::Balance(sender2),
                    StateKey::Nonce(sender2),
                    StateKey::Balance(created),
                    StateKey::Nonce(created),
                    StateKey::Code(created),
                ]
                .into(),
            },
            TxStateAccess {
                tx_index: 2,
                reads: HashSet::new(),
                writes: [
                    StateKey::Balance(sender),
                    StateKey::Nonce(sender),
                    StateKey::Balance(recipient),
                ]
                .into(),
            },
        ];

        let edges = build_dependency_edges(&nodes, &accesses);

        // tx0→tx2: WAW on Balance(sender) and Nonce(sender)
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].from_tx, 0);
        assert_eq!(edges[0].to_tx, 2);
        assert_eq!(edges[0].kind, DepEdgeKind::Waw);
        assert!(edges[0]
            .reasons
            .contains(&DepReason::Balance { address: sender }));
        assert!(edges[0]
            .reasons
            .contains(&DepReason::Nonce { address: sender }));
    }

    #[test]
    fn test_build_edges_independent() {
        let accesses = vec![
            TxStateAccess {
                tx_index: 0,
                reads: HashSet::new(),
                writes: [StateKey::Balance(addr(1))].into(),
            },
            TxStateAccess {
                tx_index: 1,
                reads: HashSet::new(),
                writes: [StateKey::Balance(addr(2))].into(),
            },
            TxStateAccess {
                tx_index: 2,
                reads: HashSet::new(),
                writes: [StateKey::Balance(addr(3))].into(),
            },
        ];
        let edges = build_dependency_edges(&[], &accesses);
        assert!(edges.is_empty());
    }

    #[test]
    fn test_build_edges_sequential() {
        let a = addr(1);
        let slot = B256::ZERO;
        let accesses = vec![
            TxStateAccess {
                tx_index: 0,
                reads: HashSet::new(),
                writes: [StateKey::Storage(a, slot)].into(),
            },
            TxStateAccess {
                tx_index: 1,
                reads: HashSet::new(),
                writes: [StateKey::Storage(a, slot)].into(),
            },
            TxStateAccess {
                tx_index: 2,
                reads: HashSet::new(),
                writes: [StateKey::Storage(a, slot)].into(),
            },
        ];
        let edges = build_dependency_edges(&[], &accesses);
        // Chain: 0→1, 1→2
        assert_eq!(edges.len(), 2);
        assert_eq!(edges[0].from_tx, 0);
        assert_eq!(edges[0].to_tx, 1);
        assert_eq!(edges[1].from_tx, 1);
        assert_eq!(edges[1].to_tx, 2);
    }

    #[test]
    fn test_edge_deduplication() {
        let a = addr(1);
        let edges = vec![
            DepEdge {
                from_tx: 0,
                to_tx: 1,
                kind: DepEdgeKind::Waw,
                reasons: vec![DepReason::Balance { address: a }],
            },
            DepEdge {
                from_tx: 0,
                to_tx: 1,
                kind: DepEdgeKind::Raw,
                reasons: vec![DepReason::Nonce { address: a }],
            },
            DepEdge {
                from_tx: 0,
                to_tx: 1,
                kind: DepEdgeKind::Nonce1d,
                reasons: vec![DepReason::NonceSequence {
                    address: a,
                    nonce_key: None,
                }],
            },
        ];
        let deduped = deduplicate_edges(edges);
        assert_eq!(deduped.len(), 1);
        assert_eq!(deduped[0].kind, DepEdgeKind::Raw); // strongest
        assert_eq!(deduped[0].reasons.len(), 3);
    }

    #[test]
    fn test_nonce_edges_1d() {
        let sender = addr(0x11);
        let nodes = vec![
            make_node(0, sender, 10, 50000),
            make_node(1, addr(0x22), 0, 125000),
            make_node(2, sender, 11, 21000),
        ];
        let edges = nonce_edges_1d(&nodes);
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].from_tx, 0);
        assert_eq!(edges[0].to_tx, 2);
        assert_eq!(edges[0].kind, DepEdgeKind::Nonce1d);
    }

    #[test]
    fn test_nonce_edges_2d_independent() {
        let sender = addr(0xAA);
        let nodes = vec![
            make_tempo_node(0, sender, 5, U256::from(0), None),
            make_tempo_node(1, sender, 0, U256::from(1), None),
            make_tempo_node(2, sender, 0, U256::from(2), None),
        ];
        let edges = nonce_edges_2d(&nodes);
        // Different nonce_keys → no edges
        assert!(edges.is_empty());
    }

    #[test]
    fn test_nonce_edges_2d_linked() {
        let sender = addr(0xAA);
        let nodes = vec![
            make_tempo_node(0, sender, 5, U256::from(0), None),
            make_tempo_node(1, sender, 0, U256::from(1), None),
            make_tempo_node(2, sender, 0, U256::from(2), None),
            make_tempo_node(3, sender, 6, U256::from(0), None),
        ];
        let edges = nonce_edges_2d(&nodes);
        // Only nonce_key=0: tx0 (nonce=5) → tx3 (nonce=6)
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].from_tx, 0);
        assert_eq!(edges[0].to_tx, 3);
        assert_eq!(edges[0].kind, DepEdgeKind::Nonce2d);
    }

    #[test]
    fn test_fee_sponsorship_edges() {
        let user1 = addr(0x01);
        let user2 = addr(0x02);
        let user3 = addr(0x03);
        let sponsor = addr(0xF0);

        let nodes = vec![
            make_tempo_node(0, user1, 0, U256::from(0), Some(sponsor)),
            make_tempo_node(1, user2, 0, U256::from(0), Some(sponsor)),
            make_tempo_node(2, user3, 0, U256::from(0), None), // self-paying
        ];

        let edges = fee_sponsorship_edges(&nodes);
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].from_tx, 0);
        assert_eq!(edges[0].to_tx, 1);
        assert_eq!(edges[0].kind, DepEdgeKind::FeeSponsorship);
    }

    #[test]
    fn test_build_edges_empty() {
        let edges = build_dependency_edges(&[], &[]);
        assert!(edges.is_empty());
    }

    #[test]
    fn test_build_edges_single_tx() {
        let accesses = vec![TxStateAccess {
            tx_index: 0,
            reads: HashSet::new(),
            writes: [StateKey::Balance(addr(1))].into(),
        }];
        let edges = build_dependency_edges(&[], &accesses);
        assert!(edges.is_empty());
    }
}
