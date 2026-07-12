use std::{
    any::TypeId,
    collections::{HashMap, HashSet},
};

use crate::{
    actor::ActorRegistry, error::BuildError, handler::HandlerRegistry, output::MessageType,
    reducer::ReducerRegistry,
};

/// Kind of registered consumer callback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConsumerKind {
    Reducer,
    Handler,
    Actor,
}

/// Stable identity for one registered consumer (reducer or handler callback).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct ConsumerId(pub(crate) usize);

/// One registered consumer of a message type.
#[expect(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct ConsumerDescriptor {
    pub(crate) id: ConsumerId,
    pub(crate) kind: ConsumerKind,
    pub(crate) consumed: MessageType,
    pub(crate) registration_order: usize,
}

/// Production edge declared by a handler (or later actor) callback.
#[expect(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct ProducerDescriptor {
    pub(crate) owner: ConsumerId,
    pub(crate) consumed: MessageType,
    pub(crate) produced: Vec<MessageType>,
}

/// Mutable collector of graph descriptors. Independent of registry internals.
#[derive(Debug, Default)]
pub(crate) struct GraphBuilder {
    consumers: Vec<ConsumerDescriptor>,
    producers: Vec<ProducerDescriptor>,
    next_id: usize,
}

impl GraphBuilder {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Record a consumer and return its graph-local id.
    pub(crate) fn add_consumer(&mut self, kind: ConsumerKind, consumed: MessageType) -> ConsumerId {
        let id = ConsumerId(self.next_id);
        let registration_order = id.0;
        self.next_id += 1;
        self.consumers.push(ConsumerDescriptor {
            id,
            kind,
            consumed,
            registration_order,
        });
        id
    }

    /// Record productions declared by `owner` while handling `consumed`.
    pub(crate) fn add_producer(
        &mut self,
        owner: ConsumerId,
        consumed: MessageType,
        produced: Vec<MessageType>,
    ) {
        if produced.is_empty() {
            return;
        }
        self.producers.push(ProducerDescriptor {
            owner,
            consumed,
            produced,
        });
    }

    /// Validate orphan productions and freeze a runtime consumer set.
    pub(crate) fn build(self) -> Result<ValidatedGraph, BuildError> {
        let mut consumer_types: HashMap<TypeId, MessageType> = HashMap::new();
        for c in &self.consumers {
            consumer_types.entry(c.consumed.id).or_insert(c.consumed);
        }

        for p in &self.producers {
            for produced in &p.produced {
                if !consumer_types.contains_key(&produced.id) {
                    return Err(BuildError::MissingConsumer {
                        message_type: produced.name,
                    });
                }
            }
        }

        Ok(ValidatedGraph {
            consumer_type_ids: consumer_types.keys().copied().collect(),
        })
    }
}

/// Immutable post-build consumer set for runtime ingress checks.
#[derive(Debug)]
pub(crate) struct ValidatedGraph {
    consumer_type_ids: HashSet<TypeId>,
}

impl ValidatedGraph {
    /// Returns true if at least one reducer, handler, or actor consumes `type_id`.
    pub(crate) fn has_consumer(&self, type_id: TypeId) -> bool {
        self.consumer_type_ids.contains(&type_id)
    }
}

