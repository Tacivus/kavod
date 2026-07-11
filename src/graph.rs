use std::{
    any::TypeId,
    collections::{HashMap, HashSet},
};

use crate::{handler::HandlerRegistry, reducer::ReducerRegistry};

/// Validate the complete message graph against structural faults.
///
/// Two checks run serially:
/// 1. **Orphan check** — every declared `.produces::<M>()` must have
///    at least one consumer (handler, reducer, or actor route).
/// 2. **Same-instant cycle check** — the directed graph of
///    `(subscribed → produced)` edges must not contain cycles.
///    Cycles at the same timestamp would loop forever; future-
///    timestamped cycles are self-limiting but we conservatively
///    reject all cycles at startup.
///
/// `route_consumers` carries `TypeId`s that are consumed by actor
/// routes (Phase 12). Today it is always empty.
pub(crate) fn validate(
    handler_reg: &HandlerRegistry,
    reducer_reg: &ReducerRegistry,
    route_consumers: &HashSet<TypeId>,
) {
    validate_consumers(
        &handler_reg.produced_types(),
        &handler_reg.subscribed_types(),
        &reducer_reg.subscribed_types(),
        route_consumers,
    );
    validate_no_cycles(&handler_reg.edges());
}

/// Panics if any type in `handler_producers` has no consumer anywhere
/// (not in `handler_consumers`, `reducer_consumers`, or
/// `route_consumers`).
fn validate_consumers(
    handler_producers: &HashSet<TypeId>,
    handler_consumers: &HashSet<TypeId>,
    reducer_consumers: &HashSet<TypeId>,
    route_consumers: &HashSet<TypeId>,
) {
    let all_consumers: HashSet<TypeId> = handler_consumers
        .iter()
        .chain(reducer_consumers.iter())
        .chain(route_consumers.iter())
        .copied()
        .collect();

    for tid in handler_producers {
        if !all_consumers.contains(tid) {
            panic!(
                "No consumer for produced type {:?}. \
                 Every `.produces::<T>()` must have at least one handler, \
                 reducer, or route subscribed to T.",
                tid,
            );
        }
    }
}

/// Builds an adjacency list from `(subscribed, produced_set)` pairs
/// and runs DFS to detect back edges. Panics on the first cycle found.
fn validate_no_cycles(edges: &[(TypeId, HashSet<TypeId>)]) {
    let mut adj: HashMap<TypeId, Vec<TypeId>> = HashMap::new();
    for (src, targets) in edges {
        adj.entry(*src).or_default().extend(targets.iter().copied());
    }

    let mut all_nodes = HashSet::new();
    for (src, targets) in edges {
        all_nodes.insert(*src);
        all_nodes.extend(targets.iter().copied());
    }

    let mut visited = HashSet::new();
    let mut on_path = HashSet::new();
    let mut path: Vec<TypeId> = Vec::new();

    for node in &all_nodes {
        if !visited.contains(node) {
            if dfs_cycle(*node, &adj, &mut visited, &mut on_path, &mut path) {
                panic!(
                    "Same-instant message cycle detected: {}",
                    cycle_description(&path),
                );
            }
        }
    }
}

fn dfs_cycle(
    node: TypeId,
    adj: &HashMap<TypeId, Vec<TypeId>>,
    visited: &mut HashSet<TypeId>,
    on_path: &mut HashSet<TypeId>,
    path: &mut Vec<TypeId>,
) -> bool {
    visited.insert(node);
    on_path.insert(node);
    path.push(node);

    if let Some(neighbors) = adj.get(&node) {
        for &next in neighbors {
            if on_path.contains(&next) {
                path.push(next);
                return true;
            }
            if !visited.contains(&next) {
                if dfs_cycle(next, adj, visited, on_path, path) {
                    return true;
                }
            }
        }
    }

    path.pop();
    on_path.remove(&node);
    false
}

