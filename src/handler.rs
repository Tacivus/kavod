use std::{
    any::{Any, TypeId},
    collections::HashMap,
};

use crate::{
    cache::Cache,
    context::handler::HandlerCtx,
    message::Message,
    output::{HandlerOutput, ProductionSet},
    schedule::Scheduler,
    sequence::Sequence,
    time::timestamp::Timestamp,
};

// ---------------------------------------------------------------------------
// Erased handler type – single shape for stateless and (later) stateful
// ---------------------------------------------------------------------------

type ErasedHandler =
    Box<dyn Fn(Option<&mut (dyn Any + Send)>, &mut HandlerCtx<'_>, &dyn Message) + Send>;

// ---------------------------------------------------------------------------
// Erasure helpers
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// HandlerId – index into the flat entries vector
// ---------------------------------------------------------------------------

type HandlerId = usize;

// ---------------------------------------------------------------------------
// HandlerEntry (crate-private)
// ---------------------------------------------------------------------------

pub(crate) struct HandlerEntry {
    consumed_type_name: &'static str,
    consumed: TypeId,
    state_slot: Option<usize>,
    invoke: ErasedHandler,
    productions: ProductionSet,
}

// ---------------------------------------------------------------------------
// HandlerRegistry (crate-private)
// ---------------------------------------------------------------------------

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
        sequence: &mut Sequence,
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
                    let sequence = &mut *sequence;
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

// ---------------------------------------------------------------------------
// HandlerRegistrar – opaque public handle for production declarations
// ---------------------------------------------------------------------------

/// Opaque handle returned by handler registration.
///
/// The only supported operation is declaring the message types this handler
/// may produce via [`.produces::<M>()`](HandlerRegistrar::produces).
///
/// The handle borrows the registry – it must be used or dropped before
/// another call to [`HandlerRegistry::on`].
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

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        cache::State, message::Message, output::HandlerOutput, schedule::Scheduler,
        sequence::Sequence, time::timestamp::Timestamp,
    };
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
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
        let mut seq = Sequence::initial();
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
        let mut seq = Sequence::initial();
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
        let mut seq = Sequence::initial();
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
        let mut seq = Sequence::initial();
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
        let mut seq = Sequence::initial();
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
        let mut seq = Sequence::initial();
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
        let mut seq = Sequence::initial();
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
        let mut seq = Sequence::initial();
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
        let mut seq = Sequence::initial();
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
        let mut seq = Sequence::initial();
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
        let mut seq = Sequence::initial();
        let ts = Timestamp::new(0);
        let mut output = HandlerOutput::new(&mut sched, &mut seq, ts);
        let productions = ProductionSet::new();
        let mut ctx = HandlerCtx::new(ts, &cache, &mut output, &productions);

        let msg = OtherMsg(1);
        erased(None, &mut ctx, &msg);
    }
}
