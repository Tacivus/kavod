use std::{collections::BinaryHeap, sync::Arc};

use crate::{
    message::{Message, SharedMessage},
    sequence::SeqNo,
    time::timestamp::Timestamp,
};

#[derive(Debug)]
pub(crate) struct Scheduler {
    heap: BinaryHeap<ScheduledItem>,
}

impl Scheduler {
    pub(crate) fn new() -> Self {
        Scheduler {
            heap: BinaryHeap::new(),
        }
    }

    pub(crate) fn push_msg(&mut self, ts: Timestamp, seq: SeqNo, msg: impl Message) {
        self.heap.push(ScheduledItem::new(ts, seq, msg))
    }

    pub(crate) fn pop(&mut self) -> Option<ScheduledItem> {
        self.heap.pop()
    }
}

#[cfg(test)]
impl Scheduler {
    pub(crate) fn len(&self) -> usize {
        self.heap.len()
    }
}

#[derive(Debug)]
pub(crate) struct ScheduledItem {
    dispatch_time: Timestamp,
    sequence_num: SeqNo,
    payload: SharedMessage,
}

impl ScheduledItem {
    fn new(ts: Timestamp, seq: SeqNo, payload: impl Message) -> Self {
        Self {
            dispatch_time: ts,
            sequence_num: seq,
            payload: Arc::new(payload),
        }
    }

    pub(crate) fn dispatch_time(&self) -> Timestamp {
        self.dispatch_time
    }

    pub(crate) fn payload(&self) -> SharedMessage {
        self.payload.clone()
    }
}

#[cfg(test)]
impl ScheduledItem {
    pub(crate) fn sequence_num(&self) -> SeqNo {
        self.sequence_num
    }
}

impl PartialEq for ScheduledItem {
    fn eq(&self, other: &Self) -> bool {
        self.dispatch_time == other.dispatch_time && self.sequence_num == other.sequence_num
    }
}

impl Eq for ScheduledItem {}

impl PartialOrd for ScheduledItem {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ScheduledItem {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Reverse for min-heap: earlier ts = "greater", lower seq = "greater"
        other
            .dispatch_time
            .cmp(&self.dispatch_time)
            .then_with(|| other.sequence_num.cmp(&self.sequence_num))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::Message;
    use crate::sequence::SeqNo;
    use std::any::Any;

    #[derive(Clone, Debug, PartialEq)]
    struct TestMsg(u64);
    impl Message for TestMsg {}

    #[derive(Clone, Debug, PartialEq)]
    struct OtherMsg(u64);
    impl Message for OtherMsg {}

    // ========================================================================
    // Scheduler state
    // ========================================================================

    /// Invariant: a new Scheduler has no queued items
    #[test]
    fn test_new_scheduler_is_empty() {
        let sched = Scheduler::new();
        assert_eq!(sched.len(), 0);
    }

    /// Invariant: a new Scheduler pop returns None
    #[test]
    fn test_new_scheduler_pop_returns_none() {
        let mut sched = Scheduler::new();
        assert!(sched.pop().is_none());
    }

    /// Invariant: push_msg / pop roundtrip returns the same message payload
    #[test]
    fn test_push_msg_pop_roundtrip() {
        let mut sched = Scheduler::new();
        sched.push_msg(Timestamp::new(100), SeqNo::from_raw(0), TestMsg(42));

        let item = sched.pop().unwrap();
        assert_eq!(item.dispatch_time, Timestamp::new(100));
        assert_eq!(item.sequence_num, SeqNo::from_raw(0));

        let payload: &dyn Any = &*item.payload;
        assert_eq!(payload.downcast_ref::<TestMsg>(), Some(&TestMsg(42)));
    }

    /// Invariant: push N items, pop exactly N times, then None
    #[test]
    fn test_pops_exhaust_all_items() {
        let mut sched = Scheduler::new();
        let n = 5;
        for i in 0..n {
            sched.push_msg(Timestamp::new(100), SeqNo::from_raw(i), TestMsg(i));
        }
        for _ in 0..n {
            assert!(sched.pop().is_some());
        }
        assert!(sched.pop().is_none());
        assert_eq!(sched.len(), 0);
    }

    /// Invariant: pop from empty returns None repeatedly
    #[test]
    fn test_pop_from_empty_returns_none() {
        let mut sched = Scheduler::new();
        assert!(sched.pop().is_none());
        assert!(sched.pop().is_none());
    }

    /// Invariant: push after exhaustion works
    #[test]
    fn test_push_after_empty_works() {
        let mut sched = Scheduler::new();
        sched.push_msg(Timestamp::new(100), SeqNo::from_raw(0), TestMsg(1));
        assert!(sched.pop().is_some());
        assert!(sched.pop().is_none());

        sched.push_msg(Timestamp::new(200), SeqNo::from_raw(1), TestMsg(2));
        let item = sched.pop().unwrap();
        assert_eq!(item.dispatch_time, Timestamp::new(200));
        assert_eq!(sched.len(), 0);
    }

