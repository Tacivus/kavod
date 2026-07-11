use std::{
    any::{Any, TypeId},
    collections::HashSet,
    marker::PhantomData,
};

use crate::{
    cache::{Cache, State},
    clock::Clock,
    context::Context,
    log::SeqNo,
    message::Message,
    time::timestamp::Timestamp,
};

type ErasedStateless = Box<dyn Fn(&mut Context, &dyn Message)>;
type ErasedStateful = Box<dyn Fn(&mut dyn Any, &mut Context, &dyn Message)>;

/// The erased function pointer for a single handler — either stateless
/// (no per-group state) or stateful (state passed as `&mut dyn Any`).
enum HandlerFn {
    Stateless(ErasedStateless),
    Stateful(ErasedStateful),
}

/// A single handler registered for a specific message type, together with
/// the set of message types it declares that it will produce. The
/// registration order within a group determines execution order.
pub(crate) struct HandlerEntry {
    message_type: TypeId,
    handler_fn: HandlerFn,
    produces: HashSet<TypeId>,
}

impl HandlerEntry {
    /// Declare that this handler produces messages of type `M`. Chaining
    /// multiple `.produces()` calls adds to the set; the kernel checks
    /// the set at runtime whenever `ctx.send::<M>()` is called from
    /// inside a handler.
    pub fn produces<M: Message>(&mut self) -> &mut Self {
        self.produces.insert(TypeId::of::<M>());
        self
    }
}

/// A group of zero or more handlers. Groups are the unit of execution
/// ordering: groups fire in registration order, and within a group
/// handlers fire in registration order. A stateless group carries no
/// state; a stateful group owns exactly one `S: State` value shared
/// by every handler in the group.
enum Group {
    Stateless(Vec<HandlerEntry>),
    Stateful {
        state: Box<dyn Any + Send>,
        handlers: Vec<HandlerEntry>,
    },
}

impl Group {
    fn entries(&self) -> &[HandlerEntry] {
        match self {
            Group::Stateless(handlers) => handlers,
            Group::Stateful { handlers, .. } => handlers,
        }
    }
}

/// Registry of all handler groups, stored in registration order.
///
/// Handlers fire after reducers for the same message type. Multiple
/// handlers across different groups that subscribe to the same message
/// type fire in group-registration order; within a group they fire in
/// handler-registration order.
pub(crate) struct HandlerRegistry {
    groups: Vec<Group>,
}

impl HandlerRegistry {
    pub(crate) fn new() -> Self {
        Self { groups: Vec::new() }
    }

    /// Register a stateless handler for message type `M`.
    ///
    /// Returns a `&mut HandlerEntry` so the caller can chain
    /// `.produces::<T>()` declarations immediately after.
    pub(crate) fn on<M: Message>(
        &mut self,
        handler: impl Fn(&mut Context, &M) + 'static,
    ) -> &mut HandlerEntry {
        let erased: ErasedStateless = Box::new(move |ctx, msg| {
            let typed: &M = (msg as &dyn Any)
                .downcast_ref()
                .expect("Handler invoked with wrong message type");
            handler(ctx, typed);
        });

        let entry = HandlerEntry {
            message_type: TypeId::of::<M>(),
            handler_fn: HandlerFn::Stateless(erased),
            produces: HashSet::new(),
        };

        self.push_stateless(entry)
    }

    /// Create a new state group initialised with `init`, returning a
    /// handle that allows `.on::<M>(handler)` calls to register
    /// stateful handlers into the group.
    ///
    /// Handlers registered on the handle share `&mut S`, see each
    /// other's modifications, and fire in registration order.
    pub(crate) fn state<S: State + Send + 'static>(&mut self, init: S) -> StateGroupHandle<'_, S> {
        self.groups.push(Group::Stateful {
            state: Box::new(init),
            handlers: Vec::new(),
        });
        StateGroupHandle {
            registry: self,
            _marker: PhantomData,
        }
    }

