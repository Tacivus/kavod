use std::{any::TypeId, sync::Arc};

use crate::{
    builder::EngineBuilder,
    cache::Cache,
    clock::Clock,
    config::EngineConfig,
    context::reducer::ReducerCtx,
    error::EngineError,
    graph::ValidatedGraph,
    handler::HandlerRegistry,
    message::{Message, SharedMessage},
    reducer::ReducerRegistry,
    schedule::Scheduler,
    sequence::Sequencer,
    time::timestamp::Timestamp,
};

/// Runtime engine after topology freeze.
///
/// Constructed only via [`EngineBuilder::build`]. Registration methods are not
/// available on this type. External ingress is available via [`Engine::push_event`].
/// Process queued work with [`Engine::run`].
///
/// Field layout is flat for borrow splitting across reducers (mutable cache)
/// and handlers (immutable cache + mutable scheduler/sequence). Actors are Phase 16+.
pub struct Engine {
    pub(crate) config: EngineConfig,
    pub(crate) scheduler: Scheduler,
    pub(crate) sequence: Sequencer,
    pub(crate) cache: Cache,
    pub(crate) reducers: ReducerRegistry,
    pub(crate) handlers: HandlerRegistry,
    pub(crate) graph: ValidatedGraph,
    pub(crate) dispatch_time: Timestamp,
    pub(crate) clock: Box<dyn Clock>,
}

impl Engine {
    /// Start configuration with the given engine config.
    pub fn builder(config: EngineConfig) -> EngineBuilder {
        EngineBuilder::new(config)
    }

    /// Accept external input into the scheduler.
    ///
    /// Validates that `M` has at least one registered consumer and that
    /// `dispatch_time` is not before the engine's current logical time.
    /// On success, allocates exactly one kernel sequence and enqueues the
    /// message. Does not dispatch reducers or handlers.
    pub fn push_event<M: Message>(
        &mut self,
        dispatch_time: Timestamp,
        message: M,
    ) -> Result<(), EngineError> {
        if !self.graph.has_consumer(TypeId::of::<M>()) {
            return Err(EngineError::UnconsumedIngress {
                message_type: std::any::type_name::<M>(),
            });
        }
        if dispatch_time < self.dispatch_time {
            return Err(EngineError::PastEvent {
                requested: dispatch_time,
                current: self.dispatch_time,
            });
        }
        self.enqueue_validated(dispatch_time, Arc::new(message))
    }

    /// Drain the scheduler until empty.
    ///
    /// For each message:
    /// 1. Pop earliest item
    /// 2. Enforce monotonic dispatch time and same-instant bound
    /// 3. Set logical `dispatch_time` (and `SimClock`) from the item
    /// 4. Run matching reducers (mutable cache)
    /// 5. Run matching handlers (immutable cache; direct scheduling)
    ///
    /// Produced messages are not dispatched recursively; they re-enter only
    /// via a later pop. Actors are not delivered in this phase.
    pub fn run(&mut self) -> Result<(), EngineError> {
        let max_events = self.config.max_events_per_instant();
        let mut same_instant_count = 0usize;

        while let Some(item) = self.scheduler.pop() {
            let t = item.dispatch_time();
            let payload = item.payload();

            if t < self.dispatch_time {
                return Err(EngineError::PastEvent {
                    requested: t,
                    current: self.dispatch_time,
                });
            }

            if t == self.dispatch_time {
                same_instant_count = same_instant_count.saturating_add(1);
            } else {
                same_instant_count = 1;
            }
            if same_instant_count > max_events {
                return Err(EngineError::SameInstantLimitExceeded { max_events });
            }

            self.dispatch_time = t;
            self.clock.set(t);

            debug_assert!(
                self.graph.has_consumer(payload.as_ref().type_id()),
                "internal invariant: scheduled message has no consumer"
            );

            {
                let mut rctx = ReducerCtx::new(t, &mut self.cache);
                self.reducers.dispatch(&mut rctx, payload.as_ref());
            }

            self.handlers.dispatch(
                t,
                &self.cache,
                &mut self.scheduler,
                &mut self.sequence,
                payload.as_ref(),
            );
        }

        Ok(())
    }

    /// Kernel-owned path for validated scheduling and sequence assignment.
    ///
    /// Callers must already enforce consumer and causality checks when required.
    /// Handler and actor output paths may reuse this later.
    pub(crate) fn enqueue_validated(
        &mut self,
        dispatch_time: Timestamp,
        payload: SharedMessage,
    ) -> Result<(), EngineError> {
        let seq = self.sequence.next()?;
        self.scheduler.push_shared_msg(dispatch_time, seq, payload);
        Ok(())
    }
}

#[cfg(test)]
impl Engine {
    pub(crate) fn cache(&self) -> &Cache {
        &self.cache
    }

