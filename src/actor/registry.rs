use std::{
    any::{Any, TypeId},
    collections::{HashMap, HashSet},
    marker::PhantomData,
    num::NonZeroUsize,
};

#[cfg(test)]
use crate::time::Timestamp;
use crate::{
    actor::config::ActorConfig,
    context::actor::ActorCtx,
    error::BuildError,
    message::Message,
    output::{MessageType, ProductionSet},
};

/// Erased actor callback. Stored for Phase 18+; not invoked by Engine::run yet.
type ErasedActorCallback =
    Box<dyn Fn(&mut (dyn Any + Send), &mut ActorCtx<'_>, &dyn Message) + Send>;

fn erase_actor_callback<A: Send + 'static, M: Message>(
    f: impl Fn(&mut A, &mut ActorCtx<'_>, &M) + Send + 'static,
) -> ErasedActorCallback {
    Box::new(move |state, ctx, msg| {
        let concrete_state = state
            .downcast_mut::<A>()
            .expect("ActorRegistry invariant: state type mismatch");
        let concrete_msg = (msg as &dyn Any)
            .downcast_ref::<M>()
            .expect("ActorRegistry invariant: message type mismatch");
        f(concrete_state, ctx, concrete_msg);
    })
}

struct ActorCallbackEntry {
    consumed_type_name: &'static str,
    consumed: TypeId,
    productions: ProductionSet,
    /// Stored for Phase 18+; never called in Phase 16.
    #[allow(dead_code)]
    invoke: ErasedActorCallback,
}

struct ActorEntry {
    /// Stable name; diagnostics / live metrics later.
    #[allow(dead_code)]
    name: &'static str,
    /// Deterministic delivery order (Phase 18+).
    #[allow(dead_code)]
    registration_order: usize,
    config: ActorConfig,
    /// Private actor state; not invoked in Phase 16.
    #[allow(dead_code)]
    state: Box<dyn Any + Send>,
    callbacks: Vec<ActorCallbackEntry>,
}

/// Build-time and runtime storage for actor declarations.
///
/// Phase 16: metadata + graph participation only.
/// Phase 18: inline dispatch will use registration order and `by_type`.
pub(crate) struct ActorRegistry {
    actors: Vec<ActorEntry>,
    names: HashSet<&'static str>,
    /// (actor_index, callback_index) in actor-then-callback registration order.
    /// Unused until Phase 18 dispatch.
    #[allow(dead_code)]
    by_type: HashMap<TypeId, Vec<(usize, usize)>>,
}

impl ActorRegistry {
    pub(crate) fn new() -> Self {
        Self {
            actors: Vec::new(),
            names: HashSet::new(),
            by_type: HashMap::new(),
        }
    }

    /// Register an actor with unique `name` and private state `A`.
    pub(crate) fn register<A: Send + 'static>(
        &mut self,
        name: &'static str,
        state: A,
        configure: impl FnOnce(&mut ActorBuilder<'_, A>),
    ) -> Result<(), BuildError> {
        if !self.names.insert(name) {
            return Err(BuildError::DuplicateRegistrationIdentity { name });
        }

        let actor_index = self.actors.len();
        self.actors.push(ActorEntry {
            name,
            registration_order: actor_index,
            config: ActorConfig::new(),
            state: Box::new(state),
            callbacks: Vec::new(),
        });

        let mut builder = ActorBuilder {
            registry: self,
            actor_index,
            _phantom: PhantomData,
        };
        configure(&mut builder);
        Ok(())
    }

    /// Flat (consumed, productions) in actor registration order, then
    /// callback registration order within each actor.
    pub(crate) fn graph_entries(&self) -> impl Iterator<Item = (MessageType, &ProductionSet)> + '_ {
        self.actors.iter().flat_map(|actor| {
            actor.callbacks.iter().map(|cb| {
                (
                    MessageType {
                        id: cb.consumed,
                        name: cb.consumed_type_name,
                    },
                    &cb.productions,
                )
            })
        })
    }
}

#[cfg(test)]
impl ActorRegistry {
    pub(crate) fn actor_count(&self) -> usize {
        self.actors.len()
    }

    pub(crate) fn callback_count(&self) -> usize {
        self.actors.iter().map(|a| a.callbacks.len()).sum()
    }