    // ========================================================================
    // Ordering correctness
    // ========================================================================

    /// Invariant: earliest dispatch_time pops first
    #[test]
    fn test_earliest_ts_pops_first() {
        let mut sched = Scheduler::new();
        sched.push_msg(Timestamp::new(200), SeqNo::from_raw(0), TestMsg(2));
        sched.push_msg(Timestamp::new(100), SeqNo::from_raw(1), TestMsg(1));

        assert_eq!(sched.pop().unwrap().dispatch_time, Timestamp::new(100));
        assert_eq!(sched.pop().unwrap().dispatch_time, Timestamp::new(200));
    }

    /// Invariant: equal dispatch_time, lower sequence pops first
    #[test]
    fn test_same_ts_lower_seq_pops_first() {
        let mut sched = Scheduler::new();
        sched.push_msg(Timestamp::new(100), SeqNo::from_raw(1), TestMsg(2));
        sched.push_msg(Timestamp::new(100), SeqNo::from_raw(0), TestMsg(1));

        let first = sched.pop().unwrap();
        assert_eq!(first.sequence_num, SeqNo::from_raw(0));
        let second = sched.pop().unwrap();
        assert_eq!(second.sequence_num, SeqNo::from_raw(1));
    }

    /// Invariant: same ts, inserted in seq order, pops in seq order regardless
    #[test]
    fn test_same_ts_inserted_in_seq_order_pops_in_seq_order() {
        let mut sched = Scheduler::new();
        sched.push_msg(Timestamp::new(100), SeqNo::from_raw(0), TestMsg(0));
        sched.push_msg(Timestamp::new(100), SeqNo::from_raw(1), TestMsg(1));
        sched.push_msg(Timestamp::new(100), SeqNo::from_raw(2), TestMsg(2));

        assert_eq!(sched.pop().unwrap().sequence_num, SeqNo::from_raw(0));
        assert_eq!(sched.pop().unwrap().sequence_num, SeqNo::from_raw(1));
        assert_eq!(sched.pop().unwrap().sequence_num, SeqNo::from_raw(2));
    }

    /// Invariant: dispatch_time is always the primary sort key, seq only
    ///             breaks ties
    #[test]
    fn test_interleaved_timestamps_pop_in_correct_order() {
        let mut sched = Scheduler::new();
        sched.push_msg(Timestamp::new(10), SeqNo::from_raw(0), TestMsg(3));
        sched.push_msg(Timestamp::new(5), SeqNo::from_raw(1), TestMsg(2));
        sched.push_msg(Timestamp::new(15), SeqNo::from_raw(2), TestMsg(1));

        let e1 = sched.pop().unwrap();
        assert_eq!(e1.dispatch_time, Timestamp::new(5));
        assert_eq!(e1.sequence_num, SeqNo::from_raw(1));

        let e2 = sched.pop().unwrap();
        assert_eq!(e2.dispatch_time, Timestamp::new(10));
        assert_eq!(e2.sequence_num, SeqNo::from_raw(0));

        let e3 = sched.pop().unwrap();
        assert_eq!(e3.dispatch_time, Timestamp::new(15));
        assert_eq!(e3.sequence_num, SeqNo::from_raw(2));
    }

    /// Invariant: within same dispatch_time, seq is the tiebreaker
    ///             regardless of push order
    #[test]
    fn test_same_ts_seq_tiebreaker_regardless_of_push_order() {
        let mut sched = Scheduler::new();
        sched.push_msg(Timestamp::new(42), SeqNo::from_raw(5), TestMsg(5));
        sched.push_msg(Timestamp::new(42), SeqNo::from_raw(0), TestMsg(0));
        sched.push_msg(Timestamp::new(42), SeqNo::from_raw(3), TestMsg(3));
        sched.push_msg(Timestamp::new(42), SeqNo::from_raw(1), TestMsg(1));
        sched.push_msg(Timestamp::new(42), SeqNo::from_raw(4), TestMsg(4));
        sched.push_msg(Timestamp::new(42), SeqNo::from_raw(2), TestMsg(2));

        for expected in 0..=5 {
            let item = sched.pop().unwrap();
            assert_eq!(item.sequence_num(), SeqNo::from_raw(expected));
            assert_eq!(item.dispatch_time, Timestamp::new(42));
        }
    }

    // ========================================================================
    // BFS cascade
    // ========================================================================

