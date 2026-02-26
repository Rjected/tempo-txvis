use std::collections::HashMap;

use alloy_primitives::{Address, U256};
use anyhow::Result;
use txviz_core::model::{
    BlockSection, ChainKind, DepEdge, DepEdgeKind, DepReason, TempoTxMeta, TxLane, TxNode,
};

use crate::adapter::ChainAdapter;
use crate::types::{RpcReceipt, RpcTransaction};

pub struct TempoAdapter;

/// Parse a hex string (with or without 0x prefix) to U256.
fn parse_hex_u256(s: &str) -> Result<U256> {
    let s = s.strip_prefix("0x").unwrap_or(s);
    U256::from_str_radix(s, 16).map_err(|e| anyhow::anyhow!("invalid hex U256: {e}"))
}

/// Parse a hex string (e.g. "0x02") to u8.
fn parse_hex_u8(s: &str) -> Result<u8> {
    let s = s.strip_prefix("0x").unwrap_or(s);
    u8::from_str_radix(s, 16).map_err(|e| anyhow::anyhow!("invalid hex u8: {e}"))
}

/// Parse a hex string or JSON number to u64.
fn parse_nonce_value(v: &serde_json::Value) -> Result<u64> {
    match v {
        serde_json::Value::String(s) => {
            let s = s.strip_prefix("0x").unwrap_or(s);
            u64::from_str_radix(s, 16).map_err(|e| anyhow::anyhow!("invalid hex nonce: {e}"))
        }
        serde_json::Value::Number(n) => n
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("invalid nonce number")),
        _ => anyhow::bail!("nonce must be string or number"),
    }
}

/// Parse a hex string (e.g. "0x5208") to u64.
fn parse_hex_u64(s: &str) -> Result<u64> {
    let s = s.strip_prefix("0x").unwrap_or(s);
    u64::from_str_radix(s, 16).map_err(|e| anyhow::anyhow!("invalid hex u64: {e}"))
}

/// Detect lane from call targets: if any target starts with 0x20C0 prefix → Payment.
fn detect_lane(tx: &RpcTransaction) -> TxLane {
    // Check direct `to` address
    if let Some(to) = &tx.to {
        let hex = format!("{to:?}");
        if hex.to_lowercase().starts_with("0x20c0") {
            return TxLane::Payment;
        }
    }

    // Check calls array entries
    if let Some(calls) = &tx.calls {
        for call in calls {
            if let Some(to_str) = call.get("to").and_then(|v| v.as_str()) {
                if to_str.to_lowercase().starts_with("0x20c0") {
                    return TxLane::Payment;
                }
            }
        }
    }

    TxLane::General
}

/// Detect section from tx fields.
fn detect_section(tx: &RpcTransaction, nonce_key: &U256) -> BlockSection {
    // System tx: to address(0)
    if let Some(to) = &tx.to {
        if to.is_zero() {
            return BlockSection::System;
        }
    }

    // SubBlocks: nonce_key high byte is 0x5b
    let bytes = nonce_key.to_be_bytes::<32>();
    if bytes[0] == 0x5b {
        return BlockSection::SubBlocks;
    }

    BlockSection::NonShared
}

/// Parse hex address string to Address.
fn parse_address(s: &str) -> Result<Address> {
    s.parse::<Address>()
        .map_err(|e| anyhow::anyhow!("invalid address: {e}"))
}

impl ChainAdapter for TempoAdapter {
    fn chain_kind(&self) -> ChainKind {
        ChainKind::Tempo
    }

