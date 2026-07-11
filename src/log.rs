use std::any::TypeId;

use crate::{message::Message, time::timestamp::Timestamp};

/// Monotonically increase sequence for each step the engine takes
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SeqNo(u64);

impl SeqNo {
    /// Creates a new SeqNo starting at 0
    pub(crate) const fn initial() -> Self {
        SeqNo(0)
    }

    /// Gets the next SeqNo
    pub(crate) fn next(&self) -> SeqNo {
        SeqNo(self.0 + 1)
    }

    /// Gets the current SeqNo
    pub(crate) fn get(&self) -> u64 {
        self.0
    }
}

/// InboundLogEntry is the entry item for the `InboundLog`
#[derive(Debug)]
pub struct InboundLogEntry {
    pub seq: SeqNo,
    pub ts: Timestamp,
    pub type_id: TypeId,
    pub payload: Box<dyn Message>,
}

impl InboundLogEntry {
    pub fn new<M: Message>(seq: SeqNo, ts: Timestamp, msg: M) -> Self {
        Self {
            seq,
            ts,
            type_id: TypeId::of::<M>(),
            payload: Box::new(msg),
        }
    }
}

/// InboundLogEntry is the main mechanism for replay. Every message that gets
/// sent is added so that the system can be 100% deterministically replyed in
/// the exact order/times that everything was originally received in.
#[derive(Debug)]
pub struct InboundLog(Vec<InboundLogEntry>);

impl InboundLog {
    pub(crate) fn new() -> Self {
        Self(Vec::new())
    }

    pub(crate) fn append(&mut self, entry: InboundLogEntry) {
        self.0.push(entry);
    }

    pub fn iter(&self) -> impl Iterator<Item = &InboundLogEntry> {
        self.0.iter()
    }
}

