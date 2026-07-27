use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug)]
pub(super) struct OrderingNode {
    pub(super) payload:    u32,
    pub(super) diagnostic: u32,
    pub(super) order:      usize,
    index:                 usize,
}

/// Generic bounded directed graph used by compiler-hosted project macros.
///
/// Keys and payloads are opaque to the graph. Edges store dense node indexes,
/// so sorting and cycle discovery do not clone or compare caller keys on hot
/// graph operations. Stable ordering is controlled by the caller-provided
/// source rank and then insertion order.
#[derive(Clone, Debug, Default)]
pub(super) struct OrderingGraph {
    nodes: BTreeMap<String, OrderingNode>,
    edges: BTreeSet<(usize, usize)>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum EdgeStatus {
    Added,
    Duplicate,
    Contradiction,
}

impl OrderingGraph {
    pub(super) fn add_node(
        &mut self,
        key: String,
        payload: u32,
        diagnostic: u32,
        order: usize,
    ) -> bool {
        if self.nodes.contains_key(&key) {
            return false;
        }
        let index = self.nodes.len();
        self.nodes.insert(
            key,
            OrderingNode {
                payload,
                diagnostic,
                order,
                index,
            },
        );
        true
    }

    pub(super) fn contains(&self, key: &str) -> bool {
        self.nodes.contains_key(key)
    }

    pub(super) fn add_edge(&mut self, before: String, after: String) -> Option<EdgeStatus> {
        let status = self.edge_status(&before, &after)?;
        if status == EdgeStatus::Added {
            let before = self.nodes.get(&before)?.index;
            let after = self.nodes.get(&after)?.index;
            self.edges.insert((before, after));
        }
        Some(status)
    }

    pub(super) fn edge_status(&self, before: &str, after: &str) -> Option<EdgeStatus> {
        let before = self.nodes.get(before)?.index;
        let after = self.nodes.get(after)?.index;
        if self.edges.contains(&(after, before)) {
            return Some(EdgeStatus::Contradiction);
        }
        if self.edges.contains(&(before, after)) {
            return Some(EdgeStatus::Duplicate);
        }
        Some(EdgeStatus::Added)
    }

    pub(super) fn stable_sort(&self) -> Option<Vec<u32>> {
        let nodes = self.nodes_by_index()?;
        let mut indegree = vec![0_usize; nodes.len()];
        let mut outgoing = vec![Vec::<usize>::new(); nodes.len()];
        for &(before, after) in &self.edges {
            *indegree.get_mut(after)? += 1;
            outgoing.get_mut(before)?.push(after);
        }
        let mut ready = BTreeSet::new();
        for (index, node) in nodes.iter().enumerate() {
            if indegree.get(index).copied()? == 0 {
                ready.insert((node.order, node.index, index));
            }
        }
        let mut result = Vec::with_capacity(nodes.len());
        while let Some((_order, _insertion, index)) = ready.pop_first() {
            result.push(nodes.get(index)?.payload);
            for &child in outgoing.get(index)? {
                let degree = indegree.get_mut(child)?;
                *degree = degree.checked_sub(1)?;
                if *degree == 0 {
                    let node = *nodes.get(child)?;
                    ready.insert((node.order, node.index, child));
                }
            }
        }
        (result.len() == nodes.len()).then_some(result)
    }

    pub(super) fn cycle(&self) -> Vec<u32> {
        let Some(nodes) = self.nodes_by_index() else {
            return Vec::new();
        };
        let mut outgoing = vec![Vec::<usize>::new(); nodes.len()];
        for &(before, after) in &self.edges {
            let Some(children) = outgoing.get_mut(before) else {
                return Vec::new();
            };
            children.push(after);
        }
        for children in &mut outgoing {
            children.sort_by_key(|index| {
                nodes
                    .get(*index)
                    .map_or((usize::MAX, usize::MAX), |node| (node.order, node.index))
            });
        }
        let mut roots = (0..nodes.len()).collect::<Vec<_>>();
        roots.sort_by_key(|index| {
            nodes
                .get(*index)
                .map_or((usize::MAX, usize::MAX), |node| (node.order, node.index))
        });
        let mut state = vec![0_u8; nodes.len()];
        let mut stack = Vec::new();
        for root in roots {
            if state.get(root).copied().unwrap_or_default() == 0
                && let Some(cycle) = Self::visit_cycle(root, &outgoing, &mut state, &mut stack)
            {
                return cycle
                    .into_iter()
                    .filter_map(|index| nodes.get(index).map(|node| node.diagnostic))
                    .collect();
            }
        }
        Vec::new()
    }

    fn nodes_by_index(&self) -> Option<Vec<&OrderingNode>> {
        let mut nodes = vec![None; self.nodes.len()];
        for node in self.nodes.values() {
            *nodes.get_mut(node.index)? = Some(node);
        }
        nodes.into_iter().collect()
    }

    fn visit_cycle(
        index: usize,
        outgoing: &[Vec<usize>],
        state: &mut [u8],
        stack: &mut Vec<usize>,
    ) -> Option<Vec<usize>> {
        *state.get_mut(index)? = 1;
        stack.push(index);
        for &child in outgoing.get(index)? {
            match state.get(child).copied().unwrap_or_default() {
                0 => {
                    if let Some(cycle) = Self::visit_cycle(child, outgoing, state, stack) {
                        return Some(cycle);
                    }
                }
                1 => {
                    let start = stack.iter().position(|candidate| *candidate == child)?;
                    let mut cycle = stack.get(start..)?.to_vec();
                    cycle.push(child);
                    return Some(cycle);
                }
                _ => {}
            }
        }
        let popped = stack.pop();
        debug_assert_eq!(popped, Some(index));
        *state.get_mut(index)? = 2;
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_sort_respects_edges_and_source_order() {
        let mut graph = OrderingGraph::default();
        assert!(graph.add_node("b".to_string(), 20, 2, 1));
        assert!(graph.add_node("a".to_string(), 10, 1, 0));
        assert!(graph.add_node("c".to_string(), 30, 3, 2));
        assert_eq!(
            graph.add_edge("c".to_string(), "b".to_string()),
            Some(EdgeStatus::Added)
        );
        assert_eq!(graph.stable_sort(), Some(vec![10, 30, 20]));
    }

    #[test]
    fn reports_duplicate_contradictory_edges_and_full_cycles() {
        let mut graph = OrderingGraph::default();
        assert!(graph.add_node("a".to_string(), 10, 1, 0));
        assert!(graph.add_node("b".to_string(), 20, 2, 1));
        assert!(graph.add_node("c".to_string(), 30, 3, 2));
        assert_eq!(
            graph.add_edge("a".to_string(), "b".to_string()),
            Some(EdgeStatus::Added)
        );
        assert_eq!(
            graph.add_edge("a".to_string(), "b".to_string()),
            Some(EdgeStatus::Duplicate)
        );
        assert_eq!(
            graph.add_edge("b".to_string(), "a".to_string()),
            Some(EdgeStatus::Contradiction)
        );
        assert_eq!(
            graph.add_edge("b".to_string(), "c".to_string()),
            Some(EdgeStatus::Added)
        );
        assert_eq!(
            graph.add_edge("c".to_string(), "a".to_string()),
            Some(EdgeStatus::Added)
        );
        assert_eq!(graph.stable_sort(), None);
        assert_eq!(graph.cycle(), vec![1, 2, 3, 1]);
    }
}