    pub(crate) fn dispatch_time(&self) -> Timestamp {
        self.dispatch_time
    }

    pub(crate) fn scheduler_len(&self) -> usize {
        self.scheduler.len()
    }

    pub(crate) fn clock_now(&self) -> Timestamp {
        self.clock.now()
    }

    pub(crate) fn config(&self) -> &EngineConfig {
        &self.config
    }

    pub(crate) fn has_consumer(&self, type_id: TypeId) -> bool {
        self.graph.has_consumer(type_id)
    }

    pub(crate) fn sequence_current(&self) -> crate::sequence::SeqNo {
        self.sequence.get()
    }

    pub(crate) fn pop_scheduled(&mut self) -> Option<crate::schedule::ScheduledItem> {
        self.scheduler.pop()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        cache::State, context::handler::HandlerCtx, context::reducer::ReducerCtx, message::Message,
        sequence::Sequencer, time::timestamp::Timestamp,
    };
    use std::{
        any::Any,
        sync::{
            Arc, Mutex,
            atomic::{AtomicBool, AtomicUsize, Ordering},
        },
    };

    // ========================================================================
    // Test types
    // ========================================================================

    #[derive(Debug, Clone, PartialEq)]
    struct MsgA(u64);

    impl Message for MsgA {}

    #[derive(Debug, Clone, PartialEq)]
    struct MsgB(u64);

    impl Message for MsgB {}

    #[derive(Debug, Clone, PartialEq)]
    struct MsgC(u64);

    impl Message for MsgC {}

    #[derive(Debug, Clone, PartialEq)]
    struct SeededNum {
        value: u64,
    }

    impl State for SeededNum {
        type Key = ();

        fn key(&self) -> Self::Key {}
    }

    #[derive(Debug, Clone, PartialEq)]
    struct Counter {
        value: u64,
    }

    impl State for Counter {
        type Key = ();

        fn key(&self) -> Self::Key {}
    }

