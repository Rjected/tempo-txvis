use std::collections::HashSet;

use alloy_primitives::{Address, B256};
use anyhow::{Context, Result};
use serde_json::Value;

use crate::model::{StateKey, TxStateAccess};

/// Parse diff-mode prestate traces into per-tx write sets.
///
/// Accepts both wrapper formats:
/// - `[{ "txHash": "0x...", "result": { "pre": {...}, "post": {...} } }, ...]`
/// - `[{ "pre": {...}, "post": {...} }, ...]`
pub fn parse_diff_traces(json: &Value) -> Result<Vec<TxStateAccess>> {
    let arr = json.as_array().context("diff traces must be a JSON array")?;
    let mut result = Vec::with_capacity(arr.len());

    for (idx, entry) in arr.iter().enumerate() {
        // Skip entries with an "error" field
        if entry.get("error").is_some() {
            continue;
        }

        // Unwrap wrapper format: if "result" key exists, use it; otherwise use entry directly
        let diff = if let Some(r) = entry.get("result") {
            r
        } else {
            entry
        };

        let pre = diff.get("pre").and_then(Value::as_object);
        let post = diff.get("post").and_then(Value::as_object);

        let mut access = TxStateAccess {
            tx_index: idx as u32,
            reads: HashSet::new(),
            writes: HashSet::new(),
        };

        // Process post entries
        if let Some(post_map) = post {
            for (addr_str, post_acct) in post_map {
                let addr: Address = addr_str.parse().context("invalid address in post")?;
                let pre_acct = pre.and_then(|p| p.get(addr_str));

                // Check balance change
                if let Some(post_bal) = post_acct.get("balance") {
                    let pre_bal = pre_acct.and_then(|a| a.get("balance"));
                    if pre_bal != Some(post_bal) {
                        access.writes.insert(StateKey::Balance(addr));
                    }
                }

                // Check nonce change
                if let Some(post_nonce) = post_acct.get("nonce") {
                    let pre_nonce = pre_acct.and_then(|a| a.get("nonce"));
                    if pre_nonce != Some(post_nonce) {
                        access.writes.insert(StateKey::Nonce(addr));
                    }
                }

                // Check code change
                if let Some(post_code) = post_acct.get("code") {
                    let pre_code = pre_acct.and_then(|a| a.get("code"));
                    if pre_code != Some(post_code) {
                        access.writes.insert(StateKey::Code(addr));
                    }
                }

                // Check storage slots in post
                if let Some(post_storage) = post_acct.get("storage").and_then(Value::as_object) {
                    let pre_storage = pre_acct
                        .and_then(|a| a.get("storage"))
                        .and_then(Value::as_object);
                    for (slot_str, post_val) in post_storage {
                        let slot: B256 = slot_str.parse().context("invalid storage slot")?;
                        let pre_val = pre_storage.and_then(|s| s.get(slot_str));
                        if pre_val != Some(post_val) {
                            access.writes.insert(StateKey::Storage(addr, slot));
                        }
                    }
                }

                // If addr not in pre, it's a creation — mark balance/nonce as writes too
                if pre_acct.is_none() {
                    access.writes.insert(StateKey::Balance(addr));
                    access.writes.insert(StateKey::Nonce(addr));
                }
            }
        }

        // Process pre entries not in post (selfdestruct/deletion)
        if let Some(pre_map) = pre {
            let post_map = post;
            for (addr_str, pre_acct) in pre_map {
                if post_map.map_or(true, |p| !p.contains_key(addr_str)) {
                    let addr: Address = addr_str.parse().context("invalid address in pre")?;
                    // Selfdestruct: write to balance, code, and all storage slots
                    access.writes.insert(StateKey::Balance(addr));
                    if pre_acct.get("nonce").is_some() {
                        access.writes.insert(StateKey::Nonce(addr));
                    }
                    if pre_acct.get("code").is_some() {
                        access.writes.insert(StateKey::Code(addr));
                    }
                    if let Some(storage) = pre_acct.get("storage").and_then(Value::as_object) {
                        for slot_str in storage.keys() {
                            let slot: B256 = slot_str.parse().context("invalid storage slot")?;
                            access.writes.insert(StateKey::Storage(addr, slot));
                        }
                    }
                }
            }
        }

        result.push(access);
    }

    Ok(result)
}

