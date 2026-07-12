use crate::{
    ActorBuilder,
    actor::ActorRegistry,
    cache::{Cache, State},
    clock::SimClock,
    config::{EngineConfig, Mode},
    context::{handler::HandlerCtx, reducer::ReducerCtx},
    engine::Engine,
    error::BuildError,
    graph::build_graph,
    handler::{HandlerGroup, HandlerRegistrar, HandlerRegistry},
    message::Message,
    reducer::ReducerRegistry,
    schedule::Scheduler,
    sequence::Sequencer,
};

/// Configuration-time registration surface.
///
/// Owns seeded cache state, reducer/handler registrations, and engine config.
/// Call [`build`](EngineBuilder::build) to validate topology and produce an
/// [`Engine`]. After `build`, the topology is immutable.
pub struct EngineBuilder {
    config: EngineConfig,
    seed_cache: Cache,
    reducers: ReducerRegistry,
    handlers: HandlerRegistry,
    actors: ActorRegistry,
}

impl EngineBuilder {
    pub(crate) fn new(config: EngineConfig) -> Self {
        Self {
            config,
            seed_cache: Cache::new(),
            reducers: ReducerRegistry::new(),
            handlers: HandlerRegistry::new(),
            actors: ActorRegistry::new(),
        }
    }

    /// Seed global cache state. Duplicate `(type, key)` is a build error.
    pub fn seed<T: State>(&mut self, value: T) -> Result<&mut Self, BuildError> {
        self.seed_cache
            .try_insert(value)
            .map_err(|e| BuildError::DuplicateSeededState {
                type_name: e.type_name,
            })?;
        Ok(self)
    }

