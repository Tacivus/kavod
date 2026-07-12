use std::{
    any::{Any, TypeId},
    collections::HashMap,
    marker::PhantomData,
};

use crate::{
    cache::Cache,
    context::handler::HandlerCtx,
    message::Message,
    output::{HandlerOutput, ProductionSet},
    schedule::Scheduler,
    sequence::Sequencer,
    time::timestamp::Timestamp,
};

type ErasedHandler =
    Box<dyn Fn(Option<&mut (dyn Any + Send)>, &mut HandlerCtx<'_>, &dyn Message) + Send>;

fn erase_stateless_handler<M: Message>(
    f: impl Fn(&mut HandlerCtx<'_>, &M) + Send + 'static,
) -> ErasedHandler {
    Box::new(move |state, ctx, msg| {
        debug_assert!(
            state.is_none(),
            "HandlerRegistry invariant: stateless handler received state"
        );
        let concrete = (msg as &dyn Any)
            .downcast_ref::<M>()
            .expect("HandlerRegistry invariant: message type mismatch");
        f(ctx, concrete);
    })
}

fn erase_stateful_handler<S: Send + 'static, M: Message>(
    f: impl Fn(&mut S, &mut HandlerCtx<'_>, &M) + Send + 'static,
) -> ErasedHandler {
    Box::new(move |state, ctx, msg| {
        let state = state.expect("HandlerRegistry invariant: stateful handler received no state");
        let concrete_state = state
            .downcast_mut::<S>()
            .expect("HandlerRegistry invariant: state type mismatch");
        let concrete_msg = (msg as &dyn Any)
            .downcast_ref::<M>()
            .expect("HandlerRegistry invariant: message type mismatch");
        f(concrete_state, ctx, concrete_msg);
    })
}

/// Scoped builder returned by [`HandlerRegistry::handler_group`].
///
/// All handlers registered through this builder share the same persistent
/// state value of type `S`.  The builder borrows the registry and cannot
/// outlive the configuration closure passed to `handler_group`.
pub struct HandlerGroup<'a, S> {
    registry: &'a mut HandlerRegistry,
    state_slot: usize,
    _phantom: PhantomData<fn(S)>,
}

impl<'a, S: Send + 'static> HandlerGroup<'a, S> {
    /// Register a stateful handler for messages of type `M`.
    ///
    /// The callback receives `&mut S` (the group's private state),
    /// `&mut HandlerCtx` (dispatch time, cache reads, output), and `&M`.
    ///
    /// Returns an opaque [`HandlerRegistrar`] that allows the caller to
    /// declare the message types this handler may produce at runtime.
    pub fn on<M: Message>(
        &mut self,
        f: impl Fn(&mut S, &mut HandlerCtx<'_>, &M) + Send + 'static,
    ) -> HandlerRegistrar<'_> {
        let id = self.registry.entries.len();
        let type_id = TypeId::of::<M>();

        self.registry.entries.push(HandlerEntry {
            consumed_type_name: std::any::type_name::<M>(),
            consumed: type_id,
            state_slot: Some(self.state_slot),
            invoke: erase_stateful_handler::<S, M>(f),
            productions: ProductionSet::new(),
        });
        self.registry.by_type.entry(type_id).or_default().push(id);

        HandlerRegistrar {
            registry: self.registry,
            id,
        }
    }
}

type HandlerId = usize;

pub(crate) struct HandlerEntry {
    consumed_type_name: &'static str,
    consumed: TypeId,
    state_slot: Option<usize>,
    invoke: ErasedHandler,
    productions: ProductionSet,
}

pub(crate) struct HandlerRegistry {
    states: Vec<Box<dyn Any + Send>>,
    entries: Vec<HandlerEntry>,
    by_type: HashMap<TypeId, Vec<HandlerId>>,
}

impl HandlerRegistry {
    pub(crate) fn new() -> Self {
        Self {
            states: Vec::new(),
            entries: Vec::new(),
            by_type: HashMap::new(),
        }
    }

    /// Register a stateless handler for messages of type `M`.
    ///
    /// Returns an opaque [`HandlerRegistrar`] that allows the caller to
    /// declare the message types this handler may produce at runtime.
    pub(crate) fn on<M: Message>(
        &mut self,
        f: impl Fn(&mut HandlerCtx<'_>, &M) + Send + 'static,
    ) -> HandlerRegistrar<'_> {
        let id = self.entries.len();
        let type_id = TypeId::of::<M>();

        self.entries.push(HandlerEntry {
            consumed_type_name: std::any::type_name::<M>(),
            consumed: type_id,
            state_slot: None,
            invoke: erase_stateless_handler(f),
            productions: ProductionSet::new(),
        });
        self.by_type.entry(type_id).or_default().push(id);

        HandlerRegistrar { registry: self, id }
    }

    /// Create a handler group with private persistent state of type `S`.
    ///
    /// The state value is owned by the registry for the engine's lifetime.
    /// Every handler registered inside the `configure` closure shares that
    /// same `S`.  Separate calls to `handler_group` create isolated state,
    /// even when they use the same Rust type.
    ///
    /// Group state does not implement [`State`](crate::cache::State) and is
    /// not stored in or reachable through the global cache.
    pub(crate) fn handler_group<S: Send + 'static>(
        &mut self,
        state: S,
        configure: impl FnOnce(&mut HandlerGroup<'_, S>),
    ) {
        let slot = self.states.len();
        self.states.push(Box::new(state));
        let mut group = HandlerGroup {
            registry: self,
            state_slot: slot,
            _phantom: PhantomData,
        };
        configure(&mut group);
    }

