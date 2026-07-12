use std::{any::TypeId, collections::HashSet, sync::Arc};

use thiserror::Error;

use crate::{
    message::Message, schedule::Scheduler, sequence::Sequence, time::timestamp::Timestamp,
};

/// Errors that can occur when a handler produces output through [`HandlerOutput`].
#[derive(Debug, Error, PartialEq)]
pub enum HandlerOutputError {
    /// The handler attempted to send a message type that it did not declare
    /// via `.produces::<M>()` during registration.
    #[error("undeclared production: handler did not declare output of type `{type_name}`")]
    UndeclaredProduction { type_name: &'static str },

    /// The handler attempted to schedule a message at a timestamp before the
    /// engine's current logical dispatch time.
    #[error(
        "event scheduled in the past: requested {requested}, current dispatch time is {current}"
    )]
    PastEvent {
        requested: Timestamp,
        current: Timestamp,
    },

    /// The kernel sequence counter overflowed.
    #[error("sequence exhausted")]
    SequenceExhaustion,
}

/// Set of message types that a handler callback has declared it may produce.
///
/// Populated during handler registration and frozen at build time.  At
/// runtime every [`HandlerOutput::send`] and [`HandlerOutput::send_at`]
/// call checks that the outgoing message type is contained in this set.
#[derive(Debug, Default)]
pub(crate) struct ProductionSet {
    types: HashSet<TypeId>,
}

impl ProductionSet {
    /// Returns an empty production set.
    pub(crate) fn new() -> Self {
        Self {
            types: HashSet::new(),
        }
    }

    /// Register that a handler may produce messages of type `M`.
    pub(crate) fn insert<M: Message>(&mut self) {
        self.types.insert(TypeId::of::<M>());
    }

    /// Returns `true` if type `M` is in the declared production set.
    pub(crate) fn contains<M: Message>(&self) -> bool {
        self.types.contains(&TypeId::of::<M>())
    }

    /// Returns `true` if no message types have been declared.
    pub(crate) fn is_empty(&self) -> bool {
        self.types.is_empty()
    }
}

/// Internal output capability that directly allocates sequence numbers and
/// inserts into the scheduler.
///
/// This is not exposed to user code — handlers interact with it indirectly
/// through [`HandlerCtx`](crate::context::handler::HandlerCtx).
///
/// There is no `Vec`, `RefCell`, or internal channel used to schedule
/// handler output.  The output capability directly mutates the kernel-owned
/// scheduler and sequence allocator.
pub(crate) struct HandlerOutput<'a> {
    scheduler: &'a mut Scheduler,
    sequence: &'a mut Sequence,
    dispatch_time: Timestamp,
}

impl<'a> HandlerOutput<'a> {
    pub(crate) fn new(
        scheduler: &'a mut Scheduler,
        sequence: &'a mut Sequence,
        dispatch_time: Timestamp,
    ) -> Self {
        Self {
            scheduler,
            sequence,
            dispatch_time,
        }
    }

    /// Schedules `msg` at the current dispatch time after verifying it is a
    /// declared production.
    ///
    /// Allocates a new kernel sequence number and wraps the message in an
    /// `Arc` before pushing it into the scheduler.  The new sequence
    /// guarantees that this message is processed after all previously
    /// queued same-time messages.
    pub(crate) fn send<M: Message>(
        &mut self,
        msg: M,
        productions: &ProductionSet,
    ) -> Result<(), HandlerOutputError> {
        if !productions.contains::<M>() {
            return Err(HandlerOutputError::UndeclaredProduction {
                type_name: std::any::type_name::<M>(),
            });
        }
        let seq = self.next_seq()?;
        self.scheduler
            .push_shared_msg(self.dispatch_time, seq, Arc::new(msg));
        Ok(())
    }

