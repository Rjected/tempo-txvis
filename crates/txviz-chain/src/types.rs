use alloy_primitives::{Address, B256, U256};
use serde::{Deserialize, Serialize};
use txviz_core::model::ChainKind;

/// Identity of the connected chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChainIdentity {
    pub chain_id: u64,
    pub chain_kind: ChainKind,
    pub client_version: String,
}

/// A full block fetched from RPC (header + transactions + receipts).
#[derive(Debug, Clone)]
pub struct BlockEnvelope {
    pub number: u64,
    pub hash: B256,
    pub parent_hash: B256,
    pub timestamp: u64,
    pub transactions: Vec<RpcTransaction>,
    pub receipts: Vec<RpcReceipt>,
}

/// Raw transaction from RPC response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcTransaction {
    pub hash: B256,
    pub from: Address,
    #[serde(default)]
    pub to: Option<Address>,
    #[serde(rename = "type", default)]
    pub tx_type: Option<String>,
    #[serde(default)]
    pub nonce: Option<serde_json::Value>,
    #[serde(default)]
    pub gas: Option<String>,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub input: Option<String>,
    // Tempo-specific fields
    #[serde(default)]
    pub nonce_key: Option<String>,
    #[serde(default)]
    pub calls: Option<Vec<serde_json::Value>>,
    #[serde(default)]
    pub fee_token: Option<String>,
}

/// Receipt from RPC response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcReceipt {
    pub transaction_hash: B256,
    #[serde(default)]
    pub gas_used: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
}

/// Notification of a new block header.
#[derive(Debug, Clone)]
pub struct NewBlockNotification {
    pub number: u64,
    pub hash: B256,
}

/// Prestate diff trace result for a single transaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrestateDiffTrace {
    #[serde(default)]
    pub tx_hash: Option<B256>,
    pub pre: serde_json::Value,
    pub post: serde_json::Value,
    #[serde(default)]
    pub error: Option<String>,
}

/// Prestate (non-diff) trace result for a single transaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrestateTrace {
    #[serde(default)]
    pub tx_hash: Option<B256>,
    pub result: serde_json::Value,
    #[serde(default)]
    pub error: Option<String>,
}