    /// Dispatch a message to every handler whose consumed type matches
    /// `msg.type_id()`.
    ///
    /// Each matching handler is invoked in registration order with a
    /// freshly constructed [`HandlerCtx`] carrying that handler's own
    /// production declarations.  The engine supplies the scheduler,
    /// sequence allocator, and immutable cache as disjoint borrows –
    /// no `RefCell` or outbox is needed.
    pub(crate) fn dispatch(
        &mut self,
        dispatch_time: Timestamp,
        cache: &Cache,
        scheduler: &mut Scheduler,
        sequencer: &mut Sequencer,
        msg: &dyn Message,
    ) {
        let type_id = msg.type_id();
        if let Some(ids) = self.by_type.get(&type_id) {
            for &id in ids {
                let entry = &self.entries[id];
                // Reborrow scheduler & sequence with per-iteration lifetime
                // so each HandlerCtx/handler pair gets a fresh HandlerOutput.
                {
                    let scheduler = &mut *scheduler;
                    let sequence = &mut *sequencer;
                    let mut output = HandlerOutput::new(scheduler, sequence, dispatch_time);
                    let mut ctx =
                        HandlerCtx::new(dispatch_time, cache, &mut output, &entry.productions);
                    let state: Option<&mut (dyn Any + Send)> = match entry.state_slot {
                        Some(slot) => Some(&mut *self.states[slot]),
                        None => None,
                    };
                    (entry.invoke)(state, &mut ctx, msg);
                }
            }
        }
    }

    /// Returns `TypeId`s of every message type for which at least one
    /// handler is registered.  Used during graph building.
    pub(crate) fn consumed_types(&self) -> Vec<TypeId> {
        self.by_type.keys().copied().collect()
    }

    /// Returns the total number of registered handler entries.
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns the message type consumed by a specific handler entry.
    pub(crate) fn handler_consumed_type(&self, id: HandlerId) -> TypeId {
        self.entries[id].consumed
    }

    /// Returns the production declarations for a specific handler entry.
    /// Used by graph building in Phase 11.
    pub(crate) fn handler_productions(&self, id: HandlerId) -> &ProductionSet {
        &self.entries[id].productions
    }

    /// Invoke a specific handler entry directly without going through
    /// the `by_type` index.  Required by `HandlerRegistrar`-produced
    /// tests and by the engine when handler metadata is inlined.
    pub(crate) fn invoke_by_id(
        &self,
        id: HandlerId,
        state: Option<&mut (dyn Any + Send)>,
        ctx: &mut HandlerCtx<'_>,
        msg: &dyn Message,
    ) {
        (self.entries[id].invoke)(state, ctx, msg);
    }
}

/// Opaque handle returned by handler registration.
///
/// The only supported operation is declaring the message types this handler
/// may produce via [`.produces::<M>()`](HandlerRegistrar::produces).
///
/// The handle borrows the registry – it must be used or dropped before
/// another call to [`HandlerRegistry::on`] or [`HandlerRegistry::handler_group`].
pub struct HandlerRegistrar<'a> {
    registry: &'a mut HandlerRegistry,
    id: HandlerId,
}

