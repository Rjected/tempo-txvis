use std::collections::BinaryHeap;

use anyhow::{bail, Result};

use crate::model::{DepEdge, ScheduleItem, TxNode};

/// Information about the critical path through the DAG.
#[derive(Debug, Clone)]
pub struct CriticalPathInfo {
    /// Length of the critical path in weight units.
    pub weight: u64,
    /// Earliest start time per tx index.
    pub est: Vec<u64>,
    /// Latest start time per tx index.
    pub lst: Vec<u64>,
}

/// Compute the parallel schedule and critical path for a block.
///
/// Returns the schedule items and critical path info.
pub fn compute(
    tx_nodes: &[TxNode],
    dep_edges: &[DepEdge],
) -> Result<(Vec<ScheduleItem>, CriticalPathInfo)> {
    let n = tx_nodes.len();

    if n == 0 {
        return Ok((
            Vec::new(),
            CriticalPathInfo {
                weight: 0,
                est: Vec::new(),
                lst: Vec::new(),
            },
        ));
    }

    // Build adjacency lists and in-degree
    let mut successors: Vec<Vec<usize>> = vec![vec![]; n];
    let mut predecessors: Vec<Vec<usize>> = vec![vec![]; n];
    let mut in_degree: Vec<usize> = vec![0; n];

    for edge in dep_edges {
        let from = edge.from_tx as usize;
        let to = edge.to_tx as usize;
        successors[from].push(to);
        predecessors[to].push(from);
        in_degree[to] += 1;
    }

    // Weights: gas_used, fallback to 1 if 0
    let w: Vec<u64> = tx_nodes
        .iter()
        .map(|n| if n.gas_used == 0 { 1 } else { n.gas_used })
        .collect();

    // Topological sort (Kahn's algorithm)
    let mut queue: Vec<usize> = Vec::new();
    let mut temp_in_degree = in_degree.clone();
    for i in 0..n {
        if temp_in_degree[i] == 0 {
            queue.push(i);
        }
    }

    let mut topo: Vec<usize> = Vec::with_capacity(n);
    let mut head = 0;
    while head < queue.len() {
        let v = queue[head];
        head += 1;
        topo.push(v);
        for &s in &successors[v] {
            temp_in_degree[s] -= 1;
            if temp_in_degree[s] == 0 {
                queue.push(s);
            }
        }
    }

    if topo.len() != n {
        bail!("cycle detected in dependency graph");
    }

    // Forward pass: earliest start time
    let mut est = vec![0u64; n];
    for &v in &topo {
        for &s in &successors[v] {
            let candidate = est[v] + w[v];
            if candidate > est[s] {
                est[s] = candidate;
            }
        }
    }

    // Critical path weight = max earliest finish time
    let critical_path_weight = (0..n).map(|v| est[v] + w[v]).max().unwrap_or(0);

    // Backward pass: latest start time
    let mut lst = vec![0u64; n];
    for i in 0..n {
        lst[i] = critical_path_weight - w[i];
    }
    for &v in topo.iter().rev() {
        for &s in &successors[v] {
            let candidate = lst[s] - w[v];
            if candidate < lst[v] {
                lst[v] = candidate;
            }
        }
    }

    // Compute rank for scheduling priority (longest path from v to any sink)
    let mut rank = vec![0u64; n];
    for i in 0..n {
        rank[i] = w[i];
    }
    for &v in topo.iter().rev() {
        for &s in &successors[v] {
            let candidate = w[v] + rank[s];
            if candidate > rank[v] {
                rank[v] = candidate;
            }
        }
    }

    // Greedy list scheduling with unlimited lanes
    // Priority queue: max-heap by rank
    #[derive(Eq, PartialEq)]
    struct Entry {
        rank: u64,
        index: usize,
    }
    impl Ord for Entry {
        fn cmp(&self, other: &Self) -> std::cmp::Ordering {
            self.rank
                .cmp(&other.rank)
                .then_with(|| other.index.cmp(&self.index))
        }
    }
    impl PartialOrd for Entry {
        fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
            Some(self.cmp(other))
        }
    }

    let mut ready: BinaryHeap<Entry> = BinaryHeap::new();
    let mut pending_preds = in_degree.clone();
    for i in 0..n {
        if pending_preds[i] == 0 {
            ready.push(Entry {
                rank: rank[i],
                index: i,
            });
        }
    }

    let mut lane_free: Vec<u64> = Vec::new(); // free_at time per lane
    let mut finish: Vec<u64> = vec![0; n];
    let mut schedule: Vec<ScheduleItem> = Vec::with_capacity(n);

    while let Some(Entry { index: v, .. }) = ready.pop() {
        // dep_ready = max finish time of predecessors
        let dep_ready = predecessors[v]
            .iter()
            .map(|&p| finish[p])
            .max()
            .unwrap_or(0);

        // Find the best lane
        let mut best_lane: Option<usize> = None;
        let mut best_start = u64::MAX;

        for (lane_idx, &free_at) in lane_free.iter().enumerate() {
            let s = free_at.max(dep_ready);
            if s < best_start {
                best_start = s;
                best_lane = Some(lane_idx);
            }
        }

        // Check if a new lane would be better
        if best_lane.is_none() || dep_ready < best_start {
            best_lane = Some(lane_free.len());
            lane_free.push(0);
            best_start = dep_ready;
        }

        let lane = best_lane.unwrap();
        let start = best_start;
        let duration = w[v];

        finish[v] = start + duration;
        lane_free[lane] = finish[v];

        schedule.push(ScheduleItem {
            tx_index: v as u32,
            start,
            duration,
            lane: lane as u32,
            is_critical: est[v] == lst[v],
        });

        // Unblock successors
        for &s in &successors[v] {
            pending_preds[s] -= 1;
            if pending_preds[s] == 0 {
                ready.push(Entry {
                    rank: rank[s],
                    index: s,
                });
            }
        }
    }

    // Sort schedule by tx_index for deterministic output
    schedule.sort_by_key(|s| s.tx_index);

    Ok((
        schedule,
        CriticalPathInfo {
            weight: critical_path_weight,
            est,
            lst,
        },
    ))
}

