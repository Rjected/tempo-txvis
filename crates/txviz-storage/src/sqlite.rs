use std::path::PathBuf;

use alloy_primitives::B256;
use anyhow::Result;
use async_trait::async_trait;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Row, SqlitePool};
use std::io::{Read, Write};
use std::str::FromStr;
use txviz_core::model::{BlockGraph, BlockMetrics, BlockRange, BlockSummary, ChainKind};

use crate::backend::StorageBackend;

pub struct SqliteStorage {
    data_dir: PathBuf,
    pool: SqlitePool,
}

impl SqliteStorage {
    pub async fn new(data_dir: PathBuf) -> Result<Self> {
        // Create data_dir and graphs subdirectory
        tokio::fs::create_dir_all(data_dir.join("graphs")).await?;

        // Open/create SQLite DB
        let db_path = data_dir.join("txviz.db");
        let opts = SqliteConnectOptions::new()
            .filename(&db_path)
            .create_if_missing(true);

        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(opts)
            .await?;

        // Run migrations
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS blocks (
                block_number  INTEGER PRIMARY KEY,
                block_hash    TEXT NOT NULL UNIQUE,
                parent_hash   TEXT NOT NULL,
                chain         TEXT NOT NULL,
                timestamp     INTEGER NOT NULL,
                metrics_json  TEXT NOT NULL,
                graph_path    TEXT NOT NULL,
                created_at    INTEGER NOT NULL DEFAULT (unixepoch())
            )
            "#,
        )
        .execute(&pool)
        .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_blocks_hash ON blocks(block_hash)")
            .execute(&pool)
            .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_blocks_timestamp ON blocks(timestamp)")
            .execute(&pool)
            .await?;

        Ok(Self { data_dir, pool })
    }

    fn graph_path(&self, block_hash: &B256) -> PathBuf {
        self.data_dir
            .join("graphs")
            .join(format!("{block_hash:?}.json.gz"))
    }

    fn graph_relative_path(block_hash: &B256) -> String {
        format!("graphs/{block_hash:?}.json.gz")
    }

    async fn write_graph_blob(&self, block_hash: &B256, graph: &BlockGraph) -> Result<()> {
        let path = self.graph_path(block_hash);
        let json = serde_json::to_vec(graph)?;
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&json)?;
        let compressed = encoder.finish()?;
        tokio::fs::write(&path, &compressed).await?;
        Ok(())
    }

    async fn read_graph_blob(&self, graph_path: &str) -> Result<BlockGraph> {
        let full_path = self.data_dir.join(graph_path);
        let compressed = tokio::fs::read(&full_path).await?;
        let mut decoder = GzDecoder::new(&compressed[..]);
        let mut json = Vec::new();
        decoder.read_to_end(&mut json)?;
        let graph: BlockGraph = serde_json::from_slice(&json)?;
        Ok(graph)
    }

    fn chain_kind_to_str(chain: ChainKind) -> &'static str {
        match chain {
            ChainKind::Ethereum => "ethereum",
            ChainKind::Tempo => "tempo",
        }
    }

    fn str_to_chain_kind(s: &str) -> ChainKind {
        match s {
            "tempo" => ChainKind::Tempo,
            _ => ChainKind::Ethereum,
        }
    }
}