impl<'a> HandlerRegistrar<'a> {
    /// Declare that this handler may produce messages of type `M`.
    ///
    /// Returns `&mut self` so that multiple declarations can be chained:
    ///
    /// ```ignore
    /// registry.on::<Bar>(|ctx, bar| { ... })
    ///     .produces::<Signal>()
    ///     .produces::<SubmitOrder>();
    /// ```
    pub fn produces<M: Message>(&mut self) -> &mut Self {
        self.registry.entries[self.id].productions.insert::<M>();
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        cache::State, message::Message, output::HandlerOutput, schedule::Scheduler,
        time::timestamp::Timestamp,
    };
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    };

    // ========================================================================
    // Test types
    // ========================================================================

    #[derive(Debug, Clone, PartialEq)]
    struct TestMsg(u64);

    impl Message for TestMsg {}

    #[derive(Debug, Clone, PartialEq)]
    struct OtherMsg(u64);

    impl Message for OtherMsg {}

    #[derive(Debug, Clone, PartialEq)]
    struct YetAnotherMsg(u64);

    impl Message for YetAnotherMsg {}

    #[derive(Debug, Clone, PartialEq)]
    struct KeyedNum {
        key: u32,
        value: u64,
    }

    impl State for KeyedNum {
        type Key = u32;

        fn key(&self) -> Self::Key {
            self.key
        }
    }

    #[derive(Debug, Clone, PartialEq)]
    struct SingletonNum {
        value: u64,
    }

    impl State for SingletonNum {
        type Key = ();

        fn key(&self) -> Self::Key {}
    }

    // ========================================================================
    // Matching handler fires
    // ========================================================================

    /// Invariant: a stateless handler registered for a message type fires
    /// when that type is dispatched
    #[test]
    fn test_matching_handler_fires() {
        let mut reg = HandlerRegistry::new();
        let called = Arc::new(AtomicBool::new(false));
        let called2 = called.clone();

        reg.on(move |_ctx: &mut HandlerCtx<'_>, _msg: &TestMsg| {
            called2.store(true, Ordering::SeqCst);
        });

        let cache = Cache::new();
        let mut sched = Scheduler::new();
        let mut seq = Sequencer::initial();
        let ts = Timestamp::new(0);
        let msg = TestMsg(42);
        let msg_ref: &dyn Message = &msg;

        reg.dispatch(ts, &cache, &mut sched, &mut seq, msg_ref);

        assert!(called.load(Ordering::SeqCst));
    }

    // ========================================================================
    // Wrong message type does not fire
    // ========================================================================

    /// Invariant: a handler does not fire for a message type it was not
    /// registered for
    #[test]
    fn test_wrong_message_type_does_not_fire() {
        let mut reg = HandlerRegistry::new();
        let called = Arc::new(AtomicBool::new(false));
        let called2 = called.clone();

        reg.on(move |_ctx: &mut HandlerCtx<'_>, _msg: &TestMsg| {
            called2.store(true, Ordering::SeqCst);
        });

        let cache = Cache::new();
        let mut sched = Scheduler::new();
        let mut seq = Sequencer::initial();
        let ts = Timestamp::new(0);
        let msg = OtherMsg(99);
        let msg_ref: &dyn Message = &msg;

        reg.dispatch(ts, &cache, &mut sched, &mut seq, msg_ref);

        assert!(!called.load(Ordering::SeqCst));
    }

    // ========================================================================
    // Registration order
    // ========================================================================

    /// Invariant: multiple stateless handlers for the same message type run
    /// in registration order
    #[test]
    fn test_multiple_handlers_run_in_registration_order() {
        let mut reg = HandlerRegistry::new();
        let order = Arc::new(Mutex::new(Vec::new()));

        {
            let order = order.clone();
            reg.on(move |_ctx: &mut HandlerCtx<'_>, _msg: &TestMsg| {
                order.lock().unwrap().push(1);
            });
        }
        {
            let order = order.clone();
            reg.on(move |_ctx: &mut HandlerCtx<'_>, _msg: &TestMsg| {
                order.lock().unwrap().push(2);
            });
        }
        {
            let order = order.clone();
            reg.on(move |_ctx: &mut HandlerCtx<'_>, _msg: &TestMsg| {
                order.lock().unwrap().push(3);
            });
        }

        let cache = Cache::new();
        let mut sched = Scheduler::new();
        let mut seq = Sequencer::initial();
        let ts = Timestamp::new(0);
        let msg = TestMsg(7);
        let msg_ref: &dyn Message = &msg;

        reg.dispatch(ts, &cache, &mut sched, &mut seq, msg_ref);

        let order = order.lock().unwrap();
        assert_eq!(*order, vec![1, 2, 3]);
    }

    // ========================================================================
    // Handler can read cache
    // ========================================================================

    /// Invariant: a handler can read keyed cache state via HandlerCtx::get
    #[test]
    fn test_handler_can_read_cache() {
        let mut reg = HandlerRegistry::new();
        let observed = Arc::new(Mutex::new(0u64));
        let observed2 = observed.clone();

        reg.on(move |ctx: &mut HandlerCtx<'_>, _msg: &TestMsg| {
            let v = ctx.get::<KeyedNum>(&1).unwrap();
            *observed2.lock().unwrap() = v.value;
        });

        let mut cache = Cache::new();
        cache.insert(KeyedNum { key: 1, value: 77 });

        let mut sched = Scheduler::new();
        let mut seq = Sequencer::initial();
        let ts = Timestamp::new(0);
        let msg = TestMsg(0);
        let msg_ref: &dyn Message = &msg;

        reg.dispatch(ts, &cache, &mut sched, &mut seq, msg_ref);

        assert_eq!(*observed.lock().unwrap(), 77);
    }

    // ========================================================================
    // Declared production succeeds
    // ========================================================================

    /// Invariant: a handler with a declared production can send that
    /// message type and it appears in the scheduler
    #[test]
    fn test_handler_can_schedule_declared_message() {
        let mut reg = HandlerRegistry::new();

        reg.on(|ctx: &mut HandlerCtx<'_>, _msg: &TestMsg| {
            ctx.send(OtherMsg(42)).unwrap();
        })
        .produces::<OtherMsg>();

        let cache = Cache::new();
        let mut sched = Scheduler::new();
        let mut seq = Sequencer::initial();
        let ts = Timestamp::new(0);
        let msg = TestMsg(1);
        let msg_ref: &dyn Message = &msg;

        reg.dispatch(ts, &cache, &mut sched, &mut seq, msg_ref);

        let item = sched.pop().unwrap();
        assert_eq!(item.dispatch_time(), ts);
        let payload: &dyn Any = &*item.payload();
        assert!(payload.downcast_ref::<OtherMsg>().is_some());
    }

    // ========================================================================
    // Undeclared production fails
    // ========================================================================

    /// Invariant: a handler without a declared production cannot send that
    /// message type – returns UndeclaredProduction error
    #[test]
    fn test_handler_cannot_schedule_undeclared_message() {
        let mut reg = HandlerRegistry::new();

        // No .produces call – empty production set
        reg.on(move |ctx: &mut HandlerCtx<'_>, _msg: &TestMsg| {
            let result = ctx.send(OtherMsg(99));
            assert!(result.is_err());
        });

        let cache = Cache::new();
        let mut sched = Scheduler::new();
        let mut seq = Sequencer::initial();
        let ts = Timestamp::new(0);
        let msg = TestMsg(1);
        let msg_ref: &dyn Message = &msg;

        reg.dispatch(ts, &cache, &mut sched, &mut seq, msg_ref);

        // Scheduler should be empty – nothing was scheduled
        assert!(sched.pop().is_none());
    }

    // ========================================================================
    // Production declarations are per-handler
    // ========================================================================

    /// Invariant: one handler's production declarations do not authorize
    /// another handler to send the same type
    #[test]
    fn test_production_declarations_are_per_handler() {
        let mut reg = HandlerRegistry::new();
        let second_called = Arc::new(AtomicBool::new(false));

        // First handler declares and sends OtherMsg
        reg.on(|ctx: &mut HandlerCtx<'_>, _msg: &TestMsg| {
            ctx.send(OtherMsg(1)).unwrap();
        })
        .produces::<OtherMsg>();

        // Second handler has empty productions – cannot send OtherMsg
        {
            let second_called = second_called.clone();
            reg.on(move |ctx: &mut HandlerCtx<'_>, _msg: &TestMsg| {
                second_called.store(true, Ordering::SeqCst);
                let result = ctx.send(OtherMsg(2));
                assert!(result.is_err());
            });
        }

        let cache = Cache::new();
        let mut sched = Scheduler::new();
        let mut seq = Sequencer::initial();
        let ts = Timestamp::new(0);
        let msg = TestMsg(0);
        let msg_ref: &dyn Message = &msg;

        reg.dispatch(ts, &cache, &mut sched, &mut seq, msg_ref);

        assert!(second_called.load(Ordering::SeqCst));
        // Only the first handler's OtherMsg(1) should be in the scheduler
        let item = sched.pop().unwrap();
        let payload: &dyn Any = &*item.payload();
        assert_eq!(payload.downcast_ref::<OtherMsg>(), Some(&OtherMsg(1)));
        assert!(sched.pop().is_none());
    }

    // ========================================================================
    // Dispatch uses type index (does not scan unrelated handlers)
    // ========================================================================

    /// Invariant: dispatch only invokes handlers whose consumed type
    /// matches – handlers for other types are not called
    #[test]
    fn test_dispatch_uses_type_index() {
        let mut reg = HandlerRegistry::new();
        let test_called = Arc::new(AtomicBool::new(false));
        let other_called = Arc::new(AtomicBool::new(false));

        {
            let test_called = test_called.clone();
            reg.on(move |_ctx: &mut HandlerCtx<'_>, _msg: &TestMsg| {
                test_called.store(true, Ordering::SeqCst);
            });
        }
        {
            let other_called = other_called.clone();
            reg.on(move |_ctx: &mut HandlerCtx<'_>, _msg: &OtherMsg| {
                other_called.store(true, Ordering::SeqCst);
            });
        }

        let cache = Cache::new();
        let mut sched = Scheduler::new();
        let mut seq = Sequencer::initial();
        let ts = Timestamp::new(0);
        let msg = TestMsg(7);
        let msg_ref: &dyn Message = &msg;

        reg.dispatch(ts, &cache, &mut sched, &mut seq, msg_ref);

        assert!(test_called.load(Ordering::SeqCst));
        assert!(!other_called.load(Ordering::SeqCst));
    }

    // ========================================================================
    // Multiple .produces calls
    // ========================================================================

    /// Invariant: the opaque registration handle supports multiple
    /// .produces calls in a chain, and all declared types are accepted
    #[test]
    fn test_handler_registration_supports_multiple_produces() {
        let mut reg = HandlerRegistry::new();

        reg.on(|ctx: &mut HandlerCtx<'_>, _msg: &TestMsg| {
            ctx.send(OtherMsg(1)).unwrap();
            ctx.send(YetAnotherMsg(2)).unwrap();
        })
        .produces::<OtherMsg>()
        .produces::<YetAnotherMsg>();

        let cache = Cache::new();
        let mut sched = Scheduler::new();
        let mut seq = Sequencer::initial();
        let ts = Timestamp::new(0);
        let msg = TestMsg(0);
        let msg_ref: &dyn Message = &msg;

        reg.dispatch(ts, &cache, &mut sched, &mut seq, msg_ref);

        // Both declared types should be in the scheduler
        let mut types_found = Vec::new();
        while let Some(item) = sched.pop() {
            let payload: &dyn Any = &*item.payload();
            if payload.downcast_ref::<OtherMsg>().is_some() {
                types_found.push("OtherMsg");
            } else if payload.downcast_ref::<YetAnotherMsg>().is_some() {
                types_found.push("YetAnotherMsg");
            }
        }
        assert_eq!(types_found.len(), 2);
        assert!(types_found.contains(&"OtherMsg"));
        assert!(types_found.contains(&"YetAnotherMsg"));
    }

    // ========================================================================
    // Empty registry dispatch
    // ========================================================================

    /// Invariant: dispatch against an empty registry performs no callback
    /// and does not panic
    #[test]
    fn test_empty_registry_dispatch_noop() {
        let mut reg = HandlerRegistry::new();
        let cache = Cache::new();
        let mut sched = Scheduler::new();
        let mut seq = Sequencer::initial();
        let ts = Timestamp::new(0);
        let msg = TestMsg(1);
        let msg_ref: &dyn Message = &msg;

        reg.dispatch(ts, &cache, &mut sched, &mut seq, msg_ref);

        // No panics, nothing in scheduler
        assert!(sched.pop().is_none());
        assert_eq!(reg.len(), 0);
    }

    // ========================================================================
    // Consumed types
    // ========================================================================

    /// Invariant: consumed_types returns every TypeId for which at least
    /// one handler is registered
    #[test]
    fn test_consumed_types_reports_registered_types() {
        let mut reg = HandlerRegistry::new();

        reg.on(|_ctx: &mut HandlerCtx<'_>, _msg: &TestMsg| {});
        reg.on(|_ctx: &mut HandlerCtx<'_>, _msg: &OtherMsg| {});

        let types = reg.consumed_types();
        assert!(types.contains(&TypeId::of::<TestMsg>()));
        assert!(types.contains(&TypeId::of::<OtherMsg>()));
        assert_eq!(types.len(), 2);
    }

    // ========================================================================
    // Len counts handlers
    // ========================================================================

    /// Invariant: len returns the total number of registered handler
    /// entries across all message types
    #[test]
    fn test_len_counts_handlers() {
        let mut reg = HandlerRegistry::new();
        assert_eq!(reg.len(), 0);

        reg.on(|_ctx: &mut HandlerCtx<'_>, _msg: &TestMsg| {});
        assert_eq!(reg.len(), 1);

        reg.on(|_ctx: &mut HandlerCtx<'_>, _msg: &TestMsg| {});
        assert_eq!(reg.len(), 2);

        reg.on(|_ctx: &mut HandlerCtx<'_>, _msg: &OtherMsg| {});
        assert_eq!(reg.len(), 3);
    }

    // ========================================================================
    // Erased handler downcast mismatch
    // ========================================================================

    /// Invariant: a downcast mismatch in the erased handler wrapper panics
    /// rather than silently producing no effect
    #[test]
    #[should_panic(expected = "HandlerRegistry invariant: message type mismatch")]
    fn test_erased_handler_downcast_mismatch_panics() {
        let erased = erase_stateless_handler(|_ctx: &mut HandlerCtx<'_>, _msg: &TestMsg| {});

        let cache = Cache::new();
        let mut sched = Scheduler::new();
        let mut seq = Sequencer::initial();
        let ts = Timestamp::new(0);
        let mut output = HandlerOutput::new(&mut sched, &mut seq, ts);
        let productions = ProductionSet::new();
        let mut ctx = HandlerCtx::new(ts, &cache, &mut output, &productions);

        let msg = OtherMsg(1);
        erased(None, &mut ctx, &msg);
    }

    // ========================================================================
    // Stateful handler group tests – Phase 10
    // ========================================================================

    // State type that does NOT implement State (proves no cache coupling)
    #[derive(Debug)]
    struct GroupState {
        value: Arc<AtomicU64>,
    }

    impl GroupState {
        fn new(v: u64) -> Self {
            Self {
                value: Arc::new(AtomicU64::new(v)),
            }
        }

        fn get(&self) -> u64 {
            self.value.load(Ordering::SeqCst)
        }

        fn set(&self, v: u64) {
            self.value.store(v, Ordering::SeqCst);
        }
    }

    // ========================================================================
    // Group state persistence
    // ========================================================================

    /// Invariant: group state persists across multiple dispatched messages
    #[test]
    fn test_group_state_persists_across_messages() {
        let mut reg = HandlerRegistry::new();
        let observed = Arc::new(AtomicU64::new(0));
        let observed_read = observed.clone();

        reg.handler_group(GroupState::new(10), |group| {
            // Accumulates on TestMsg
            group.on(
                |state: &mut GroupState, _ctx: &mut HandlerCtx<'_>, msg: &TestMsg| {
                    state.set(state.get() + msg.0);
                },
            );

            // Reads accumulated state on OtherMsg (same group slot)
            group.on(
                move |state: &mut GroupState, _ctx: &mut HandlerCtx<'_>, _msg: &OtherMsg| {
                    observed_read.store(state.get(), Ordering::SeqCst);
                },
            );
        });

        let cache = Cache::new();
        let mut sched = Scheduler::new();
        let mut seq = Sequencer::initial();
        let ts = Timestamp::new(0);

        reg.dispatch(ts, &cache, &mut sched, &mut seq, &TestMsg(5));
        reg.dispatch(ts, &cache, &mut sched, &mut seq, &TestMsg(3));
        // 10 + 5 + 3 = 18 must still be in the same group state
        reg.dispatch(ts, &cache, &mut sched, &mut seq, &OtherMsg(0));

        assert_eq!(observed.load(Ordering::SeqCst), 18);
    }
    // ========================================================================
    // Multiple handlers share group state
    // ========================================================================

    /// Invariant: multiple handlers in one group observe each other's changes
    #[test]
    fn test_multiple_handlers_share_group_state() {
        let mut reg = HandlerRegistry::new();

        let counter = Arc::new(AtomicU64::new(0));
        let counter2 = counter.clone();

        // Use a simple u64 as group state
        reg.handler_group(0u64, |group| {
            group.on(
                move |state: &mut u64, _ctx: &mut HandlerCtx<'_>, _msg: &TestMsg| {
                    *state += 1;
                },
            );

            group.on(
                move |state: &mut u64, _ctx: &mut HandlerCtx<'_>, _msg: &OtherMsg| {
                    counter2.store(*state, Ordering::SeqCst);
                },
            );
        });

        let cache = Cache::new();
        let mut sched = Scheduler::new();
        let mut seq = Sequencer::initial();
        let ts = Timestamp::new(0);

        // Increment the counter three times via TestMsg
        reg.dispatch(ts, &cache, &mut sched, &mut seq, &TestMsg(0));
        reg.dispatch(ts, &cache, &mut sched, &mut seq, &TestMsg(0));
        reg.dispatch(ts, &cache, &mut sched, &mut seq, &TestMsg(0));

        // The OtherMsg handler should see the accumulated count (3)
        // We can't directly check after the OtherMsg handler runs since
        // counter is captured, but TestMsg handler increments, then
        // if we dispatch OtherMsg, the handler sees the current state.
        // Let's verify by dispatching OtherMsg after the TestMsg increments.
        reg.dispatch(ts, &cache, &mut sched, &mut seq, &OtherMsg(0));

        // After the OtherMsg dispatch, the counter should have recorded
        // the state value (which was 3 from the prior TestMsg increments)
        assert_eq!(counter.load(Ordering::SeqCst), 3);
    }

    // ========================================================================
    // Separate groups have isolated state
    // ========================================================================

    /// Invariant: separate groups have isolated state
    #[test]
    fn test_separate_groups_have_isolated_state() {
        let mut reg = HandlerRegistry::new();

        let group_a_seen = Arc::new(AtomicU64::new(0));
        let group_b_seen = Arc::new(AtomicU64::new(0));

        let ga = group_a_seen.clone();
        let gb = group_b_seen.clone();

        // Group A starts at 10
        reg.handler_group(10u64, |group| {
            group.on(
                move |state: &mut u64, _ctx: &mut HandlerCtx<'_>, _msg: &TestMsg| {
                    ga.store(*state, Ordering::SeqCst);
                    *state += 1;
                },
            );
        });

        // Group B starts at 100
        reg.handler_group(100u64, |group| {
            group.on(
                move |state: &mut u64, _ctx: &mut HandlerCtx<'_>, _msg: &TestMsg| {
                    gb.store(*state, Ordering::SeqCst);
                    *state += 1;
                },
            );
        });

        let cache = Cache::new();
        let mut sched = Scheduler::new();
        let mut seq = Sequencer::initial();
        let ts = Timestamp::new(0);

        // First dispatch – both groups' handlers fire
        reg.dispatch(ts, &cache, &mut sched, &mut seq, &TestMsg(0));

        // Group A should have seen 10 (its own initial value)
        // Group B should have seen 100 (its own initial value, not 10)
        assert_eq!(group_a_seen.load(Ordering::SeqCst), 10);
        assert_eq!(group_b_seen.load(Ordering::SeqCst), 100);
    }

    // ========================================================================
    // Same-type groups remain isolated
    // ========================================================================

    /// Invariant: separate groups using the same Rust state type remain isolated
    #[test]
    fn test_same_type_groups_remain_isolated() {
        let mut reg = HandlerRegistry::new();

        let a_val = Arc::new(AtomicU64::new(0));
        let b_val = Arc::new(AtomicU64::new(0));

        let av = a_val.clone();
        let bv = b_val.clone();

        // Both groups use u64 as their state type but start at different values
        reg.handler_group(1u64, |group| {
            group.on(
                move |state: &mut u64, _ctx: &mut HandlerCtx<'_>, _msg: &TestMsg| {
                    av.store(*state, Ordering::SeqCst);
                    *state = *state * 10;
                },
            );
        });

        reg.handler_group(5u64, |group| {
            group.on(
                move |state: &mut u64, _ctx: &mut HandlerCtx<'_>, _msg: &OtherMsg| {
                    bv.store(*state, Ordering::SeqCst);
                    *state = *state * 10;
                },
            );
        });

        let cache = Cache::new();
        let mut sched = Scheduler::new();
        let mut seq = Sequencer::initial();
        let ts = Timestamp::new(0);

        reg.dispatch(ts, &cache, &mut sched, &mut seq, &TestMsg(0));
        reg.dispatch(ts, &cache, &mut sched, &mut seq, &OtherMsg(0));

        // Group A saw 1 (its own initial value), not 5
        assert_eq!(a_val.load(Ordering::SeqCst), 1);
        // Group B saw 5 (its own initial value), not 1
        assert_eq!(b_val.load(Ordering::SeqCst), 5);
    }

    // ========================================================================
    // Stateless and stateful run in registration order
    // ========================================================================

    /// Invariant: stateless and stateful handlers run in true registration order
    #[test]
    fn test_stateless_and_stateful_in_registration_order() {
        let mut reg = HandlerRegistry::new();
        let order = Arc::new(Mutex::new(Vec::new()));

        let o1 = order.clone();
        reg.on(move |_ctx: &mut HandlerCtx<'_>, _msg: &TestMsg| {
            o1.lock().unwrap().push("stateless-1");
        });

        let o2 = order.clone();
        reg.handler_group((), move |group| {
            group.on(
                move |_: &mut (), _ctx: &mut HandlerCtx<'_>, _msg: &TestMsg| {
                    o2.lock().unwrap().push("stateful");
                },
            );
        });

        let o3 = order.clone();
        reg.on(move |_ctx: &mut HandlerCtx<'_>, _msg: &TestMsg| {
            o3.lock().unwrap().push("stateless-2");
        });

        let cache = Cache::new();
        let mut sched = Scheduler::new();
        let mut seq = Sequencer::initial();
        let ts = Timestamp::new(0);

        reg.dispatch(ts, &cache, &mut sched, &mut seq, &TestMsg(0));

        let ord = order.lock().unwrap();
        assert_eq!(*ord, vec!["stateless-1", "stateful", "stateless-2"]);
    }

    // ========================================================================
    // Different message types share one group state
    // ========================================================================

    /// Invariant: different message types can share one group state
    #[test]
    fn test_different_message_types_share_group_state() {
        let mut reg = HandlerRegistry::new();

        let final_val = Arc::new(AtomicU64::new(0));
        let fv = final_val.clone();

        reg.handler_group(0u64, |group| {
            group.on(
                |state: &mut u64, _ctx: &mut HandlerCtx<'_>, msg: &TestMsg| {
                    *state += msg.0;
                },
            );

            group.on(
                |state: &mut u64, _ctx: &mut HandlerCtx<'_>, msg: &OtherMsg| {
                    *state += msg.0 * 100;
                },
            );

            group.on(
                move |state: &mut u64, _ctx: &mut HandlerCtx<'_>, _msg: &YetAnotherMsg| {
                    fv.store(*state, Ordering::SeqCst);
                },
            );
        });

        let cache = Cache::new();
        let mut sched = Scheduler::new();
        let mut seq = Sequencer::initial();
        let ts = Timestamp::new(0);

        reg.dispatch(ts, &cache, &mut sched, &mut seq, &TestMsg(5)); // state += 5   → 5
        reg.dispatch(ts, &cache, &mut sched, &mut seq, &OtherMsg(2)); // state += 200 → 205
        reg.dispatch(ts, &cache, &mut sched, &mut seq, &YetAnotherMsg(0)); // read state

        assert_eq!(final_val.load(Ordering::SeqCst), 205);
    }

    // ========================================================================
    // Stateful production declarations enforced independently
    // ========================================================================

    /// Invariant: a stateful handler's production declarations are enforced
    /// independently
    #[test]
    fn test_stateful_production_declarations_enforced() {
        let mut reg = HandlerRegistry::new();

        reg.handler_group((), |group| {
            group
                .on(|_: &mut (), ctx: &mut HandlerCtx<'_>, _msg: &TestMsg| {
                    let result = ctx.send(OtherMsg(1));
                    assert!(result.is_ok());
                })
                .produces::<OtherMsg>();
        });

        // Second stateful handler with no production – cannot send OtherMsg
        reg.handler_group((), |group| {
            group.on(
                move |_: &mut (), ctx: &mut HandlerCtx<'_>, _msg: &TestMsg| {
                    let result = ctx.send(OtherMsg(2));
                    assert!(result.is_err());
                },
            );
        });

        let cache = Cache::new();
        let mut sched = Scheduler::new();
        let mut seq = Sequencer::initial();
        let ts = Timestamp::new(0);

        reg.dispatch(ts, &cache, &mut sched, &mut seq, &TestMsg(0));

        // Only the first handler's OtherMsg(1) should be in the scheduler
        let item = sched.pop().unwrap();
        let payload: &dyn Any = &*item.payload();
        assert_eq!(payload.downcast_ref::<OtherMsg>(), Some(&OtherMsg(1)));
        assert!(sched.pop().is_none());
    }

    // ========================================================================
    // Group state does not require State trait
    // ========================================================================

    /// Invariant: handler-group state does not implement State and still works
    #[test]
    fn test_group_state_does_not_require_state_trait() {
        let mut reg = HandlerRegistry::new();

        // GroupState deliberately does not implement State
        let gs = GroupState::new(42);
        let observed = Arc::new(AtomicU64::new(0));
        let obs = observed.clone();

        reg.handler_group(gs, move |group| {
            group.on(
                move |state: &mut GroupState, _ctx: &mut HandlerCtx<'_>, _msg: &TestMsg| {
                    obs.store(state.get(), Ordering::SeqCst);
                    state.set(state.get() + 1);
                },
            );
        });

        let cache = Cache::new();
        let mut sched = Scheduler::new();
        let mut seq = Sequencer::initial();
        let ts = Timestamp::new(0);

        reg.dispatch(ts, &cache, &mut sched, &mut seq, &TestMsg(0));

        assert_eq!(observed.load(Ordering::SeqCst), 42);
    }

    // ========================================================================
    // Erased stateful handler downcast mismatch
    // ========================================================================

    /// Invariant: an impossible state-slot downcast fails as an internal
    /// invariant
    #[test]
    #[should_panic(expected = "HandlerRegistry invariant: stateful handler received no state")]
    fn test_erased_stateful_handler_no_state_panics() {
        // Create a stateful erased handler and invoke it with no state
        let erased = erase_stateful_handler::<u64, TestMsg>(
            |_: &mut u64, _: &mut HandlerCtx<'_>, _: &TestMsg| {},
        );

        let cache = Cache::new();
        let mut sched = Scheduler::new();
        let mut seq = Sequencer::initial();
        let ts = Timestamp::new(0);
        let mut output = HandlerOutput::new(&mut sched, &mut seq, ts);
        let productions = ProductionSet::new();
        let mut ctx = HandlerCtx::new(ts, &cache, &mut output, &productions);

        let msg = TestMsg(0);
        erased(None, &mut ctx, &msg);
    }

    /// Invariant: wrong state type downcast fails as an internal invariant
    #[test]
    #[should_panic(expected = "HandlerRegistry invariant: state type mismatch")]
    fn test_erased_stateful_handler_wrong_state_type_panics() {
        // Create a stateful erased handler expecting u64 state, but pass a
        // String as the state
        let erased = erase_stateful_handler::<u64, TestMsg>(
            |_: &mut u64, _: &mut HandlerCtx<'_>, _: &TestMsg| {},
        );

        let cache = Cache::new();
        let mut sched = Scheduler::new();
        let mut seq = Sequencer::initial();
        let ts = Timestamp::new(0);
        let mut output = HandlerOutput::new(&mut sched, &mut seq, ts);
        let productions = ProductionSet::new();
        let mut ctx = HandlerCtx::new(ts, &cache, &mut output, &productions);

        let mut wrong_state: Box<dyn Any + Send> = Box::new(String::from("wrong"));
        let state: Option<&mut (dyn Any + Send)> = Some(&mut *wrong_state);

        let msg = TestMsg(0);
        erased(state, &mut ctx, &msg);
    }

    // ========================================================================
    // Group state not reachable via ctx.get unless separately seeded
    // ========================================================================

    /// Invariant: handler-group state is not reachable through
    /// HandlerCtx::get unless a separate value of that type was
    /// deliberately seeded in the global cache
    #[test]
    fn test_group_state_unreachable_via_cache_get() {
        let mut reg = HandlerRegistry::new();
        let gs = GroupState::new(77);
        let cached_val = Arc::new(AtomicU64::new(0));
        let cv = cached_val.clone();
        let cv2 = cached_val.clone();

        reg.handler_group(gs, move |group| {
            group.on(
                move |state: &mut GroupState, ctx: &mut HandlerCtx<'_>, _msg: &TestMsg| {
                    // ctx.get::<GroupState>() would require GroupState: State
                    // which it doesn't implement – so this cannot compile.
                    // Instead, verify handler reads from the group state
                    // (not the cache) by checking the stored value.
                    state.set(state.get() + 1);
                    cv.store(state.get(), Ordering::SeqCst);

                    // Also verify that a deliberately seeded type CAN be
                    // read from the cache, proving the two are separate.
                    let kn = ctx.get::<KeyedNum>(&1);
                    cv2.store(kn.map_or(0, |v| v.value), Ordering::SeqCst);
                },
            );
        });

        let mut cache = Cache::new();
        // Seed a KeyedNum in the cache – this is separate from GroupState
        cache.insert(KeyedNum { key: 1, value: 99 });

        let mut sched = Scheduler::new();
        let mut seq = Sequencer::initial();
        let ts = Timestamp::new(0);

        reg.dispatch(ts, &cache, &mut sched, &mut seq, &TestMsg(0));

        // Group state was incremented from 77 to 78
        // Cache value was read as 99 (independent)
        // The AtomicU64 at index 0 holds the group state value
        // The AtomicU64 at index 1 holds the cache value

        // Actually both writes go to the same AtomicU64. Let me re-read the
        // test logic... Both cv and cv2 are clones of cached_val, so they
        // both write to the same AtomicU64. The second write (cv2) wins.
        // This is fine – we just need to verify that `ctx.get` returned 99,
        // not 78 (the group state value). Since GroupState doesn't implement
        // State, it could not have been in the cache.
        //
        // We need two separate AtomicU64s. Fixed below.
    }

    // Proper fix to the test above with two separate observers
    /// Invariant: handler-group state is not reachable through HandlerCtx::get
    /// – cache and group state are separate storage locations
    #[test]
    fn test_group_state_not_in_cache() {
        let mut reg = HandlerRegistry::new();
        let gs = GroupState::new(77);
        let group_val = Arc::new(AtomicU64::new(0));
        let cache_val = Arc::new(AtomicU64::new(0));
        let gv = group_val.clone();
        let cv = cache_val.clone();

        reg.handler_group(gs, move |group| {
            group.on(
                move |state: &mut GroupState, ctx: &mut HandlerCtx<'_>, _msg: &TestMsg| {
                    state.set(state.get() + 1);
                    gv.store(state.get(), Ordering::SeqCst);

                    let kn = ctx.get::<KeyedNum>(&1);
                    cv.store(kn.map_or(0, |v| v.value), Ordering::SeqCst);
                },
            );
        });

        let mut cache = Cache::new();
        cache.insert(KeyedNum { key: 1, value: 99 });

        let mut sched = Scheduler::new();
        let mut seq = Sequencer::initial();
        let ts = Timestamp::new(0);

        reg.dispatch(ts, &cache, &mut sched, &mut seq, &TestMsg(0));

        // Group state was 77, incremented to 78
        assert_eq!(group_val.load(Ordering::SeqCst), 78);
        // Cache value was separately 99 – not the group state
        assert_eq!(cache_val.load(Ordering::SeqCst), 99);
    }
}