    /// Invariant: a same-instant cascade resolves before time advances.
    /// Push A@T, pop A, push B@T (simulating handler output), pop B,
    /// then C@T+1 pops.
    #[test]
    fn test_same_instant_cascade_resolves_before_time_advances() {
        let mut sched = Scheduler::new();
        sched.push_msg(Timestamp::new(100), SeqNo::from_raw(0), TestMsg(0)); // A@T
        sched.push_msg(Timestamp::new(101), SeqNo::from_raw(2), TestMsg(2)); // C@T+1

        let a = sched.pop().unwrap();
        assert_eq!(a.dispatch_time, Timestamp::new(100));
        assert_eq!(a.sequence_num(), SeqNo::from_raw(0));

        // Simulate handler producing B@T with a higher seq
        sched.push_msg(Timestamp::new(100), SeqNo::from_raw(1), TestMsg(1)); // B@T

        let b = sched.pop().unwrap();
        assert_eq!(b.dispatch_time, Timestamp::new(100));
        assert_eq!(b.sequence_num(), SeqNo::from_raw(1));

        // Only now does time advance
        let c = sched.pop().unwrap();
        assert_eq!(c.dispatch_time, Timestamp::new(101));
        assert_eq!(c.sequence_num(), SeqNo::from_raw(2));
    }

    // ========================================================================
    // ScheduledItem PartialEq / Eq
    // ========================================================================

    /// Invariant: same dispatch_time + same sequence = equal regardless
    ///             of payload value
    #[test]
    fn test_item_eq_same_ts_same_seq_different_payload() {
        let a = ScheduledItem::new(Timestamp::new(100), SeqNo::from_raw(0), TestMsg(1));
        let b = ScheduledItem::new(Timestamp::new(100), SeqNo::from_raw(0), TestMsg(2));
        assert_eq!(a, b);
    }

    /// Invariant: same dispatch_time + same sequence + same payload = equal
    #[test]
    fn test_item_eq_same_ts_same_seq_same_payload() {
        let a = ScheduledItem::new(Timestamp::new(100), SeqNo::from_raw(0), TestMsg(42));
        let b = ScheduledItem::new(Timestamp::new(100), SeqNo::from_raw(0), TestMsg(42));
        assert_eq!(a, b);
    }

    /// Invariant: payload message type does not participate in equality
    #[test]
    fn test_item_eq_independent_of_message_type() {
        let a = ScheduledItem::new(Timestamp::new(100), SeqNo::from_raw(0), TestMsg(1));
        let b = ScheduledItem::new(Timestamp::new(100), SeqNo::from_raw(0), OtherMsg(1));
        assert_eq!(a, b);
    }

    /// Invariant: different dispatch_time => not equal
    #[test]
    fn test_item_ne_different_ts() {
        let a = ScheduledItem::new(Timestamp::new(100), SeqNo::from_raw(0), TestMsg(1));
        let b = ScheduledItem::new(Timestamp::new(200), SeqNo::from_raw(0), TestMsg(1));
        assert_ne!(a, b);
    }

    /// Invariant: different sequence => not equal
    #[test]
    fn test_item_ne_different_seq() {
        let a = ScheduledItem::new(Timestamp::new(100), SeqNo::from_raw(0), TestMsg(1));
        let b = ScheduledItem::new(Timestamp::new(100), SeqNo::from_raw(1), TestMsg(1));
        assert_ne!(a, b);
    }

    // ========================================================================
    // ScheduledItem PartialOrd / Ord
    // ========================================================================

    /// Invariant: earlier dispatch_time is greater (pops first via reversed
    ///             Ord for min-heap)
    #[test]
    fn test_item_ord_earlier_ts_greater() {
        let early = ScheduledItem::new(Timestamp::new(50), SeqNo::from_raw(0), TestMsg(1));
        let late = ScheduledItem::new(Timestamp::new(100), SeqNo::from_raw(0), TestMsg(2));
        assert!(early > late);
        assert!(late <= early);
    }

    /// Invariant: equal dispatch_time, lower sequence is greater (pops first)
    #[test]
    fn test_item_ord_same_ts_lower_seq_greater() {
        let low_seq = ScheduledItem::new(Timestamp::new(100), SeqNo::from_raw(0), TestMsg(1));
        let high_seq = ScheduledItem::new(Timestamp::new(100), SeqNo::from_raw(1), TestMsg(2));
        assert!(low_seq > high_seq);
        assert!(high_seq <= low_seq);
    }

    /// Invariant: dispatch_time dominates sequence in ordering
    #[test]
    fn test_item_ord_ts_dominates_seq() {
        let early_high_seq =
            ScheduledItem::new(Timestamp::new(50), SeqNo::from_raw(100), TestMsg(1));
        let late_low_seq = ScheduledItem::new(Timestamp::new(100), SeqNo::from_raw(0), TestMsg(2));
        assert!(early_high_seq > late_low_seq);
    }