    fn engine_with_msg_a_consumer(t0: Timestamp) -> Engine {
        let mut builder = Engine::builder(EngineConfig::backtest(t0));
        builder.on(|_ctx: &mut HandlerCtx<'_>, _msg: &MsgA| {});
        builder.build().unwrap()
    }

    // ========================================================================
    // Accepted ingress
    // ========================================================================

    /// Invariant: consumed input is accepted and enqueued
    #[test]
    fn test_consumed_input_is_accepted() {
        let mut engine = engine_with_msg_a_consumer(Timestamp::new(0));
        engine.push_event(Timestamp::new(0), MsgA(1)).unwrap();
        assert_eq!(engine.scheduler_len(), 1);
    }

    /// Invariant: input at the current logical dispatch time is accepted
    #[test]
    fn test_input_at_current_time_is_accepted() {
        let t0 = Timestamp::new(100);
        let mut engine = engine_with_msg_a_consumer(t0);
        engine.push_event(t0, MsgA(1)).unwrap();
        assert_eq!(engine.scheduler_len(), 1);
        let item = engine.pop_scheduled().unwrap();
        assert_eq!(item.dispatch_time(), t0);
    }

    /// Invariant: input in the future relative to current logical time is accepted
    #[test]
    fn test_input_in_the_future_is_accepted() {
        let t0 = Timestamp::new(100);
        let future = Timestamp::new(200);
        let mut engine = engine_with_msg_a_consumer(t0);
        engine.push_event(future, MsgA(1)).unwrap();
        let item = engine.pop_scheduled().unwrap();
        assert_eq!(item.dispatch_time(), future);
    }

    // ========================================================================
    // Rejected ingress
    // ========================================================================

    /// Invariant: unconsumed input is rejected before scheduling
    #[test]
    fn test_unconsumed_input_is_rejected() {
        let mut engine = Engine::builder(EngineConfig::backtest(Timestamp::new(0)))
            .build()
            .unwrap();
        let err = engine.push_event(Timestamp::new(0), MsgA(1)).unwrap_err();
        assert!(matches!(
            err,
            EngineError::UnconsumedIngress { message_type }
                if message_type.contains("MsgA")
        ));
        assert_eq!(engine.scheduler_len(), 0);
    }

    /// Invariant: input before the current logical dispatch time is rejected
    #[test]
    fn test_input_before_current_logical_time_is_rejected() {
        let current = Timestamp::new(100);
        let past = Timestamp::new(50);
        let mut engine = engine_with_msg_a_consumer(current);
        let err = engine.push_event(past, MsgA(1)).unwrap_err();
        assert_eq!(
            err,
            EngineError::PastEvent {
                requested: past,
                current,
            }
        );
        assert_eq!(engine.scheduler_len(), 0);
    }

    // ========================================================================
    // Sequence and ordering
    // ========================================================================

    /// Invariant: two same-time inputs receive increasing sequences and
    /// preserve insertion order when popped
    #[test]
    fn test_two_same_time_inputs_receive_increasing_sequence_and_preserve_order() {
        let t = Timestamp::new(42);
        let mut engine = engine_with_msg_a_consumer(t);

        engine.push_event(t, MsgA(10)).unwrap();
        engine.push_event(t, MsgA(20)).unwrap();

        let first = engine.pop_scheduled().unwrap();
        let second = engine.pop_scheduled().unwrap();

        assert_eq!(first.dispatch_time(), t);
        assert_eq!(second.dispatch_time(), t);
        assert!(first.sequence_num() < second.sequence_num());

        let first_payload = first.payload();
        let second_payload = second.payload();
        let a: &MsgA = (&*first_payload as &dyn Any)
            .downcast_ref::<MsgA>()
            .unwrap();
        let b: &MsgA = (&*second_payload as &dyn Any)
            .downcast_ref::<MsgA>()
            .unwrap();
        assert_eq!(a, &MsgA(10));
        assert_eq!(b, &MsgA(20));
        assert!(engine.pop_scheduled().is_none());
    }

    /// Invariant: pushed payload round-trips through the scheduler
    #[test]
    fn test_payload_roundtrips_through_scheduler_after_push() {
        let mut engine = engine_with_msg_a_consumer(Timestamp::new(0));
        engine.push_event(Timestamp::new(7), MsgA(99)).unwrap();
        let item = engine.pop_scheduled().unwrap();
        assert_eq!(item.dispatch_time(), Timestamp::new(7));
        let payload: &dyn Any = &*item.payload();
        assert_eq!(payload.downcast_ref::<MsgA>(), Some(&MsgA(99)));
    }

    // ========================================================================
    // Failure side effects
    // ========================================================================

    /// Invariant: failed unconsumed input does not enqueue or advance sequence
    #[test]
    fn test_failed_unconsumed_input_does_not_enqueue_or_advance_sequence() {
        let mut engine = Engine::builder(EngineConfig::backtest(Timestamp::new(0)))
            .build()
            .unwrap();
        let before_seq = engine.sequence_current();
        let before_len = engine.scheduler_len();

        let _ = engine.push_event(Timestamp::new(0), MsgA(1));

        assert_eq!(engine.sequence_current(), before_seq);
        assert_eq!(engine.scheduler_len(), before_len);
    }

    /// Invariant: failed past input does not enqueue or advance sequence
    #[test]
    fn test_failed_past_input_does_not_enqueue_or_advance_sequence() {
        let mut engine = engine_with_msg_a_consumer(Timestamp::new(100));
        let before_seq = engine.sequence_current();
        let before_len = engine.scheduler_len();

        let _ = engine.push_event(Timestamp::new(50), MsgA(1));

        assert_eq!(engine.sequence_current(), before_seq);
        assert_eq!(engine.scheduler_len(), before_len);
    }

    /// Invariant: failed input does not mutate the cache
    #[test]
    fn test_failed_input_does_not_mutate_cache() {
        let mut builder = Engine::builder(EngineConfig::backtest(Timestamp::new(100)));
        builder.seed(SeededNum { value: 7 }).unwrap();
        builder.on(|_ctx: &mut HandlerCtx<'_>, _msg: &MsgA| {});
        let mut engine = builder.build().unwrap();

        let _ = engine.push_event(Timestamp::new(0), MsgB(1)); // unconsumed
        let _ = engine.push_event(Timestamp::new(50), MsgA(1)); // past

        assert_eq!(
            engine.cache().get_singleton::<SeededNum>().map(|s| s.value),
            Some(7)
        );
        assert_eq!(engine.scheduler_len(), 0);
    }

    // ========================================================================
    // Diagnostics and edges
    // ========================================================================

    /// Invariant: UnconsumedIngress error Display includes the type name
    #[test]
    fn test_unconsumed_error_contains_type_name() {
        let mut engine = Engine::builder(EngineConfig::backtest(Timestamp::new(0)))
            .build()
            .unwrap();
        let err = engine.push_event(Timestamp::new(0), MsgB(0)).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("MsgB"), "expected MsgB in error, got: {msg}");
    }

    /// Invariant: PastEvent error Display includes both timestamps
    #[test]
    fn test_past_event_error_contains_timestamps() {
        let err = EngineError::PastEvent {
            requested: Timestamp::new(50),
            current: Timestamp::new(100),
        };
        let msg = err.to_string();
        assert!(
            msg.contains("50ns"),
            "expected requested timestamp in error, got: {msg}"
        );
        assert!(
            msg.contains("100ns"),
            "expected current timestamp in error, got: {msg}"
        );
    }

    /// Invariant: sequence exhaustion maps to EngineError and does not enqueue
    #[test]
    fn test_sequence_exhaustion_maps_to_engine_error() {
        let mut engine = engine_with_msg_a_consumer(Timestamp::new(0));
        engine.sequence = Sequencer::from_raw(u64::MAX);

        let err = engine.push_event(Timestamp::new(0), MsgA(1)).unwrap_err();
        assert!(matches!(err, EngineError::SequenceExhaustion));
        assert_eq!(engine.scheduler_len(), 0);
        assert_eq!(
            engine.sequence_current(),
            crate::sequence::SeqNo::from_raw(u64::MAX)
        );
    }

    /// Invariant: a reducer alone is a sufficient consumer for push_event
    #[test]
    fn test_reducer_consumer_accepts_push_event() {
        let mut builder = Engine::builder(EngineConfig::backtest(Timestamp::new(0)));
        builder.reduce(|_ctx: &mut ReducerCtx<'_>, _msg: &MsgA| {});
        let mut engine = builder.build().unwrap();
        engine.push_event(Timestamp::new(0), MsgA(3)).unwrap();
        assert_eq!(engine.scheduler_len(), 1);
    }

    /// Invariant: successful push advances the sequencer by exactly one
    #[test]
    fn test_successful_push_advances_sequence_by_one() {
        let mut engine = engine_with_msg_a_consumer(Timestamp::new(0));
        let before = engine.sequence_current();
        engine.push_event(Timestamp::new(0), MsgA(1)).unwrap();
        assert!(engine.sequence_current() > before);
        assert_eq!(
            engine.sequence_current(),
            crate::sequence::SeqNo::from_raw(1)
        );
    }

    // ========================================================================
    // Phase 14: empty run
    // ========================================================================

    /// Invariant: empty engine run exits cleanly without changing dispatch time
    #[test]
    fn test_empty_engine_run_exits_cleanly() {
        let t0 = Timestamp::new(100);
        let mut engine = Engine::builder(EngineConfig::backtest(t0)).build().unwrap();
        engine.run().unwrap();
        assert_eq!(engine.dispatch_time(), t0);
        assert_eq!(engine.scheduler_len(), 0);
        assert_eq!(engine.clock_now(), t0);
    }

    // ========================================================================
    // Phase 14: reducer / handler basic dispatch
    // ========================================================================

    /// Invariant: reducer-only message updates cache during run
    #[test]
    fn test_reducer_only_updates_cache() {
        let t0 = Timestamp::new(0);
        let mut builder = Engine::builder(EngineConfig::backtest(t0));
        builder.seed(Counter { value: 0 }).unwrap();
        builder.reduce(|ctx: &mut ReducerCtx<'_>, msg: &MsgA| {
            let c = ctx.get_singleton_mut::<Counter>().unwrap();
            c.value += msg.0;
        });
        let mut engine = builder.build().unwrap();
        engine.push_event(t0, MsgA(5)).unwrap();
        engine.run().unwrap();
        assert_eq!(
            engine.cache().get_singleton::<Counter>().map(|c| c.value),
            Some(5)
        );
        assert_eq!(engine.scheduler_len(), 0);
    }

    /// Invariant: handler-only message runs during run
    #[test]
    fn test_handler_only_runs() {
        let t0 = Timestamp::new(0);
        let called = Arc::new(AtomicBool::new(false));
        let called2 = called.clone();

        let mut builder = Engine::builder(EngineConfig::backtest(t0));
        builder.on(move |_ctx: &mut HandlerCtx<'_>, msg: &MsgA| {
            assert_eq!(msg.0, 9);
            called2.store(true, Ordering::SeqCst);
        });
        let mut engine = builder.build().unwrap();
        engine.push_event(t0, MsgA(9)).unwrap();
        engine.run().unwrap();
        assert!(called.load(Ordering::SeqCst));
    }

