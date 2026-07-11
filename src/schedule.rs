use std::collections::BinaryHeap;

use crate::{log::SeqNo, message::Message, time::timestamp::Timestamp};

#[derive(Debug)]
pub(crate) struct Scheduler(BinaryHeap<Event>);

impl Scheduler {
    pub(crate) fn new() -> Self {
        Scheduler(BinaryHeap::new())
    }

    pub(crate) fn push(&mut self, ts: Timestamp, seq: SeqNo, msg: impl Message) {
        self.0.push(Event::new(ts, seq, msg))
    }

    pub(crate) fn pop(&mut self) -> Option<Event> {
        self.0.pop()
    }

    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }

    /// Push an already-boxed message into the scheduler.
    ///
    /// Used by the kernel drain loop when handler-produced messages
    /// (which arrive as `Box<dyn Message>`) must re-enter the heap.
    pub(crate) fn push_boxed(&mut self, ts: Timestamp, seq: SeqNo, payload: Box<dyn Message>) {
        self.0.push(Event { ts, seq, payload });
    }
}

#[derive(Debug)]
pub(crate) struct Event {
    pub(crate) ts: Timestamp,
    pub(crate) seq: SeqNo,
    pub(crate) payload: Box<dyn Message>,
}

impl Event {
    fn new(ts: Timestamp, seq: SeqNo, payload: impl Message) -> Self {
        Self {
            ts,
            seq,
            payload: Box::new(payload),
        }
    }
}

impl PartialEq for Event {
    fn eq(&self, other: &Self) -> bool {
        self.ts == other.ts && self.seq == other.seq
    }
}

impl Eq for Event {}

impl PartialOrd for Event {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Event {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Reverse for min-heap: earlier ts = "greater", lower seq = "greater"
        other
            .ts
            .cmp(&self.ts)
            .then_with(|| other.seq.cmp(&self.seq))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::Message;
    use std::any::Any;

    fn seq(n: u64) -> SeqNo {
        let mut s = SeqNo::initial();
        for _ in 0..n {
            s = s.next();
        }
        s
    }

    #[derive(Clone, Debug, PartialEq)]
    struct TestMsg(u64);
    impl Message for TestMsg {}

    #[derive(Clone, Debug, PartialEq)]
    struct OtherMsg(u64);
    impl Message for OtherMsg {}

    // ==================================================================
    // Scheduler state
    // ==================================================================

    /// Invariant: a new Scheduler is empty
    #[test]
    fn new_scheduler_is_empty() {
        let mut sched = Scheduler::new();
        assert_eq!(sched.len(), 0);
        assert!(sched.pop().is_none());
    }

    /// Invariant: push/pop roundtrip returns the same event
    #[test]
    fn push_pop_roundtrip() {
        let mut sched = Scheduler::new();
        sched.push(Timestamp::new(100), seq(0), TestMsg(42));
        let event = sched.pop().unwrap();
        assert_eq!(event.ts, Timestamp::new(100));
        assert_eq!(event.seq, seq(0));
        let payload: &dyn Any = &*event.payload;
        assert_eq!(payload.downcast_ref::<TestMsg>(), Some(&TestMsg(42)));
    }

    /// Invariant: push N events, pop exactly N times, then None
    #[test]
    fn pops_exhaust_all_events() {
        let mut sched = Scheduler::new();
        let n = 5;
        let mut s = SeqNo::initial();
        for i in 0..n {
            sched.push(Timestamp::new(100), s, TestMsg(i));
            s = s.next();
        }
        for _ in 0..n {
            assert!(sched.pop().is_some());
        }
        assert!(sched.pop().is_none());
        assert_eq!(sched.len(), 0);
    }

    /// Invariant: pop from empty returns None
    #[test]
    fn pop_from_empty_returns_none() {
        let mut sched = Scheduler::new();
        assert!(sched.pop().is_none());
        assert!(sched.pop().is_none());
    }