#[async_trait]
impl StorageBackend for SqliteStorage {
    async fn put_block_graph(&self, graph: &BlockGraph) -> Result<()> {
        // Write compressed graph blob
        self.write_graph_blob(&graph.block_hash, graph).await?;

        let block_number = graph.block_number as i64;
        let block_hash = format!("{:?}", graph.block_hash);
        let parent_hash = format!("{:?}", graph.parent_hash);
        let chain = Self::chain_kind_to_str(graph.chain);
        let timestamp = graph.timestamp as i64;
        let metrics_json = serde_json::to_string(&graph.metrics)?;
        let graph_path = Self::graph_relative_path(&graph.block_hash);

        sqlx::query(
            r#"
            INSERT OR REPLACE INTO blocks
                (block_number, block_hash, parent_hash, chain, timestamp, metrics_json, graph_path)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            "#,
        )
        .bind(block_number)
        .bind(&block_hash)
        .bind(&parent_hash)
        .bind(chain)
        .bind(timestamp)
        .bind(&metrics_json)
        .bind(&graph_path)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn get_block_graph(&self, number: u64) -> Result<Option<BlockGraph>> {
        let row = sqlx::query("SELECT graph_path FROM blocks WHERE block_number = ?1")
            .bind(number as i64)
            .fetch_optional(&self.pool)
            .await?;

        match row {
            Some(row) => {
                let graph_path: String = row.get("graph_path");
                let graph = self.read_graph_blob(&graph_path).await?;
                Ok(Some(graph))
            }
            None => Ok(None),
        }
    }

    async fn get_block_graph_by_hash(&self, hash: B256) -> Result<Option<BlockGraph>> {
        let hash_str = format!("{hash:?}");
        let row = sqlx::query("SELECT graph_path FROM blocks WHERE block_hash = ?1")
            .bind(&hash_str)
            .fetch_optional(&self.pool)
            .await?;

        match row {
            Some(row) => {
                let graph_path: String = row.get("graph_path");
                let graph = self.read_graph_blob(&graph_path).await?;
                Ok(Some(graph))
            }
            None => Ok(None),
        }
    }

    async fn list_blocks(&self, range: &BlockRange) -> Result<Vec<BlockSummary>> {
        let mut query = String::from(
            "SELECT block_number, block_hash, chain, timestamp, metrics_json FROM blocks",
        );
        let mut conditions = Vec::new();
        let mut params: Vec<i64> = Vec::new();

        if let Some(from) = range.from {
            params.push(from as i64);
            conditions.push(format!("block_number >= ?{}", params.len()));
        }
        if let Some(to) = range.to {
            params.push(to as i64);
            conditions.push(format!("block_number <= ?{}", params.len()));
        }

        if !conditions.is_empty() {
            query.push_str(" WHERE ");
            query.push_str(&conditions.join(" AND "));
        }

        query.push_str(" ORDER BY block_number DESC");

        let limit = if range.limit == 0 { 50 } else { range.limit };
        params.push(limit as i64);
        query.push_str(&format!(" LIMIT ?{}", params.len()));

        let mut q = sqlx::query(&query);
        for p in &params {
            q = q.bind(p);
        }

        let rows = q.fetch_all(&self.pool).await?;

        let mut summaries = Vec::with_capacity(rows.len());
        for row in rows {
            let block_number: i64 = row.get("block_number");
            let block_hash_str: String = row.get("block_hash");
            let chain_str: String = row.get("chain");
            let timestamp: i64 = row.get("timestamp");
            let metrics_json_str: String = row.get("metrics_json");

            let block_hash = B256::from_str(&block_hash_str)?;
            let chain = Self::str_to_chain_kind(&chain_str);
            let metrics: BlockMetrics = serde_json::from_str(&metrics_json_str)?;

            summaries.push(BlockSummary {
                chain,
                block_number: block_number as u64,
                block_hash,
                timestamp: timestamp as u64,
                metrics,
            });
        }

        Ok(summaries)
    }

    async fn latest_block_number(&self) -> Result<Option<u64>> {
        let row = sqlx::query("SELECT MAX(block_number) as max_num FROM blocks")
            .fetch_one(&self.pool)
            .await?;

        let max_num: Option<i64> = row.get("max_num");
        Ok(max_num.map(|n| n as u64))
    }

    async fn delete_blocks_before(&self, number: u64) -> Result<u64> {
        let number = number as i64;

        // Get graph paths to delete files
        let rows =
            sqlx::query("SELECT graph_path FROM blocks WHERE block_number < ?1")
                .bind(number)
                .fetch_all(&self.pool)
                .await?;

        // Delete graph files
        for row in &rows {
            let graph_path: String = row.get("graph_path");
            let full_path = self.data_dir.join(&graph_path);
            let _ = tokio::fs::remove_file(&full_path).await;
        }

        // Delete from DB
        let result = sqlx::query("DELETE FROM blocks WHERE block_number < ?1")
            .bind(number)
            .execute(&self.pool)
            .await?;

        Ok(result.rows_affected())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::{Address, B256};
    use tempfile::TempDir;
    use txviz_core::model::*;

    fn make_block_graph(block_number: u64, block_hash: B256) -> BlockGraph {
        BlockGraph {
            chain: ChainKind::Ethereum,
            block_number,
            block_hash,
            parent_hash: B256::ZERO,
            timestamp: 1_700_000_000 + block_number,
            tx_nodes: vec![TxNode {
                tx_hash: B256::from([1u8; 32]),
                tx_index: 0,
                from: Address::ZERO,
                to: Some(Address::from([2u8; 20])),
                tx_type: 2,
                nonce: 0,
                gas_used: 21000,
                tempo: None,
            }],
            dep_edges: vec![],
            schedule: vec![ScheduleItem {
                tx_index: 0,
                start: 0,
                duration: 21000,
                lane: 0,
                is_critical: true,
            }],
            metrics: BlockMetrics {
                tx_count: 1,
                edge_count: 0,
                component_count: 1,
                total_weight: 21000,
                critical_path_weight: 21000,
                speedup_upper_bound: 1.0,
                max_parallelism: 1,
                makespan: 21000,
                payment_tx_count: None,
                general_tx_count: None,
                subblock_count: None,
                unique_nonce_keys: None,
            },
        }
    }

    fn hash_from_num(n: u8) -> B256 {
        B256::from([n; 32])
    }

    async fn setup() -> (TempDir, SqliteStorage) {
        let tmp = TempDir::new().unwrap();
        let storage = SqliteStorage::new(tmp.path().to_path_buf()).await.unwrap();
        (tmp, storage)
    }

    #[tokio::test]
    async fn test_put_get_roundtrip() {
        let (_tmp, storage) = setup().await;
        let graph = make_block_graph(100, hash_from_num(0xAA));

        storage.put_block_graph(&graph).await.unwrap();
        let retrieved = storage.get_block_graph(100).await.unwrap();

        assert_eq!(retrieved, Some(graph));
    }

    #[tokio::test]
    async fn test_get_nonexistent() {
        let (_tmp, storage) = setup().await;

        let result = storage.get_block_graph(999).await.unwrap();
        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn test_get_by_hash() {
        let (_tmp, storage) = setup().await;
        let hash = hash_from_num(0xBB);
        let graph = make_block_graph(200, hash);

        storage.put_block_graph(&graph).await.unwrap();
        let retrieved = storage.get_block_graph_by_hash(hash).await.unwrap();

        assert_eq!(retrieved, Some(graph));
    }

    #[tokio::test]
    async fn test_list_blocks_range() {
        let (_tmp, storage) = setup().await;

        for i in 1..=10u64 {
            let graph = make_block_graph(i, hash_from_num(i as u8));
            storage.put_block_graph(&graph).await.unwrap();
        }

        let range = BlockRange {
            from: Some(3),
            to: Some(7),
            limit: 50,
        };
        let summaries = storage.list_blocks(&range).await.unwrap();

        assert_eq!(summaries.len(), 5);
        // All returned blocks should be in range 3..=7
        for s in &summaries {
            assert!(s.block_number >= 3 && s.block_number <= 7);
        }
    }

    #[tokio::test]
    async fn test_list_blocks_limit() {
        let (_tmp, storage) = setup().await;

        for i in 1..=10u64 {
            let graph = make_block_graph(i, hash_from_num(i as u8));
            storage.put_block_graph(&graph).await.unwrap();
        }

        let range = BlockRange {
            from: None,
            to: None,
            limit: 3,
        };
        let summaries = storage.list_blocks(&range).await.unwrap();

        assert_eq!(summaries.len(), 3);
    }

    #[tokio::test]
    async fn test_list_blocks_order() {
        let (_tmp, storage) = setup().await;

        for i in 1..=5u64 {
            let graph = make_block_graph(i, hash_from_num(i as u8));
            storage.put_block_graph(&graph).await.unwrap();
        }

        let range = BlockRange {
            from: None,
            to: None,
            limit: 50,
        };
        let summaries = storage.list_blocks(&range).await.unwrap();

        // Should be newest first (descending)
        assert_eq!(summaries.len(), 5);
        assert_eq!(summaries[0].block_number, 5);
        assert_eq!(summaries[1].block_number, 4);
        assert_eq!(summaries[2].block_number, 3);
        assert_eq!(summaries[3].block_number, 2);
        assert_eq!(summaries[4].block_number, 1);
    }

    #[tokio::test]
    async fn test_latest_block_number() {
        let (_tmp, storage) = setup().await;

        for i in [5u64, 10, 3] {
            let graph = make_block_graph(i, hash_from_num(i as u8));
            storage.put_block_graph(&graph).await.unwrap();
        }

        let latest = storage.latest_block_number().await.unwrap();
        assert_eq!(latest, Some(10));
    }

    #[tokio::test]
    async fn test_latest_block_number_empty() {
        let (_tmp, storage) = setup().await;

        let latest = storage.latest_block_number().await.unwrap();
        assert_eq!(latest, None);
    }

    #[tokio::test]
    async fn test_put_overwrite() {
        let (_tmp, storage) = setup().await;
        let hash = hash_from_num(0xCC);
        let mut graph = make_block_graph(100, hash);

        storage.put_block_graph(&graph).await.unwrap();

        // Modify the graph and put again with same block number and hash
        graph.timestamp = 9_999_999;
        storage.put_block_graph(&graph).await.unwrap();

        let retrieved = storage.get_block_graph(100).await.unwrap().unwrap();
        assert_eq!(retrieved.timestamp, 9_999_999);
    }

    #[tokio::test]
    async fn test_delete_old_blocks() {
        let (_tmp, storage) = setup().await;

        for i in 1..=10u64 {
            let graph = make_block_graph(i, hash_from_num(i as u8));
            storage.put_block_graph(&graph).await.unwrap();
        }

        // Delete blocks before block 6 (deletes 1..5)
        let deleted = storage.delete_blocks_before(6).await.unwrap();
        assert_eq!(deleted, 5);

        // Blocks 1-5 should be gone
        for i in 1..=5u64 {
            assert_eq!(storage.get_block_graph(i).await.unwrap(), None);
        }
        // Blocks 6-10 should still exist
        for i in 6..=10u64 {
            assert!(storage.get_block_graph(i).await.unwrap().is_some());
        }
    }

    #[tokio::test]
    async fn test_graph_file_created() {
        let (tmp, storage) = setup().await;
        let hash = hash_from_num(0xDD);
        let graph = make_block_graph(300, hash);

        storage.put_block_graph(&graph).await.unwrap();

        let file_path = tmp
            .path()
            .join("graphs")
            .join(format!("{hash:?}.json.gz"));
        assert!(file_path.exists(), "graph file should exist on disk");

        // File should be readable and deserialize back
        let compressed = std::fs::read(&file_path).unwrap();
        let mut decoder = GzDecoder::new(&compressed[..]);
        let mut json = Vec::new();
        decoder.read_to_end(&mut json).unwrap();
        let deserialized: BlockGraph = serde_json::from_slice(&json).unwrap();
        assert_eq!(deserialized, graph);
    }

    #[tokio::test]
    async fn test_graph_file_compressed() {
        let (tmp, storage) = setup().await;
        let hash = hash_from_num(0xEE);
        let graph = make_block_graph(400, hash);

        storage.put_block_graph(&graph).await.unwrap();

        let file_path = tmp
            .path()
            .join("graphs")
            .join(format!("{hash:?}.json.gz"));
        let compressed = std::fs::read(&file_path).unwrap();

        // Gzip files start with magic bytes 0x1f 0x8b
        assert!(
            compressed.len() >= 2,
            "compressed file should have at least 2 bytes"
        );
        assert_eq!(
            compressed[0], 0x1f,
            "first byte should be gzip magic byte 0x1f"
        );
        assert_eq!(
            compressed[1], 0x8b,
            "second byte should be gzip magic byte 0x8b"
        );
    }
}
