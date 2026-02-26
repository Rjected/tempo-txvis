pub mod adapter;
pub mod provider;
pub mod stream;
pub mod tempo;
pub mod types;

use adapter::ChainAdapter;
use alloy_primitives::Address;
use anyhow::Result;
use txviz_core::model::{ChainKind, DepEdge, DepEdgeKind, DepReason, TxNode};
use types::{RpcReceipt, RpcTransaction};

/// Ethereum chain adapter.
pub struct EthereumAdapter;

/// Detect chain kind from chain_id and client version.
pub fn detect_chain(chain_id: u64, client_version: &str, force: Option<ChainKind>) -> ChainKind {
    if let Some(forced) = force {
        return forced;
    }
    match chain_id {
        42431 => ChainKind::Tempo,
        _ => {
            if client_version.to_lowercase().contains("tempo") {
                ChainKind::Tempo
            } else {
                ChainKind::Ethereum
            }
        }
    }
}

/// Parse a hex string (e.g. "0x02") to u8.
fn parse_hex_u8(s: &str) -> Result<u8> {
    let s = s.strip_prefix("0x").unwrap_or(s);
    u8::from_str_radix(s, 16).map_err(|e| anyhow::anyhow!("invalid hex u8: {e}"))
}

/// Parse a hex string or JSON number to u64.
fn parse_nonce(v: &serde_json::Value) -> Result<u64> {
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
pub(crate) fn parse_hex_u64(s: &str) -> Result<u64> {
    let s = s.strip_prefix("0x").unwrap_or(s);
    u64::from_str_radix(s, 16).map_err(|e| anyhow::anyhow!("invalid hex u64: {e}"))
}

impl ChainAdapter for EthereumAdapter {
    fn chain_kind(&self) -> ChainKind {
        ChainKind::Ethereum
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
            .map(parse_nonce)
            .transpose()?
            .unwrap_or(0);

        let gas_used = receipt
            .gas_used
            .as_deref()
            .map(parse_hex_u64)
            .transpose()?
            .unwrap_or(0);

        Ok(TxNode {
            tx_hash: tx.hash,
            tx_index,
            from: tx.from,
            to: tx.to,
            tx_type,
            nonce,
            gas_used,
            tempo: None,
        })
    }

    fn nonce_edges(&self, nodes: &[TxNode]) -> Vec<DepEdge> {
        use std::collections::HashMap;

        // Group by sender
        let mut by_sender: HashMap<Address, Vec<(u32, u64)>> = HashMap::new();
        for node in nodes {
            by_sender
                .entry(node.from)
                .or_default()
                .push((node.tx_index, node.nonce));
        }

        let mut edges = Vec::new();
        for (sender, mut txs) in by_sender {
            txs.sort_by_key(|&(_, nonce)| nonce);
            for pair in txs.windows(2) {
                let (from_idx, _) = pair[0];
                let (to_idx, _) = pair[1];
                edges.push(DepEdge {
                    from_tx: from_idx,
                    to_tx: to_idx,
                    kind: DepEdgeKind::Nonce1d,
                    reasons: vec![DepReason::NonceSequence {
                        address: sender,
                        nonce_key: None,
                    }],
                });
            }
        }
        edges.sort_by_key(|e| (e.from_tx, e.to_tx));
        edges
    }

    fn structural_edges(&self, _nodes: &[TxNode]) -> Vec<DepEdge> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::{Address, B256};

    // ---- B1: Chain Detection Tests ----

    #[test]
    fn test_detect_ethereum_mainnet() {
        assert_eq!(
            detect_chain(1, "Geth/v1.13.0", None),
            ChainKind::Ethereum
        );
    }

    #[test]
    fn test_detect_tempo_moderato() {
        assert_eq!(
            detect_chain(42431, "reth/v1.0.0", None),
            ChainKind::Tempo
        );
    }

    #[test]
    fn test_detect_tempo_from_client_version() {
        assert_eq!(
            detect_chain(99999, "Tempo-reth/v0.1.0", None),
            ChainKind::Tempo
        );
    }

    #[test]
    fn test_detect_unknown_defaults_ethereum() {
        assert_eq!(
            detect_chain(99999, "SomeClient/v1.0", None),
            ChainKind::Ethereum
        );
    }

    #[test]
    fn test_force_chain_override() {
        // Force Tempo even though chain_id is 1 and client is "Geth"
        assert_eq!(
            detect_chain(1, "Geth/v1.13.0", Some(ChainKind::Tempo)),
            ChainKind::Tempo
        );
        // Force Ethereum even though chain_id is Tempo's
        assert_eq!(
            detect_chain(42431, "Tempo-reth/v0.1", Some(ChainKind::Ethereum)),
            ChainKind::Ethereum
        );
    }

    // ---- B2: Ethereum Adapter Tests ----

    fn make_eth_tx(
        hash: B256,
        from: Address,
        to: Option<Address>,
        tx_type: &str,
        nonce: serde_json::Value,
    ) -> RpcTransaction {
        RpcTransaction {
            hash,
            from,
            to,
            tx_type: Some(tx_type.to_string()),
            nonce: Some(nonce),
            gas: None,
            value: None,
            input: None,
            nonce_key: None,
            calls: None,
            fee_token: None,
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
    fn test_parse_ethereum_tx() {
        let adapter = EthereumAdapter;
        let hash = B256::with_last_byte(1);
        let from = Address::with_last_byte(0x11);
        let to = Address::with_last_byte(0x22);

        let tx = make_eth_tx(hash, from, Some(to), "0x02", serde_json::json!("0x0a"));
        let receipt = make_receipt(hash, "0xc350");

        let node = adapter.parse_tx_node(&tx, &receipt, 0).unwrap();
        assert_eq!(node.tx_hash, hash);
        assert_eq!(node.tx_index, 0);
        assert_eq!(node.from, from);
        assert_eq!(node.to, Some(to));
        assert_eq!(node.tx_type, 2);
        assert_eq!(node.nonce, 10);
        assert_eq!(node.gas_used, 0xc350); // 50000
        assert!(node.tempo.is_none());
    }

    #[test]
    fn test_parse_legacy_tx() {
        let adapter = EthereumAdapter;
        let hash = B256::with_last_byte(2);
        let from = Address::with_last_byte(0x33);

        let tx = make_eth_tx(hash, from, None, "0x00", serde_json::json!(0));
        let receipt = make_receipt(hash, "0x5208");

        let node = adapter.parse_tx_node(&tx, &receipt, 1).unwrap();
        assert_eq!(node.tx_type, 0);
        assert_eq!(node.nonce, 0);
        assert_eq!(node.gas_used, 0x5208); // 21000
        assert!(node.to.is_none());
    }

    #[test]
    fn test_ethereum_nonce_edges() {
        let adapter = EthereumAdapter;
        let sender = Address::with_last_byte(0x11);
        let nodes = vec![
            TxNode {
                tx_hash: B256::with_last_byte(1),
                tx_index: 0,
                from: sender,
                to: None,
                tx_type: 2,
                nonce: 5,
                gas_used: 21000,
                tempo: None,
            },
            TxNode {
                tx_hash: B256::with_last_byte(2),
                tx_index: 1,
                from: sender,
                to: None,
                tx_type: 2,
                nonce: 6,
                gas_used: 21000,
                tempo: None,
            },
            TxNode {
                tx_hash: B256::with_last_byte(3),
                tx_index: 2,
                from: sender,
                to: None,
                tx_type: 2,
                nonce: 7,
                gas_used: 21000,
                tempo: None,
            },
        ];

        let edges = adapter.nonce_edges(&nodes);
        assert_eq!(edges.len(), 2);
        assert_eq!(edges[0].from_tx, 0);
        assert_eq!(edges[0].to_tx, 1);
        assert_eq!(edges[0].kind, DepEdgeKind::Nonce1d);
        assert_eq!(edges[1].from_tx, 1);
        assert_eq!(edges[1].to_tx, 2);
        assert_eq!(edges[1].kind, DepEdgeKind::Nonce1d);
    }

    #[test]
    fn test_ethereum_nonce_edges_different_senders() {
        let adapter = EthereumAdapter;
        let nodes = vec![
            TxNode {
                tx_hash: B256::with_last_byte(1),
                tx_index: 0,
                from: Address::with_last_byte(0x11),
                to: None,
                tx_type: 2,
                nonce: 5,
                gas_used: 21000,
                tempo: None,
            },
            TxNode {
                tx_hash: B256::with_last_byte(2),
                tx_index: 1,
                from: Address::with_last_byte(0x22),
                to: None,
                tx_type: 2,
                nonce: 10,
                gas_used: 21000,
                tempo: None,
            },
        ];

        let edges = adapter.nonce_edges(&nodes);
        assert!(edges.is_empty());
    }

    #[test]
    fn test_ethereum_structural_edges_empty() {
        let adapter = EthereumAdapter;
        let edges = adapter.structural_edges(&[]);
        assert!(edges.is_empty());
    }
}