    pub(crate) fn actor_name(&self, index: usize) -> &'static str {
        self.actors[index].name
    }

    pub(crate) fn actor_config(&self, index: usize) -> &ActorConfig {
        &self.actors[index].config
    }

    pub(crate) fn registration_orders(&self) -> Vec<usize> {
        self.actors.iter().map(|a| a.registration_order).collect()
    }

    /// Ordered list of consumed type names for subscription-order tests.
    pub(crate) fn consumed_type_names(&self) -> Vec<&'static str> {
        self.graph_entries().map(|(mt, _)| mt.name).collect()
    }

    /// Invoke first callback of actor 0 for tests (Phase 17 only).
    pub(crate) fn invoke_first_for_test(
        &mut self,
        dispatch_time: Timestamp,
        sink: &mut dyn crate::actor::output::ActorOutputSink,
        msg: &dyn Message,
    ) {
        let actor = &mut self.actors[0];
        let cb = &actor.callbacks[0];
        let mut ctx = ActorCtx::new(dispatch_time, sink, &cb.productions);
        (cb.invoke)(actor.state.as_mut(), &mut ctx, msg);
    }
}

/// Scoped builder returned by [`EngineBuilder::actor`](crate::builder::EngineBuilder::actor).
///
/// Users configure capacity and subscribe with `.on`; they never construct
/// mailboxes, channels, or handles.
pub struct ActorBuilder<'a, A> {
    registry: &'a mut ActorRegistry,
    actor_index: usize,
    _phantom: PhantomData<fn(A)>,
}

impl<'a, A: Send + 'static> ActorBuilder<'a, A> {
    pub fn inbox_capacity(&mut self, capacity: usize) -> &mut Self {
        let n = NonZeroUsize::new(capacity).expect("inbox_capacity must be non-zero");
        self.registry.actors[self.actor_index]
            .config
            .set_inbox_capacity(n);
        self
    }

    /// Subscribe this actor to messages of type `M`.
    ///
    /// Counts as a graph consumer of `M`. Callbacks are not executed until
    /// inline actor dispatch (Phase 18).
    pub fn on<M: Message>(
        &mut self,
        f: impl Fn(&mut A, &mut ActorCtx<'_>, &M) + Send + 'static,
    ) -> ActorRegistrar<'_> {
        let type_id = TypeId::of::<M>();
        let callback_index = self.registry.actors[self.actor_index].callbacks.len();

        self.registry.actors[self.actor_index]
            .callbacks
            .push(ActorCallbackEntry {
                consumed_type_name: std::any::type_name::<M>(),
                consumed: type_id,
                productions: ProductionSet::new(),
                invoke: erase_actor_callback::<A, M>(f),
            });

        self.registry
            .by_type
            .entry(type_id)
            .or_default()
            .push((self.actor_index, callback_index));

        ActorRegistrar {
            registry: self.registry,
            actor_index: self.actor_index,
            callback_index,
        }
    }
}

/// Opaque handle for declaring actor productions.
pub struct ActorRegistrar<'a> {
    registry: &'a mut ActorRegistry,
    actor_index: usize,
    callback_index: usize,
}

