use alloy_consensus::{Transaction as _, Typed2718 as _};
use alloy_primitives::B256;
use anyhow::{Context, Result};
use async_trait::async_trait;
use futures::stream::BoxStream;
use std::time::Duration;
use tokio::sync::RwLock;

use crate::detect_chain;
use crate::types::{BlockEnvelope, ChainIdentity, NewBlockNotification, RpcReceipt, RpcTransaction};
use txviz_core::model::ChainKind;

#[async_trait]
pub trait ChainProvider: Send + Sync {
    async fn chain_identity(&self) -> Result<ChainIdentity>;
    async fn latest_block_number(&self) -> Result<u64>;
    async fn get_block(&self, number: u64) -> Result<BlockEnvelope>;
    async fn trace_block_prestate_diff(&self, number: u64) -> Result<serde_json::Value>;
    async fn trace_block_prestate(&self, number: u64) -> Result<serde_json::Value>;
    fn subscribe_new_blocks(&self) -> BoxStream<'static, Result<NewBlockNotification>>;
}

enum ProviderInner {
    Ethereum(alloy_provider::RootProvider<alloy_network::Ethereum>),
    Tempo(alloy_provider::RootProvider<tempo_alloy::TempoNetwork>),
}

pub struct RpcProvider {
    inner: RwLock<ProviderInner>,
    http_url: String,
    timeout: Duration,
    poll_interval: Duration,
    force_chain: Option<ChainKind>,
}

fn make_eth_provider(
    http_url: &str,
    timeout: Duration,
) -> alloy_provider::RootProvider<alloy_network::Ethereum> {
    let url: reqwest::Url = http_url.parse().expect("invalid RPC URL");
    let client = reqwest::Client::builder()
        .timeout(timeout)
        .build()
        .expect("failed to build HTTP client");
    let transport = alloy_transport_http::Http::with_client(client, url);
    let rpc_client = alloy_rpc_client::RpcClient::new(transport, false);
    alloy_provider::RootProvider::new(rpc_client)
}

fn make_tempo_provider(
    http_url: &str,
    timeout: Duration,
) -> alloy_provider::RootProvider<tempo_alloy::TempoNetwork> {
    let url: reqwest::Url = http_url.parse().expect("invalid RPC URL");
    let client = reqwest::Client::builder()
        .timeout(timeout)
        .build()
        .expect("failed to build HTTP client");
    let transport = alloy_transport_http::Http::with_client(client, url);
    let rpc_client = alloy_rpc_client::RpcClient::new(transport, false);
    alloy_provider::RootProvider::new(rpc_client)
}

impl RpcProvider {
    pub fn new(
        http_url: String,
        _ws_url: Option<String>,
        timeout: Duration,
        poll_interval: Duration,
        force_chain: Option<ChainKind>,
    ) -> Self {
        let inner = if force_chain == Some(ChainKind::Tempo) {
            ProviderInner::Tempo(make_tempo_provider(&http_url, timeout))
        } else {
            ProviderInner::Ethereum(make_eth_provider(&http_url, timeout))
        };
        Self {
            inner: RwLock::new(inner),
            http_url,
            timeout,
            poll_interval,
            force_chain,
        }
    }
}

#[async_trait]
impl ChainProvider for RpcProvider {
    async fn chain_identity(&self) -> Result<ChainIdentity> {
        use alloy_provider::Provider;

        let (chain_id, client_version) = {
            let inner = self.inner.read().await;
            let chain_id = match &*inner {
                ProviderInner::Ethereum(p) => p.get_chain_id().await,
                ProviderInner::Tempo(p) => p.get_chain_id().await,
            }
            .context("failed to get chain ID")?;

            let client_version = match &*inner {
                ProviderInner::Ethereum(p) => p.get_client_version().await,
                ProviderInner::Tempo(p) => p.get_client_version().await,
            }
            .unwrap_or_else(|_| "unknown".to_string());

            (chain_id, client_version)
        };

        let chain_kind = detect_chain(chain_id, &client_version, self.force_chain);

        if chain_kind == ChainKind::Tempo {
            let mut inner = self.inner.write().await;
            if matches!(*inner, ProviderInner::Ethereum(_)) {
                *inner = ProviderInner::Tempo(make_tempo_provider(&self.http_url, self.timeout));
            }
        }

        Ok(ChainIdentity {
            chain_id,
            chain_kind,
            client_version,
        })
    }

