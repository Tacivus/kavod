use std::{any::TypeId, collections::HashMap};

use thiserror::Error;

use crate::{
    message::Message,
    schedule::Scheduler,
    sequence::{SeqNo, Sequencer},
    time::timestamp::Timestamp,
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

/// Identity of a message type for graph metadata and diagnostics.
///
/// `id` is used for equality and runtime checks.
/// `name` is captured at registration via `type_name::<M>()` for build errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MessageType {
    pub(crate) id: TypeId,
    pub(crate) name: &'static str,
}

impl MessageType {
    pub(crate) fn of<M: Message>() -> Self {
        Self {
            id: TypeId::of::<M>(),
            name: std::any::type_name::<M>(),
        }
    }
}

/// Set of message types that a handler callback has declared it may produce.
///
/// Populated during handler registration and frozen at build time. At
/// runtime every [`HandlerOutput::send`] / [`send_at`] checks membership by
/// `TypeId`. At build time the graph walks [`iter`] for orphan validation
/// and readable `MissingConsumer` diagnostics.
#[derive(Debug, Default)]
pub(crate) struct ProductionSet {
    /// Keyed by TypeId so runtime checks stay O(1).
    /// Value is the printable type name captured at insert.
    types: HashMap<TypeId, &'static str>,
}

impl ProductionSet {
    pub(crate) fn new() -> Self {
        Self {
            types: HashMap::new(),
        }
    }

    /// Register that a handler may produce messages of type `M`.
    ///
    /// Stores both `TypeId` (runtime checks) and `type_name::<M>()` (graph
    /// diagnostics). Re-inserting the same type is a no-op.
    pub(crate) fn insert<M: Message>(&mut self) {
        let mt = MessageType::of::<M>();
        self.types.entry(mt.id).or_insert(mt.name);
    }

    /// Returns `true` if type `M` is in the declared production set.
    pub(crate) fn contains<M: Message>(&self) -> bool {
        self.types.contains_key(&TypeId::of::<M>())
    }

    /// Iterate declared productions for graph validation.
    ///
    /// Order is not significant (HashMap). Callers that need stable order
    /// should sort by `name` or registration order at a higher layer.
    pub(crate) fn iter(&self) -> impl Iterator<Item = MessageType> + '_ {
        self.types
            .iter()
            .map(|(&id, &name)| MessageType { id, name })
    }

    /// Collect declared productions into a Vec (convenience for graph builder).
    pub(crate) fn to_vec(&self) -> Vec<MessageType> {
        self.iter().collect()
    }
}

#[cfg(test)]
impl ProductionSet {
    /// Returns `true` if no message types have been declared.
    pub(crate) fn is_empty(&self) -> bool {
        self.types.is_empty()
    }

    /// Number of distinct declared production types.
    pub(crate) fn len(&self) -> usize {
        self.types.len()
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
    sequencer: &'a mut Sequencer,
    dispatch_time: Timestamp,
}

impl<'a> HandlerOutput<'a> {
    pub(crate) fn new(
        scheduler: &'a mut Scheduler,
        sequencer: &'a mut Sequencer,
        dispatch_time: Timestamp,
    ) -> Self {
        Self {
            scheduler,
            sequencer,
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
        let seq = self.next_seq_num()?;
        self.scheduler.push_msg(self.dispatch_time, seq, msg);
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
        let seq = self.next_seq_num()?;
        self.scheduler.push_msg(ts, seq, msg);
        Ok(())
    }

    fn next_seq_num(&mut self) -> Result<SeqNo, HandlerOutputError> {
        self.sequencer
            .next()
            .map_err(|_| HandlerOutputError::SequenceExhaustion)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    /// Invariant: insert stores a non-empty type name for diagnostics
    #[test]
    fn test_production_set_stores_type_name() {
        let mut set = ProductionSet::new();
        set.insert::<TestMsg>();

        let produced: Vec<_> = set.iter().collect();
        assert_eq!(produced.len(), 1);
        assert_eq!(produced[0].id, TypeId::of::<TestMsg>());
        assert!(
            produced[0].name.contains("TestMsg"),
            "expected type name to mention TestMsg, got {}",
            produced[0].name
        );
    }

    /// Invariant: iter returns every inserted type with its TypeId
    #[test]
    fn test_production_set_iter_all_inserted_types() {
        let mut set = ProductionSet::new();
        set.insert::<TestMsg>();
        set.insert::<OtherMsg>();

        let ids: Vec<TypeId> = set.iter().map(|mt| mt.id).collect();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&TypeId::of::<TestMsg>()));
        assert!(ids.contains(&TypeId::of::<OtherMsg>()));
    }