    fn parse_tx_node(
        &self,
        tx: &RpcTransaction,
        receipt: &RpcReceipt,
        tx_index: u32,
    ) -> Result<TxNode> {
        let tx_type = tx
            .tx_type
            .as_deref()
            .map(parse_hex_u8)
            .transpose()?
            .unwrap_or(0);

        let nonce = tx
            .nonce
            .as_ref()
            .map(parse_nonce_value)
            .transpose()?
            .unwrap_or(0);

        let gas_used = receipt
            .gas_used
            .as_deref()
            .map(parse_hex_u64)
            .transpose()?
            .unwrap_or(0);

        // Tempo-specific fields
        let nonce_key = tx
            .nonce_key
            .as_deref()
            .map(parse_hex_u256)
            .transpose()?
            .unwrap_or(U256::ZERO);

        let lane = detect_lane(tx);
        let section = detect_section(tx, &nonce_key);

        let subblock_index = if section == BlockSection::SubBlocks {
            // Derive from nonce_key lower bytes (simplified: use lower u32)
            let bytes = nonce_key.to_be_bytes::<32>();
            let idx = u32::from_be_bytes([bytes[28], bytes[29], bytes[30], bytes[31]]);
            Some(idx)
        } else {
            None
        };

        let call_count = tx.calls.as_ref().map(|c| c.len() as u32).unwrap_or(1);

        let fee_token = tx
            .fee_token
            .as_deref()
            .map(parse_address)
            .transpose()?;

        let tempo = TempoTxMeta {
            nonce_key,
            lane,
            section,
            subblock_index,
            fee_payer: None, // fee_payer is derived externally if needed
            call_count,
            fee_token,
        };

        Ok(TxNode {
            tx_hash: tx.hash,
            tx_index,
            from: tx.from,
            to: tx.to,
            tx_type,
            nonce,
            gas_used,
            tempo: Some(tempo),
        })
    }

    fn nonce_edges(&self, nodes: &[TxNode]) -> Vec<DepEdge> {
        // Group by (sender, nonce_key)
        let mut by_key: HashMap<(Address, U256), Vec<(u32, u64)>> = HashMap::new();
        for node in nodes {
            let nonce_key = node
                .tempo
                .as_ref()
                .map(|t| t.nonce_key)
                .unwrap_or(U256::ZERO);

            // Expiring nonce (U256::MAX) → no nonce edges
            if nonce_key == U256::MAX {
                continue;
            }

            by_key
                .entry((node.from, nonce_key))
                .or_default()
                .push((node.tx_index, node.nonce));
        }

        let mut edges = Vec::new();
        for ((sender, nonce_key), mut txs) in by_key {
            txs.sort_by_key(|&(_, nonce)| nonce);
            for pair in txs.windows(2) {
                let (from_idx, _) = pair[0];
                let (to_idx, _) = pair[1];
                edges.push(DepEdge {
                    from_tx: from_idx,
                    to_tx: to_idx,
                    kind: DepEdgeKind::Nonce2d,
                    reasons: vec![DepReason::NonceSequence {
                        address: sender,
                        nonce_key: Some(nonce_key),
                    }],
                });
            }
        }
        edges.sort_by_key(|e| (e.from_tx, e.to_tx));
        edges
    }