    async fn latest_block_number(&self) -> Result<u64> {
        use alloy_provider::Provider;

        let inner = self.inner.read().await;
        match &*inner {
            ProviderInner::Ethereum(p) => p.get_block_number().await,
            ProviderInner::Tempo(p) => p.get_block_number().await,
        }
        .context("failed to get block number")
    }

    async fn get_block(&self, number: u64) -> Result<BlockEnvelope> {
        let inner = self.inner.read().await;
        match &*inner {
            ProviderInner::Ethereum(p) => get_block_ethereum(p, number).await,
            ProviderInner::Tempo(p) => get_block_tempo(p, number).await,
        }
    }

    async fn trace_block_prestate_diff(&self, number: u64) -> Result<serde_json::Value> {
        use alloy_provider::ext::DebugApi;
        use alloy_rpc_types_trace::geth::{GethDebugTracingOptions, PreStateConfig};

        let opts = GethDebugTracingOptions::prestate_tracer(PreStateConfig {
            diff_mode: Some(true),
            disable_code: None,
            disable_storage: None,
        });

        let inner = self.inner.read().await;
        let results = match &*inner {
            ProviderInner::Ethereum(p) => {
                p.debug_trace_block_by_number(alloy_eips::BlockNumberOrTag::Number(number), opts)
                    .await
            }
            ProviderInner::Tempo(p) => {
                p.debug_trace_block_by_number(alloy_eips::BlockNumberOrTag::Number(number), opts)
                    .await
            }
        }
        .context("failed to fetch prestate diff traces")?;

        serde_json::to_value(&results).context("failed to serialize trace results")
    }

    async fn trace_block_prestate(&self, number: u64) -> Result<serde_json::Value> {
        use alloy_provider::ext::DebugApi;
        use alloy_rpc_types_trace::geth::{GethDebugTracingOptions, PreStateConfig};

        let opts = GethDebugTracingOptions::prestate_tracer(PreStateConfig {
            diff_mode: Some(false),
            disable_code: None,
            disable_storage: None,
        });

        let inner = self.inner.read().await;
        let results = match &*inner {
            ProviderInner::Ethereum(p) => {
                p.debug_trace_block_by_number(alloy_eips::BlockNumberOrTag::Number(number), opts)
                    .await
            }
            ProviderInner::Tempo(p) => {
                p.debug_trace_block_by_number(alloy_eips::BlockNumberOrTag::Number(number), opts)
                    .await
            }
        }
        .context("failed to fetch prestate traces")?;

        serde_json::to_value(&results).context("failed to serialize trace results")
    }

    fn subscribe_new_blocks(&self) -> BoxStream<'static, Result<NewBlockNotification>> {
        let http_url = self.http_url.clone();
        let poll_interval = self.poll_interval;

        let stream = futures::stream::unfold(
            (http_url, None::<u64>),
            move |(url, last_seen)| async move {
                use alloy_provider::Provider;

                loop {
                    tokio::time::sleep(poll_interval).await;

                    let prov_url: reqwest::Url = url.parse().expect("invalid RPC URL");
                    let prov: alloy_provider::RootProvider =
                        alloy_provider::RootProvider::new_http(prov_url);

                    match prov.get_block_number().await {
                        Ok(num) => {
                            if last_seen.map_or(true, |last| num > last) {
                                return Some((
                                    Ok(NewBlockNotification {
                                        number: num,
                                        hash: B256::ZERO,
                                    }),
                                    (url, Some(num)),
                                ));
                            }
                        }
                        Err(e) => {
                            return Some((
                                Err(anyhow::anyhow!("poll failed: {e}")),
                                (url, last_seen),
                            ));
                        }
                    }
                }
            },
        );

        Box::pin(stream)
    }
}