/// Parse prestate (non-diff) traces into per-tx touched sets.
///
/// For each tx result (map of addr → account state):
/// - Every addr present → touched Balance(addr), Nonce(addr)
/// - If code present → touched Code(addr)
/// - Every slot in storage → touched Storage(addr, slot)
pub fn parse_prestate_traces(json: &Value) -> Result<Vec<TxStateAccess>> {
    let arr = json.as_array().context("prestate traces must be a JSON array")?;
    let mut result = Vec::with_capacity(arr.len());

    for (idx, entry) in arr.iter().enumerate() {
        // Skip entries with an "error" field
        if entry.get("error").is_some() {
            continue;
        }

        // Unwrap wrapper format
        let state_map = if let Some(r) = entry.get("result") {
            r
        } else {
            entry
        };

        let mut access = TxStateAccess {
            tx_index: idx as u32,
            reads: HashSet::new(),
            writes: HashSet::new(),
        };

        // In prestate mode, everything goes into "reads" (touched set).
        // We'll subtract writes later in extract_state_access.
        if let Some(obj) = state_map.as_object() {
            for (addr_str, acct) in obj {
                let addr: Address = addr_str.parse().context("invalid address in prestate")?;
                access.reads.insert(StateKey::Balance(addr));
                access.reads.insert(StateKey::Nonce(addr));

                if acct.get("code").is_some() {
                    access.reads.insert(StateKey::Code(addr));
                }

                if let Some(storage) = acct.get("storage").and_then(Value::as_object) {
                    for slot_str in storage.keys() {
                        let slot: B256 = slot_str.parse().context("invalid storage slot")?;
                        access.reads.insert(StateKey::Storage(addr, slot));
                    }
                }
            }
        }

        result.push(access);
    }

    Ok(result)
}