    fn structural_edges(&self, nodes: &[TxNode]) -> Vec<DepEdge> {
        // Fee sponsorship: track last tx per fee_payer, create FeeSponsorship edges
        let mut last_by_payer: HashMap<Address, u32> = HashMap::new();
        let mut edges = Vec::new();

        for node in nodes {
            if let Some(tempo) = &node.tempo {
                if let Some(fee_payer) = &tempo.fee_payer {
                    if fee_payer != &node.from {
                        if let Some(prev_idx) = last_by_payer.get(fee_payer) {
                            edges.push(DepEdge {
                                from_tx: *prev_idx,
                                to_tx: node.tx_index,
                                kind: DepEdgeKind::FeeSponsorship,
                                reasons: vec![DepReason::FeePayer {
                                    payer: *fee_payer,
                                }],
                            });
                        }
                        last_by_payer.insert(*fee_payer, node.tx_index);
                    }
                }
            }
        }
        edges
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::B256;

    fn make_tempo_tx(
        hash: B256,
        from: Address,
        to: Option<Address>,
        nonce: serde_json::Value,
        nonce_key: Option<&str>,
        calls: Option<Vec<serde_json::Value>>,
        fee_token: Option<&str>,
    ) -> RpcTransaction {
        RpcTransaction {
            hash,
            from,
            to,
            tx_type: Some("0x76".to_string()),
            nonce: Some(nonce),
            gas: None,
            value: None,
            input: None,
            nonce_key: nonce_key.map(|s| s.to_string()),
            calls,
            fee_token: fee_token.map(|s| s.to_string()),
        }
    }

    fn make_receipt(hash: B256, gas_used: &str) -> RpcReceipt {
        RpcReceipt {
            transaction_hash: hash,
            gas_used: Some(gas_used.to_string()),
            status: Some("0x1".to_string()),
        }
    }

    #[test]
    fn test_parse_tempo_tx_full() {
        let adapter = TempoAdapter;
        let hash = B256::with_last_byte(1);
        let from = Address::with_last_byte(0xaa);
        let to = Address::with_last_byte(0xbb);
        let fee_token_addr = "0x20c0000000000000000000000000000000000000";

        let calls = vec![
            serde_json::json!({"to": "0x20c0000000000000000000000000000000000000", "value": "0x0", "input": "0x"}),
            serde_json::json!({"to": "0x1234567890123456789012345678901234567890", "value": "0x100", "input": "0x"}),
        ];

        let tx = make_tempo_tx(
            hash,
            from,
            Some(to),
            serde_json::json!("0x05"),
            Some("0x01"),
            Some(calls),
            Some(fee_token_addr),
        );
        let receipt = make_receipt(hash, "0xc350");

        let node = adapter.parse_tx_node(&tx, &receipt, 0).unwrap();
        assert_eq!(node.tx_type, 0x76);
        assert_eq!(node.nonce, 5);
        assert_eq!(node.gas_used, 0xc350);

        let tempo = node.tempo.unwrap();
        assert_eq!(tempo.nonce_key, U256::from(1));
        assert_eq!(tempo.call_count, 2);
        assert_eq!(tempo.section, BlockSection::NonShared);
        assert!(tempo.fee_token.is_some());
    }

    #[test]
    fn test_parse_tempo_tx_partial() {
        let adapter = TempoAdapter;
        let hash = B256::with_last_byte(2);
        let from = Address::with_last_byte(0xcc);

        let tx = make_tempo_tx(hash, from, None, serde_json::json!(0), None, None, None);
        let receipt = make_receipt(hash, "0x5208");

        let node = adapter.parse_tx_node(&tx, &receipt, 1).unwrap();
        let tempo = node.tempo.unwrap();
        assert_eq!(tempo.nonce_key, U256::ZERO);
        assert_eq!(tempo.lane, TxLane::General);
        assert_eq!(tempo.call_count, 1);
        assert!(tempo.fee_token.is_none());
    }

    #[test]
    fn test_tempo_lane_detection_payment() {
        let adapter = TempoAdapter;
        let hash = B256::with_last_byte(3);
        let from = Address::with_last_byte(0xdd);

        let calls = vec![
            serde_json::json!({"to": "0x20c0000000000000000000000000000000000001", "value": "0x0", "input": "0x"}),
        ];
        let tx = make_tempo_tx(hash, from, None, serde_json::json!("0x0"), Some("0x0"), Some(calls), None);
        let receipt = make_receipt(hash, "0x5208");

        let node = adapter.parse_tx_node(&tx, &receipt, 0).unwrap();
        assert_eq!(node.tempo.unwrap().lane, TxLane::Payment);
    }

    #[test]
    fn test_tempo_lane_detection_general() {
        let adapter = TempoAdapter;
        let hash = B256::with_last_byte(4);
        let from = Address::with_last_byte(0xee);

        let calls = vec![
            serde_json::json!({"to": "0x1234567890123456789012345678901234567890", "value": "0x0", "input": "0x"}),
        ];
        let tx = make_tempo_tx(hash, from, None, serde_json::json!("0x0"), Some("0x0"), Some(calls), None);
        let receipt = make_receipt(hash, "0x5208");

        let node = adapter.parse_tx_node(&tx, &receipt, 0).unwrap();
        assert_eq!(node.tempo.unwrap().lane, TxLane::General);
    }

    #[test]
    fn test_tempo_section_subblock() {
        let adapter = TempoAdapter;
        let hash = B256::with_last_byte(5);
        let from = Address::with_last_byte(0x11);

        // nonce_key with high byte 0x5b → SubBlocks
        let tx = make_tempo_tx(
            hash,
            from,
            None,
            serde_json::json!("0x0"),
            Some("0x5b00000000000000000000000000000000000000000000000000000000000003"),
            None,
            None,
        );
        let receipt = make_receipt(hash, "0x5208");

        let node = adapter.parse_tx_node(&tx, &receipt, 0).unwrap();
        let tempo = node.tempo.unwrap();
        assert_eq!(tempo.section, BlockSection::SubBlocks);
        assert_eq!(tempo.subblock_index, Some(3));
    }

    #[test]
    fn test_tempo_section_system() {
        let adapter = TempoAdapter;
        let hash = B256::with_last_byte(6);
        let from = Address::with_last_byte(0x22);

        // System tx: to = address(0)
        let tx = make_tempo_tx(
            hash,
            from,
            Some(Address::ZERO),
            serde_json::json!("0x0"),
            Some("0x0"),
            None,
            None,
        );
        let receipt = make_receipt(hash, "0x0");

        let node = adapter.parse_tx_node(&tx, &receipt, 0).unwrap();
        assert_eq!(node.tempo.unwrap().section, BlockSection::System);
    }

    #[test]
    fn test_tempo_nonce_edges_parallel() {
        // Same sender, different nonce_keys → independent (no edges)
        let adapter = TempoAdapter;
        let sender = Address::with_last_byte(0xaa);
        let nodes = vec![
            TxNode {
                tx_hash: B256::with_last_byte(1),
                tx_index: 0,
                from: sender,
                to: None,
                tx_type: 0x76,
                nonce: 0,
                gas_used: 21000,
                tempo: Some(TempoTxMeta {
                    nonce_key: U256::from(1),
                    lane: TxLane::General,
                    section: BlockSection::NonShared,
                    subblock_index: None,
                    fee_payer: None,
                    call_count: 1,
                    fee_token: None,
                }),
            },
            TxNode {
                tx_hash: B256::with_last_byte(2),
                tx_index: 1,
                from: sender,
                to: None,
                tx_type: 0x76,
                nonce: 0,
                gas_used: 21000,
                tempo: Some(TempoTxMeta {
                    nonce_key: U256::from(2),
                    lane: TxLane::General,
                    section: BlockSection::NonShared,
                    subblock_index: None,
                    fee_payer: None,
                    call_count: 1,
                    fee_token: None,
                }),
            },
        ];

        let edges = adapter.nonce_edges(&nodes);
        assert!(edges.is_empty());
    }

    #[test]
    fn test_tempo_nonce_edges_sequential() {
        // Same sender, same nonce_key → chained
        let adapter = TempoAdapter;
        let sender = Address::with_last_byte(0xaa);
        let nk = U256::from(0);
        let nodes = vec![
            TxNode {
                tx_hash: B256::with_last_byte(1),
                tx_index: 0,
                from: sender,
                to: None,
                tx_type: 0x76,
                nonce: 5,
                gas_used: 21000,
                tempo: Some(TempoTxMeta {
                    nonce_key: nk,
                    lane: TxLane::General,
                    section: BlockSection::NonShared,
                    subblock_index: None,
                    fee_payer: None,
                    call_count: 1,
                    fee_token: None,
                }),
            },
            TxNode {
                tx_hash: B256::with_last_byte(2),
                tx_index: 3,
                from: sender,
                to: None,
                tx_type: 0x76,
                nonce: 6,
                gas_used: 21000,
                tempo: Some(TempoTxMeta {
                    nonce_key: nk,
                    lane: TxLane::General,
                    section: BlockSection::NonShared,
                    subblock_index: None,
                    fee_payer: None,
                    call_count: 1,
                    fee_token: None,
                }),
            },
        ];

        let edges = adapter.nonce_edges(&nodes);
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].from_tx, 0);
        assert_eq!(edges[0].to_tx, 3);
        assert_eq!(edges[0].kind, DepEdgeKind::Nonce2d);
    }

    #[test]
    fn test_tempo_expiring_nonce_no_edge() {
        // nonce_key = U256::MAX → no nonce edge
        let adapter = TempoAdapter;
        let sender = Address::with_last_byte(0xaa);
        let nodes = vec![
            TxNode {
                tx_hash: B256::with_last_byte(1),
                tx_index: 0,
                from: sender,
                to: None,
                tx_type: 0x76,
                nonce: 5,
                gas_used: 21000,
                tempo: Some(TempoTxMeta {
                    nonce_key: U256::MAX,
                    lane: TxLane::General,
                    section: BlockSection::NonShared,
                    subblock_index: None,
                    fee_payer: None,
                    call_count: 1,
                    fee_token: None,
                }),
            },
            TxNode {
                tx_hash: B256::with_last_byte(2),
                tx_index: 1,
                from: sender,
                to: None,
                tx_type: 0x76,
                nonce: 6,
                gas_used: 21000,
                tempo: Some(TempoTxMeta {
                    nonce_key: U256::MAX,
                    lane: TxLane::General,
                    section: BlockSection::NonShared,
                    subblock_index: None,
                    fee_payer: None,
                    call_count: 1,
                    fee_token: None,
                }),
            },
        ];

        let edges = adapter.nonce_edges(&nodes);
        assert!(edges.is_empty());
    }

    #[test]
    fn test_tempo_fee_sponsorship_edges() {
        let adapter = TempoAdapter;
        let sponsor = Address::with_last_byte(0xff);
        let user1 = Address::with_last_byte(0x01);
        let user2 = Address::with_last_byte(0x02);
        let user3 = Address::with_last_byte(0x03);

        let nodes = vec![
            TxNode {
                tx_hash: B256::with_last_byte(1),
                tx_index: 0,
                from: user1,
                to: None,
                tx_type: 0x76,
                nonce: 0,
                gas_used: 21000,
                tempo: Some(TempoTxMeta {
                    nonce_key: U256::ZERO,
                    lane: TxLane::General,
                    section: BlockSection::NonShared,
                    subblock_index: None,
                    fee_payer: Some(sponsor),
                    call_count: 1,
                    fee_token: None,
                }),
            },
            TxNode {
                tx_hash: B256::with_last_byte(2),
                tx_index: 1,
                from: user2,
                to: None,
                tx_type: 0x76,
                nonce: 0,
                gas_used: 21000,
                tempo: Some(TempoTxMeta {
                    nonce_key: U256::ZERO,
                    lane: TxLane::General,
                    section: BlockSection::NonShared,
                    subblock_index: None,
                    fee_payer: Some(sponsor),
                    call_count: 1,
                    fee_token: None,
                }),
            },
            TxNode {
                tx_hash: B256::with_last_byte(3),
                tx_index: 2,
                from: user3,
                to: None,
                tx_type: 0x76,
                nonce: 0,
                gas_used: 21000,
                tempo: Some(TempoTxMeta {
                    nonce_key: U256::ZERO,
                    lane: TxLane::General,
                    section: BlockSection::NonShared,
                    subblock_index: None,
                    fee_payer: None, // self-paying
                    call_count: 1,
                    fee_token: None,
                }),
            },
        ];

        let edges = adapter.structural_edges(&nodes);
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].from_tx, 0);
        assert_eq!(edges[0].to_tx, 1);
        assert_eq!(edges[0].kind, DepEdgeKind::FeeSponsorship);
    }
}
