use alloy_primitives::B256;
use anyhow::Result;
use async_trait::async_trait;
use txviz_core::model::{BlockGraph, BlockRange, BlockSummary};

#[async_trait]
pub trait StorageBackend: Send + Sync {
    async fn put_block_graph(&self, graph: &BlockGraph) -> Result<()>;
    async fn get_block_graph(&self, number: u64) -> Result<Option<BlockGraph>>;
    async fn get_block_graph_by_hash(&self, hash: B256) -> Result<Option<BlockGraph>>;
    async fn list_blocks(&self, range: &BlockRange) -> Result<Vec<BlockSummary>>;
    async fn latest_block_number(&self) -> Result<Option<u64>>;
    async fn delete_blocks_before(&self, number: u64) -> Result<u64>;
}