async fn get_block_ethereum(
    provider: &alloy_provider::RootProvider<alloy_network::Ethereum>,
    number: u64,
) -> Result<BlockEnvelope> {
    use alloy_network_primitives::TransactionResponse;
    use alloy_provider::Provider;

    let block = provider
        .get_block_by_number(alloy_eips::BlockNumberOrTag::Number(number))
        .full()
        .await
        .context("failed to fetch block")?
        .ok_or_else(|| anyhow::anyhow!("block {number} not found"))?;

    let block_hash = block.header.hash;
    let parent_hash = block.header.parent_hash;
    let timestamp = block.header.timestamp;
    let beneficiary = block.header.beneficiary;

    let transactions: Vec<RpcTransaction> = block
        .transactions
        .txns()
        .map(|tx| RpcTransaction {
            hash: tx.tx_hash(),
            from: tx.from(),
            to: tx.to(),
            tx_type: Some(format!("0x{:02x}", tx.inner.ty())),
            nonce: Some(serde_json::json!(format!("0x{:x}", tx.inner.nonce()))),
            gas: Some(format!("0x{:x}", tx.inner.gas_limit())),
            value: Some(format!("0x{:x}", tx.inner.value())),
            input: Some(format!(
                "0x{}",
                alloy_primitives::hex::encode(tx.inner.input())
            )),
            nonce_key: None,
            calls: None,
            fee_token: None,
        })
        .collect();

    let receipts_result = provider
        .get_block_receipts(alloy_eips::BlockId::number(number))
        .await
        .context("failed to fetch block receipts")?
        .unwrap_or_default();

    let receipts: Vec<RpcReceipt> = receipts_result
        .iter()
        .map(|r| RpcReceipt {
            transaction_hash: r.transaction_hash,
            gas_used: Some(format!("0x{:x}", r.gas_used)),
            status: Some(if r.status() {
                "0x1".to_string()
            } else {
                "0x0".to_string()
            }),
        })
        .collect();

    Ok(BlockEnvelope {
        number,
        hash: block_hash,
        parent_hash,
        timestamp,
        beneficiary,
        transactions,
        receipts,
    })
}