impl ActorRegistrar<'_> {
    /// Declare that this actor callback may produce messages of type `M`.
    pub fn produces<M: Message>(&mut self) -> &mut Self {
        self.registry.actors[self.actor_index].callbacks[self.callback_index]
            .productions
            .insert::<M>();
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::Message;
    use std::sync::atomic::{AtomicU64, Ordering};

    // ========================================================================
    // Test types
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

    struct Venue;

    // ========================================================================
    // Names
    // ========================================================================

    /// Invariant: duplicate actor names fail with DuplicateRegistrationIdentity
    #[test]
    fn test_duplicate_actor_names_fail() {
        let mut reg = ActorRegistry::new();
        reg.register("venue", Venue, |_a| {}).unwrap();
        let err = reg.register("venue", Venue, |_a| {}).unwrap_err();
        assert!(matches!(
            err,
            BuildError::DuplicateRegistrationIdentity { name: "venue" }
        ));
    }

    /// Invariant: distinct actor names register successfully
    #[test]
    fn test_distinct_actor_names_ok() {
        let mut reg = ActorRegistry::new();
        reg.register("a", Venue, |_a| {}).unwrap();
        reg.register("b", Venue, |_a| {}).unwrap();
        assert_eq!(reg.actor_count(), 2);
        assert_eq!(reg.actor_name(0), "a");
        assert_eq!(reg.actor_name(1), "b");
    }

    // ========================================================================
    // Subscriptions / order
    // ========================================================================

    /// Invariant: actor subscription metadata preserves registration order
    #[test]
    fn test_subscription_order_preserved() {
        let mut reg = ActorRegistry::new();
        reg.register("first", Venue, |actor| {
            actor.on(|_s, _ctx, _m: &MsgB| {});
            actor.on(|_s, _ctx, _m: &MsgA| {});
        })
        .unwrap();
        reg.register("second", Venue, |actor| {
            actor.on(|_s, _ctx, _m: &MsgC| {});
        })
        .unwrap();

        let names = reg.consumed_type_names();
        assert!(names[0].contains("MsgB"));
        assert!(names[1].contains("MsgA"));
        assert!(names[2].contains("MsgC"));
        assert_eq!(reg.registration_orders(), vec![0, 1]);
    }

    /// Invariant: .produces populates ProductionSet for graph export
    #[test]
    fn test_produces_appears_in_graph_entries() {
        let mut reg = ActorRegistry::new();
        reg.register("venue", Venue, |actor| {
            actor
                .on(|_s, _ctx, _m: &MsgA| {})
                .produces::<MsgB>()
                .produces::<MsgC>();
        })
        .unwrap();

        let entries: Vec<_> = reg.graph_entries().collect();
        assert_eq!(entries.len(), 1);
        assert!(entries[0].0.name.contains("MsgA"));
        let produced: Vec<_> = entries[0].1.iter().map(|m| m.name).collect();
        assert!(produced.iter().any(|n| n.contains("MsgB")));
        assert!(produced.iter().any(|n| n.contains("MsgC")));
    }

    // ========================================================================
    // Capacity
    // ========================================================================

    /// Invariant: inbox capacity is stored on the actor config
    #[test]
    fn test_inbox_capacity_stored() {
        let mut reg = ActorRegistry::new();
        reg.register("venue", Venue, |actor| {
            actor.inbox_capacity(4_096);
        })
        .unwrap();
        assert_eq!(
            reg.actor_config(0).inbox_capacity().map(|n| n.get()),
            Some(4_096)
        );
    }

    /// Invariant: zero inbox capacity panics
    #[test]
    #[should_panic(expected = "inbox_capacity must be non-zero")]
    fn test_inbox_capacity_rejects_zero() {
        let mut reg = ActorRegistry::new();
        let _ = reg.register("venue", Venue, |actor| {
            actor.inbox_capacity(0);
        });
    }

    // ========================================================================
    // No execution
    // ========================================================================

    /// Invariant: registering and exporting graph metadata does not invoke callbacks
    #[test]
    fn test_callbacks_not_invoked_on_register() {
        static HITS: AtomicU64 = AtomicU64::new(0);
        let mut reg = ActorRegistry::new();
        reg.register("venue", Venue, |actor| {
            actor.on(|_s, _ctx, _m: &MsgA| {
                HITS.fetch_add(1, Ordering::SeqCst);
            });
        })
        .unwrap();
        let _ = reg.graph_entries().count();
        assert_eq!(HITS.load(Ordering::SeqCst), 0);
        assert_eq!(reg.callback_count(), 1);
    }

    /// Invariant: erased callback runs with ActorCtx; declared send hits sink
    #[test]
    fn test_erased_callback_invokes_with_actor_ctx() {
        use crate::actor::output::{ActorEmission, RecordingActorSink};
        use crate::time::Timestamp;
        use std::any::Any;
        use std::sync::atomic::{AtomicU64, Ordering};

        #[derive(Debug, PartialEq)]
        struct Out(u64);
        impl Message for Out {}

        static HITS: AtomicU64 = AtomicU64::new(0);
        let mut reg = ActorRegistry::new();
        reg.register("venue", Venue, |actor| {
            actor
                .on(|_s, ctx, _m: &MsgA| {
                    HITS.fetch_add(1, Ordering::SeqCst);
                    ctx.send(Out(9)).unwrap();
                })
                .produces::<Out>();
        })
        .unwrap();

        let mut sink = RecordingActorSink::new();
        reg.invoke_first_for_test(Timestamp::new(10), &mut sink, &MsgA);
        assert_eq!(HITS.load(Ordering::SeqCst), 1);
        assert_eq!(sink.emissions.len(), 1);
        match &sink.emissions[0] {
            ActorEmission::Immediate { payload } => {
                let p: &dyn Any = &**payload;
                assert_eq!(p.downcast_ref::<Out>(), Some(&Out(9)));
            }
            ActorEmission::At { .. } => panic!("expected Immediate"),
        }
    }
}