fn cycle_description(path: &[TypeId]) -> String {
    let last = *path.last().unwrap();
    let start = path.iter().position(|&n| n == last).unwrap();
    path[start..]
        .iter()
        .map(|t| format!("{:?}", t))
        .collect::<Vec<_>>()
        .join(" → ")
}

// ============================================================================
// Tests
// ============================================================================
#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::Message;
    use std::collections::HashSet;

    // ── test message types ─────────────────────────────────────────────────

    #[derive(Clone, Debug, PartialEq)]
    struct MsgA;
    impl Message for MsgA {}

    #[derive(Clone, Debug, PartialEq)]
    struct MsgB;
    impl Message for MsgB {}

    #[derive(Clone, Debug, PartialEq)]
    struct MsgC;
    impl Message for MsgC {}

    #[derive(Clone, Debug, PartialEq)]
    struct MsgD;
    impl Message for MsgD {}

    // ── helpers ────────────────────────────────────────────────────────────

    fn tid<M: Message>() -> TypeId {
        TypeId::of::<M>()
    }

    // ── validate_consumers ─────────────────────────────────────────────────

    /// A valid graph where every produced type has a consumer passes.
    #[test]
    fn valid_consumers_pass() {
        let mut hr = HandlerRegistry::new();
        hr.on::<MsgA>(|_, _| {}).produces::<MsgB>();
        hr.on::<MsgB>(|_, _| {}); // consumes MsgB

        let rr = ReducerRegistry::new();
        let routes = HashSet::new();

        validate_consumers(
            &hr.produced_types(),
            &hr.subscribed_types(),
            &rr.subscribed_types(),
            &routes,
        );
    }

    /// A type declared via `.produces` but never subscribed to by any
    /// handler, reducer, or route panics.
    #[test]
    #[should_panic(expected = "No consumer")]
    fn orphan_production_panics() {
        let mut hr = HandlerRegistry::new();
        hr.on::<MsgA>(|_, _| {}).produces::<MsgB>();
        // Nobody consumes MsgB

        let rr = ReducerRegistry::new();
        let routes = HashSet::new();

        validate_consumers(
            &hr.produced_types(),
            &hr.subscribed_types(),
            &rr.subscribed_types(),
            &routes,
        );
    }

    /// A reducer registered for a type counts as a consumer.
    #[test]
    fn reducer_counts_as_consumer() {
        let mut hr = HandlerRegistry::new();
        hr.on::<MsgA>(|_, _| {}).produces::<MsgB>();
        // No handler for MsgB, but a reducer consumes it

        let mut rr = ReducerRegistry::new();
        rr.register::<MsgB>(|_, _| {});

        let routes = HashSet::new();

        validate_consumers(
            &hr.produced_types(),
            &hr.subscribed_types(),
            &rr.subscribed_types(),
            &routes,
        );
    }

    /// An actor route counts as a consumer (Phase 12 extension point).
    #[test]
    fn route_counts_as_consumer() {
        let mut hr = HandlerRegistry::new();
        hr.on::<MsgA>(|_, _| {}).produces::<MsgB>();
        // No handler or reducer, but a route exists

        let rr = ReducerRegistry::new();
        let mut routes = HashSet::new();
        routes.insert(tid::<MsgB>());

        validate_consumers(
            &hr.produced_types(),
            &hr.subscribed_types(),
            &rr.subscribed_types(),
            &routes,
        );
    }

    /// Empty registries never panic.
    #[test]
    fn empty_registries_pass_consumers() {
        let hr = HandlerRegistry::new();
        let rr = ReducerRegistry::new();
        let routes = HashSet::new();

        validate_consumers(
            &hr.produced_types(),
            &hr.subscribed_types(),
            &rr.subscribed_types(),
            &routes,
        );
    }

    // ── validate_no_cycles ─────────────────────────────────────────────────

    /// A simple two-node cycle A → B → A is detected.
    #[test]
    #[should_panic(expected = "cycle")]
    fn simple_cycle_detected() {
        let mut hr = HandlerRegistry::new();
        hr.on::<MsgA>(|_, _| {}).produces::<MsgB>();
        hr.on::<MsgB>(|_, _| {}).produces::<MsgA>();

        validate_no_cycles(&hr.edges());
    }

    /// A self-loop A → A is detected.
    #[test]
    #[should_panic(expected = "cycle")]
    fn self_loop_detected() {
        let mut hr = HandlerRegistry::new();
        hr.on::<MsgA>(|_, _| {}).produces::<MsgA>();

        validate_no_cycles(&hr.edges());
    }

    /// A three-node cycle A → B → C → A is detected.
    #[test]
    #[should_panic(expected = "cycle")]
    fn three_node_cycle_detected() {
        let mut hr = HandlerRegistry::new();
        hr.on::<MsgA>(|_, _| {}).produces::<MsgB>();
        hr.on::<MsgB>(|_, _| {}).produces::<MsgC>();
        hr.on::<MsgC>(|_, _| {}).produces::<MsgA>();

        validate_no_cycles(&hr.edges());
    }

    /// A long linear chain A → B → C → D passes (no cycles).
    #[test]
    fn linear_chain_passes() {
        let mut hr = HandlerRegistry::new();
        hr.on::<MsgA>(|_, _| {}).produces::<MsgB>();
        hr.on::<MsgB>(|_, _| {}).produces::<MsgC>();
        hr.on::<MsgC>(|_, _| {}).produces::<MsgD>();

        validate_no_cycles(&hr.edges());
    }

    /// A diamond-graph (branching and merging, no back edges) passes.
    #[test]
    fn diamond_graph_passes() {
        let mut hr = HandlerRegistry::new();
        hr.on::<MsgA>(|_, _| {})
            .produces::<MsgB>()
            .produces::<MsgC>();
        hr.on::<MsgB>(|_, _| {}).produces::<MsgD>();
        hr.on::<MsgC>(|_, _| {}).produces::<MsgD>();

        validate_no_cycles(&hr.edges());
    }

    /// Two independent cycles A → B → A and C → D → C both trigger
    /// detection (first one found panics).
    #[test]
    #[should_panic(expected = "cycle")]
    fn multiple_cycles_first_one_panics() {
        let mut hr = HandlerRegistry::new();
        hr.on::<MsgA>(|_, _| {}).produces::<MsgB>();
        hr.on::<MsgB>(|_, _| {}).produces::<MsgA>();
        hr.on::<MsgC>(|_, _| {}).produces::<MsgD>();
        hr.on::<MsgD>(|_, _| {}).produces::<MsgC>();

        validate_no_cycles(&hr.edges());
    }

    /// Empty edges never panic.
    #[test]
    fn empty_edges_pass() {
        validate_no_cycles(&[]);
    }

    /// Handlers without `.produces` don't contribute edges and don't
    /// cause false positives.
    #[test]
    fn handler_without_produces_no_cycle() {
        let mut hr = HandlerRegistry::new();
        hr.on::<MsgA>(|_, _| {}).produces::<MsgB>();
        hr.on::<MsgB>(|_, _| {}); // consumes MsgB, produces nothing

        validate_no_cycles(&hr.edges()); // only edge: A → B
    }

    // ── validate (end-to-end) ──────────────────────────────────────────────

    /// Full validation passes with a well-formed graph.
    #[test]
    fn validate_well_formed_graph_passes() {
        let mut hr = HandlerRegistry::new();
        hr.on::<MsgA>(|_, _| {}).produces::<MsgB>();
        hr.on::<MsgB>(|_, _| {}).produces::<MsgC>();
        hr.on::<MsgC>(|_, _| {}); // terminal consumer

        let mut rr = ReducerRegistry::new();
        rr.register::<MsgD>(|_, _| {}); // unused reducer, harmless

        let routes = HashSet::new();

        validate(&hr, &rr, &routes);
    }
}