    /// Dispatch a message to all registered handlers whose
    /// `message_type` matches `type_id`.
    ///
    /// For each matching handler a `Context` is constructed with that
    /// handler's `produces` set so that `ctx.send` / `ctx.send_at` can
    /// verify declarations at runtime.
    pub(crate) fn dispatch(
        &mut self,
        type_id: TypeId,
        msg: &dyn Message,
        cache: &Cache,
        clock: &dyn Clock,
        seq: &SeqNo,
        outbox: &mut Vec<(Timestamp, Box<dyn Message>)>,
    ) {
        for group in &mut self.groups {
            match group {
                Group::Stateless(handlers) => {
                    for entry in handlers {
                        if entry.message_type == type_id {
                            let mut ctx = Context::new_for_handler(
                                cache,
                                clock,
                                seq,
                                outbox,
                                &entry.produces,
                            );
                            if let HandlerFn::Stateless(ref f) = entry.handler_fn {
                                f(&mut ctx, msg);
                            }
                        }
                    }
                }
                Group::Stateful { state, handlers } => {
                    for entry in handlers {
                        if entry.message_type == type_id {
                            let state_any: &mut dyn Any = state.as_mut();
                            let mut ctx = Context::new_for_handler(
                                cache,
                                clock,
                                seq,
                                outbox,
                                &entry.produces,
                            );
                            if let HandlerFn::Stateful(ref f) = entry.handler_fn {
                                f(state_any, &mut ctx, msg);
                            }
                        }
                    }
                }
            }
        }
    }

    /// Coalesce consecutive stateless `.on()` calls into one group so
    /// they form a single flat list in registration order.
    fn push_stateless(&mut self, entry: HandlerEntry) -> &mut HandlerEntry {
        if matches!(self.groups.last(), Some(Group::Stateless(_))) {
            match self.groups.last_mut() {
                Some(Group::Stateless(handlers)) => {
                    handlers.push(entry);
                    handlers.last_mut().unwrap()
                }
                _ => unreachable!(),
            }
        } else {
            self.groups.push(Group::Stateless(vec![entry]));
            match self.groups.last_mut() {
                Some(Group::Stateless(handlers)) => handlers.last_mut().unwrap(),
                _ => unreachable!(),
            }
        }
    }

    /// Returns every `TypeId` that at least one handler is registered to
    /// receive (via `.on::<M>()`).
    pub(crate) fn subscribed_types(&self) -> HashSet<TypeId> {
        let mut types = HashSet::new();
        for group in &self.groups {
            for entry in group.entries() {
                types.insert(entry.message_type);
            }
        }
        types
    }

    /// Returns every `TypeId` declared via `.produces::<M>()` across all
    /// handlers.
    pub(crate) fn produced_types(&self) -> HashSet<TypeId> {
        let mut types = HashSet::new();
        for group in &self.groups {
            for entry in group.entries() {
                types.extend(&entry.produces);
            }
        }
        types
    }

    /// Returns one `(subscribed_type, produced_set)` pair per handler
    /// entry that declares at least one production. Entries with an empty
    /// `.produces` set are omitted — they contribute no edges to the
    /// message graph.
    pub(crate) fn edges(&self) -> Vec<(TypeId, HashSet<TypeId>)> {
        let mut out = Vec::new();
        for group in &self.groups {
            for entry in group.entries() {
                if !entry.produces.is_empty() {
                    out.push((entry.message_type, entry.produces.clone()));
                }
            }
        }
        out
    }
}

/// Handle returned by [`HandlerRegistry::state`]. Allows chaining
/// `.on::<M>(handler)` calls to register multiple stateful handlers
/// into the newly-created group.
pub(crate) struct StateGroupHandle<'a, S: State + Send + 'static> {
    registry: &'a mut HandlerRegistry,
    _marker: PhantomData<S>,
}