    /// Invariant: reducers run before handlers for the same message
    #[test]
    fn test_reducers_run_before_handlers() {
        let t0 = Timestamp::new(0);
        let order = Arc::new(Mutex::new(Vec::new()));

        let mut builder = Engine::builder(EngineConfig::backtest(t0));
        builder.seed(Counter { value: 0 }).unwrap();

        let o1 = order.clone();
        builder.reduce(move |ctx: &mut ReducerCtx<'_>, _msg: &MsgA| {
            ctx.get_singleton_mut::<Counter>().unwrap().value = 1;
            o1.lock().unwrap().push("reducer");
        });

        let o2 = order.clone();
        builder.on(move |ctx: &mut HandlerCtx<'_>, _msg: &MsgA| {
            let v = ctx.get_singleton::<Counter>().unwrap().value;
            o2.lock().unwrap().push(if v == 1 {
                "handler_after_reducer"
            } else {
                "handler_before_reducer"
            });
        });

        let mut engine = builder.build().unwrap();
        engine.push_event(t0, MsgA(1)).unwrap();
        engine.run().unwrap();

        assert_eq!(
            *order.lock().unwrap(),
            vec!["reducer", "handler_after_reducer"]
        );
    }

    /// Invariant: multiple reducers preserve registration order
    #[test]
    fn test_multiple_reducers_preserve_order() {
        let t0 = Timestamp::new(0);
        let order = Arc::new(Mutex::new(Vec::new()));

        let mut builder = Engine::builder(EngineConfig::backtest(t0));
        let o1 = order.clone();
        builder.reduce(move |_ctx: &mut ReducerCtx<'_>, _msg: &MsgA| {
            o1.lock().unwrap().push(1u32);
        });
        let o2 = order.clone();
        builder.reduce(move |_ctx: &mut ReducerCtx<'_>, _msg: &MsgA| {
            o2.lock().unwrap().push(2);
        });
        let o3 = order.clone();
        builder.reduce(move |_ctx: &mut ReducerCtx<'_>, _msg: &MsgA| {
            o3.lock().unwrap().push(3);
        });

        let mut engine = builder.build().unwrap();
        engine.push_event(t0, MsgA(0)).unwrap();
        engine.run().unwrap();
        assert_eq!(*order.lock().unwrap(), vec![1u32, 2, 3]);
    }

    /// Invariant: multiple handlers preserve registration order
    #[test]
    fn test_multiple_handlers_preserve_order() {
        let t0 = Timestamp::new(0);
        let order = Arc::new(Mutex::new(Vec::new()));

        let mut builder = Engine::builder(EngineConfig::backtest(t0));
        let o1 = order.clone();
        builder.on(move |_ctx: &mut HandlerCtx<'_>, _msg: &MsgA| {
            o1.lock().unwrap().push(1u32);
        });
        let o2 = order.clone();
        builder.on(move |_ctx: &mut HandlerCtx<'_>, _msg: &MsgA| {
            o2.lock().unwrap().push(2);
        });
        let o3 = order.clone();
        builder.on(move |_ctx: &mut HandlerCtx<'_>, _msg: &MsgA| {
            o3.lock().unwrap().push(3);
        });

        let mut engine = builder.build().unwrap();
        engine.push_event(t0, MsgA(0)).unwrap();
        engine.run().unwrap();
        assert_eq!(*order.lock().unwrap(), vec![1u32, 2, 3]);
    }

    /// Invariant: handler observes completed reducer state for the same message
    #[test]
    fn test_handler_observes_completed_reducer_state() {
        let t0 = Timestamp::new(0);
        let mut builder = Engine::builder(EngineConfig::backtest(t0));
        builder.seed(Counter { value: 0 }).unwrap();
        builder.reduce(|ctx: &mut ReducerCtx<'_>, msg: &MsgA| {
            ctx.get_singleton_mut::<Counter>().unwrap().value = msg.0 * 10;
        });
        let seen = Arc::new(AtomicUsize::new(0));
        let seen2 = seen.clone();
        builder.on(move |ctx: &mut HandlerCtx<'_>, _msg: &MsgA| {
            let v = ctx.get_singleton::<Counter>().unwrap().value;
            seen2.store(v as usize, Ordering::SeqCst);
        });
        let mut engine = builder.build().unwrap();
        engine.push_event(t0, MsgA(4)).unwrap();
        engine.run().unwrap();
        assert_eq!(seen.load(Ordering::SeqCst), 40);
    }

    // ========================================================================
    // Phase 14: same-time cascades and breadth-first
    // ========================================================================

    /// Invariant: handler same-time send is processed later in the same run
    #[test]
    fn test_handler_same_time_send_processed_later() {
        let t0 = Timestamp::new(100);
        let order = Arc::new(Mutex::new(Vec::new()));

        let mut builder = Engine::builder(EngineConfig::backtest(t0));
        let o1 = order.clone();
        builder
            .on(move |ctx: &mut HandlerCtx<'_>, msg: &MsgA| {
                o1.lock().unwrap().push(format!("A{}", msg.0));
                ctx.send(MsgB(msg.0 + 1)).unwrap();
            })
            .produces::<MsgB>();
        let o2 = order.clone();
        builder.on(move |_ctx: &mut HandlerCtx<'_>, msg: &MsgB| {
            o2.lock().unwrap().push(format!("B{}", msg.0));
        });

        let mut engine = builder.build().unwrap();
        engine.push_event(t0, MsgA(1)).unwrap();
        engine.run().unwrap();
        assert_eq!(
            *order.lock().unwrap(),
            vec!["A1".to_string(), "B2".to_string()]
        );
    }

    /// Invariant: same-time causal chain completes before future time advances
    #[test]
    fn test_same_time_chain_before_future() {
        let t0 = Timestamp::new(100);
        let t1 = Timestamp::new(200);
        let order = Arc::new(Mutex::new(Vec::new()));

        let mut builder = Engine::builder(EngineConfig::backtest(t0));
        let o = order.clone();
        builder
            .on(move |ctx: &mut HandlerCtx<'_>, msg: &MsgA| {
                o.lock().unwrap().push(format!("A{}", msg.0));
                ctx.send(MsgB(msg.0)).unwrap();
            })
            .produces::<MsgB>();
        let o2 = order.clone();
        builder
            .on(move |ctx: &mut HandlerCtx<'_>, msg: &MsgB| {
                o2.lock().unwrap().push(format!("B{}", msg.0));
                ctx.send(MsgC(msg.0)).unwrap();
            })
            .produces::<MsgC>();
        let o3 = order.clone();
        builder.on(move |_ctx: &mut HandlerCtx<'_>, msg: &MsgC| {
            o3.lock().unwrap().push(format!("C{}", msg.0));
        });

        let mut engine = builder.build().unwrap();
        engine.push_event(t0, MsgA(1)).unwrap();
        engine.push_event(t1, MsgA(9)).unwrap();
        engine.run().unwrap();

        assert_eq!(
            *order.lock().unwrap(),
            vec![
                "A1".to_string(),
                "B1".to_string(),
                "C1".to_string(),
                "A9".to_string(),
                "B9".to_string(),
                "C9".to_string(),
            ]
        );
    }

    /// Invariant: existing equal-time ingress stays ahead of newly produced output
    #[test]
    fn test_equal_time_ingress_ahead_of_new_output() {
        let t = Timestamp::new(50);
        let order = Arc::new(Mutex::new(Vec::new()));

        let mut builder = Engine::builder(EngineConfig::backtest(t));
        let o1 = order.clone();
        builder
            .on(move |ctx: &mut HandlerCtx<'_>, msg: &MsgA| {
                o1.lock().unwrap().push(format!("A{}", msg.0));
                if msg.0 == 1 {
                    ctx.send(MsgB(99)).unwrap();
                }
            })
            .produces::<MsgB>();
        let o2 = order.clone();
        builder.on(move |_ctx: &mut HandlerCtx<'_>, msg: &MsgB| {
            o2.lock().unwrap().push(format!("B{}", msg.0));
        });

        let mut engine = builder.build().unwrap();
        engine.push_event(t, MsgA(1)).unwrap();
        engine.push_event(t, MsgA(2)).unwrap();
        engine.run().unwrap();

        assert_eq!(
            *order.lock().unwrap(),
            vec!["A1".to_string(), "A2".to_string(), "B99".to_string()]
        );
    }

    /// Invariant: send_at inserts between surrounding future messages correctly
    #[test]
    fn test_send_at_inserts_between_future_messages() {
        let t0 = Timestamp::new(0);
        let t1 = Timestamp::new(100);
        let t2 = Timestamp::new(200);
        let t3 = Timestamp::new(300);
        let order = Arc::new(Mutex::new(Vec::new()));

        let mut builder = Engine::builder(EngineConfig::backtest(t0));
        let o = order.clone();
        builder
            .on(move |ctx: &mut HandlerCtx<'_>, msg: &MsgA| {
                o.lock().unwrap().push(format!("A{}", msg.0));
                if msg.0 == 0 {
                    ctx.send_at(t2, MsgB(2)).unwrap();
                }
            })
            .produces::<MsgB>();
        let o2 = order.clone();
        builder.on(move |_ctx: &mut HandlerCtx<'_>, msg: &MsgB| {
            o2.lock().unwrap().push(format!("B{}", msg.0));
        });

        let mut engine = builder.build().unwrap();
        engine.push_event(t0, MsgA(0)).unwrap();
        engine.push_event(t1, MsgB(1)).unwrap();
        engine.push_event(t3, MsgB(3)).unwrap();
        engine.run().unwrap();

        assert_eq!(
            *order.lock().unwrap(),
            vec![
                "A0".to_string(),
                "B1".to_string(),
                "B2".to_string(),
                "B3".to_string()
            ]
        );
    }

    // ========================================================================
    // Phase 14: past scheduling / bounds
    // ========================================================================

    /// Invariant: past send_at returns Err in the handler and does not enqueue
    #[test]
    fn test_past_send_at_returns_err_and_does_not_enqueue() {
        let t0 = Timestamp::new(100);
        let past_err = Arc::new(AtomicBool::new(false));
        let past_err2 = past_err.clone();

        let mut builder = Engine::builder(EngineConfig::backtest(t0));
        builder
            .on(move |ctx: &mut HandlerCtx<'_>, _msg: &MsgA| {
                let err = ctx.send_at(Timestamp::new(50), MsgB(1));
                past_err2.store(err.is_err(), Ordering::SeqCst);
            })
            .produces::<MsgB>();
        builder.on(|_ctx: &mut HandlerCtx<'_>, _msg: &MsgB| {
            panic!("MsgB must not be dispatched");
        });

        let mut engine = builder.build().unwrap();
        engine.push_event(t0, MsgA(1)).unwrap();
        engine.run().unwrap();
        assert!(past_err.load(Ordering::SeqCst));
        assert_eq!(engine.scheduler_len(), 0);
    }

    /// Invariant: same-instant bound triggers deterministically
    #[test]
    fn test_same_instant_bound_triggers() {
        let t0 = Timestamp::new(0);
        let config = EngineConfig::backtest(t0).with_max_events_per_instant(2);
        let mut builder = Engine::builder(config);
        builder.on(|_ctx: &mut HandlerCtx<'_>, _msg: &MsgA| {});
        let mut engine = builder.build().unwrap();

        engine.push_event(t0, MsgA(1)).unwrap();
        engine.push_event(t0, MsgA(2)).unwrap();
        engine.push_event(t0, MsgA(3)).unwrap();

        let err = engine.run().unwrap_err();
        assert_eq!(err, EngineError::SameInstantLimitExceeded { max_events: 2 });
    }

    /// Invariant: same-instant bound also applies to handler-produced cascades
    #[test]
    fn test_same_instant_bound_on_handler_cascade() {
        let t0 = Timestamp::new(0);
        let config = EngineConfig::backtest(t0).with_max_events_per_instant(2);
        let mut builder = Engine::builder(config);

        // Each MsgA produces one MsgA at same time → unbounded cascade without the bound.
        builder
            .on(|ctx: &mut HandlerCtx<'_>, msg: &MsgA| {
                if msg.0 < 10 {
                    ctx.send(MsgA(msg.0 + 1)).unwrap();
                }
            })
            .produces::<MsgA>();

        let mut engine = builder.build().unwrap();
        engine.push_event(t0, MsgA(0)).unwrap();

        let err = engine.run().unwrap_err();
        assert_eq!(err, EngineError::SameInstantLimitExceeded { max_events: 2 });
    }

    /// Invariant: unconsumed input is rejected before run (never reaches loop)
    #[test]
    fn test_unconsumed_input_rejected_before_run() {
        let mut engine = Engine::builder(EngineConfig::backtest(Timestamp::new(0)))
            .build()
            .unwrap();
        assert!(engine.push_event(Timestamp::new(0), MsgA(1)).is_err());
        engine.run().unwrap();
        assert_eq!(engine.scheduler_len(), 0);
    }

    /// Invariant: final dispatch time equals the last processed scheduler timestamp
    #[test]
    fn test_final_dispatch_time_equals_last_processed() {
        let t0 = Timestamp::new(10);
        let t_last = Timestamp::new(500);
        let mut builder = Engine::builder(EngineConfig::backtest(t0));
        builder.on(|_ctx: &mut HandlerCtx<'_>, _msg: &MsgA| {});
        let mut engine = builder.build().unwrap();
        engine.push_event(Timestamp::new(100), MsgA(1)).unwrap();
        engine.push_event(t_last, MsgA(2)).unwrap();
        engine.run().unwrap();
        assert_eq!(engine.dispatch_time(), t_last);
        assert_eq!(engine.clock_now(), t_last);
    }

    // ========================================================================
    // Phase 14: clock / dispatch_time stability
    // ========================================================================

    /// Invariant: callback dispatch_time equals the scheduler timestamp
    #[test]
    fn test_ctx_dispatch_time_equals_scheduler_timestamp() {
        let t = Timestamp::new(12345);
        let seen = Arc::new(Mutex::new(None::<Timestamp>));

        let mut builder = Engine::builder(EngineConfig::backtest(Timestamp::new(0)));
        let seen_r = seen.clone();
        builder.reduce(move |ctx: &mut ReducerCtx<'_>, _msg: &MsgA| {
            *seen_r.lock().unwrap() = Some(ctx.dispatch_time());
        });
        let seen_h = seen.clone();
        builder.on(move |ctx: &mut HandlerCtx<'_>, _msg: &MsgA| {
            assert_eq!(*seen_h.lock().unwrap(), Some(ctx.dispatch_time()));
        });
        let mut engine = builder.build().unwrap();
        engine.push_event(t, MsgA(1)).unwrap();
        engine.run().unwrap();
        assert_eq!(*seen.lock().unwrap(), Some(t));
    }

    /// Invariant: two handlers for one message see exactly the same dispatch time
    #[test]
    fn test_two_handlers_see_identical_dispatch_time() {
        let t = Timestamp::new(77);
        let times = Arc::new(Mutex::new(Vec::new()));

        let mut builder = Engine::builder(EngineConfig::backtest(Timestamp::new(0)));
        let t1 = times.clone();
        builder.on(move |ctx: &mut HandlerCtx<'_>, _msg: &MsgA| {
            t1.lock().unwrap().push(ctx.dispatch_time());
        });
        let t2 = times.clone();
        builder.on(move |ctx: &mut HandlerCtx<'_>, _msg: &MsgA| {
            t2.lock().unwrap().push(ctx.dispatch_time());
        });
        let mut engine = builder.build().unwrap();
        engine.push_event(t, MsgA(1)).unwrap();
        engine.run().unwrap();
        let times = times.lock().unwrap();
        assert_eq!(times.len(), 2);
        assert_eq!(times[0], t);
        assert_eq!(times[1], t);
    }

    /// Invariant: callback wall-clock duration does not change logical dispatch time
    #[test]
    fn test_callback_wall_time_does_not_affect_dispatch_time() {
        let t = Timestamp::new(1_000);
        let seen = Arc::new(Mutex::new(Vec::new()));

        let mut builder = Engine::builder(EngineConfig::backtest(Timestamp::new(0)));
        let s1 = seen.clone();
        builder.on(move |ctx: &mut HandlerCtx<'_>, _msg: &MsgA| {
            s1.lock().unwrap().push(ctx.dispatch_time());
            std::thread::sleep(std::time::Duration::from_millis(5));
        });
        let s2 = seen.clone();
        builder.on(move |ctx: &mut HandlerCtx<'_>, _msg: &MsgA| {
            s2.lock().unwrap().push(ctx.dispatch_time());
        });

        let mut engine = builder.build().unwrap();
        engine.push_event(t, MsgA(1)).unwrap();
        engine.run().unwrap();

        let times = seen.lock().unwrap();
        assert_eq!(*times, vec![t, t]);
        assert_eq!(engine.dispatch_time(), t);
        assert_eq!(engine.clock_now(), t);
    }

    /// Invariant: a produced same-time message inherits dispatch time when processed
    #[test]
    fn test_produced_same_time_message_inherits_dispatch_time() {
        let t = Timestamp::new(42);
        let child_time = Arc::new(Mutex::new(None::<Timestamp>));
        let child_time2 = child_time.clone();

        let mut builder = Engine::builder(EngineConfig::backtest(t));
        builder
            .on(move |ctx: &mut HandlerCtx<'_>, _msg: &MsgA| {
                ctx.send(MsgB(1)).unwrap();
            })
            .produces::<MsgB>();
        builder.on(move |ctx: &mut HandlerCtx<'_>, _msg: &MsgB| {
            *child_time2.lock().unwrap() = Some(ctx.dispatch_time());
        });
        let mut engine = builder.build().unwrap();
        engine.push_event(t, MsgA(0)).unwrap();
        engine.run().unwrap();
        assert_eq!(*child_time.lock().unwrap(), Some(t));
    }

    /// Invariant: SimClock tracks last processed dispatch time after run
    #[test]
    fn test_sim_clock_tracks_last_dispatch_time() {
        let t0 = Timestamp::new(0);
        let t1 = Timestamp::new(999);
        let mut builder = Engine::builder(EngineConfig::backtest(t0));
        builder.on(|_ctx: &mut HandlerCtx<'_>, _msg: &MsgA| {});
        let mut engine = builder.build().unwrap();
        engine.push_event(t1, MsgA(1)).unwrap();
        engine.run().unwrap();
        assert_eq!(engine.clock_now(), t1);
        assert_eq!(engine.dispatch_time(), t1);
    }

    /// Invariant: handler-group state persists across messages processed by run
    #[test]
    fn test_handler_group_state_persists_across_run() {
        let t0 = Timestamp::new(0);

        struct GroupState {
            n: u64,
        }

        let seen = Arc::new(AtomicUsize::new(0));
        let seen2 = seen.clone();

        let mut builder = Engine::builder(EngineConfig::backtest(t0));
        builder.handler_group(GroupState { n: 0 }, |group| {
            group.on(
                move |state: &mut GroupState, _ctx: &mut HandlerCtx<'_>, _msg: &MsgA| {
                    state.n += 1;
                    seen2.store(state.n as usize, Ordering::SeqCst);
                },
            );
        });
        let mut engine = builder.build().unwrap();
        engine.push_event(t0, MsgA(1)).unwrap();
        engine.push_event(t0, MsgA(2)).unwrap();
        engine.run().unwrap();
        assert_eq!(seen.load(Ordering::SeqCst), 2);
    }
}