/// Combine diff traces (writes) and prestate traces (touched) to produce
/// accurate read and write sets. Read = touched - writes.
///
/// If prestate_traces is None, use conservative approximation:
/// treat everything in `pre` as a read (in addition to writes from diff).
pub fn extract_state_access(
    diff_traces: &Value,
    prestate_traces: Option<&Value>,
) -> Result<Vec<TxStateAccess>> {
    let mut diff_accesses = parse_diff_traces(diff_traces)?;

    if let Some(prestate_json) = prestate_traces {
        let touched_accesses = parse_prestate_traces(prestate_json)?;

        // Merge: writes come from diff, reads = touched - writes
        for (diff_acc, touched_acc) in diff_accesses.iter_mut().zip(touched_accesses.iter()) {
            let reads: HashSet<StateKey> = touched_acc
                .reads
                .difference(&diff_acc.writes)
                .cloned()
                .collect();
            diff_acc.reads = reads;
        }
    } else {
        // Conservative: treat pre entries as reads
        let arr = diff_traces
            .as_array()
            .context("diff traces must be a JSON array")?;

        for (idx, entry) in arr.iter().enumerate() {
            if entry.get("error").is_some() {
                continue;
            }

            let diff = if let Some(r) = entry.get("result") {
                r
            } else {
                entry
            };

            if let Some(pre_map) = diff.get("pre").and_then(Value::as_object) {
                // Find the corresponding diff_access entry
                if let Some(acc) = diff_accesses.iter_mut().find(|a| a.tx_index == idx as u32) {
                    for (addr_str, acct) in pre_map {
                        let addr: Address = addr_str.parse().context("invalid address")?;
                        // Add as reads (will include writes too, but that's conservative)
                        if acct.get("balance").is_some() {
                            acc.reads.insert(StateKey::Balance(addr));
                        }
                        if acct.get("nonce").is_some() {
                            acc.reads.insert(StateKey::Nonce(addr));
                        }
                        if acct.get("code").is_some() {
                            acc.reads.insert(StateKey::Code(addr));
                        }
                        if let Some(storage) = acct.get("storage").and_then(Value::as_object) {
                            for slot_str in storage.keys() {
                                let slot: B256 = slot_str.parse().context("invalid slot")?;
                                acc.reads.insert(StateKey::Storage(addr, slot));
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(diff_accesses)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture_diff_traces() -> Value {
        json!([
            {
                "txHash": "0xaaaa000000000000000000000000000000000000000000000000000000000000",
                "result": {
                    "pre": {
                        "0x1111111111111111111111111111111111111111": {
                            "balance": "0x1000000000000000000",
                            "nonce": 10
                        },
                        "0xcccccccccccccccccccccccccccccccccccccccc": {
                            "storage": {
                                "0x0000000000000000000000000000000000000000000000000000000000000001": "0x00000000000000000000000000000000000000000000000000000000000003e8"
                            }
                        }
                    },
                    "post": {
                        "0x1111111111111111111111111111111111111111": {
                            "balance": "0x0f00000000000000000",
                            "nonce": 11
                        },
                        "0xcccccccccccccccccccccccccccccccccccccccc": {
                            "storage": {
                                "0x0000000000000000000000000000000000000000000000000000000000000001": "0x00000000000000000000000000000000000000000000000000000000000002d0"
                            }
                        }
                    }
                }
            },
            {
                "txHash": "0xbbbb000000000000000000000000000000000000000000000000000000000000",
                "result": {
                    "pre": {
                        "0x2222222222222222222222222222222222222222": {
                            "balance": "0x2000000000000000000",
                            "nonce": 0
                        }
                    },
                    "post": {
                        "0x2222222222222222222222222222222222222222": {
                            "balance": "0x1f00000000000000000",
                            "nonce": 1
                        },
                        "0x4444444444444444444444444444444444444444": {
                            "balance": "0x0",
                            "nonce": 1,
                            "code": "0x6080604052",
                            "storage": {
                                "0x0000000000000000000000000000000000000000000000000000000000000000": "0x0000000000000000000000000000000000000000000000000000000000000001"
                            }
                        }
                    }
                }
            },
            {
                "txHash": "0xcccc000000000000000000000000000000000000000000000000000000000000",
                "result": {
                    "pre": {
                        "0x1111111111111111111111111111111111111111": {
                            "balance": "0x0f00000000000000000",
                            "nonce": 11
                        },
                        "0x3333333333333333333333333333333333333333": {
                            "balance": "0x0500000000000000000"
                        }
                    },
                    "post": {
                        "0x1111111111111111111111111111111111111111": {
                            "balance": "0x0e00000000000000000",
                            "nonce": 12
                        },
                        "0x3333333333333333333333333333333333333333": {
                            "balance": "0x0600000000000000000"
                        }
                    }
                }
            }
        ])
    }

    #[test]
    fn test_parse_diff_traces_simple_3tx() {
        let traces = fixture_diff_traces();
        let result = parse_diff_traces(&traces).unwrap();
        assert_eq!(result.len(), 3);

        let addr_1111: Address = "0x1111111111111111111111111111111111111111".parse().unwrap();
        let addr_cccc: Address = "0xcccccccccccccccccccccccccccccccccccccccc".parse().unwrap();
        let addr_2222: Address = "0x2222222222222222222222222222222222222222".parse().unwrap();
        let addr_4444: Address = "0x4444444444444444444444444444444444444444".parse().unwrap();
        let addr_3333: Address = "0x3333333333333333333333333333333333333333".parse().unwrap();
        let slot_0: B256 = "0x0000000000000000000000000000000000000000000000000000000000000000"
            .parse()
            .unwrap();
        let slot_1: B256 = "0x0000000000000000000000000000000000000000000000000000000000000001"
            .parse()
            .unwrap();

        // tx0: writes Balance(0x1111), Nonce(0x1111), Storage(0xcccc, slot1)
        let tx0 = &result[0];
        assert_eq!(tx0.tx_index, 0);
        assert!(tx0.writes.contains(&StateKey::Balance(addr_1111)));
        assert!(tx0.writes.contains(&StateKey::Nonce(addr_1111)));
        assert!(tx0.writes.contains(&StateKey::Storage(addr_cccc, slot_1)));
        assert_eq!(tx0.writes.len(), 3);

        // tx1: writes Balance(0x2222), Nonce(0x2222), and creates 0x4444
        // 0x4444 is in post but not pre → Balance, Nonce, Code, Storage
        let tx1 = &result[1];
        assert_eq!(tx1.tx_index, 1);
        assert!(tx1.writes.contains(&StateKey::Balance(addr_2222)));
        assert!(tx1.writes.contains(&StateKey::Nonce(addr_2222)));
        assert!(tx1.writes.contains(&StateKey::Balance(addr_4444)));
        assert!(tx1.writes.contains(&StateKey::Nonce(addr_4444)));
        assert!(tx1.writes.contains(&StateKey::Code(addr_4444)));
        assert!(tx1.writes.contains(&StateKey::Storage(addr_4444, slot_0)));
        assert_eq!(tx1.writes.len(), 6);

        // tx2: writes Balance(0x1111), Nonce(0x1111), Balance(0x3333)
        let tx2 = &result[2];
        assert_eq!(tx2.tx_index, 2);
        assert!(tx2.writes.contains(&StateKey::Balance(addr_1111)));
        assert!(tx2.writes.contains(&StateKey::Nonce(addr_1111)));
        assert!(tx2.writes.contains(&StateKey::Balance(addr_3333)));
        assert_eq!(tx2.writes.len(), 3);
    }

    #[test]
    fn test_parse_diff_traces_empty() {
        let traces = json!([]);
        let result = parse_diff_traces(&traces).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_diff_traces_with_error() {
        let traces = json!([
            {
                "txHash": "0xaaaa000000000000000000000000000000000000000000000000000000000000",
                "error": "execution reverted",
                "result": {}
            },
            {
                "txHash": "0xbbbb000000000000000000000000000000000000000000000000000000000000",
                "result": {
                    "pre": {},
                    "post": {}
                }
            }
        ]);
        let result = parse_diff_traces(&traces).unwrap();
        // First entry skipped due to error, second parsed
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].tx_index, 1);
    }

    #[test]
    fn test_parse_nonce_formats() {
        // nonce as JSON number
        let traces_num = json!([{
            "pre": {
                "0x1111111111111111111111111111111111111111": {
                    "nonce": 5
                }
            },
            "post": {
                "0x1111111111111111111111111111111111111111": {
                    "nonce": 6
                }
            }
        }]);
        let result = parse_diff_traces(&traces_num).unwrap();
        assert_eq!(result.len(), 1);
        let addr: Address = "0x1111111111111111111111111111111111111111".parse().unwrap();
        assert!(result[0].writes.contains(&StateKey::Nonce(addr)));

        // nonce as hex string
        let traces_hex = json!([{
            "pre": {
                "0x1111111111111111111111111111111111111111": {
                    "nonce": "0x05"
                }
            },
            "post": {
                "0x1111111111111111111111111111111111111111": {
                    "nonce": "0x06"
                }
            }
        }]);
        let result2 = parse_diff_traces(&traces_hex).unwrap();
        assert_eq!(result2.len(), 1);
        assert!(result2[0].writes.contains(&StateKey::Nonce(addr)));
    }

    #[test]
    fn test_parse_selfdestruct() {
        let traces = json!([{
            "pre": {
                "0xdead000000000000000000000000000000000000": {
                    "balance": "0x100",
                    "nonce": 1,
                    "code": "0x6080",
                    "storage": {
                        "0x0000000000000000000000000000000000000000000000000000000000000001": "0xff"
                    }
                }
            },
            "post": {}
        }]);
        let result = parse_diff_traces(&traces).unwrap();
        assert_eq!(result.len(), 1);
        let addr: Address = "0xdead000000000000000000000000000000000000".parse().unwrap();
        let slot: B256 = "0x0000000000000000000000000000000000000000000000000000000000000001"
            .parse()
            .unwrap();
        assert!(result[0].writes.contains(&StateKey::Balance(addr)));
        assert!(result[0].writes.contains(&StateKey::Nonce(addr)));
        assert!(result[0].writes.contains(&StateKey::Code(addr)));
        assert!(result[0].writes.contains(&StateKey::Storage(addr, slot)));
    }

    #[test]
    fn test_parse_contract_creation() {
        let traces = json!([{
            "pre": {},
            "post": {
                "0x4444444444444444444444444444444444444444": {
                    "balance": "0x0",
                    "nonce": 1,
                    "code": "0x6080604052"
                }
            }
        }]);
        let result = parse_diff_traces(&traces).unwrap();
        assert_eq!(result.len(), 1);
        let addr: Address = "0x4444444444444444444444444444444444444444".parse().unwrap();
        assert!(result[0].writes.contains(&StateKey::Balance(addr)));
        assert!(result[0].writes.contains(&StateKey::Nonce(addr)));
        assert!(result[0].writes.contains(&StateKey::Code(addr)));
    }

    #[test]
    fn test_parse_both_json_formats() {
        // Format 1: with txHash wrapper
        let format1 = json!([{
            "txHash": "0xaaaa000000000000000000000000000000000000000000000000000000000000",
            "result": {
                "pre": {
                    "0x1111111111111111111111111111111111111111": { "balance": "0x100" }
                },
                "post": {
                    "0x1111111111111111111111111111111111111111": { "balance": "0x200" }
                }
            }
        }]);

        // Format 2: without wrapper
        let format2 = json!([{
            "pre": {
                "0x1111111111111111111111111111111111111111": { "balance": "0x100" }
            },
            "post": {
                "0x1111111111111111111111111111111111111111": { "balance": "0x200" }
            }
        }]);

        let r1 = parse_diff_traces(&format1).unwrap();
        let r2 = parse_diff_traces(&format2).unwrap();
        assert_eq!(r1.len(), 1);
        assert_eq!(r2.len(), 1);
        assert_eq!(r1[0].writes, r2[0].writes);
    }

    #[test]
    fn test_parse_prestate_traces() {
        let traces = json!([
            {
                "result": {
                    "0x1111111111111111111111111111111111111111": {
                        "balance": "0x100",
                        "nonce": 5
                    },
                    "0xcccccccccccccccccccccccccccccccccccccccc": {
                        "code": "0x6080",
                        "storage": {
                            "0x0000000000000000000000000000000000000000000000000000000000000001": "0xff"
                        }
                    }
                }
            }
        ]);

        let result = parse_prestate_traces(&traces).unwrap();
        assert_eq!(result.len(), 1);

        let addr_1111: Address = "0x1111111111111111111111111111111111111111".parse().unwrap();
        let addr_cccc: Address = "0xcccccccccccccccccccccccccccccccccccccccc".parse().unwrap();
        let slot_1: B256 = "0x0000000000000000000000000000000000000000000000000000000000000001"
            .parse()
            .unwrap();

        // All addresses touched → Balance + Nonce
        assert!(result[0].reads.contains(&StateKey::Balance(addr_1111)));
        assert!(result[0].reads.contains(&StateKey::Nonce(addr_1111)));
        assert!(result[0].reads.contains(&StateKey::Balance(addr_cccc)));
        assert!(result[0].reads.contains(&StateKey::Nonce(addr_cccc)));
        assert!(result[0].reads.contains(&StateKey::Code(addr_cccc)));
        assert!(result[0].reads.contains(&StateKey::Storage(addr_cccc, slot_1)));
    }

    #[test]
    fn test_combine_diff_and_prestate() {
        let diff = json!([{
            "pre": {
                "0x1111111111111111111111111111111111111111": {
                    "balance": "0x100",
                    "nonce": 5
                }
            },
            "post": {
                "0x1111111111111111111111111111111111111111": {
                    "balance": "0x200",
                    "nonce": 6
                }
            }
        }]);

        // Prestate (touched): touched balance, nonce of 0x1111, and also 0x2222
        let prestate = json!([{
            "0x1111111111111111111111111111111111111111": {
                "balance": "0x100",
                "nonce": 5
            },
            "0x2222222222222222222222222222222222222222": {
                "balance": "0x500"
            }
        }]);

        let result = extract_state_access(&diff, Some(&prestate)).unwrap();
        assert_eq!(result.len(), 1);

        let addr_1111: Address = "0x1111111111111111111111111111111111111111".parse().unwrap();
        let addr_2222: Address = "0x2222222222222222222222222222222222222222".parse().unwrap();

        // Writes: Balance(0x1111), Nonce(0x1111)
        assert!(result[0].writes.contains(&StateKey::Balance(addr_1111)));
        assert!(result[0].writes.contains(&StateKey::Nonce(addr_1111)));

        // Reads = touched - writes = Balance(0x2222), Nonce(0x2222)
        // Balance(0x1111) and Nonce(0x1111) are in writes, so NOT in reads
        assert!(!result[0].reads.contains(&StateKey::Balance(addr_1111)));
        assert!(!result[0].reads.contains(&StateKey::Nonce(addr_1111)));
        assert!(result[0].reads.contains(&StateKey::Balance(addr_2222)));
        assert!(result[0].reads.contains(&StateKey::Nonce(addr_2222)));
    }
}