    /// Schedules `msg` at the requested future `ts` after verifying it is a
    /// declared production.
    ///
    /// Rejects timestamps strictly before the current dispatch time.
    /// Allocates a new kernel sequence number and wraps the message in an
    /// `Arc` before pushing it into the scheduler.
    pub(crate) fn send_at<M: Message>(
        &mut self,
        ts: Timestamp,
        msg: M,
        productions: &ProductionSet,
    ) -> Result<(), HandlerOutputError> {
        if ts < self.dispatch_time {
            return Err(HandlerOutputError::PastEvent {
                requested: ts,
                current: self.dispatch_time,
            });
        }
        if !productions.contains::<M>() {
            return Err(HandlerOutputError::UndeclaredProduction {
                type_name: std::any::type_name::<M>(),
            });
        }
        let seq = self.next_seq()?;
        self.scheduler.push_shared_msg(ts, seq, Arc::new(msg));
        Ok(())
    }

    fn next_seq(&mut self) -> Result<Sequence, HandlerOutputError> {
        self.sequence
            .next()
            .map_err(|_| HandlerOutputError::SequenceExhaustion)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Arc;

    // ========================================================================
    // Test message types
    // ========================================================================

    #[derive(Debug, Clone, PartialEq)]
    struct TestMsg(u64);

    impl Message for TestMsg {}

    #[derive(Debug, Clone, PartialEq)]
    struct OtherMsg(u64);

    impl Message for OtherMsg {}

    // ========================================================================
    // ProductionSet
    // ========================================================================

    /// Invariant: a new ProductionSet has no declared types
    #[test]
    fn test_production_set_new_is_empty() {
        let set = ProductionSet::new();
        assert!(set.is_empty());
        assert!(!set.contains::<TestMsg>());
    }

    /// Invariant: insert followed by contains returns true
    #[test]
    fn test_production_set_insert_contains_roundtrip() {
        let mut set = ProductionSet::new();
        set.insert::<TestMsg>();
        assert!(set.contains::<TestMsg>());
    }

    /// Invariant: inserting one type does not make contains true for a
    /// different type
    #[test]
    fn test_production_set_different_types_do_not_collide() {
        let mut set = ProductionSet::new();
        set.insert::<TestMsg>();
        assert!(!set.contains::<OtherMsg>());
    }

    /// Invariant: multiple insert calls work correctly
    #[test]
    fn test_production_set_multiple_inserts() {
        let mut set = ProductionSet::new();
        set.insert::<TestMsg>();
        set.insert::<OtherMsg>();
        assert!(set.contains::<TestMsg>());
        assert!(set.contains::<OtherMsg>());
        assert!(!set.is_empty());
    }

    // ========================================================================
    // HandlerOutput helpers
    // ========================================================================

    fn seq_val(n: u64) -> Sequence {
        let mut s = Sequence::initial();
        for _ in 0..n {
            s.next().unwrap();
        }
        s
    }

    // ========================================================================
    // HandlerOutput::send
    // ========================================================================

    /// Invariant: send schedules the message at the current dispatch time
    #[test]
    fn test_send_schedules_at_dispatch_time() {
        let mut sched = Scheduler::new();
        let mut seq = Sequence::initial();
        let ts = Timestamp::new(100);

        let mut output = HandlerOutput::new(&mut sched, &mut seq, ts);
        let mut productions = ProductionSet::new();
        productions.insert::<TestMsg>();

        output.send(TestMsg(42), &productions).unwrap();

        let item = sched.pop().unwrap();
        assert_eq!(item.dispatch_time(), ts);
    }

    /// Invariant: send creates a shared Arc payload that still roundtrips
    /// through the scheduler
    #[test]
    fn test_send_payload_roundtrips() {
        let mut sched = Scheduler::new();
        let mut seq = Sequence::initial();
        let ts = Timestamp::new(0);

        let mut output = HandlerOutput::new(&mut sched, &mut seq, ts);
        let mut productions = ProductionSet::new();
        productions.insert::<TestMsg>();

        output.send(TestMsg(42), &productions).unwrap();

        let item = sched.pop().unwrap();
        let payload: &dyn std::any::Any = &*item.payload();
        assert_eq!(payload.downcast_ref::<TestMsg>(), Some(&TestMsg(42)));
    }

    /// Invariant: sending an undeclared message type returns
    /// UndeclaredProduction error and does not add anything to the scheduler
    #[test]
    fn test_undeclared_production_returns_error() {
        let mut sched = Scheduler::new();
        let mut seq = Sequence::initial();
        let ts = Timestamp::new(0);

        let mut output = HandlerOutput::new(&mut sched, &mut seq, ts);
        let productions = ProductionSet::new();

        let result = output.send(TestMsg(42), &productions);
        assert!(matches!(
            result,
            Err(HandlerOutputError::UndeclaredProduction { .. })
        ));
        assert!(sched.pop().is_none());
    }

    // ========================================================================
    // HandlerOutput::send_at
    // ========================================================================

    /// Invariant: send_at schedules at the requested future time
    #[test]
    fn test_send_at_schedules_at_future_time() {
        let mut sched = Scheduler::new();
        let mut seq = Sequence::initial();
        let dispatch_ts = Timestamp::new(100);
        let future_ts = Timestamp::new(200);

        let mut output = HandlerOutput::new(&mut sched, &mut seq, dispatch_ts);
        let mut productions = ProductionSet::new();
        productions.insert::<TestMsg>();

        output.send_at(future_ts, TestMsg(7), &productions).unwrap();

        let item = sched.pop().unwrap();
        assert_eq!(item.dispatch_time(), future_ts);
    }

    /// Invariant: send_at at the same dispatch time succeeds
    #[test]
    fn test_send_at_same_dispatch_time_succeeds() {
        let mut sched = Scheduler::new();
        let mut seq = Sequence::initial();
        let ts = Timestamp::new(100);

        let mut output = HandlerOutput::new(&mut sched, &mut seq, ts);
        let mut productions = ProductionSet::new();
        productions.insert::<TestMsg>();

        let result = output.send_at(ts, TestMsg(5), &productions);
        assert!(result.is_ok());

        let item = sched.pop().unwrap();
        assert_eq!(item.dispatch_time(), ts);
    }

    /// Invariant: send_at rejects a timestamp strictly before dispatch_time
    #[test]
    fn test_send_at_rejects_past_timestamp() {
        let mut sched = Scheduler::new();
        let mut seq = Sequence::initial();
        let dispatch_ts = Timestamp::new(100);
        let past_ts = Timestamp::new(50);

        let mut output = HandlerOutput::new(&mut sched, &mut seq, dispatch_ts);
        let mut productions = ProductionSet::new();
        productions.insert::<TestMsg>();

        let result = output.send_at(past_ts, TestMsg(3), &productions);
        assert_eq!(
            result,
            Err(HandlerOutputError::PastEvent {
                requested: past_ts,
                current: dispatch_ts,
            })
        );
        assert!(sched.pop().is_none());
    }

    /// Invariant: send_at with an undeclared type returns
    /// UndeclaredProduction error
    #[test]
    fn test_send_at_undeclared_production_returns_error() {
        let mut sched = Scheduler::new();
        let mut seq = Sequence::initial();
        let ts = Timestamp::new(100);

        let mut output = HandlerOutput::new(&mut sched, &mut seq, ts);
        let productions = ProductionSet::new();

        let result = output.send_at(Timestamp::new(200), TestMsg(1), &productions);
        assert!(matches!(
            result,
            Err(HandlerOutputError::UndeclaredProduction { .. })
        ));
        assert!(sched.pop().is_none());
    }

    /// Invariant: past-time check runs before production check (send_at
    /// rejects past first)
    #[test]
    fn test_send_at_rejects_past_before_checking_productions() {
        let mut sched = Scheduler::new();
        let mut seq = Sequence::initial();
        let dispatch_ts = Timestamp::new(100);
        let past_ts = Timestamp::new(50);

        let mut output = HandlerOutput::new(&mut sched, &mut seq, dispatch_ts);
        let productions = ProductionSet::new();

        let result = output.send_at(past_ts, TestMsg(1), &productions);
        assert!(matches!(result, Err(HandlerOutputError::PastEvent { .. })));
    }

    // ========================================================================
    // Sequence ordering
    // ========================================================================

    /// Invariant: two sends receive increasing sequences
    #[test]
    fn test_two_sends_receive_increasing_sequences() {
        let mut sched = Scheduler::new();
        let mut seq = Sequence::initial();
        let ts = Timestamp::new(42);

        let mut productions = ProductionSet::new();
        productions.insert::<TestMsg>();

        let mut output = HandlerOutput::new(&mut sched, &mut seq, ts);
        output.send(TestMsg(0), &productions).unwrap();
        output.send(TestMsg(1), &productions).unwrap();

        let item_a = sched.pop().unwrap();
        let item_b = sched.pop().unwrap();
        assert!(item_a.sequence() < item_b.sequence());
    }

    /// Invariant: existing same-time items with lower sequence remain ahead
    /// of newly produced messages
    #[test]
    fn test_existing_same_time_items_stay_ahead_of_newly_produced() {
        let mut sched = Scheduler::new();
        let ts = Timestamp::new(100);

        // Pre-populate scheduler with two items at time T, sharing the same
        // sequence allocator with HandlerOutput later so the newly produced
        // message receives a later sequence.
        let mut shared_seq = Sequence::initial();
        let seq_a = shared_seq.next().unwrap();
        let seq_b = shared_seq.next().unwrap();
        sched.push_shared_msg(ts, seq_a, Arc::new(OtherMsg(0)));
        sched.push_shared_msg(ts, seq_b, Arc::new(OtherMsg(1)));

        let mut productions = ProductionSet::new();
        productions.insert::<TestMsg>();

        let mut output = HandlerOutput::new(&mut sched, &mut shared_seq, ts);
        output.send(TestMsg(42), &productions).unwrap();

        // Pop order: pre-pushed with seq_a, pre-pushed with seq_b,
        // then newly produced (highest seq)
        let first = sched.pop().unwrap();
        assert_eq!(first.sequence(), seq_a);
        assert!(
            (&*first.payload() as &dyn std::any::Any)
                .downcast_ref::<OtherMsg>()
                .is_some()
        );

        let second = sched.pop().unwrap();
        assert_eq!(second.sequence(), seq_b);
        assert!(
            (&*second.payload() as &dyn std::any::Any)
                .downcast_ref::<OtherMsg>()
                .is_some()
        );

        let third = sched.pop().unwrap();
        assert!(
            (&*third.payload() as &dyn std::any::Any)
                .downcast_ref::<TestMsg>()
                .is_some_and(|m| m.0 == 42)
        );
        assert!(third.sequence() > seq_b);

        assert!(sched.pop().is_none());
    }

    // ========================================================================
    // Error formatting
    // ========================================================================

    /// Invariant: UndeclaredProduction error message contains the type name
    #[test]
    fn test_undeclared_production_error_is_readable() {
        let err = HandlerOutputError::UndeclaredProduction {
            type_name: "some::Msg",
        };
        let msg = err.to_string();
        assert!(
            msg.contains("some::Msg"),
            "error message should contain the type name, got: {msg}"
        );
    }

    /// Invariant: PastEvent error message contains both timestamps
    #[test]
    fn test_past_event_error_is_readable() {
        let err = HandlerOutputError::PastEvent {
            requested: Timestamp::new(50),
            current: Timestamp::new(100),
        };
        let msg = err.to_string();
        assert!(
            msg.contains("50ns"),
            "error message should contain the requested timestamp, got: {msg}"
        );
        assert!(
            msg.contains("100ns"),
            "error message should contain the current timestamp, got: {msg}"
        );
    }
}