/// Calculate max parallelism using sweep line algorithm.
pub fn max_parallelism(schedule: &[ScheduleItem]) -> u32 {
    if schedule.is_empty() {
        return 0;
    }

    let mut events: Vec<(u64, i32)> = Vec::with_capacity(schedule.len() * 2);
    for item in schedule {
        events.push((item.start, 1));
        events.push((item.start + item.duration, -1));
    }

    // Sort by time, then -1 before +1 at same time (ends before starts)
    events.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));

    let mut current: i32 = 0;
    let mut max_parallel: i32 = 0;
    for (_time, delta) in events {
        current += delta;
        if current > max_parallel {
            max_parallel = current;
        }
    }

    max_parallel as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    use crate::model::*;
    use alloy_primitives::{Address, B256};

    fn addr(n: u8) -> Address {
        let mut bytes = [0u8; 20];
        bytes[0] = n;
        Address::from(bytes)
    }

    fn hash(n: u8) -> B256 {
        let mut bytes = [0u8; 32];
        bytes[0] = n;
        B256::from(bytes)
    }

    fn make_node(index: u32, gas_used: u64) -> TxNode {
        TxNode {
            tx_hash: hash(index as u8),
            tx_index: index,
            from: addr(index as u8),
            to: None,
            tx_type: 2,
            nonce: 0,
            gas_used,
            tempo: None,
        }
    }

    #[test]
    fn test_critical_path_chain() {
        // 3 txs chained: 0→1→2, weights 100, 200, 300
        let nodes = vec![make_node(0, 100), make_node(1, 200), make_node(2, 300)];
        let edges = vec![
            DepEdge {
                from_tx: 0,
                to_tx: 1,
                kind: DepEdgeKind::Raw,
                reasons: vec![],
            },
            DepEdge {
                from_tx: 1,
                to_tx: 2,
                kind: DepEdgeKind::Raw,
                reasons: vec![],
            },
        ];

        let (schedule, crit) = compute(&nodes, &edges).unwrap();
        assert_eq!(crit.weight, 600);
        assert_eq!(schedule.len(), 3);

        // All on critical path
        for item in &schedule {
            assert!(item.is_critical);
        }
    }

    #[test]
    fn test_critical_path_diamond() {
        // Diamond: A→B, A→C, B→D, C→D
        // Weights: A=10, B=50, C=20, D=10
        // Critical path: A→B→D = 70
        let nodes = vec![
            make_node(0, 10),
            make_node(1, 50),
            make_node(2, 20),
            make_node(3, 10),
        ];
        let edges = vec![
            DepEdge {
                from_tx: 0,
                to_tx: 1,
                kind: DepEdgeKind::Raw,
                reasons: vec![],
            },
            DepEdge {
                from_tx: 0,
                to_tx: 2,
                kind: DepEdgeKind::Raw,
                reasons: vec![],
            },
            DepEdge {
                from_tx: 1,
                to_tx: 3,
                kind: DepEdgeKind::Raw,
                reasons: vec![],
            },
            DepEdge {
                from_tx: 2,
                to_tx: 3,
                kind: DepEdgeKind::Raw,
                reasons: vec![],
            },
        ];

        let (_schedule, crit) = compute(&nodes, &edges).unwrap();
        assert_eq!(crit.weight, 70);

        // EST: A=0, B=10, C=10, D=60
        assert_eq!(crit.est[0], 0);
        assert_eq!(crit.est[1], 10);
        assert_eq!(crit.est[2], 10);
        assert_eq!(crit.est[3], 60);
    }

    #[test]
    fn test_critical_path_independent() {
        // No edges → critical path = max single weight
        let nodes = vec![make_node(0, 100), make_node(1, 200), make_node(2, 50)];
        let edges: Vec<DepEdge> = vec![];

        let (_schedule, crit) = compute(&nodes, &edges).unwrap();
        assert_eq!(crit.weight, 200);
    }

    #[test]
    fn test_schedule_chain() {
        let nodes = vec![make_node(0, 100), make_node(1, 200), make_node(2, 300)];
        let edges = vec![
            DepEdge {
                from_tx: 0,
                to_tx: 1,
                kind: DepEdgeKind::Raw,
                reasons: vec![],
            },
            DepEdge {
                from_tx: 1,
                to_tx: 2,
                kind: DepEdgeKind::Raw,
                reasons: vec![],
            },
        ];

        let (schedule, _crit) = compute(&nodes, &edges).unwrap();

        // All in same lane, sequential starts
        let s0 = schedule.iter().find(|s| s.tx_index == 0).unwrap();
        let s1 = schedule.iter().find(|s| s.tx_index == 1).unwrap();
        let s2 = schedule.iter().find(|s| s.tx_index == 2).unwrap();

        assert_eq!(s0.start, 0);
        assert_eq!(s1.start, 100);
        assert_eq!(s2.start, 300);
        assert_eq!(s0.lane, s1.lane);
        assert_eq!(s1.lane, s2.lane);
    }

    #[test]
    fn test_schedule_independent() {
        let nodes = vec![make_node(0, 100), make_node(1, 200), make_node(2, 50)];
        let edges: Vec<DepEdge> = vec![];

        let (schedule, _crit) = compute(&nodes, &edges).unwrap();

        // All start at 0
        for item in &schedule {
            assert_eq!(item.start, 0);
        }

        // All in different lanes
        let lanes: HashSet<u32> = schedule.iter().map(|s| s.lane).collect();
        assert_eq!(lanes.len(), 3);
    }

    #[test]
    fn test_schedule_diamond() {
        // Diamond: A→B, A→C, B→D, C→D
        let nodes = vec![
            make_node(0, 10),
            make_node(1, 50),
            make_node(2, 20),
            make_node(3, 10),
        ];
        let edges = vec![
            DepEdge {
                from_tx: 0,
                to_tx: 1,
                kind: DepEdgeKind::Raw,
                reasons: vec![],
            },
            DepEdge {
                from_tx: 0,
                to_tx: 2,
                kind: DepEdgeKind::Raw,
                reasons: vec![],
            },
            DepEdge {
                from_tx: 1,
                to_tx: 3,
                kind: DepEdgeKind::Raw,
                reasons: vec![],
            },
            DepEdge {
                from_tx: 2,
                to_tx: 3,
                kind: DepEdgeKind::Raw,
                reasons: vec![],
            },
        ];

        let (schedule, _crit) = compute(&nodes, &edges).unwrap();

        let s0 = schedule.iter().find(|s| s.tx_index == 0).unwrap();
        let s1 = schedule.iter().find(|s| s.tx_index == 1).unwrap();
        let s2 = schedule.iter().find(|s| s.tx_index == 2).unwrap();
        let s3 = schedule.iter().find(|s| s.tx_index == 3).unwrap();

        assert_eq!(s0.start, 0);
        assert_eq!(s1.start, 10);
        assert_eq!(s2.start, 10);
        // D must start after both B and C finish
        assert!(s3.start >= s1.start + s1.duration);
        assert!(s3.start >= s2.start + s2.duration);
        assert_eq!(s3.start, 60); // max(10+50, 10+20) = 60
    }

    #[test]
    fn test_schedule_critical_marking() {
        // Diamond: A=10, B=50, C=20, D=10
        // Critical path: A→B→D
        // C is not critical (has slack)
        let nodes = vec![
            make_node(0, 10),
            make_node(1, 50),
            make_node(2, 20),
            make_node(3, 10),
        ];
        let edges = vec![
            DepEdge {
                from_tx: 0,
                to_tx: 1,
                kind: DepEdgeKind::Raw,
                reasons: vec![],
            },
            DepEdge {
                from_tx: 0,
                to_tx: 2,
                kind: DepEdgeKind::Raw,
                reasons: vec![],
            },
            DepEdge {
                from_tx: 1,
                to_tx: 3,
                kind: DepEdgeKind::Raw,
                reasons: vec![],
            },
            DepEdge {
                from_tx: 2,
                to_tx: 3,
                kind: DepEdgeKind::Raw,
                reasons: vec![],
            },
        ];

        let (schedule, crit) = compute(&nodes, &edges).unwrap();

        let s0 = schedule.iter().find(|s| s.tx_index == 0).unwrap();
        let s1 = schedule.iter().find(|s| s.tx_index == 1).unwrap();
        let s2 = schedule.iter().find(|s| s.tx_index == 2).unwrap();
        let s3 = schedule.iter().find(|s| s.tx_index == 3).unwrap();

        assert!(s0.is_critical); // A: est=0, lst=0
        assert!(s1.is_critical); // B: est=10, lst=10
        assert!(!s2.is_critical); // C: est=10, lst=40 (slack=30)
        assert!(s3.is_critical); // D: est=60, lst=60
    }

    #[test]
    fn test_max_parallelism_independent() {
        let nodes = vec![make_node(0, 100), make_node(1, 200), make_node(2, 50)];
        let edges: Vec<DepEdge> = vec![];
        let (schedule, _) = compute(&nodes, &edges).unwrap();
        assert_eq!(max_parallelism(&schedule), 3);
    }

    #[test]
    fn test_max_parallelism_chain() {
        let nodes = vec![make_node(0, 100), make_node(1, 200), make_node(2, 300)];
        let edges = vec![
            DepEdge {
                from_tx: 0,
                to_tx: 1,
                kind: DepEdgeKind::Raw,
                reasons: vec![],
            },
            DepEdge {
                from_tx: 1,
                to_tx: 2,
                kind: DepEdgeKind::Raw,
                reasons: vec![],
            },
        ];
        let (schedule, _) = compute(&nodes, &edges).unwrap();
        assert_eq!(max_parallelism(&schedule), 1);
    }

    #[test]
    fn test_empty_schedule() {
        let (schedule, crit) = compute(&[], &[]).unwrap();
        assert!(schedule.is_empty());
        assert_eq!(crit.weight, 0);
        assert_eq!(max_parallelism(&schedule), 0);
    }

    #[test]
    fn test_single_tx() {
        let nodes = vec![make_node(0, 500)];
        let (schedule, crit) = compute(&nodes, &[]).unwrap();
        assert_eq!(schedule.len(), 1);
        assert_eq!(schedule[0].start, 0);
        assert_eq!(schedule[0].duration, 500);
        assert!(schedule[0].is_critical);
        assert_eq!(crit.weight, 500);
        assert_eq!(max_parallelism(&schedule), 1);
    }

    #[test]
    fn test_zero_gas_fallback() {
        let nodes = vec![make_node(0, 0)];
        let (schedule, crit) = compute(&nodes, &[]).unwrap();
        assert_eq!(schedule[0].duration, 1); // fallback
        assert_eq!(crit.weight, 1);
    }
}