impl<'a, S: State + Send + 'static> StateGroupHandle<'a, S> {
    /// Register a stateful handler for message type `M` that receives
    /// `&mut S` (the group's shared state), `&mut Context`, and `&M`.
    ///
    /// Returns a `&mut HandlerEntry` so the caller can chain
    /// `.produces::<T>()` declarations immediately after.
    pub fn on<M: Message>(
        &mut self,
        handler: impl Fn(&mut S, &mut Context, &M) + 'static,
    ) -> &mut HandlerEntry {
        let erased: ErasedStateful = Box::new(
            move |state: &mut dyn Any, ctx: &mut Context, msg: &dyn Message| {
                let typed_state: &mut S = state
                    .downcast_mut()
                    .expect("Stateful handler received wrong state type");
                let typed_msg: &M = (msg as &dyn Any)
                    .downcast_ref()
                    .expect("Stateful handler invoked with wrong message type");
                handler(typed_state, ctx, typed_msg);
            },
        );

        let entry = HandlerEntry {
            message_type: TypeId::of::<M>(),
            handler_fn: HandlerFn::Stateful(erased),
            produces: HashSet::new(),
        };

        let group = self
            .registry
            .groups
            .last_mut()
            .expect("StateGroupHandle should have a backing group");
        match group {
            Group::Stateful { handlers, .. } => {
                handlers.push(entry);
                handlers.last_mut().unwrap()
            }
            _ => unreachable!("state() always creates a Stateful group"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::sim::SimClock;

    // ==================================================================
    // Test types
    // ==================================================================

    #[derive(Clone, Debug, PartialEq)]
    struct Bar {
        instrument: u32,
        price: i64,
    }
    impl Message for Bar {}

    #[derive(Clone, Debug, PartialEq)]
    struct Signal {
        strength: f64,
    }
    impl Message for Signal {}

    #[derive(Clone, Debug, PartialEq)]
    struct NewOrder {
        instrument: u32,
        quantity: i64,
    }
    impl Message for NewOrder {}

    #[derive(Clone, Debug, PartialEq)]
    struct OtherMsg {
        data: u64,
    }
    impl Message for OtherMsg {}

    #[derive(Clone, Debug, PartialEq)]
    struct SmaCounter {
        count: u64,
    }
    impl State for SmaCounter {
        type Key = ();
        fn key(&self) {}
    }

    #[derive(Clone, Debug, PartialEq)]
    struct OrderTracker {
        orders: u64,
    }
    impl State for OrderTracker {
        type Key = ();
        fn key(&self) {}
    }

    #[derive(Clone, Debug, PartialEq)]
    struct CachedEntry {
        id: u32,
        value: i64,
    }
    impl State for CachedEntry {
        type Key = u32;
        fn key(&self) -> u32 {
            self.id
        }
    }

    // ==================================================================
    // Helpers
    // ==================================================================

    fn seq(n: u64) -> SeqNo {
        let mut s = SeqNo::initial();
        for _ in 0..n {
            s = s.next();
        }
        s
    }

    // ==================================================================
    // Stateless handler
    // ==================================================================

    /// Invariant: a stateless handler fires when dispatched with its
    /// message type and produces the declared message.
    #[test]
    fn stateless_handler_fires_on_matching_type() {
        let mut reg = HandlerRegistry::new();

        reg.on::<Bar>(|ctx, bar| {
            ctx.send(Signal {
                strength: bar.price as f64,
            });
        })
        .produces::<Signal>();

        let cache = Cache::new();
        let clock = SimClock::new(Timestamp::new(1000));
        let s = seq(0);
        let mut outbox = Vec::new();

        reg.dispatch(
            TypeId::of::<Bar>(),
            &Bar {
                instrument: 1,
                price: 100,
            },
            &cache,
            &clock,
            &s,
            &mut outbox,
        );

        assert_eq!(outbox.len(), 1);
        let payload: &dyn Any = &*outbox[0].1;
        let sig = payload.downcast_ref::<Signal>().unwrap();
        assert_eq!(sig.strength, 100.0);
    }

    /// Invariant: a stateless handler does not fire for a message type
    /// it is not registered for.
    #[test]
    fn stateless_handler_ignores_wrong_type() {
        let mut reg = HandlerRegistry::new();

        reg.on::<Bar>(|ctx, _bar| {
            ctx.send(Signal { strength: 1.0 });
        })
        .produces::<Signal>();

        let cache = Cache::new();
        let clock = SimClock::new(Timestamp::new(0));
        let s = seq(0);
        let mut outbox = Vec::new();

        reg.dispatch(
            TypeId::of::<OtherMsg>(),
            &OtherMsg { data: 42 },
            &cache,
            &clock,
            &s,
            &mut outbox,
        );

        assert!(outbox.is_empty());
    }

    // ==================================================================
    // Stateful handler
    // ==================================================================

    /// Invariant: per-handler state persists across multiple dispatches.
    #[test]
    fn stateful_handler_state_persists() {
        let mut reg = HandlerRegistry::new();

        reg.state(SmaCounter { count: 0 })
            .on::<Bar>(|state, ctx, _bar| {
                state.count += 1;
                ctx.send(Signal {
                    strength: state.count as f64,
                });
            })
            .produces::<Signal>();

        let cache = Cache::new();
        let clock = SimClock::new(Timestamp::new(0));
        let s = seq(0);
        let mut outbox = Vec::new();

        reg.dispatch(
            TypeId::of::<Bar>(),
            &Bar {
                instrument: 1,
                price: 100,
            },
            &cache,
            &clock,
            &s,
            &mut outbox,
        );
        reg.dispatch(
            TypeId::of::<Bar>(),
            &Bar {
                instrument: 1,
                price: 200,
            },
            &cache,
            &clock,
            &s,
            &mut outbox,
        );

        assert_eq!(outbox.len(), 2);
        let sig1: &dyn Any = &*outbox[0].1;
        assert_eq!(sig1.downcast_ref::<Signal>().unwrap().strength, 1.0);
        let sig2: &dyn Any = &*outbox[1].1;
        assert_eq!(sig2.downcast_ref::<Signal>().unwrap().strength, 2.0);
    }

    /// Invariant: two state groups maintain independent state.
    #[test]
    fn state_isolated_between_groups() {
        let mut reg = HandlerRegistry::new();

        reg.state(SmaCounter { count: 100 })
            .on::<Bar>(|state, ctx, _bar| {
                state.count += 1;
                ctx.send(Signal {
                    strength: state.count as f64,
                });
            })
            .produces::<Signal>();

        reg.state(OrderTracker { orders: 0 })
            .on::<Bar>(|state, ctx, _bar| {
                state.orders += 1;
                ctx.send(NewOrder {
                    instrument: 1,
                    quantity: state.orders as i64,
                });
            })
            .produces::<NewOrder>();

        let cache = Cache::new();
        let clock = SimClock::new(Timestamp::new(0));
        let s = seq(0);
        let mut outbox = Vec::new();

        reg.dispatch(
            TypeId::of::<Bar>(),
            &Bar {
                instrument: 1,
                price: 50,
            },
            &cache,
            &clock,
            &s,
            &mut outbox,
        );

        assert_eq!(outbox.len(), 2);

        let sig: &dyn Any = &*outbox[0].1;
        assert_eq!(sig.downcast_ref::<Signal>().unwrap().strength, 101.0);

        let ord: &dyn Any = &*outbox[1].1;
        assert_eq!(ord.downcast_ref::<NewOrder>().unwrap().quantity, 1);
    }

    // ==================================================================
    // Ordering
    // ==================================================================

    /// Invariant: handlers in the same state group run in registration
    /// order for the same message type.
    #[test]
    fn handlers_in_same_group_run_in_order() {
        let mut reg = HandlerRegistry::new();

        let mut group = reg.state(SmaCounter { count: 0 });

        group
            .on::<Bar>(|state, ctx, _bar| {
                state.count += 1;
                ctx.send(Signal {
                    strength: state.count as f64,
                });
            })
            .produces::<Signal>();

        group
            .on::<Bar>(|state, ctx, _bar| {
                state.count += 1;
                ctx.send(NewOrder {
                    instrument: 1,
                    quantity: state.count as i64,
                });
            })
            .produces::<NewOrder>();

        group
            .on::<Bar>(|state, ctx, _bar| {
                state.count += 1;
                ctx.send(Signal {
                    strength: state.count as f64,
                });
            })
            .produces::<Signal>();

        let cache = Cache::new();
        let clock = SimClock::new(Timestamp::new(0));
        let s = seq(0);
        let mut outbox = Vec::new();

        reg.dispatch(
            TypeId::of::<Bar>(),
            &Bar {
                instrument: 1,
                price: 0,
            },
            &cache,
            &clock,
            &s,
            &mut outbox,
        );

        assert_eq!(outbox.len(), 3);
        let s0: &dyn Any = &*outbox[0].1;
        assert_eq!(s0.downcast_ref::<Signal>().unwrap().strength, 1.0);
        let o: &dyn Any = &*outbox[1].1;
        assert_eq!(o.downcast_ref::<NewOrder>().unwrap().quantity, 2);
        let s1: &dyn Any = &*outbox[2].1;
        assert_eq!(s1.downcast_ref::<Signal>().unwrap().strength, 3.0);
    }

    /// Invariant: stateless and stateful handlers interleaved in
    /// registration order both fire for the same message.
    #[test]
    fn stateless_and_stateful_interleaved() {
        let mut reg = HandlerRegistry::new();

        reg.on::<Bar>(|ctx, _bar| {
            ctx.send(Signal { strength: 10.0 });
        })
        .produces::<Signal>();

        reg.state(SmaCounter { count: 0 })
            .on::<Bar>(|state, ctx, _bar| {
                state.count = 999;
                ctx.send(NewOrder {
                    instrument: 1,
                    quantity: state.count as i64,
                });
            })
            .produces::<NewOrder>();

        reg.on::<Bar>(|ctx, _bar| {
            ctx.send(Signal { strength: 20.0 });
        })
        .produces::<Signal>();

        let cache = Cache::new();
        let clock = SimClock::new(Timestamp::new(0));
        let s = seq(0);
        let mut outbox = Vec::new();

        reg.dispatch(
            TypeId::of::<Bar>(),
            &Bar {
                instrument: 1,
                price: 0,
            },
            &cache,
            &clock,
            &s,
            &mut outbox,
        );

        assert_eq!(outbox.len(), 3);
        let s0: &dyn Any = &*outbox[0].1;
        assert_eq!(s0.downcast_ref::<Signal>().unwrap().strength, 10.0);
        let o: &dyn Any = &*outbox[1].1;
        assert_eq!(o.downcast_ref::<NewOrder>().unwrap().quantity, 999);
        let s1: &dyn Any = &*outbox[2].1;
        assert_eq!(s1.downcast_ref::<Signal>().unwrap().strength, 20.0);
    }

    // ==================================================================
    // Productions
    // ==================================================================

    /// Invariant: ctx.send with a declared `.produces` succeeds without
    /// panicking.
    #[test]
    fn send_with_declared_produces_works() {
        let mut reg = HandlerRegistry::new();

        reg.on::<Bar>(|ctx, bar| {
            ctx.send(Signal {
                strength: bar.price as f64,
            });
        })
        .produces::<Signal>();

        let cache = Cache::new();
        let clock = SimClock::new(Timestamp::new(0));
        let s = seq(0);
        let mut outbox = Vec::new();

        reg.dispatch(
            TypeId::of::<Bar>(),
            &Bar {
                instrument: 1,
                price: 42,
            },
            &cache,
            &clock,
            &s,
            &mut outbox,
        );

        assert_eq!(outbox.len(), 1);
    }

    /// Invariant: ctx.send without a declared `.produces` panics.
    #[test]
    #[should_panic(expected = "did not declare")]
    fn send_without_produces_panics() {
        let mut reg = HandlerRegistry::new();

        reg.on::<Bar>(|ctx, _bar| {
            ctx.send(Signal { strength: 1.0 });
        });
        // NOTE: no .produces::<Signal>() — should panic on send

        let cache = Cache::new();
        let clock = SimClock::new(Timestamp::new(0));
        let s = seq(0);
        let mut outbox = Vec::new();

        reg.dispatch(
            TypeId::of::<Bar>(),
            &Bar {
                instrument: 1,
                price: 0,
            },
            &cache,
            &clock,
            &s,
            &mut outbox,
        );
    }

    /// Invariant: ctx.send_at without a declared `.produces` also panics.
    #[test]
    #[should_panic(expected = "did not declare")]
    fn send_at_without_produces_panics() {
        let mut reg = HandlerRegistry::new();

        reg.on::<Bar>(|ctx, _bar| {
            ctx.send_at(Timestamp::new(5000), Signal { strength: 1.0 });
        });
        // NOTE: no .produces::<Signal>()

        let cache = Cache::new();
        let clock = SimClock::new(Timestamp::new(0));
        let s = seq(0);
        let mut outbox = Vec::new();

        reg.dispatch(
            TypeId::of::<Bar>(),
            &Bar {
                instrument: 1,
                price: 0,
            },
            &cache,
            &clock,
            &s,
            &mut outbox,
        );
    }

    // ==================================================================
    // Cache read
    // ==================================================================

    /// Invariant: a handler can read the cache via ctx.get().
    #[test]
    fn handler_reads_cache_via_ctx_get() {
        let mut reg = HandlerRegistry::new();
        let mut cache = Cache::new();

        cache.insert(CachedEntry { id: 1, value: 500 });

        reg.on::<Bar>(|ctx, bar| {
            let entry = ctx.get::<CachedEntry>(&bar.instrument).unwrap();
            ctx.send(Signal {
                strength: entry.value as f64,
            });
        })
        .produces::<Signal>();

        let clock = SimClock::new(Timestamp::new(0));
        let s = seq(0);
        let mut outbox = Vec::new();

        reg.dispatch(
            TypeId::of::<Bar>(),
            &Bar {
                instrument: 1,
                price: 0,
            },
            &cache,
            &clock,
            &s,
            &mut outbox,
        );

        let sig: &dyn Any = &*outbox[0].1;
        assert_eq!(sig.downcast_ref::<Signal>().unwrap().strength, 500.0);
    }

    /// Invariant: produced messages appear in the outbox with the
    /// current clock timestamp.
    #[test]
    fn handler_produces_messages_with_now_timestamp() {
        let mut reg = HandlerRegistry::new();

        reg.on::<Bar>(|ctx, _bar| {
            ctx.send(Signal { strength: 1.0 });
            ctx.send_at(
                Timestamp::new(9999),
                NewOrder {
                    instrument: 1,
                    quantity: 5,
                },
            );
        })
        .produces::<Signal>()
        .produces::<NewOrder>();

        let cache = Cache::new();
        let clock = SimClock::new(Timestamp::new(3000));
        let s = seq(0);
        let mut outbox = Vec::new();

        reg.dispatch(
            TypeId::of::<Bar>(),
            &Bar {
                instrument: 1,
                price: 0,
            },
            &cache,
            &clock,
            &s,
            &mut outbox,
        );

        assert_eq!(outbox.len(), 2);
        assert_eq!(outbox[0].0, Timestamp::new(3000));
        assert_eq!(outbox[1].0, Timestamp::new(9999));
    }

    // ==================================================================
    // Edge cases
    // ==================================================================

    /// Invariant: dispatch on an empty registry is a no-op.
    #[test]
    fn empty_registry_dispatch_is_noop() {
        let mut reg = HandlerRegistry::new();

        let cache = Cache::new();
        let clock = SimClock::new(Timestamp::new(0));
        let s = seq(0);
        let mut outbox = Vec::new();

        reg.dispatch(
            TypeId::of::<Bar>(),
            &Bar {
                instrument: 1,
                price: 0,
            },
            &cache,
            &clock,
            &s,
            &mut outbox,
        );

        assert!(outbox.is_empty());
    }

    /// Invariant: a new HandlerRegistry has no groups.
    #[test]
    fn new_registry_has_no_groups() {
        let reg = HandlerRegistry::new();
        assert!(reg.groups.is_empty());
    }
}