    /// Invariant: push after exhaustion works
    #[test]
    fn push_after_empty_works() {
        let mut sched = Scheduler::new();
        sched.push(Timestamp::new(100), seq(0), TestMsg(1));
        assert!(sched.pop().is_some());
        assert!(sched.pop().is_none());
        sched.push(Timestamp::new(200), seq(1), TestMsg(2));
        let event = sched.pop().unwrap();
        assert_eq!(event.ts, Timestamp::new(200));
        assert_eq!(sched.len(), 0);
    }

    /// Invariant: push_boxed works identically to push for a boxed message
    #[test]
    fn push_boxed_roundtrip() {
        let mut sched = Scheduler::new();
        let payload: Box<dyn Message> = Box::new(TestMsg(42));
        sched.push_boxed(Timestamp::new(100), seq(0), payload);
        let event = sched.pop().unwrap();
        assert_eq!(event.ts, Timestamp::new(100));
        assert_eq!(event.seq, seq(0));
        let p: &dyn Any = &*event.payload;
        assert_eq!(p.downcast_ref::<TestMsg>(), Some(&TestMsg(42)));
    }

    // ==================================================================
    // Ordering correctness
    // ==================================================================

    /// Invariant: earliest timestamp pops first
    #[test]
    fn earliest_ts_pops_first() {
        let mut sched = Scheduler::new();
        sched.push(Timestamp::new(200), seq(0), TestMsg(2));
        sched.push(Timestamp::new(100), seq(1), TestMsg(1));
        assert_eq!(sched.pop().unwrap().ts, Timestamp::new(100));
        assert_eq!(sched.pop().unwrap().ts, Timestamp::new(200));
    }

    /// Invariant: same ts, lower seq pops first
    #[test]
    fn same_ts_lower_seq_pops_first() {
        let mut sched = Scheduler::new();
        sched.push(Timestamp::new(100), seq(1), TestMsg(2));
        sched.push(Timestamp::new(100), seq(0), TestMsg(1));
        let first = sched.pop().unwrap();
        assert_eq!(first.seq, seq(0));
        let second = sched.pop().unwrap();
        assert_eq!(second.seq, seq(1));
    }

    /// Invariant: same ts, inserted in seq order, still pops in seq order
    #[test]
    fn same_ts_inserted_in_seq_order_pops_in_seq_order() {
        let mut sched = Scheduler::new();
        sched.push(Timestamp::new(100), seq(0), TestMsg(0));
        sched.push(Timestamp::new(100), seq(1), TestMsg(1));
        sched.push(Timestamp::new(100), seq(2), TestMsg(2));
        assert_eq!(sched.pop().unwrap().seq, seq(0));
        assert_eq!(sched.pop().unwrap().seq, seq(1));
        assert_eq!(sched.pop().unwrap().seq, seq(2));
    }

    /// Invariant: ts is always the primary sort key, seq only breaks ties
    #[test]
    fn interleaved_timestamps_pop_in_correct_order() {
        let mut sched = Scheduler::new();
        sched.push(Timestamp::new(10), seq(0), TestMsg(3));
        sched.push(Timestamp::new(5), seq(1), TestMsg(2));
        sched.push(Timestamp::new(15), seq(2), TestMsg(1));

        let e1 = sched.pop().unwrap();
        assert_eq!(e1.ts, Timestamp::new(5));
        assert_eq!(e1.seq, seq(1));

        let e2 = sched.pop().unwrap();
        assert_eq!(e2.ts, Timestamp::new(10));
        assert_eq!(e2.seq, seq(0));

        let e3 = sched.pop().unwrap();
        assert_eq!(e3.ts, Timestamp::new(15));
        assert_eq!(e3.seq, seq(2));
    }

    /// Invariant: within same ts, seq is tiebreaker regardless of push order
    #[test]
    fn same_ts_seq_tiebreaker_regardless_of_push_order() {
        let mut sched = Scheduler::new();
        sched.push(Timestamp::new(42), seq(5), TestMsg(5));
        sched.push(Timestamp::new(42), seq(0), TestMsg(0));
        sched.push(Timestamp::new(42), seq(3), TestMsg(3));
        sched.push(Timestamp::new(42), seq(1), TestMsg(1));
        sched.push(Timestamp::new(42), seq(4), TestMsg(4));
        sched.push(Timestamp::new(42), seq(2), TestMsg(2));

        for expected in 0..=5 {
            let event = sched.pop().unwrap();
            assert_eq!(event.seq.get(), expected);
            assert_eq!(event.ts, Timestamp::new(42));
        }
    }