    /// Invariant: transitivity of the reversed Ord
    #[test]
    fn test_item_ord_transitive() {
        let a = ScheduledItem::new(Timestamp::new(10), SeqNo::from_raw(0), TestMsg(1));
        let b = ScheduledItem::new(Timestamp::new(10), SeqNo::from_raw(1), TestMsg(2));
        let c = ScheduledItem::new(Timestamp::new(20), SeqNo::from_raw(0), TestMsg(3));
        assert!(a > b); // lower seq > higher seq
        assert!(b > c); // earlier ts > later ts
        assert!(a > c); // transitivity: T=10 > T=20
    }

    /// Invariant: antisymmetry — if a > b then not (b > a)
    #[test]
    fn test_item_ord_antisymmetric() {
        let a = ScheduledItem::new(Timestamp::new(10), SeqNo::from_raw(0), TestMsg(1));
        let b = ScheduledItem::new(Timestamp::new(10), SeqNo::from_raw(1), TestMsg(2));
        assert!(a > b);
        assert!(b <= a);
    }

    /// Invariant: payload does not affect ordering
    #[test]
    fn test_item_ord_payload_irrelevant() {
        let a = ScheduledItem::new(Timestamp::new(100), SeqNo::from_raw(0), TestMsg(999));
        let b = ScheduledItem::new(Timestamp::new(100), SeqNo::from_raw(0), OtherMsg(1));
        assert!(a >= b);
        assert!(b >= a);
        assert_eq!(a, b);
    }

    // ========================================================================
    // Shared-message ownership
    // ========================================================================

    /// Invariant: a popped payload is an Arc that can be cloned, and both
    ///             clones refer to the same allocation
    #[test]
    fn test_popped_payload_is_shared_arc() {
        let mut sched = Scheduler::new();
        sched.push_msg(Timestamp::new(100), SeqNo::from_raw(0), TestMsg(42));

        let item = sched.pop().unwrap();
        let payload_a = &*item.payload;
        let payload_b = item.payload.clone();

        let a: &TestMsg = (payload_a as &dyn Any).downcast_ref::<TestMsg>().unwrap();
        let b: &TestMsg = (&*payload_b as &dyn Any).downcast_ref::<TestMsg>().unwrap();

        assert_eq!(a, b);
        assert_eq!(a.0, 42);
    }

    // ========================================================================
    // Bulk / stress
    // ========================================================================

    /// Invariant: out-of-order insertion yields monotonic pops
    #[test]
    fn test_out_of_order_insertion_yields_monotonic_pops() {
        let mut sched = Scheduler::new();
        let events = [
            (Timestamp::new(300), SeqNo::from_raw(0)),
            (Timestamp::new(100), SeqNo::from_raw(2)),
            (Timestamp::new(200), SeqNo::from_raw(1)),
            (Timestamp::new(100), SeqNo::from_raw(0)),
            (Timestamp::new(100), SeqNo::from_raw(1)),
            (Timestamp::new(200), SeqNo::from_raw(0)),
        ];
        for (i, (ts, s)) in events.iter().enumerate() {
            sched.push_msg(*ts, *s, TestMsg(i as u64));
        }

        let mut prev_ts = Timestamp::new(-1);
        let mut prev_seq = SeqNo::from_raw(0);
        let mut first = true;
        while let Some(item) = sched.pop() {
            if !first {
                assert!(
                    item.dispatch_time > prev_ts
                        || (item.dispatch_time == prev_ts && item.sequence_num > prev_seq),
                    "pop out of order: ({:?}, {:?}) after ({:?}, {:?})",
                    item.dispatch_time,
                    item.sequence_num,
                    prev_ts,
                    prev_seq
                );
            }
            first = false;
            prev_ts = item.dispatch_time;
            prev_seq = item.sequence_num;
        }
    }

    /// Invariant: large count of items pops in monotonic order
    #[test]
    fn test_large_count_monotonic_pops() {
        let mut sched = Scheduler::new();
        let n = 1000;

        for i in 0..n {
            let ts = Timestamp::new(((i * 97 + 13) % n) as i128);
            sched.push_msg(ts, SeqNo::from_raw(i), TestMsg(i));
        }

        let mut prev_ts = Timestamp::new(-1);
        let mut prev_seq = SeqNo::from_raw(0);
        let mut first = true;
        let mut count = 0;
        while let Some(item) = sched.pop() {
            if !first {
                assert!(
                    item.dispatch_time > prev_ts
                        || (item.dispatch_time == prev_ts && item.sequence_num > prev_seq),
                    "pop out of order at count={}: ({:?}, {:?}) after ({:?}, {:?})",
                    count,
                    item.dispatch_time,
                    item.sequence_num,
                    prev_ts,
                    prev_seq
                );
            }
            first = false;
            prev_ts = item.dispatch_time;
            prev_seq = item.sequence_num;
            count += 1;
        }
        assert_eq!(count, n);
    }
}
