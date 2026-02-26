use anyhow::{Context, Result};
use async_trait::async_trait;
use futures::stream::BoxStream;
use futures::StreamExt;
use std::time::Duration;

use crate::types::{
    BlockEnvelope, ChainIdentity, NewBlockNotification, PrestateDiffTrace, PrestateTrace,
    RpcReceipt, RpcTransaction,
};
use crate::{detect_chain, parse_hex_u64};
use alloy_primitives::B256;
use txviz_core::model::ChainKind;

#[async_trait]
pub trait ChainProvider: Send + Sync {
    async fn chain_identity(&self) -> Result<ChainIdentity>;
    async fn latest_block_number(&self) -> Result<u64>;
    async fn get_block(&self, number: u64) -> Result<BlockEnvelope>;
    async fn trace_block_prestate_diff(&self, number: u64) -> Result<Vec<PrestateDiffTrace>>;
    async fn trace_block_prestate(&self, number: u64) -> Result<Vec<PrestateTrace>>;
    fn subscribe_new_blocks(&self) -> BoxStream<'static, Result<NewBlockNotification>>;
}

pub struct RpcProvider {
    http_url: String,
    ws_url: Option<String>,
    client: reqwest::Client,
    poll_interval: Duration,
    force_chain: Option<ChainKind>,
}

impl RpcProvider {
    pub fn new(
        http_url: String,
        ws_url: Option<String>,
        timeout: Duration,
        poll_interval: Duration,
        force_chain: Option<ChainKind>,
    ) -> Self {
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .expect("failed to build HTTP client");
        Self {
            http_url,
            ws_url,
            client,
            poll_interval,
            force_chain,
        }
    }

    async fn rpc_call(&self, method: &str, params: serde_json::Value) -> Result<serde_json::Value> {
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
            "id": 1
        });

        let resp = self
            .client
            .post(&self.http_url)
            .json(&body)
            .send()
            .await
            .context("RPC request failed")?;

        let status = resp.status();
        let json: serde_json::Value = resp.json().await.context("failed to parse RPC response")?;

        if let Some(error) = json.get("error") {
            anyhow::bail!("RPC error (HTTP {}): {}", status, error);
        }

        json.get("result")
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("RPC response missing 'result' field"))
    }
}

#[async_trait]
impl ChainProvider for RpcProvider {
    async fn chain_identity(&self) -> Result<ChainIdentity> {
        let chain_id_hex = self
            .rpc_call("eth_chainId", serde_json::json!([]))
            .await?;
        let chain_id_str = chain_id_hex
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("eth_chainId result not a string"))?;
        let chain_id = parse_hex_u64(chain_id_str)?;

        let client_version_val = self
            .rpc_call("web3_clientVersion", serde_json::json!([]))
            .await?;
        let client_version = client_version_val
            .as_str()
            .unwrap_or("unknown")
            .to_string();

        let chain_kind = detect_chain(chain_id, &client_version, self.force_chain);