    // ==================================================================
    // BFS cascade
    // ==================================================================

    /// Invariant: a same-instant cascade resolves before time advances.
    /// Push A@T, pop A, push B@T (from "handler"), pop B, then C@T+1 pops.
    #[test]
    fn same_instant_cascade_resolves_before_time_advances() {
        let mut sched = Scheduler::new();
        sched.push(Timestamp::new(100), seq(0), TestMsg(0)); // A@T
        sched.push(Timestamp::new(101), seq(2), TestMsg(2)); // C@T+1

        let a = sched.pop().unwrap();
        assert_eq!(a.ts, Timestamp::new(100));
        assert_eq!(a.seq.get(), 0);

        // Simulate handler producing B@T with a higher seq
        sched.push(Timestamp::new(100), seq(1), TestMsg(1)); // B@T

        let b = sched.pop().unwrap();
        assert_eq!(b.ts, Timestamp::new(100));
        assert_eq!(b.seq.get(), 1);

        // Only now does time advance
        let c = sched.pop().unwrap();
        assert_eq!(c.ts, Timestamp::new(101));
        assert_eq!(c.seq.get(), 2);
    }

    // ==================================================================
    // Event PartialEq / Eq
    // ==================================================================

    /// Invariant: same ts + same seq = equal
    #[test]
    fn event_eq_same_ts_same_seq() {
        let a = Event::new(Timestamp::new(100), seq(0), TestMsg(1));
        let b = Event::new(Timestamp::new(100), seq(0), TestMsg(2));
        assert_eq!(a, b);
    }

    /// Invariant: same ts + same seq + same payload = equal
    #[test]
    fn event_eq_same_ts_seq_and_payload() {
        let a = Event::new(Timestamp::new(100), seq(0), TestMsg(42));
        let b = Event::new(Timestamp::new(100), seq(0), TestMsg(42));
        assert_eq!(a, b);
    }

    /// Invariant: same ts + same seq + different message types = equal
    /// (payload does not participate in equality)
    #[test]
    fn event_eq_independent_of_message_type() {
        let a = Event::new(Timestamp::new(100), seq(0), TestMsg(1));
        let b = Event::new(Timestamp::new(100), seq(0), OtherMsg(1));
        assert_eq!(a, b);
    }

    /// Invariant: different ts = not equal
    #[test]
    fn event_ne_different_ts() {
        let a = Event::new(Timestamp::new(100), seq(0), TestMsg(1));
        let b = Event::new(Timestamp::new(200), seq(0), TestMsg(1));
        assert_ne!(a, b);
    }

    /// Invariant: different seq = not equal
    #[test]
    fn event_ne_different_seq() {
        let a = Event::new(Timestamp::new(100), seq(0), TestMsg(1));
        let b = Event::new(Timestamp::new(100), seq(1), TestMsg(1));
        assert_ne!(a, b);
    }

    // ==================================================================
    // Event PartialOrd / Ord
    // ==================================================================

    /// Invariant: earlier ts is greater (pops first, heap is min-heap via reversed Ord)
    #[test]
    fn event_ord_earlier_ts_greater() {
        let early = Event::new(Timestamp::new(50), seq(0), TestMsg(1));
        let late = Event::new(Timestamp::new(100), seq(0), TestMsg(2));
        assert!(early > late);
        assert!(!(late > early));
    }

    /// Invariant: same ts, lower seq is greater (pops first)
    #[test]
    fn event_ord_same_ts_lower_seq_greater() {
        let low_seq = Event::new(Timestamp::new(100), seq(0), TestMsg(1));
        let high_seq = Event::new(Timestamp::new(100), seq(1), TestMsg(2));
        assert!(low_seq > high_seq);
        assert!(!(high_seq > low_seq));
    }