    /// Register a reducer for messages of type `M`.
    pub fn reduce<M: Message>(
        &mut self,
        f: impl Fn(&mut ReducerCtx<'_>, &M) + Send + 'static,
    ) -> &mut Self {
        self.reducers.register(f);
        self
    }

    /// Register a stateless handler for messages of type `M`.
    ///
    /// Chain [`.produces::<T>()`](HandlerRegistrar::produces) for each type
    /// the handler may emit.
    pub fn on<M: Message>(
        &mut self,
        f: impl Fn(&mut HandlerCtx<'_>, &M) + Send + 'static,
    ) -> HandlerRegistrar<'_> {
        self.handlers.on(f)
    }

    /// Register a handler group with private persistent state of type `S`.
    ///
    /// `S` need not implement [`State`](crate::cache::State). Separate calls
    /// create isolated state even when `S` is the same Rust type.
    pub fn handler_group<S: Send + 'static>(
        &mut self,
        state: S,
        configure: impl FnOnce(&mut HandlerGroup<'_, S>),
    ) {
        self.handlers.handler_group(state, configure);
    }

    /// Register an actor with private state and declarative configuration.
    ///
    /// `name` must be unique. Users configure capacity and subscriptions;
    /// they never construct mailboxes, channels, or handles.
    ///
    /// Actor callbacks are stored for later execution phases but are not
    /// invoked by the Phase 16 engine loop.
    pub fn actor<A: Send + 'static>(
        &mut self,
        name: &'static str,
        state: A,
        configure: impl FnOnce(&mut ActorBuilder<'_, A>),
    ) -> Result<&mut Self, BuildError> {
        self.actors.register(name, state, configure)?;
        Ok(self)
    }

    /// Validate topology, freeze registrations, and construct an [`Engine`].
    ///
    /// - Rejects non-backtest modes until those runtimes exist.
    /// - Validates every declared production has a consumer.
    /// - Initializes logical time and `SimClock` from config.
    /// - Leaves the scheduler empty (no queued events).
    pub fn build(self) -> Result<Engine, BuildError> {
        match self.config.mode() {
            Mode::Backtest => {}
            Mode::Live => {
                return Err(BuildError::UnsupportedMode { mode: "Live" });
            }
            Mode::Replay => {
                return Err(BuildError::UnsupportedMode { mode: "Replay" });
            }
        }

        let graph = build_graph(&self.reducers, &self.handlers, &self.actors)?;
        let dispatch_time = self.config.initial_dispatch_time();

        Ok(Engine {
            config: self.config,
            scheduler: Scheduler::new(),
            sequence: Sequencer::initial(),
            cache: self.seed_cache,
            reducers: self.reducers,
            handlers: self.handlers,
            actors: self.actors,
            graph,
            dispatch_time,
            clock: Box::new(SimClock::from_ts(dispatch_time)),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{cache::State, message::Message, time::Timestamp};
    use std::any::TypeId;

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
    struct Orphan;

    impl Message for Orphan {}

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

    struct GroupState {
        n: u64,
    }

    // ========================================================================
    // Empty / mode
    // ========================================================================

    /// Invariant: an empty backtest builder builds successfully
    #[test]
    fn test_empty_builder_builds() {
        let engine = Engine::builder(EngineConfig::backtest(Timestamp::new(0)))
            .build()
            .unwrap();
        assert_eq!(engine.scheduler_len(), 0);
        assert_eq!(engine.dispatch_time(), Timestamp::new(0));
    }

    /// Invariant: live and replay config constructors report unsupported mode
    #[test]
    fn test_unsupported_modes_fail_at_config() {
        assert!(matches!(
            EngineConfig::live(Timestamp::new(0)),
            Err(BuildError::UnsupportedMode { mode: "Live" })
        ));
        assert!(matches!(
            EngineConfig::replay(Timestamp::new(0)),
            Err(BuildError::UnsupportedMode { mode: "Replay" })
        ));
    }

    // ========================================================================
    // Seeding
    // ========================================================================

    /// Invariant: seeded keyed state appears in the built engine cache
    #[test]
    fn test_seeded_keyed_state_in_engine() {
        let mut builder = Engine::builder(EngineConfig::backtest(Timestamp::new(0)));
        builder.seed(KeyedNum { key: 7, value: 42 }).unwrap();
        let engine = builder.build().unwrap();
        assert_eq!(
            engine.cache().get::<KeyedNum>(&7).map(|s| s.value),
            Some(42)
        );
    }

    /// Invariant: seeded singleton state appears in the built engine cache
    #[test]
    fn test_seeded_singleton_state_in_engine() {
        let mut builder = Engine::builder(EngineConfig::backtest(Timestamp::new(0)));
        builder.seed(SingletonNum { value: 99 }).unwrap();
        let engine = builder.build().unwrap();
        assert_eq!(
            engine
                .cache()
                .get_singleton::<SingletonNum>()
                .map(|s| s.value),
            Some(99)
        );
    }

    /// Invariant: duplicate seed returns BuildError::DuplicateSeededState
    #[test]
    fn test_duplicate_seed_returns_build_error() {
        let mut builder = Engine::builder(EngineConfig::backtest(Timestamp::new(0)));
        builder.seed(KeyedNum { key: 1, value: 10 }).unwrap();
        let err = match builder.seed(KeyedNum { key: 1, value: 99 }) {
            Err(e) => e,
            Ok(_) => panic!("expected duplicate seed error"),
        };
        assert!(matches!(
            err,
            BuildError::DuplicateSeededState { type_name } if type_name.contains("KeyedNum")
        ));
    }

    /// Invariant: a failed seed does not leave a replaced value
    #[test]
    fn test_failed_seed_does_not_replace_existing() {
        let mut builder = Engine::builder(EngineConfig::backtest(Timestamp::new(0)));
        builder.seed(KeyedNum { key: 1, value: 10 }).unwrap();
        let _ = builder.seed(KeyedNum { key: 1, value: 99 });
        let engine = builder.build().unwrap();
        assert_eq!(
            engine.cache().get::<KeyedNum>(&1).map(|s| s.value),
            Some(10)
        );
    }

    /// Invariant: multiple distinct seed types/keys all appear in the built engine
    #[test]
    fn test_multiple_distinct_seeds_in_engine() {
        let mut builder = Engine::builder(EngineConfig::backtest(Timestamp::new(0)));
        builder.seed(KeyedNum { key: 1, value: 10 }).unwrap();
        builder.seed(KeyedNum { key: 2, value: 20 }).unwrap();
        builder.seed(SingletonNum { value: 30 }).unwrap();
        let engine = builder.build().unwrap();
        assert_eq!(
            engine.cache().get::<KeyedNum>(&1).map(|s| s.value),
            Some(10)
        );
        assert_eq!(
            engine.cache().get::<KeyedNum>(&2).map(|s| s.value),
            Some(20)
        );
        assert_eq!(
            engine
                .cache()
                .get_singleton::<SingletonNum>()
                .map(|s| s.value),
            Some(30)
        );
    }

    // ========================================================================
    // Registration + graph
    // ========================================================================

    /// Invariant: valid reducer and handler registrations build
    #[test]
    fn test_valid_registrations_build() {
        let mut builder = Engine::builder(EngineConfig::backtest(Timestamp::new(0)));
        builder.reduce(|_ctx: &mut ReducerCtx<'_>, _msg: &MsgA| {});
        builder
            .on(|_ctx: &mut HandlerCtx<'_>, _msg: &MsgB| {})
            .produces::<MsgA>();
        builder.build().unwrap();
    }

    /// Invariant: orphan production prevents build with readable type name
    #[test]
    fn test_orphan_production_prevents_build() {
        let mut builder = Engine::builder(EngineConfig::backtest(Timestamp::new(0)));
        builder
            .on(|_ctx: &mut HandlerCtx<'_>, _msg: &MsgA| {})
            .produces::<Orphan>();
        let err = match builder.build() {
            Err(e) => e,
            Ok(_) => panic!("expected MissingConsumer"),
        };
        match err {
            BuildError::MissingConsumer { message_type } => {
                assert!(
                    message_type.contains("Orphan"),
                    "expected Orphan in error, got {message_type}"
                );
            }
            other => panic!("expected MissingConsumer, got {other:?}"),
        }
    }

    /// Invariant: a reducer alone counts as a consumer for a produced type
    #[test]
    fn test_reducer_satisfies_handler_production() {
        let mut builder = Engine::builder(EngineConfig::backtest(Timestamp::new(0)));
        builder.reduce(|_ctx: &mut ReducerCtx<'_>, _msg: &MsgB| {});
        builder
            .on(|_ctx: &mut HandlerCtx<'_>, _msg: &MsgA| {})
            .produces::<MsgB>();
        let engine = builder.build().unwrap();
        assert!(engine.has_consumer(TypeId::of::<MsgB>()));
        assert!(engine.has_consumer(TypeId::of::<MsgA>()));
    }

    /// Invariant: handler-group registrations participate in build validation
    #[test]
    fn test_handler_group_registrations_build() {
        let mut builder = Engine::builder(EngineConfig::backtest(Timestamp::new(0)));
        builder.reduce(|_ctx: &mut ReducerCtx<'_>, _msg: &MsgB| {});
        builder.handler_group(GroupState { n: 0 }, |group| {
            group
                .on(
                    |state: &mut GroupState, _ctx: &mut HandlerCtx<'_>, _msg: &MsgA| {
                        state.n += 1;
                    },
                )
                .produces::<MsgB>();
        });
        builder.build().unwrap();
    }

    /// Invariant: terminal consumer with no productions builds
    #[test]
    fn test_terminal_handler_builds() {
        let mut builder = Engine::builder(EngineConfig::backtest(Timestamp::new(0)));
        builder.on(|_ctx: &mut HandlerCtx<'_>, _msg: &MsgA| {});
        builder.build().unwrap();
    }

    // ========================================================================
    // Builder / Engine separation
    // ========================================================================

    /// Invariant: build consumes the builder by value
    #[test]
    fn test_build_consumes_builder() {
        let builder = Engine::builder(EngineConfig::backtest(Timestamp::new(0)));
        let _engine = builder.build().unwrap();
        // builder is moved; cannot call build again
    }

    /// Invariant: built engine has empty scheduler and config initial time
    #[test]
    fn test_engine_starts_with_empty_scheduler_and_initial_time() {
        let t0 = Timestamp::new(9_600);
        let engine = Engine::builder(EngineConfig::backtest(t0)).build().unwrap();
        assert_eq!(engine.scheduler_len(), 0);
        assert_eq!(engine.dispatch_time(), t0);
        assert_eq!(engine.config().initial_dispatch_time(), t0);
        assert_eq!(engine.config().mode(), Mode::Backtest);
    }

    /// Invariant: SimClock starts at config initial_dispatch_time
    #[test]
    fn test_engine_clock_initialized_from_config() {
        let t0 = Timestamp::new(1_234_567);
        let engine = Engine::builder(EngineConfig::backtest(t0)).build().unwrap();
        assert_eq!(engine.clock_now(), t0);
    }

    /// Invariant: validated graph is available for runtime consumer checks
    #[test]
    fn test_built_engine_exposes_consumer_set() {
        let mut builder = Engine::builder(EngineConfig::backtest(Timestamp::new(0)));
        builder.on(|_ctx: &mut HandlerCtx<'_>, _msg: &MsgA| {});
        let engine = builder.build().unwrap();
        assert!(engine.has_consumer(TypeId::of::<MsgA>()));
        assert!(!engine.has_consumer(TypeId::of::<MsgB>()));
    }

    /// Invariant: max_events_per_instant from config is preserved on the engine
    #[test]
    fn test_max_events_per_instant_preserved() {
        let config = EngineConfig::backtest(Timestamp::new(0)).with_max_events_per_instant(500);
        let engine = Engine::builder(config).build().unwrap();
        assert_eq!(engine.config().max_events_per_instant(), 500);
    }

    // ========================================================================
    // Actors (metadata only)
    // ========================================================================

    struct VenueState;

    // Invariant: duplicate actor names fail registration
    #[test]
    fn test_duplicate_actor_names_fail() {
        let mut builder = Engine::builder(EngineConfig::backtest(Timestamp::new(0)));
        builder.actor("venue", VenueState, |_a| {}).unwrap();
        let err = match builder.actor("venue", VenueState, |_a| {}) {
            Err(e) => e,
            Ok(_) => panic!("expected DuplicateRegistrationIdentity"),
        };
        assert!(matches!(
            err,
            BuildError::DuplicateRegistrationIdentity { name: "venue" }
        ));
    }

    /// Invariant: actor .on satisfies orphan handler production
    #[test]
    fn test_actor_satisfies_handler_orphan_production() {
        let mut builder = Engine::builder(EngineConfig::backtest(Timestamp::new(0)));
        builder
            .on(|_ctx: &mut HandlerCtx<'_>, _msg: &MsgA| {})
            .produces::<MsgB>();
        builder
            .actor("venue", VenueState, |actor| {
                actor.on(|_s, _ctx, _msg: &MsgB| {});
            })
            .unwrap();
        let engine = builder.build().unwrap();
        assert!(engine.has_consumer(TypeId::of::<MsgB>()));
    }

    /// Invariant: actor production with no consumer fails build
    #[test]
    fn test_actor_orphan_production_prevents_build() {
        let mut builder = Engine::builder(EngineConfig::backtest(Timestamp::new(0)));
        builder
            .actor("venue", VenueState, |actor| {
                actor.on(|_s, _ctx, _msg: &MsgA| {}).produces::<Orphan>();
            })
            .unwrap();
        let err = match builder.build() {
            Err(e) => e,
            Ok(_) => panic!("expected MissingConsumer"),
        };
        match err {
            BuildError::MissingConsumer { message_type } => {
                assert!(message_type.contains("Orphan"));
            }
            other => panic!("expected MissingConsumer, got {other:?}"),
        }
    }

    /// Invariant: actor-only terminal consumer builds and is in the consumer set
    #[test]
    fn test_actor_terminal_consumer_builds() {
        let mut builder = Engine::builder(EngineConfig::backtest(Timestamp::new(0)));
        builder
            .actor("venue", VenueState, |actor| {
                actor.inbox_capacity(64);
                actor.on(|_s, _ctx, _msg: &MsgA| {});
            })
            .unwrap();
        let engine = builder.build().unwrap();
        assert!(engine.has_consumer(TypeId::of::<MsgA>()));
    }
}