/// User facing debugging/tracing.
///
/// Will be more expanded upon later, temp for now!
#[derive(Debug)]
pub enum EngineEvent {
    Ingest {
        seq: SeqNo,
        ts: Timestamp,
        type_id: TypeId,
        payload: Box<dyn Message>,
    },
    Dispatch {
        seq: SeqNo,
        ts: Timestamp,
        type_id: TypeId,
        payload: Box<dyn Message>,
        handler: &'static str,
    },
    Produce {
        seq: SeqNo,
        ts: Timestamp,
        type_id: TypeId,
        payload: Box<dyn Message>,
        producer: &'static str,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::Message;
    use std::any::{Any, TypeId};

    // ==================================================================
    // SeqNo tests
    // ==================================================================

    /// Invariant: initial SeqNo starts at 0
    #[test]
    fn seq_no_initial_is_zero() {
        let seq = SeqNo::initial();
        assert_eq!(seq.get(), 0);
    }

    /// Invariant: next() increments by exactly 1
    #[test]
    fn seq_no_next_increments() {
        let seq = SeqNo::initial();
        assert_eq!(seq.next().get(), 1);
        assert_eq!(seq.next().next().get(), 2);
    }

    /// Invariant: next() is non-mutating (returns new value, original unchanged)
    #[test]
    fn seq_no_next_does_not_mutate() {
        let seq = SeqNo::initial();
        let _next = seq.next();
        assert_eq!(seq.get(), 0);
    }

    /// Invariant: next() called repeatedly produces monotonic increasing sequence
    #[test]
    fn seq_no_monotonic() {
        let mut seq = SeqNo::initial();
        for i in 1..1000 {
            seq = seq.next();
            assert_eq!(seq.get(), i);
        }
    }

    /// Invariant: smaller SeqNo < larger SeqNo
    #[test]
    fn seq_no_ordering() {
        let a = SeqNo::initial();
        let b = a.next();
        let c = b.next();
        assert!(a < b);
        assert!(b < c);
        assert!(a < c);
        assert!(c > a);
        assert!(b > a);
    }

    /// Invariant: equal SeqNos compare equal
    #[test]
    fn seq_no_equal() {
        let a = SeqNo::initial().next();
        let b = SeqNo::initial().next();
        assert_eq!(a, b);
        assert!(!(a < b));
        assert!(!(a > b));
    }

    // ==================================================================
    // InboundLogEntry tests
    // ==================================================================

    #[derive(Clone, Debug, PartialEq)]
    struct TestMsg(u64);

    impl Message for TestMsg {}

    /// Invariant: new() correctly populates seq, ts, type_id, and payload
    #[test]
    fn entry_constructor() {
        let seq = SeqNo::initial();
        let ts = Timestamp::new(100);
        let entry = InboundLogEntry::new(seq, ts, TestMsg(42));

        assert_eq!(entry.seq, seq);
        assert_eq!(entry.ts, ts);
        assert_eq!(entry.type_id, TypeId::of::<TestMsg>());
        let payload: &dyn Any = &*entry.payload;
        assert_eq!(payload.downcast_ref::<TestMsg>(), Some(&TestMsg(42)));
    }

    /// Invariant: downcasting the payload recovers the original message
    #[test]
    fn entry_downcast_roundtrip() {
        let entry = InboundLogEntry::new(SeqNo::initial(), Timestamp::new(200), TestMsg(99));

        let payload: &dyn Any = &*entry.payload;
        assert_eq!(payload.downcast_ref::<TestMsg>(), Some(&TestMsg(99)));
    }

    /// Invariant: TypeId stored in entry matches the actual message type
    #[test]
    fn entry_type_id_matches_payload() {
        let entry = InboundLogEntry::new(SeqNo::initial(), Timestamp::new(0), TestMsg(7));

        assert_eq!(entry.type_id, TypeId::of::<TestMsg>());
    }

    /// Invariant: different message types have different TypeIds in entries
    #[test]
    fn entry_different_types_have_different_type_ids() {
        #[derive(Debug)]
        struct AnotherMsg(u64);
        impl Message for AnotherMsg {}

        let entry_a = InboundLogEntry::new(SeqNo::initial(), Timestamp::new(0), TestMsg(1));
        let entry_b = InboundLogEntry::new(SeqNo::initial(), Timestamp::new(0), AnotherMsg(2));

        assert_ne!(entry_a.type_id, entry_b.type_id);
    }

    // ==================================================================
    // InboundLog tests
    // ==================================================================

    /// Invariant: a new InboundLog is empty
    #[test]
    fn log_new_is_empty() {
        let log = InboundLog::new();
        assert_eq!(log.iter().count(), 0);
    }

    /// Invariant: appending an entry increases the count by 1
    #[test]
    fn log_append_increases_count() {
        let ts = Timestamp::new(0);
        let mut log = InboundLog::new();

        log.append(InboundLogEntry::new(SeqNo::initial(), ts, TestMsg(1)));
        assert_eq!(log.iter().count(), 1);

        log.append(InboundLogEntry::new(
            SeqNo::initial().next(),
            ts,
            TestMsg(2),
        ));
        assert_eq!(log.iter().count(), 2);
    }

    /// Invariant: iter() returns entries in append order
    #[test]
    fn log_iter_preserves_insertion_order() {
        let ts = Timestamp::new(0);
        let mut log = InboundLog::new();

        log.append(InboundLogEntry::new(SeqNo::initial(), ts, TestMsg(1)));
        log.append(InboundLogEntry::new(
            SeqNo::initial().next(),
            ts,
            TestMsg(2),
        ));
        log.append(InboundLogEntry::new(
            SeqNo::initial().next().next(),
            ts,
            TestMsg(3),
        ));

        let entries: Vec<&InboundLogEntry> = log.iter().collect();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].seq.get(), 0);
        assert_eq!(entries[1].seq.get(), 1);
        assert_eq!(entries[2].seq.get(), 2);
    }

    /// Invariant: replaying the log produces identical entries (roundtrip)
    #[test]
    fn log_replay_roundtrip() {
        let mut log = InboundLog::new();
        let ts = Timestamp::new(42);

        let entries_in = vec![
            InboundLogEntry::new(SeqNo::initial(), ts, TestMsg(10)),
            InboundLogEntry::new(SeqNo::initial().next(), ts, TestMsg(20)),
        ];

        for entry in entries_in {
            log.append(entry);
        }

        let entries_out: Vec<&InboundLogEntry> = log.iter().collect();
        assert_eq!(entries_out.len(), 2);
        assert_eq!(entries_out[0].seq.get(), 0);
        assert_eq!(entries_out[0].ts, ts);
        assert_eq!(entries_out[0].type_id, TypeId::of::<TestMsg>());
        let payload0: &dyn Any = &*entries_out[0].payload;
        assert_eq!(payload0.downcast_ref::<TestMsg>(), Some(&TestMsg(10)));

        assert_eq!(entries_out[1].seq.get(), 1);
        assert_eq!(entries_out[1].ts, ts);
        assert_eq!(entries_out[1].type_id, TypeId::of::<TestMsg>());
        let payload1: &dyn Any = &*entries_out[1].payload;
        assert_eq!(payload1.downcast_ref::<TestMsg>(), Some(&TestMsg(20)));
    }

    /// Invariant: empty log iter produces no entries
    #[test]
    fn log_empty_iter() {
        let log = InboundLog::new();
        assert!(log.iter().next().is_none());
    }

    /// Invariant: appending with mixed timestamps preserves order
    #[test]
    fn log_preserves_append_order_regardless_of_timestamp() {
        let mut log = InboundLog::new();

        log.append(InboundLogEntry::new(
            SeqNo::initial(),
            Timestamp::new(200),
            TestMsg(1),
        ));
        log.append(InboundLogEntry::new(
            SeqNo::initial().next(),
            Timestamp::new(100),
            TestMsg(2),
        ));

        let entries: Vec<&InboundLogEntry> = log.iter().collect();
        assert_eq!(entries[0].ts, Timestamp::new(200));
        assert_eq!(entries[1].ts, Timestamp::new(100));
    }
}