async fn get_block_tempo(
    provider: &alloy_provider::RootProvider<tempo_alloy::TempoNetwork>,
    number: u64,
) -> Result<BlockEnvelope> {
    use alloy_consensus::BlockHeader;
    use alloy_network::ReceiptResponse;
    use alloy_network_primitives::{HeaderResponse, TransactionResponse};
    use alloy_provider::Provider;

    let block = provider
        .get_block_by_number(alloy_eips::BlockNumberOrTag::Number(number))
        .full()
        .await
        .context("failed to fetch block")?
        .ok_or_else(|| anyhow::anyhow!("block {number} not found"))?;

    let block_hash = block.header.hash();
    let parent_hash = block.header.parent_hash();
    let timestamp = block.header.timestamp();
    let beneficiary = block.header.beneficiary();

    let transactions: Vec<RpcTransaction> = block
        .transactions
        .txns()
        .map(|tx| {
            let hash = tx.tx_hash();
            let from = tx.from();
            let to = tx.to();
            let ty = tx.inner.ty();
            let nonce_val = tx.inner.nonce();
            let gas = tx.inner.gas_limit();
            let value = tx.inner.value();
            let input_data = tx.inner.input();

            let (nonce_key, calls, fee_token) = match &*tx.inner {
                tempo_primitives::TempoTxEnvelope::AA(signed) => {
                    let tempo_tx = signed.tx();
                    let nonce_key = Some(format!("0x{:x}", tempo_tx.nonce_key));
                    let fee_token = tempo_tx.fee_token.map(|addr| format!("{addr:?}"));
                    let calls: Option<Vec<serde_json::Value>> = if !tempo_tx.calls.is_empty() {
                        Some(
                            tempo_tx
                                .calls
                                .iter()
                                .map(|call| {
                                    serde_json::json!({
                                        "to": format!("{:?}", call.to),
                                        "value": format!("0x{:x}", call.value),
                                        "input": format!("0x{}", alloy_primitives::hex::encode(&call.input)),
                                    })
                                })
                                .collect(),
                        )
                    } else {
                        None
                    };
                    (nonce_key, calls, fee_token)
                }
                _ => (None, None, None),
            };

            RpcTransaction {
                hash,
                from,
                to,
                tx_type: Some(format!("0x{:02x}", ty)),
                nonce: Some(serde_json::json!(format!("0x{:x}", nonce_val))),
                gas: Some(format!("0x{:x}", gas)),
                value: Some(format!("0x{:x}", value)),
                input: Some(format!(
                    "0x{}",
                    alloy_primitives::hex::encode(input_data)
                )),
                nonce_key,
                calls,
                fee_token,
            }
        })
        .collect();

    let receipts_result = provider
        .get_block_receipts(alloy_eips::BlockId::number(number))
        .await
        .context("failed to fetch block receipts")?
        .unwrap_or_default();

    let receipts: Vec<RpcReceipt> = receipts_result
        .iter()
        .map(|r| RpcReceipt {
            transaction_hash: r.transaction_hash(),
            gas_used: Some(format!("0x{:x}", r.gas_used())),
            status: Some(if r.status() {
                "0x1".to_string()
            } else {
                "0x0".to_string()
            }),
        })
        .collect();

    Ok(BlockEnvelope {
        number,
        hash: block_hash,
        parent_hash,
        timestamp,
        beneficiary,
        transactions,
        receipts,
    })
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
            .and(body_partial_json(
                serde_json::json!({"method": "eth_chainId"}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": "0x1"
            })))
            .mount(&mock_server)
            .await;

        // Mock web3_clientVersion
        Mock::given(method("POST"))
            .and(body_partial_json(
                serde_json::json!({"method": "web3_clientVersion"}),
            ))
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
            .and(body_partial_json(
                serde_json::json!({"method": "debug_traceBlockByNumber"}),
            ))
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

        let arr = traces.as_array().expect("traces should be an array");
        assert_eq!(arr.len(), 1);
    }

    #[tokio::test]
    async fn test_rpc_get_block() {
        let mock_server = MockServer::start().await;

        // Mock eth_getBlockByNumber
        Mock::given(method("POST"))
            .and(body_partial_json(
                serde_json::json!({"method": "eth_getBlockByNumber"}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": {
                    "number": "0x5",
                    "hash": "0x0000000000000000000000000000000000000000000000000000000000000005",
                    "parentHash": "0x0000000000000000000000000000000000000000000000000000000000000004",
                    "timestamp": "0x64",
                    "miner": "0x0000000000000000000000000000000000000000",
                    "gasLimit": "0x1c9c380",
                    "gasUsed": "0x5208",
                    "baseFeePerGas": "0x3b9aca00",
                    "difficulty": "0x0",
                    "totalDifficulty": "0x0",
                    "extraData": "0x",
                    "logsBloom": "0x00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
                    "receiptsRoot": "0x0000000000000000000000000000000000000000000000000000000000000000",
                    "sha3Uncles": "0x1dcc4de8dec75d7aab85b567b6ccd41ad312451b948a7413f0a142fd40d49347",
                    "size": "0x100",
                    "stateRoot": "0x0000000000000000000000000000000000000000000000000000000000000000",
                    "transactionsRoot": "0x0000000000000000000000000000000000000000000000000000000000000000",
                    "uncles": [],
                    "nonce": "0x0000000000000000",
                    "mixHash": "0x0000000000000000000000000000000000000000000000000000000000000000",
                    "transactions": [
                        {
                            "hash": "0xaaaa000000000000000000000000000000000000000000000000000000000000",
                            "from": "0x1111111111111111111111111111111111111111",
                            "to": "0x2222222222222222222222222222222222222222",
                            "type": "0x02",
                            "nonce": "0x0a",
                            "blockHash": "0x0000000000000000000000000000000000000000000000000000000000000005",
                            "blockNumber": "0x5",
                            "transactionIndex": "0x0",
                            "gas": "0x5208",
                            "gasPrice": "0x3b9aca00",
                            "maxFeePerGas": "0x3b9aca00",
                            "maxPriorityFeePerGas": "0x0",
                            "value": "0x0",
                            "input": "0x",
                            "v": "0x0",
                            "r": "0x0000000000000000000000000000000000000000000000000000000000000000",
                            "s": "0x0000000000000000000000000000000000000000000000000000000000000000",
                            "chainId": "0x1",
                            "accessList": [],
                            "yParity": "0x0"
                        }
                    ]
                }
            })))
            .mount(&mock_server)
            .await;

        // Mock eth_getBlockReceipts
        Mock::given(method("POST"))
            .and(body_partial_json(
                serde_json::json!({"method": "eth_getBlockReceipts"}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": [
                    {
                        "transactionHash": "0xaaaa000000000000000000000000000000000000000000000000000000000000",
                        "transactionIndex": "0x0",
                        "blockHash": "0x0000000000000000000000000000000000000000000000000000000000000005",
                        "blockNumber": "0x5",
                        "gasUsed": "0x5208",
                        "cumulativeGasUsed": "0x5208",
                        "effectiveGasPrice": "0x3b9aca00",
                        "from": "0x1111111111111111111111111111111111111111",
                        "to": "0x2222222222222222222222222222222222222222",
                        "contractAddress": null,
                        "logs": [],
                        "logsBloom": "0x00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
                        "status": "0x1",
                        "type": "0x02"
                    }
                ]
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
