use anyhow::Result;
use txviz_core::model::{ChainKind, DepEdge, TxNode};

use crate::types::{RpcReceipt, RpcTransaction};

pub trait ChainAdapter: Send + Sync {
    fn chain_kind(&self) -> ChainKind;

    /// Parse chain-specific tx metadata from RPC transaction object.
    fn parse_tx_node(&self, tx: &RpcTransaction, receipt: &RpcReceipt, tx_index: u32) -> Result<TxNode>;

    /// Generate nonce sequencing edges.
    fn nonce_edges(&self, nodes: &[TxNode]) -> Vec<DepEdge>;

    /// Generate chain-specific structural edges (fee sponsorship, etc.).
    fn structural_edges(&self, nodes: &[TxNode]) -> Vec<DepEdge>;
}
