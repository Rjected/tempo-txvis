use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};
use serde::Deserialize;
use txviz_core::model::BlockRange;

use crate::state::AppState;

#[derive(Deserialize)]
pub struct BlockListQuery {
    pub from: Option<u64>,
    pub to: Option<u64>,
    pub limit: Option<u32>,
}

pub async fn chain_handler(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    Json(serde_json::to_value(&state.chain_identity).unwrap())
}

pub async fn get_block_handler(
    State(state): State<Arc<AppState>>,
    Path(number): Path<String>,
) -> Response {
    let block_number = if number == "latest" {
        match state.storage.latest_block_number().await {
            Ok(Some(n)) => n,
            Ok(None) => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({"error": "Block not found"})),
                )
                    .into_response();
            }
            Err(_) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": "Storage error"})),
                )
                    .into_response();
            }
        }
    } else {
        match number.parse::<u64>() {
            Ok(n) => n,
            Err(_) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({"error": "Invalid block number"})),
                )
                    .into_response();
            }
        }
    };

    match state.storage.get_block_graph(block_number).await {
        Ok(Some(graph)) => Json(serde_json::to_value(&graph).unwrap()).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Block not found"})),
        )
            .into_response(),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "Storage error"})),
        )
            .into_response(),
    }
}

pub async fn get_block_by_hash_handler(
    State(state): State<Arc<AppState>>,
    Path(hash): Path<String>,
) -> Response {
    let hash = match hash.parse() {
        Ok(h) => h,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid hash"})),
            )
                .into_response();
        }
    };

    match state.storage.get_block_graph_by_hash(hash).await {
        Ok(Some(graph)) => Json(serde_json::to_value(&graph).unwrap()).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Block not found"})),
        )
            .into_response(),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "Storage error"})),
        )
            .into_response(),
    }
}