        Ok(ChainIdentity {
            chain_id,
            chain_kind,
            client_version,
        })
    }

    async fn latest_block_number(&self) -> Result<u64> {
        let result = self
            .rpc_call("eth_blockNumber", serde_json::json!([]))
            .await?;
        let hex = result
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("eth_blockNumber result not a string"))?;
        parse_hex_u64(hex)
    }

    async fn get_block(&self, number: u64) -> Result<BlockEnvelope> {
        let block_num_hex = format!("0x{number:x}");

        // Fetch block with full transaction objects
        let block_json = self
            .rpc_call(
                "eth_getBlockByNumber",
                serde_json::json!([block_num_hex, true]),
            )
            .await
            .context("failed to fetch block")?;

        let block_hash: B256 = serde_json::from_value(
            block_json
                .get("hash")
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("block missing hash"))?,
        )?;
        let parent_hash: B256 = serde_json::from_value(
            block_json
                .get("parentHash")
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("block missing parentHash"))?,
        )?;
        let timestamp_hex = block_json
            .get("timestamp")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("block missing timestamp"))?;
        let timestamp = parse_hex_u64(timestamp_hex)?;

        let transactions: Vec<RpcTransaction> = block_json
            .get("transactions")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();

        // Fetch receipts for each transaction
        let mut receipts = Vec::with_capacity(transactions.len());
        for tx in &transactions {
            let receipt_json = self
                .rpc_call(
                    "eth_getTransactionReceipt",
                    serde_json::json!([tx.hash]),
                )
                .await
                .with_context(|| format!("failed to fetch receipt for {:?}", tx.hash))?;

            let receipt: RpcReceipt = serde_json::from_value(receipt_json)
                .context("failed to parse receipt")?;
            receipts.push(receipt);
        }

        Ok(BlockEnvelope {
            number,
            hash: block_hash,
            parent_hash,
            timestamp,
            transactions,
            receipts,
        })
    }

    async fn trace_block_prestate_diff(&self, number: u64) -> Result<Vec<PrestateDiffTrace>> {
        let block_num_hex = format!("0x{number:x}");
        let result = self
            .rpc_call(
                "debug_traceBlockByNumber",
                serde_json::json!([
                    block_num_hex,
                    {"tracer": "prestateTracer", "tracerConfig": {"diffMode": true}}
                ]),
            )
            .await
            .context("failed to fetch prestate diff traces")?;

        // Handle both formats: array of {txHash, result: {pre, post}} or array of {pre, post}
        let arr = result
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("trace result not an array"))?;

        let mut traces = Vec::with_capacity(arr.len());
        for item in arr {
            // Wrapped format: { "txHash": ..., "result": { "pre": ..., "post": ... } }
            if let Some(inner) = item.get("result") {
                let tx_hash = item.get("txHash").and_then(|v| {
                    serde_json::from_value::<B256>(v.clone()).ok()
                });
                let error = item.get("error").and_then(|v| v.as_str()).map(String::from);
                traces.push(PrestateDiffTrace {
                    tx_hash,
                    pre: inner.get("pre").cloned().unwrap_or(serde_json::json!({})),
                    post: inner.get("post").cloned().unwrap_or(serde_json::json!({})),
                    error,
                });
            } else {
                // Direct format: { "pre": ..., "post": ... }
                let trace: PrestateDiffTrace = serde_json::from_value(item.clone())
                    .context("failed to parse prestate diff trace")?;
                traces.push(trace);
            }
        }

        Ok(traces)
    }

    async fn trace_block_prestate(&self, number: u64) -> Result<Vec<PrestateTrace>> {
        let block_num_hex = format!("0x{number:x}");
        let result = self
            .rpc_call(
                "debug_traceBlockByNumber",
                serde_json::json!([
                    block_num_hex,
                    {"tracer": "prestateTracer", "tracerConfig": {"diffMode": false}}
                ]),
            )
            .await
            .context("failed to fetch prestate traces")?;

        let arr = result
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("trace result not an array"))?;

        let mut traces = Vec::with_capacity(arr.len());
        for item in arr {
            if let Some(inner) = item.get("result") {
                let tx_hash = item.get("txHash").and_then(|v| {
                    serde_json::from_value::<B256>(v.clone()).ok()
                });
                let error = item.get("error").and_then(|v| v.as_str()).map(String::from);
                traces.push(PrestateTrace {
                    tx_hash,
                    result: inner.clone(),
                    error,
                });
            } else {
                let trace: PrestateTrace = serde_json::from_value(item.clone())
                    .context("failed to parse prestate trace")?;
                traces.push(trace);
            }
        }

        Ok(traces)
    }

    fn subscribe_new_blocks(&self) -> BoxStream<'static, Result<NewBlockNotification>> {
        let http_url = self.http_url.clone();
        let poll_interval = self.poll_interval;
        let client = self.client.clone();

        // Polling fallback
        let stream = futures::stream::unfold(
            (client, http_url, None::<u64>),
            move |(client, url, last_seen)| async move {
                loop {
                    tokio::time::sleep(poll_interval).await;

                    let body = serde_json::json!({
                        "jsonrpc": "2.0",
                        "method": "eth_blockNumber",
                        "params": [],
                        "id": 1
                    });

                    let resp = match client.post(&url).json(&body).send().await {
                        Ok(r) => r,
                        Err(e) => {
                            return Some((
                                Err(anyhow::anyhow!("poll failed: {e}")),
                                (client, url, last_seen),
                            ));
                        }
                    };

                    let json: serde_json::Value = match resp.json().await {
                        Ok(j) => j,
                        Err(e) => {
                            return Some((
                                Err(anyhow::anyhow!("poll parse failed: {e}")),
                                (client, url, last_seen),
                            ));
                        }
                    };

                    if let Some(hex) = json.get("result").and_then(|v| v.as_str()) {
                        if let Ok(num) = parse_hex_u64(hex) {
                            if last_seen.map_or(true, |last| num > last) {
                                return Some((
                                    Ok(NewBlockNotification {
                                        number: num,
                                        hash: B256::ZERO, // Hash not available from eth_blockNumber
                                    }),
                                    (client, url, Some(num)),
                                ));
                            }
                        }
                    }
                }
            },
        );

        Box::pin(stream)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_partial_json, method};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn make_provider(url: &str) -> RpcProvider {
        RpcProvider::new(
            url.to_string(),
            None,
            Duration::from_secs(5),
            Duration::from_secs(1),
            None,
        )
    }

    #[tokio::test]
    async fn test_rpc_chain_identity() {
        let mock_server = MockServer::start().await;

        // Mock eth_chainId
        Mock::given(method("POST"))
            .and(body_partial_json(serde_json::json!({"method": "eth_chainId"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": "0x1"
            })))
            .mount(&mock_server)
            .await;

        // Mock web3_clientVersion
        Mock::given(method("POST"))
            .and(body_partial_json(serde_json::json!({"method": "web3_clientVersion"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": "Geth/v1.13.0-stable/linux-amd64/go1.21.0"
            })))
            .mount(&mock_server)
            .await;

        let provider = make_provider(&mock_server.uri());
        let identity = provider.chain_identity().await.unwrap();

        assert_eq!(identity.chain_id, 1);
        assert_eq!(identity.chain_kind, ChainKind::Ethereum);
        assert!(identity.client_version.contains("Geth"));
    }

    #[tokio::test]
    async fn test_rpc_trace_request_format() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(body_partial_json(serde_json::json!({
                "method": "debug_traceBlockByNumber",
                "params": ["0xa", {"tracer": "prestateTracer", "tracerConfig": {"diffMode": true}}]
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": [
                    {
                        "txHash": "0xaaaa000000000000000000000000000000000000000000000000000000000000",
                        "result": {
                            "pre": {
                                "0x1111111111111111111111111111111111111111": {
                                    "balance": "0x1000"
                                }
                            },
                            "post": {
                                "0x1111111111111111111111111111111111111111": {
                                    "balance": "0x0f00"
                                }
                            }
                        }
                    }
                ]
            })))
            .mount(&mock_server)
            .await;

        let provider = make_provider(&mock_server.uri());
        let traces = provider.trace_block_prestate_diff(10).await.unwrap();

        assert_eq!(traces.len(), 1);
        assert!(traces[0].tx_hash.is_some());
    }

    #[tokio::test]
    async fn test_rpc_get_block() {
        let mock_server = MockServer::start().await;

        // Mock eth_getBlockByNumber
        Mock::given(method("POST"))
            .and(body_partial_json(serde_json::json!({"method": "eth_getBlockByNumber"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": {
                    "number": "0x5",
                    "hash": "0x0000000000000000000000000000000000000000000000000000000000000005",
                    "parentHash": "0x0000000000000000000000000000000000000000000000000000000000000004",
                    "timestamp": "0x64",
                    "transactions": [
                        {
                            "hash": "0xaaaa000000000000000000000000000000000000000000000000000000000000",
                            "from": "0x1111111111111111111111111111111111111111",
                            "to": "0x2222222222222222222222222222222222222222",
                            "type": "0x02",
                            "nonce": "0x0a"
                        }
                    ]
                }
            })))
            .mount(&mock_server)
            .await;

        // Mock eth_getTransactionReceipt
        Mock::given(method("POST"))
            .and(body_partial_json(serde_json::json!({"method": "eth_getTransactionReceipt"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": {
                    "transactionHash": "0xaaaa000000000000000000000000000000000000000000000000000000000000",
                    "gasUsed": "0x5208",
                    "status": "0x1"
                }
            })))
            .mount(&mock_server)
            .await;

        let provider = make_provider(&mock_server.uri());
        let block = provider.get_block(5).await.unwrap();

        assert_eq!(block.number, 5);
        assert_eq!(block.timestamp, 100);
        assert_eq!(block.transactions.len(), 1);
        assert_eq!(block.receipts.len(), 1);
        assert_eq!(
            block.transactions[0].from,
            "0x1111111111111111111111111111111111111111"
                .parse::<alloy_primitives::Address>()
                .unwrap()
        );
    }

    #[tokio::test]
    async fn test_rpc_error_handling() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "error": {
                    "code": -32601,
                    "message": "Method not found"
                }
            })))
            .mount(&mock_server)
            .await;

        let provider = make_provider(&mock_server.uri());
        let result = provider.latest_block_number().await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("RPC error"));
    }

    #[tokio::test]
    async fn test_rpc_timeout() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": 1,
                        "result": "0x1"
                    }))
                    .set_delay(Duration::from_secs(10)),
            )
            .mount(&mock_server)
            .await;

        // 1 second timeout
        let provider = RpcProvider::new(
            mock_server.uri(),
            None,
            Duration::from_secs(1),
            Duration::from_secs(1),
            None,
        );

        let result = provider.latest_block_number().await;
        assert!(result.is_err());
    }
}