/// Assemble descriptors from registries without reading private entry fields.
pub(crate) fn build_graph(
    reducers: &ReducerRegistry,
    handlers: &HandlerRegistry,
    actors: &ActorRegistry,
) -> Result<ValidatedGraph, BuildError> {
    let mut g = GraphBuilder::new();

    for mt in reducers.consumer_message_types() {
        g.add_consumer(ConsumerKind::Reducer, mt);
    }

    for (consumed, productions) in handlers.graph_entries() {
        let owner = g.add_consumer(ConsumerKind::Handler, consumed);
        g.add_producer(owner, consumed, productions.to_vec());
    }

    for (consumed, productions) in actors.graph_entries() {
        let owner = g.add_consumer(ConsumerKind::Actor, consumed);
        g.add_producer(owner, consumed, productions.to_vec());
    }

    g.build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{context::handler::HandlerCtx, context::reducer::ReducerCtx, message::Message};

    // ========================================================================
    // Test message types
    // ========================================================================

    #[derive(Debug)]
    struct MsgA;

    impl Message for MsgA {}

    #[derive(Debug)]
    struct MsgB;

    impl Message for MsgB {}

    #[derive(Debug)]
    struct MsgC;

    impl Message for MsgC {}

    #[derive(Debug)]
    struct Orphan;

    impl Message for Orphan {}

    // ========================================================================
    // GraphBuilder unit tests (no registries)
    // ========================================================================

    /// Empty graph validates.
    #[test]
    fn empty_graph_validates() {
        let graph = GraphBuilder::new().build().unwrap();
        assert!(!graph.has_consumer(TypeId::of::<MsgA>()));
    }

    /// Terminal consumer with no outputs validates.
    #[test]
    fn terminal_consumer_validates() {
        let mut g = GraphBuilder::new();
        g.add_consumer(ConsumerKind::Handler, MessageType::of::<MsgA>());
        g.build().unwrap();
    }

    /// Linear production chain A → B → C validates.
    #[test]
    fn linear_production_chain_validates() {
        let mut g = GraphBuilder::new();
        let a = g.add_consumer(ConsumerKind::Handler, MessageType::of::<MsgA>());
        let b = g.add_consumer(ConsumerKind::Handler, MessageType::of::<MsgB>());
        g.add_consumer(ConsumerKind::Handler, MessageType::of::<MsgC>());

        g.add_producer(
            a,
            MessageType::of::<MsgA>(),
            vec![MessageType::of::<MsgB>()],
        );
        g.add_producer(
            b,
            MessageType::of::<MsgB>(),
            vec![MessageType::of::<MsgC>()],
        );

        g.build().unwrap();
    }

    /// Reducer counts as consumer of a produced type.
    #[test]
    fn reducer_counts_as_consumer() {
        let mut g = GraphBuilder::new();
        let h = g.add_consumer(ConsumerKind::Handler, MessageType::of::<MsgA>());
        g.add_consumer(ConsumerKind::Reducer, MessageType::of::<MsgB>());
        g.add_producer(
            h,
            MessageType::of::<MsgA>(),
            vec![MessageType::of::<MsgB>()],
        );
        g.build().unwrap();
    }

    /// Handler counts as consumer of a produced type.
    #[test]
    fn handler_counts_as_consumer() {
        let mut g = GraphBuilder::new();
        let h = g.add_consumer(ConsumerKind::Handler, MessageType::of::<MsgA>());
        g.add_consumer(ConsumerKind::Handler, MessageType::of::<MsgB>());
        g.add_producer(
            h,
            MessageType::of::<MsgA>(),
            vec![MessageType::of::<MsgB>()],
        );
        g.build().unwrap();
    }

    /// Orphan production returns MissingConsumer with the produced type name.
    #[test]
    fn orphan_production_returns_build_error_with_type_name() {
        let mut g = GraphBuilder::new();
        let h = g.add_consumer(ConsumerKind::Handler, MessageType::of::<MsgA>());
        g.add_producer(
            h,
            MessageType::of::<MsgA>(),
            vec![MessageType::of::<Orphan>()],
        );

        let err = g.build().unwrap_err();
        match err {
            BuildError::MissingConsumer { message_type } => {
                assert!(
                    message_type.contains("Orphan"),
                    "expected type name containing Orphan, got {message_type}"
                );
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    /// Multiple producers of one consumed type validate.
    #[test]
    fn multiple_producers_of_one_type_validate() {
        let mut g = GraphBuilder::new();
        let h1 = g.add_consumer(ConsumerKind::Handler, MessageType::of::<MsgA>());
        let h2 = g.add_consumer(ConsumerKind::Handler, MessageType::of::<MsgB>());
        g.add_consumer(ConsumerKind::Handler, MessageType::of::<MsgC>());

        g.add_producer(
            h1,
            MessageType::of::<MsgA>(),
            vec![MessageType::of::<MsgC>()],
        );
        g.add_producer(
            h2,
            MessageType::of::<MsgB>(),
            vec![MessageType::of::<MsgC>()],
        );

        g.build().unwrap();
    }

    /// Multiple consumers of one produced type validate.
    #[test]
    fn multiple_consumers_of_one_produced_type_validate() {
        let mut g = GraphBuilder::new();
        let h = g.add_consumer(ConsumerKind::Handler, MessageType::of::<MsgA>());
        g.add_consumer(ConsumerKind::Handler, MessageType::of::<MsgB>());
        g.add_consumer(ConsumerKind::Reducer, MessageType::of::<MsgB>());

        g.add_producer(
            h,
            MessageType::of::<MsgA>(),
            vec![MessageType::of::<MsgB>()],
        );

        g.build().unwrap();
    }

    /// ValidatedGraph answers has_consumer(TypeId).
    #[test]
    fn has_consumer_answers_runtime_lookup() {
        let mut g = GraphBuilder::new();
        g.add_consumer(ConsumerKind::Handler, MessageType::of::<MsgA>());
        let graph = g.build().unwrap();

        assert!(graph.has_consumer(TypeId::of::<MsgA>()));
        assert!(!graph.has_consumer(TypeId::of::<MsgB>()));
    }

    /// Same-type cycle is allowed (static cycle rejection deferred).
    #[test]
    fn type_cycle_is_allowed() {
        let mut g = GraphBuilder::new();
        let a = g.add_consumer(ConsumerKind::Handler, MessageType::of::<MsgA>());
        let b = g.add_consumer(ConsumerKind::Handler, MessageType::of::<MsgB>());
        g.add_producer(
            a,
            MessageType::of::<MsgA>(),
            vec![MessageType::of::<MsgB>()],
        );
        g.add_producer(
            b,
            MessageType::of::<MsgB>(),
            vec![MessageType::of::<MsgA>()],
        );
        g.build().unwrap();
    }

    /// Same consumer type registered twice still validates.
    #[test]
    fn duplicate_consumer_type_validates() {
        let mut g = GraphBuilder::new();
        g.add_consumer(ConsumerKind::Handler, MessageType::of::<MsgA>());
        g.add_consumer(ConsumerKind::Reducer, MessageType::of::<MsgA>());
        g.build().unwrap();
    }

    /// Self-loop at the descriptor level is allowed.
    #[test]
    fn self_loop_production_validates() {
        let mut g = GraphBuilder::new();
        let a = g.add_consumer(ConsumerKind::Handler, MessageType::of::<MsgA>());
        g.add_producer(
            a,
            MessageType::of::<MsgA>(),
            vec![MessageType::of::<MsgA>()],
        );
        g.build().unwrap();
    }

    // ========================================================================
    // Registry integration via build_graph
    // ========================================================================

    #[test]
    fn build_graph_empty_registries() {
        let graph = build_graph(
            &ReducerRegistry::new(),
            &HandlerRegistry::new(),
            &ActorRegistry::new(),
        )
        .unwrap();
        assert!(!graph.has_consumer(TypeId::of::<MsgA>()));
    }

    #[test]
    fn build_graph_reducer_only_consumer() {
        let mut reducers = ReducerRegistry::new();
        reducers.register(|_ctx: &mut ReducerCtx<'_>, _m: &MsgA| {});

        let graph = build_graph(&reducers, &HandlerRegistry::new(), &ActorRegistry::new()).unwrap();
        assert!(graph.has_consumer(TypeId::of::<MsgA>()));
    }

    #[test]
    fn build_graph_handler_only_consumer() {
        let mut handlers = HandlerRegistry::new();
        handlers.on(|_ctx: &mut HandlerCtx<'_>, _m: &MsgA| {});

        let graph = build_graph(&ReducerRegistry::new(), &handlers, &ActorRegistry::new()).unwrap();
        assert!(graph.has_consumer(TypeId::of::<MsgA>()));
    }

    #[test]
    fn build_graph_linear_chain_via_registries() {
        let mut handlers = HandlerRegistry::new();
        handlers
            .on(|_ctx: &mut HandlerCtx<'_>, _m: &MsgA| {})
            .produces::<MsgB>();
        handlers
            .on(|_ctx: &mut HandlerCtx<'_>, _m: &MsgB| {})
            .produces::<MsgC>();
        handlers.on(|_ctx: &mut HandlerCtx<'_>, _m: &MsgC| {});

        build_graph(&ReducerRegistry::new(), &handlers, &ActorRegistry::new()).unwrap();
    }

    #[test]
    fn build_graph_orphan_via_handler_production() {
        let mut handlers = HandlerRegistry::new();
        handlers
            .on(|_ctx: &mut HandlerCtx<'_>, _m: &MsgA| {})
            .produces::<Orphan>();

        let err =
            build_graph(&ReducerRegistry::new(), &handlers, &ActorRegistry::new()).unwrap_err();
        match err {
            BuildError::MissingConsumer { message_type } => {
                assert!(
                    message_type.contains("Orphan"),
                    "expected type name containing Orphan, got {message_type}"
                );
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn build_graph_handler_produce_consumed_by_reducer() {
        let mut reducers = ReducerRegistry::new();
        reducers.register(|_ctx: &mut ReducerCtx<'_>, _m: &MsgB| {});

        let mut handlers = HandlerRegistry::new();
        handlers
            .on(|_ctx: &mut HandlerCtx<'_>, _m: &MsgA| {})
            .produces::<MsgB>();

        let graph = build_graph(&reducers, &handlers, &ActorRegistry::new()).unwrap();
        assert!(graph.has_consumer(TypeId::of::<MsgA>()));
        assert!(graph.has_consumer(TypeId::of::<MsgB>()));
    }

    #[test]
    fn build_graph_stateful_handler_productions() {
        struct S;

        let mut handlers = HandlerRegistry::new();
        handlers.handler_group(S, |group| {
            group
                .on(|_s: &mut S, _ctx: &mut HandlerCtx<'_>, _m: &MsgA| {})
                .produces::<MsgB>();
            group.on(|_s: &mut S, _ctx: &mut HandlerCtx<'_>, _m: &MsgB| {});
        });

        build_graph(&ReducerRegistry::new(), &handlers, &ActorRegistry::new()).unwrap();
    }

    #[test]
    fn build_graph_multiple_produces_on_one_handler() {
        let mut handlers = HandlerRegistry::new();
        handlers
            .on(|_ctx: &mut HandlerCtx<'_>, _m: &MsgA| {})
            .produces::<MsgB>()
            .produces::<MsgC>();
        handlers.on(|_ctx: &mut HandlerCtx<'_>, _m: &MsgB| {});
        handlers.on(|_ctx: &mut HandlerCtx<'_>, _m: &MsgC| {});

        build_graph(&ReducerRegistry::new(), &handlers, &ActorRegistry::new()).unwrap();
    }

    /// Reducer and handler both consuming the same type still validates.
    #[test]
    fn build_graph_reducer_and_handler_same_type_validates() {
        let mut reducers = ReducerRegistry::new();
        reducers.register(|_ctx: &mut ReducerCtx<'_>, _m: &MsgA| {});

        let mut handlers = HandlerRegistry::new();
        handlers.on(|_ctx: &mut HandlerCtx<'_>, _m: &MsgA| {});

        let graph = build_graph(&reducers, &handlers, &ActorRegistry::new()).unwrap();
        assert!(graph.has_consumer(TypeId::of::<MsgA>()));
    }

    /// Handler that produces a type it also consumes (self-loop) is allowed.
    /// Static cycle rejection is deferred; runtime same-instant bound later.
    #[test]
    fn build_graph_self_loop_production_allowed() {
        let mut handlers = HandlerRegistry::new();
        handlers
            .on(|_ctx: &mut HandlerCtx<'_>, _m: &MsgA| {})
            .produces::<MsgA>();

        build_graph(&ReducerRegistry::new(), &handlers, &ActorRegistry::new()).unwrap();
    }

    /// Multiple reducers on the same message type all count as consumers.
    #[test]
    fn build_graph_multiple_reducers_same_type_are_consumers() {
        let mut reducers = ReducerRegistry::new();
        reducers.register(|_ctx: &mut ReducerCtx<'_>, _m: &MsgB| {});
        reducers.register(|_ctx: &mut ReducerCtx<'_>, _m: &MsgB| {});

        let mut handlers = HandlerRegistry::new();
        handlers
            .on(|_ctx: &mut HandlerCtx<'_>, _m: &MsgA| {})
            .produces::<MsgB>();

        let graph = build_graph(&reducers, &handlers, &ActorRegistry::new()).unwrap();
        assert!(graph.has_consumer(TypeId::of::<MsgA>()));
        assert!(graph.has_consumer(TypeId::of::<MsgB>()));
    }

    /// Handler producing a type only consumed by another handler (no reducer).
    #[test]
    fn build_graph_handler_only_satisfies_orphan_check() {
        let mut handlers = HandlerRegistry::new();
        handlers
            .on(|_ctx: &mut HandlerCtx<'_>, _m: &MsgA| {})
            .produces::<MsgB>();
        handlers.on(|_ctx: &mut HandlerCtx<'_>, _m: &MsgB| {});

        let graph = build_graph(&ReducerRegistry::new(), &handlers, &ActorRegistry::new()).unwrap();
        assert!(graph.has_consumer(TypeId::of::<MsgB>()));
    }

    /// Invariant: actor counts as consumer of a produced type
    #[test]
    fn actor_counts_as_consumer() {
        let mut g = GraphBuilder::new();
        let h = g.add_consumer(ConsumerKind::Handler, MessageType::of::<MsgA>());
        g.add_consumer(ConsumerKind::Actor, MessageType::of::<MsgB>());
        g.add_producer(
            h,
            MessageType::of::<MsgA>(),
            vec![MessageType::of::<MsgB>()],
        );
        g.build().unwrap();
    }

    /// Invariant: orphan actor production returns MissingConsumer with type name
    #[test]
    fn actor_orphan_production_returns_build_error() {
        let mut g = GraphBuilder::new();
        let a = g.add_consumer(ConsumerKind::Actor, MessageType::of::<MsgA>());
        g.add_producer(
            a,
            MessageType::of::<MsgA>(),
            vec![MessageType::of::<Orphan>()],
        );
        let err = g.build().unwrap_err();
        match err {
            BuildError::MissingConsumer { message_type } => {
                assert!(message_type.contains("Orphan"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    /// Invariant: actor registry subscriptions participate in build_graph
    #[test]
    fn build_graph_actor_satisfies_handler_production() {
        struct S;

        let mut handlers = HandlerRegistry::new();
        handlers
            .on(|_ctx: &mut HandlerCtx<'_>, _m: &MsgA| {})
            .produces::<MsgB>();

        let mut actors = ActorRegistry::new();
        actors
            .register("venue", S, |actor| {
                actor.on(|_s, _ctx, _m: &MsgB| {});
            })
            .unwrap();

        let graph = build_graph(&ReducerRegistry::new(), &handlers, &actors).unwrap();
        assert!(graph.has_consumer(TypeId::of::<MsgA>()));
        assert!(graph.has_consumer(TypeId::of::<MsgB>()));
    }

    /// Invariant: actor orphan production fails via build_graph
    #[test]
    fn build_graph_actor_orphan_production() {
        struct S;

        let mut actors = ActorRegistry::new();
        actors
            .register("venue", S, |actor| {
                actor.on(|_s, _ctx, _m: &MsgA| {}).produces::<Orphan>();
            })
            .unwrap();

        let err =
            build_graph(&ReducerRegistry::new(), &HandlerRegistry::new(), &actors).unwrap_err();
        match err {
            BuildError::MissingConsumer { message_type } => {
                assert!(message_type.contains("Orphan"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }
}