pub async fn list_blocks_handler(
    State(state): State<Arc<AppState>>,
    Query(query): Query<BlockListQuery>,
) -> Response {
    let range = BlockRange {
        from: query.from,
        to: query.to,
        limit: query.limit.unwrap_or(50),
    };

    match state.storage.list_blocks(&range).await {
        Ok(blocks) => Json(serde_json::json!({"blocks": blocks})).into_response(),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "Storage error"})),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use alloy_primitives::B256;
    use anyhow::Result;
    use async_trait::async_trait;
    use axum_test::TestServer;
    use tokio::sync::broadcast;
    use txviz_chain::types::ChainIdentity;
    use txviz_core::model::*;
    use txviz_storage::backend::StorageBackend;

    use crate::api_router;
    use crate::state::AppState;

    // --- Mock Storage ---

    struct MockStorage {
        blocks: HashMap<u64, BlockGraph>,
    }

    impl MockStorage {
        fn new() -> Self {
            Self {
                blocks: HashMap::new(),
            }
        }

        fn with_block(mut self, graph: BlockGraph) -> Self {
            self.blocks.insert(graph.block_number, graph);
            self
        }
    }

    #[async_trait]
    impl StorageBackend for MockStorage {
        async fn put_block_graph(&self, _graph: &BlockGraph) -> Result<()> {
            Ok(())
        }

        async fn get_block_graph(&self, number: u64) -> Result<Option<BlockGraph>> {
            Ok(self.blocks.get(&number).cloned())
        }

        async fn get_block_graph_by_hash(&self, hash: B256) -> Result<Option<BlockGraph>> {
            Ok(self.blocks.values().find(|b| b.block_hash == hash).cloned())
        }

        async fn list_blocks(&self, range: &BlockRange) -> Result<Vec<BlockSummary>> {
            let mut blocks: Vec<_> = self.blocks.values().collect();
            blocks.sort_by(|a, b| b.block_number.cmp(&a.block_number));

            let blocks: Vec<_> = blocks
                .into_iter()
                .filter(|b| {
                    if let Some(from) = range.from {
                        if b.block_number < from {
                            return false;
                        }
                    }
                    if let Some(to) = range.to {
                        if b.block_number > to {
                            return false;
                        }
                    }
                    true
                })
                .take(range.limit as usize)
                .map(|b| BlockSummary {
                    chain: b.chain,
                    block_number: b.block_number,
                    block_hash: b.block_hash,
                    timestamp: b.timestamp,
                    metrics: b.metrics.clone(),
                })
                .collect();
            Ok(blocks)
        }

        async fn latest_block_number(&self) -> Result<Option<u64>> {
            Ok(self.blocks.keys().max().copied())
        }

        async fn delete_blocks_before(&self, _number: u64) -> Result<u64> {
            Ok(0)
        }
    }

    // --- Helpers ---

    fn test_block_graph(number: u64) -> BlockGraph {
        BlockGraph {
            chain: ChainKind::Ethereum,
            block_number: number,
            block_hash: B256::from([number as u8; 32]),
            parent_hash: B256::ZERO,
            timestamp: 1700000000 + number,
            tx_nodes: vec![TxNode {
                tx_hash: B256::from([0xaa; 32]),
                tx_index: 0,
                from: Default::default(),
                to: None,
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

    fn test_app_state(storage: MockStorage) -> Arc<AppState> {
        let (live_tx, _) = broadcast::channel(16);
        Arc::new(AppState {
            storage: Arc::new(storage),
            chain_identity: ChainIdentity {
                chain_id: 1,
                chain_kind: ChainKind::Ethereum,
                client_version: "reth/v1.0.0".to_string(),
            },
            live_tx,
        })
    }

    fn test_server(state: Arc<AppState>) -> TestServer {
        let app = api_router(state);
        TestServer::new(app).unwrap()
    }

    // --- Tests ---

    #[tokio::test]
    async fn test_chain_endpoint() {
        let state = test_app_state(MockStorage::new());
        let server = test_server(state);

        let response = server.get("/api/chain").await;
        response.assert_status_ok();

        let body: serde_json::Value = response.json();
        assert_eq!(body["chainId"], 1);
        assert_eq!(body["chainKind"], "ethereum");
        assert_eq!(body["clientVersion"], "reth/v1.0.0");
    }

    #[tokio::test]
    async fn test_get_block_by_number() {
        let storage = MockStorage::new().with_block(test_block_graph(19000000));
        let state = test_app_state(storage);
        let server = test_server(state);

        let response = server.get("/api/block/19000000").await;
        response.assert_status_ok();

        let body: serde_json::Value = response.json();
        assert_eq!(body["blockNumber"], 19000000);
        assert_eq!(body["chain"], "ethereum");
        assert!(body["txNodes"].is_array());
        assert!(body["metrics"].is_object());
    }

    #[tokio::test]
    async fn test_get_block_not_found() {
        let state = test_app_state(MockStorage::new());
        let server = test_server(state);

        let response = server.get("/api/block/99999999").await;
        response.assert_status(axum::http::StatusCode::NOT_FOUND);

        let body: serde_json::Value = response.json();
        assert_eq!(body["error"], "Block not found");
    }

    #[tokio::test]
    async fn test_get_block_by_hash() {
        let graph = test_block_graph(19000000);
        let hash = graph.block_hash;
        let storage = MockStorage::new().with_block(graph);
        let state = test_app_state(storage);
        let server = test_server(state);

        let response = server.get(&format!("/api/block/hash/{hash}")).await;
        response.assert_status_ok();

        let body: serde_json::Value = response.json();
        assert_eq!(body["blockNumber"], 19000000);
    }

    #[tokio::test]
    async fn test_get_block_by_hash_not_found() {
        let state = test_app_state(MockStorage::new());
        let server = test_server(state);

        let response = server
            .get("/api/block/hash/0x0000000000000000000000000000000000000000000000000000000000000099")
            .await;
        response.assert_status(axum::http::StatusCode::NOT_FOUND);

        let body: serde_json::Value = response.json();
        assert_eq!(body["error"], "Block not found");
    }

    #[tokio::test]
    async fn test_list_blocks() {
        let storage = MockStorage::new()
            .with_block(test_block_graph(100))
            .with_block(test_block_graph(101))
            .with_block(test_block_graph(102));
        let state = test_app_state(storage);
        let server = test_server(state);

        let response = server.get("/api/blocks").await;
        response.assert_status_ok();

        let body: serde_json::Value = response.json();
        let blocks = body["blocks"].as_array().unwrap();
        assert_eq!(blocks.len(), 3);
        // Should be newest first
        assert_eq!(blocks[0]["blockNumber"], 102);
        assert_eq!(blocks[1]["blockNumber"], 101);
        assert_eq!(blocks[2]["blockNumber"], 100);
    }

    #[tokio::test]
    async fn test_list_blocks_with_range() {
        let storage = MockStorage::new()
            .with_block(test_block_graph(100))
            .with_block(test_block_graph(150))
            .with_block(test_block_graph(200))
            .with_block(test_block_graph(250));
        let state = test_app_state(storage);
        let server = test_server(state);

        let response = server.get("/api/blocks?from=100&to=200").await;
        response.assert_status_ok();

        let body: serde_json::Value = response.json();
        let blocks = body["blocks"].as_array().unwrap();
        assert_eq!(blocks.len(), 3);
        for b in blocks {
            let num = b["blockNumber"].as_u64().unwrap();
            assert!(num >= 100 && num <= 200);
        }
    }

    #[tokio::test]
    async fn test_list_blocks_with_limit() {
        let mut storage = MockStorage::new();
        for i in 0..20 {
            storage = storage.with_block(test_block_graph(100 + i));
        }
        let state = test_app_state(storage);
        let server = test_server(state);

        let response = server.get("/api/blocks?limit=10").await;
        response.assert_status_ok();

        let body: serde_json::Value = response.json();
        let blocks = body["blocks"].as_array().unwrap();
        assert_eq!(blocks.len(), 10);
    }

    #[tokio::test]
    async fn test_cors_headers() {
        let state = test_app_state(MockStorage::new());
        let server = test_server(state);

        let response = server.get("/api/chain").await;
        let headers = response.headers();
        assert!(headers.contains_key("access-control-allow-origin"));
    }

    #[tokio::test]
    async fn test_get_block_latest() {
        let storage = MockStorage::new()
            .with_block(test_block_graph(100))
            .with_block(test_block_graph(200))
            .with_block(test_block_graph(150));
        let state = test_app_state(storage);
        let server = test_server(state);

        let response = server.get("/api/block/latest").await;
        response.assert_status_ok();

        let body: serde_json::Value = response.json();
        assert_eq!(body["blockNumber"], 200);
    }

    #[tokio::test]
    async fn test_sse_content_type() {
        use axum::body::Body;
        use http::Request;
        use tower::ServiceExt;

        let state = test_app_state(MockStorage::new());
        let app = api_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/live")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let content_type = response
            .headers()
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(
            content_type.contains("text/event-stream"),
            "Expected text/event-stream, got: {content_type}"
        );
    }

    #[tokio::test]
    async fn test_sse_receives_broadcast() {
        use axum::body::Body;
        use http::Request;
        use http_body_util::BodyExt;
        use tower::ServiceExt;

        let (live_tx, _) = broadcast::channel(16);
        let state = Arc::new(AppState {
            storage: Arc::new(MockStorage::new()),
            chain_identity: ChainIdentity {
                chain_id: 1,
                chain_kind: ChainKind::Ethereum,
                client_version: "reth/v1.0.0".to_string(),
            },
            live_tx: live_tx.clone(),
        });
        let app = api_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/live")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), axum::http::StatusCode::OK);

        let mut body = response.into_body();

        // Send event after subscribing
        let event = BlockUpdateEvent {
            block_number: 42,
            block_hash: B256::from([0x42; 32]),
            timestamp: 1700000042,
            tx_count: 5,
            speedup_upper_bound: 2.5,
            critical_path_weight: 100000,
        };
        live_tx.send(event).unwrap();

        // Read the first frame from the SSE stream
        let frame = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            body.frame(),
        )
        .await
        .expect("timed out waiting for SSE frame")
        .expect("stream ended")
        .expect("frame error");

        let data = frame.into_data().expect("expected data frame");
        let text = String::from_utf8(data.to_vec()).unwrap();
        assert!(
            text.contains("event: block"),
            "Expected 'event: block' in SSE output, got: {text}"
        );
        assert!(
            text.contains("\"blockNumber\":42"),
            "Expected blockNumber 42 in SSE output, got: {text}"
        );
    }
}