    /// Invariant: ts dominates seq in ordering
    #[test]
    fn event_ord_ts_dominates_seq() {
        let early_high_seq = Event::new(Timestamp::new(50), seq(100), TestMsg(1));
        let late_low_seq = Event::new(Timestamp::new(100), seq(0), TestMsg(2));
        assert!(early_high_seq > late_low_seq);
    }

    /// Invariant: transitivity of the reversed Ord
    #[test]
    fn event_ord_transitive() {
        let a = Event::new(Timestamp::new(10), seq(0), TestMsg(1));
        let b = Event::new(Timestamp::new(10), seq(1), TestMsg(2));
        let c = Event::new(Timestamp::new(20), seq(0), TestMsg(3));
        assert!(a > b); // lower seq > higher seq
        assert!(b > c); // earlier ts > later ts
        assert!(a > c); // transitivity: T=10 > T=20
    }

    /// Invariant: antisymmetry
    #[test]
    fn event_ord_antisymmetric() {
        let a = Event::new(Timestamp::new(10), seq(0), TestMsg(1));
        let b = Event::new(Timestamp::new(10), seq(1), TestMsg(2));
        assert!(a > b);
        assert!(!(b > a));
    }

    /// Invariant: payload does not affect ordering
    #[test]
    fn event_ord_payload_irrelevant() {
        let a = Event::new(Timestamp::new(100), seq(0), TestMsg(999));
        let b = Event::new(Timestamp::new(100), seq(0), OtherMsg(1));
        assert!(!(a < b));
        assert!(!(b < a));
        assert_eq!(a, b);
    }

    // ==================================================================
    // Bulk / stress
    // ==================================================================

    /// Invariant: out-of-order insertion yields monotonic pops
    #[test]
    fn out_of_order_insertion_yields_monotonic_pops() {
        let mut sched = Scheduler::new();
        let events = [
            (Timestamp::new(300), seq(0)),
            (Timestamp::new(100), seq(2)),
            (Timestamp::new(200), seq(1)),
            (Timestamp::new(100), seq(0)),
            (Timestamp::new(100), seq(1)),
            (Timestamp::new(200), seq(0)),
        ];
        for (i, (ts, s)) in events.iter().enumerate() {
            sched.push(*ts, *s, TestMsg(i as u64));
        }

        let mut prev_ts = Timestamp::new(-1);
        let mut prev_seq = seq(0);
        let mut first = true;
        while let Some(event) = sched.pop() {
            if !first {
                assert!(
                    event.ts > prev_ts || (event.ts == prev_ts && event.seq > prev_seq),
                    "pop out of order: ({:?}, {:?}) after ({:?}, {:?})",
                    event.ts,
                    event.seq,
                    prev_ts,
                    prev_seq
                );
            }
            first = false;
            prev_ts = event.ts;
            prev_seq = event.seq;
        }
    }

    /// Invariant: large count of events pops in monotonic order
    #[test]
    fn large_count_monotonic_pops() {
        let mut sched = Scheduler::new();
        let n = 1000;

        let mut s = SeqNo::initial();
        for i in 0..n {
            let ts = Timestamp::new(((i * 97 + 13) % n) as i128);
            sched.push(ts, s, TestMsg(i as u64));
            s = s.next();
        }

        let mut prev_ts = Timestamp::new(-1);
        let mut prev_seq = seq(0);
        let mut first = true;
        let mut count = 0;
        while let Some(event) = sched.pop() {
            if !first {
                assert!(
                    event.ts > prev_ts || (event.ts == prev_ts && event.seq > prev_seq),
                    "pop out of order at count={}: ({:?}, {:?}) after ({:?}, {:?})",
                    count,
                    event.ts,
                    event.seq,
                    prev_ts,
                    prev_seq
                );
            }
            first = false;
            prev_ts = event.ts;
            prev_seq = event.seq;
            count += 1;
        }
        assert_eq!(count, n);
    }
}