    /// Invariant: duplicate insert does not grow the set
    #[test]
    fn test_production_set_duplicate_insert_is_idempotent() {
        let mut set = ProductionSet::new();
        set.insert::<TestMsg>();
        set.insert::<TestMsg>();
        assert_eq!(set.len(), 1);
        assert!(set.contains::<TestMsg>());
    }

    /// Invariant: MessageType::of captures id and name together
    #[test]
    fn test_message_type_of() {
        let mt = MessageType::of::<TestMsg>();
        assert_eq!(mt.id, TypeId::of::<TestMsg>());
        assert!(mt.name.contains("TestMsg"));
    }

    // ========================================================================
    // HandlerOutput::send
    // ========================================================================

    /// Invariant: send schedules the message at the current dispatch time
    #[test]
    fn test_send_schedules_at_dispatch_time() {
        let mut sched = Scheduler::new();
        let mut seq = Sequencer::initial();
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
        let mut seq = Sequencer::initial();
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
        let mut seq = Sequencer::initial();
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
        let mut seq = Sequencer::initial();
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
        let mut seq = Sequencer::initial();
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
        let mut seq = Sequencer::initial();
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
        let mut seq = Sequencer::initial();
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
        let mut seq = Sequencer::initial();
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
        let mut seq = Sequencer::initial();
        let ts = Timestamp::new(42);

        let mut productions = ProductionSet::new();
        productions.insert::<TestMsg>();

        let mut output = HandlerOutput::new(&mut sched, &mut seq, ts);
        output.send(TestMsg(0), &productions).unwrap();
        output.send(TestMsg(1), &productions).unwrap();

        let item_a = sched.pop().unwrap();
        let item_b = sched.pop().unwrap();
        assert!(item_a.sequence_num() < item_b.sequence_num());
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
        let mut shared_seq = Sequencer::initial();
        let seq_a = shared_seq.next().unwrap();
        let seq_b = shared_seq.next().unwrap();
        sched.push_msg(ts, seq_a, OtherMsg(0));
        sched.push_msg(ts, seq_b, OtherMsg(1));

        let mut productions = ProductionSet::new();
        productions.insert::<TestMsg>();

        let mut output = HandlerOutput::new(&mut sched, &mut shared_seq, ts);
        output.send(TestMsg(42), &productions).unwrap();

        // Pop order: pre-pushed with seq_a, pre-pushed with seq_b,
        // then newly produced (highest seq)
        let first = sched.pop().unwrap();
        assert_eq!(first.sequence_num(), seq_a);
        assert!(
            (&*first.payload() as &dyn std::any::Any)
                .downcast_ref::<OtherMsg>()
                .is_some()
        );

        let second = sched.pop().unwrap();
        assert_eq!(second.sequence_num(), seq_b);
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
        assert!(third.sequence_num() > seq_b);

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

    // ========================================================================
    // Sequence allocation edges
    // ========================================================================

    /// Invariant: the first successful send allocates SeqNo(1) (initial is 0)
    #[test]
    fn test_first_send_allocates_seq_one() {
        let mut sched = Scheduler::new();
        let mut seq = Sequencer::initial();
        let ts = Timestamp::new(0);

        let mut productions = ProductionSet::new();
        productions.insert::<TestMsg>();

        let mut output = HandlerOutput::new(&mut sched, &mut seq, ts);
        output.send(TestMsg(0), &productions).unwrap();

        assert_eq!(sched.pop().unwrap().sequence_num(), SeqNo::from_raw(1));
        assert_eq!(seq.get(), SeqNo::from_raw(1));
    }

    /// Invariant: failed undeclared send does not advance the sequencer
    /// and does not enqueue anything
    #[test]
    fn test_undeclared_send_does_not_advance_sequencer() {
        let mut sched = Scheduler::new();
        let mut seq = Sequencer::initial();
        let before = seq.get();
        let ts = Timestamp::new(0);

        let mut output = HandlerOutput::new(&mut sched, &mut seq, ts);
        let productions = ProductionSet::new(); // empty — nothing declared

        let result = output.send(TestMsg(1), &productions);
        assert!(matches!(
            result,
            Err(HandlerOutputError::UndeclaredProduction { .. })
        ));
        assert_eq!(seq.get(), before);
        assert!(sched.pop().is_none());
    }

    /// Invariant: failed past send_at does not advance the sequencer
    /// and does not enqueue anything
    #[test]
    fn test_past_send_at_does_not_advance_sequencer() {
        let mut sched = Scheduler::new();
        let mut seq = Sequencer::initial();
        let before = seq.get();
        let dispatch_ts = Timestamp::new(100);
        let past_ts = Timestamp::new(50);

        let mut productions = ProductionSet::new();
        productions.insert::<TestMsg>();

        let mut output = HandlerOutput::new(&mut sched, &mut seq, dispatch_ts);
        let result = output.send_at(past_ts, TestMsg(1), &productions);

        assert!(matches!(result, Err(HandlerOutputError::PastEvent { .. })));
        assert_eq!(seq.get(), before);
        assert!(sched.pop().is_none());
    }

    /// Invariant: two send_at calls allocate strictly increasing sequences
    #[test]
    fn test_two_send_ats_receive_increasing_sequences() {
        let mut sched = Scheduler::new();
        let mut seq = Sequencer::initial();
        let dispatch_ts = Timestamp::new(100);
        let t1 = Timestamp::new(200);
        let t2 = Timestamp::new(300);

        let mut productions = ProductionSet::new();
        productions.insert::<TestMsg>();

        let mut output = HandlerOutput::new(&mut sched, &mut seq, dispatch_ts);
        output.send_at(t1, TestMsg(0), &productions).unwrap();
        output.send_at(t2, TestMsg(1), &productions).unwrap();

        // Earlier time pops first regardless of seq; check seqs on the items
        let earlier = sched.pop().unwrap();
        let later = sched.pop().unwrap();
        assert_eq!(earlier.dispatch_time(), t1);
        assert_eq!(later.dispatch_time(), t2);
        assert!(earlier.sequence_num() < later.sequence_num());
    }

    /// Invariant: interleaved send / send_at each advance the sequencer;
    /// pop order is time-primary so seqs are not monotonic across times.
    #[test]
    fn test_send_and_send_at_interleaved_increasing_sequences() {
        let mut sched = Scheduler::new();
        let mut seq = Sequencer::initial();
        let ts = Timestamp::new(100);
        let future = Timestamp::new(200);

        let mut productions = ProductionSet::new();
        productions.insert::<TestMsg>();

        {
            let mut output = HandlerOutput::new(&mut sched, &mut seq, ts);
            output.send(TestMsg(0), &productions).unwrap();
            output.send_at(future, TestMsg(1), &productions).unwrap();
            output.send(TestMsg(2), &productions).unwrap();
        }
        // `output` dropped — sequencer borrow ends

        // Three successful allocations from initial 0 → current is 3
        assert_eq!(seq.get(), SeqNo::from_raw(3));

        // Pop order: both at `ts` (by seq), then future
        let a = sched.pop().unwrap();
        let b = sched.pop().unwrap();
        let c = sched.pop().unwrap();

        assert_eq!(a.dispatch_time(), ts);
        assert_eq!(b.dispatch_time(), ts);
        assert_eq!(c.dispatch_time(), future);

        // Same-time: first send (seq smaller) before third send
        assert!(a.sequence_num() < b.sequence_num());
        // Future message was ticketed between them
        assert!(a.sequence_num() < c.sequence_num());
        assert!(c.sequence_num() < b.sequence_num());

        // Payload identity (match style used elsewhere in this module)
        assert_eq!(
            (&*a.payload() as &dyn std::any::Any).downcast_ref::<TestMsg>(),
            Some(&TestMsg(0))
        );
        assert_eq!(
            (&*b.payload() as &dyn std::any::Any).downcast_ref::<TestMsg>(),
            Some(&TestMsg(2))
        );
        assert_eq!(
            (&*c.payload() as &dyn std::any::Any).downcast_ref::<TestMsg>(),
            Some(&TestMsg(1))
        );
    }
    /// Invariant: sequencer overflow maps to SequenceExhaustion and does not
    /// enqueue; current value stays at MAX
    #[test]
    fn test_send_sequence_exhaustion() {
        let mut sched = Scheduler::new();
        let mut seq = Sequencer::from_raw(u64::MAX);
        let ts = Timestamp::new(0);

        let mut productions = ProductionSet::new();
        productions.insert::<TestMsg>();

        let mut output = HandlerOutput::new(&mut sched, &mut seq, ts);
        let result = output.send(TestMsg(0), &productions);

        assert_eq!(result, Err(HandlerOutputError::SequenceExhaustion));
        assert_eq!(seq.get(), SeqNo::from_raw(u64::MAX));
        assert!(sched.pop().is_none());
    }

    /// Invariant: SequenceExhaustion error Display is non-empty / readable
    #[test]
    fn test_sequence_exhaustion_error_is_readable() {
        let err = HandlerOutputError::SequenceExhaustion;
        let msg = err.to_string();
        assert!(
            msg.to_lowercase().contains("sequence"),
            "expected readable exhaustion message, got: {msg}"
        );
    }
}
