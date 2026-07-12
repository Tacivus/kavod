use std::{any::TypeId, sync::Arc};

use crate::{
    builder::EngineBuilder,
    cache::Cache,
    clock::Clock,
    config::EngineConfig,
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
/// The event loop (`run`) is added in a later phase.
///
/// Field layout is flat for now; Phase 14 may group mutable runtime fields
/// into an internal `Runtime` for borrow splitting. Actors are Phase 16+.
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
    use std::any::Any;

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
    struct SeededNum {
        value: u64,
    }

    impl State for SeededNum {
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
}
