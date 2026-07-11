use std::{any::TypeId, collections::HashSet};

use crate::{
    cache::{Cache, State},
    clock::Clock,
    log::SeqNo,
    message::Message,
    time::timestamp::Timestamp,
};

pub struct Context<'a> {
    cache: &'a Cache,
    clock: &'a dyn Clock,
    seq: &'a SeqNo,
    otubox: &'a mut Vec<(Timestamp, Box<dyn Message>)>,
    declared_productions: Option<&'a HashSet<TypeId>>,
}

impl<'a> Context<'a> {
    pub fn new(
        cache: &'a Cache,
        clock: &'a dyn Clock,
        seq: &'a SeqNo,
        otubox: &'a mut Vec<(Timestamp, Box<dyn Message>)>,
    ) -> Self {
        Self {
            cache,
            clock,
            seq,
            otubox,
            declared_productions: None,
        }
    }

    /// Create a Context whose `send` / `send_at` will verify that the
    /// message type being sent was declared via `.produces::<M>()`.
    pub(crate) fn new_for_handler(
        cache: &'a Cache,
        clock: &'a dyn Clock,
        seq: &'a SeqNo,
        otubox: &'a mut Vec<(Timestamp, Box<dyn Message>)>,
        declared_productions: &'a HashSet<TypeId>,
    ) -> Self {
        Self {
            cache,
            clock,
            seq,
            otubox,
            declared_productions: Some(declared_productions),
        }
    }

    pub fn now(&self) -> Timestamp {
        self.clock.now()
    }

    pub fn seq(&self) -> SeqNo {
        *self.seq
    }

    pub fn get<T: State>(&self, key: &T::Key) -> Option<&T> {
        self.cache.get_keyed::<T>(key)
    }

    /// Panics if a declared_productions set is active and `M` is not
    /// in it. No-op otherwise (used outside of handler dispatch).
    fn check_produces<M: Message>(&self) {
        if let Some(allowed) = self.declared_productions {
            let tid = TypeId::of::<M>();
            assert!(
                allowed.contains(&tid),
                "handler sent message of type `{}` but did not declare `.produces::<{}>()`",
                std::any::type_name::<M>(),
                std::any::type_name::<M>(),
            );
        }
    }

    pub fn send<M: Message>(&mut self, msg: M) {
        self.check_produces::<M>();
        self.otubox.push((self.now(), Box::new(msg)));
    }

    pub fn send_at<M: Message>(&mut self, ts: Timestamp, msg: M) {
        assert!(ts >= self.now(), "cannot send to the past");
        self.check_produces::<M>();
        self.otubox.push((ts, Box::new(msg)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::sim::SimClock;
    use std::any::Any;

    // ------------------------------------------------------------------------
    // Test types
    // ------------------------------------------------------------------------

    #[derive(Clone, Debug, PartialEq)]
    struct Account {
        id: u32,
        cash: i64,
    }
    impl State for Account {
        type Key = u32;
        fn key(&self) -> u32 {
            self.id
        }
    }

    #[derive(Clone, Debug, PartialEq)]
    struct Config {
        max_risk: f64,
    }
    impl State for Config {
        type Key = ();
        fn key(&self) {}
    }

    #[derive(Clone, Debug, PartialEq)]
    struct Book {
        instr: u32,
        orders: u64,
    }
    impl State for Book {
        type Key = u32;
        fn key(&self) -> u32 {
            self.instr
        }
    }

    #[derive(Clone, Debug, PartialEq)]
    struct Order {
        id: u64,
    }
    impl Message for Order {}

    #[derive(Clone, Debug, PartialEq)]
    struct Fill {
        id: u64,
    }
    impl Message for Fill {}

    // ------------------------------------------------------------------------
    // Helpers
    // ------------------------------------------------------------------------

    fn seq(n: u64) -> SeqNo {
        let mut s = SeqNo::initial();
        for _ in 0..n {
            s = s.next();
        }
        s
    }

    // ------------------------------------------------------------------------
    // now()
    // ------------------------------------------------------------------------

    /// Invariant: now() returns the current clock time
    #[test]
    fn now_returns_clock_time() {
        let clock = SimClock::new(Timestamp::new(1000));
        let cache = Cache::new();
        let s = seq(0);
        let mut outbox = Vec::new();

        let ctx = Context::new(&cache, &clock, &s, &mut outbox);
        assert_eq!(ctx.now(), Timestamp::new(1000));
    }

    /// Invariant: now() returns zero when the clock starts at zero
    #[test]
    fn now_zero_clock() {
        let clock = SimClock::new(Timestamp::new(0));
        let cache = Cache::new();
        let s = seq(0);
        let mut outbox = Vec::new();

        let ctx = Context::new(&cache, &clock, &s, &mut outbox);
        assert_eq!(ctx.now(), Timestamp::new(0));
    }

    /// Invariant: now() returns the same value across repeated calls
    #[test]
    fn now_idempotent() {
        let clock = SimClock::new(Timestamp::new(500));
        let cache = Cache::new();
        let s = seq(0);
        let mut outbox = Vec::new();

        let ctx = Context::new(&cache, &clock, &s, &mut outbox);
        assert_eq!(ctx.now(), Timestamp::new(500));
        assert_eq!(ctx.now(), Timestamp::new(500));
    }

    // ------------------------------------------------------------------------
    // seq()
    // ------------------------------------------------------------------------

    /// Invariant: seq() returns the value of the SeqNo reference passed at
    /// construction
    #[test]
    fn seq_returns_referenced_value() {
        let clock = SimClock::new(Timestamp::new(0));
        let cache = Cache::new();
        let s = seq(7);
        let mut outbox = Vec::new();

        let ctx = Context::new(&cache, &clock, &s, &mut outbox);
        assert_eq!(ctx.seq(), s);
    }

    /// Invariant: an initial SeqNo produces seq() == 0
    #[test]
    fn seq_initial_is_zero() {
        let clock = SimClock::new(Timestamp::new(0));
        let cache = Cache::new();
        let s = SeqNo::initial();
        let mut outbox = Vec::new();

        let ctx = Context::new(&cache, &clock, &s, &mut outbox);
        assert_eq!(ctx.seq(), SeqNo::initial());
    }

    /// Invariant: seq() is stable across repeated calls
    #[test]
    fn seq_idempotent() {
        let clock = SimClock::new(Timestamp::new(0));
        let cache = Cache::new();
        let s = seq(10_000);
        let mut outbox = Vec::new();

        let ctx = Context::new(&cache, &clock, &s, &mut outbox);
        let first = ctx.seq();
        let second = ctx.seq();
        assert_eq!(first, second);
        assert_eq!(first, s);
    }

    // ------------------------------------------------------------------------
    // get()
    // ------------------------------------------------------------------------

    /// Invariant: get() returns a reference to an existing keyed value
    #[test]
    fn get_returns_existing_keyed_value() {
        let clock = SimClock::new(Timestamp::new(0));
        let mut cache = Cache::new();
        cache.insert(Account { id: 1, cash: 100 });
        let s = seq(0);
        let mut outbox = Vec::new();

        let ctx = Context::new(&cache, &clock, &s, &mut outbox);
        let acc = ctx.get::<Account>(&1);
        assert!(acc.is_some());
        assert_eq!(acc.unwrap().cash, 100);
    }

    /// Invariant: get() returns None for a key that was never inserted
    #[test]
    fn get_returns_none_for_missing_key() {
        let clock = SimClock::new(Timestamp::new(0));
        let mut cache = Cache::new();
        cache.insert(Account { id: 1, cash: 100 });
        let s = seq(0);
        let mut outbox = Vec::new();

        let ctx = Context::new(&cache, &clock, &s, &mut outbox);
        assert_eq!(ctx.get::<Account>(&2), None);
    }

    /// Invariant: get() on an empty cache returns None for any key
    #[test]
    fn get_empty_cache_returns_none() {
        let clock = SimClock::new(Timestamp::new(0));
        let cache = Cache::new();
        let s = seq(0);
        let mut outbox = Vec::new();

        let ctx = Context::new(&cache, &clock, &s, &mut outbox);
        assert_eq!(ctx.get::<Account>(&1), None);
        assert_eq!(ctx.get::<Account>(&0), None);
    }

    /// Invariant: different types stored under the same key value do not collide
    #[test]
    fn get_different_types_same_key_no_collision() {
        let clock = SimClock::new(Timestamp::new(0));
        let mut cache = Cache::new();
        cache.insert(Account { id: 1, cash: 100 });
        cache.insert(Book {
            instr: 1,
            orders: 42,
        });
        let s = seq(0);
        let mut outbox = Vec::new();

        let ctx = Context::new(&cache, &clock, &s, &mut outbox);
        assert_eq!(ctx.get::<Account>(&1).unwrap().cash, 100);
        assert_eq!(ctx.get::<Book>(&1).unwrap().orders, 42);
    }

    /// Invariant: get() works for singletons using Key = ()
    #[test]
    fn get_singleton_returns_value() {
        let clock = SimClock::new(Timestamp::new(0));
        let mut cache = Cache::new();
        cache.insert(Config { max_risk: 0.05 });
        let s = seq(0);
        let mut outbox = Vec::new();

        let ctx = Context::new(&cache, &clock, &s, &mut outbox);
        assert_eq!(ctx.get::<Config>(&()).unwrap().max_risk, 0.05);
    }

    /// Invariant: get() on a missing singleton returns None
    #[test]
    fn get_singleton_empty_cache_returns_none() {
        let clock = SimClock::new(Timestamp::new(0));
        let cache = Cache::new();
        let s = seq(0);
        let mut outbox = Vec::new();

        let ctx = Context::new(&cache, &clock, &s, &mut outbox);
        assert_eq!(ctx.get::<Config>(&()), None);
    }

    /// Invariant: multiple instances of the same type with different keys are
    /// independently retrievable
    #[test]
    fn get_multiple_same_type_different_keys() {
        let clock = SimClock::new(Timestamp::new(0));
        let mut cache = Cache::new();
        cache.insert(Account { id: 0, cash: 1000 });
        cache.insert(Account { id: 1, cash: 2000 });
        let s = seq(0);
        let mut outbox = Vec::new();

        let ctx = Context::new(&cache, &clock, &s, &mut outbox);
        assert_eq!(ctx.get::<Account>(&0).unwrap().cash, 1000);
        assert_eq!(ctx.get::<Account>(&1).unwrap().cash, 2000);
        assert_eq!(ctx.get::<Account>(&2), None);
    }

    /// Invariant: after upserting a key, get() returns the last-inserted value
    #[test]
    fn get_reflects_upserted_value() {
        let clock = SimClock::new(Timestamp::new(0));
        let mut cache = Cache::new();
        cache.insert(Account { id: 1, cash: 100 });
        cache.insert(Account { id: 1, cash: 999 });
        let s = seq(0);
        let mut outbox = Vec::new();

        let ctx = Context::new(&cache, &clock, &s, &mut outbox);
        assert_eq!(ctx.get::<Account>(&1).unwrap().cash, 999);
    }

    // ------------------------------------------------------------------------
    // send()
    // ------------------------------------------------------------------------

    /// Invariant: send() pushes a message with the current clock timestamp
    #[test]
    fn send_pushes_with_now_timestamp() {
        let clock = SimClock::new(Timestamp::new(1000));
        let cache = Cache::new();
        let s = seq(0);
        let mut outbox = Vec::new();

        {
            let mut ctx = Context::new(&cache, &clock, &s, &mut outbox);
            ctx.send(Order { id: 1 });
        }

        assert_eq!(outbox.len(), 1);
        assert_eq!(outbox[0].0, Timestamp::new(1000));
    }

    /// Invariant: the sent message preserves its type and content
    #[test]
    fn send_message_type_preserved() {
        let clock = SimClock::new(Timestamp::new(0));
        let cache = Cache::new();
        let s = seq(0);
        let mut outbox = Vec::new();

        {
            let mut ctx = Context::new(&cache, &clock, &s, &mut outbox);
            ctx.send(Order { id: 42 });
        }

        let payload: &dyn Any = &*outbox[0].1;
        let order = payload.downcast_ref::<Order>().unwrap();
        assert_eq!(order.id, 42);
    }

    /// Invariant: multiple sends accumulate in push order
    #[test]
    fn send_multiple_accumulate_in_push_order() {
        let clock = SimClock::new(Timestamp::new(0));
        let cache = Cache::new();
        let s = seq(0);
        let mut outbox = Vec::new();

        {
            let mut ctx = Context::new(&cache, &clock, &s, &mut outbox);
            ctx.send(Order { id: 1 });
            ctx.send(Order { id: 2 });
            ctx.send(Order { id: 3 });
        }

        assert_eq!(outbox.len(), 3);
        let ids: Vec<u64> = outbox
            .iter()
            .map(|(_, msg)| {
                let payload: &dyn Any = &**msg;
                payload.downcast_ref::<Order>().unwrap().id
            })
            .collect();
        assert_eq!(ids, vec![1, 2, 3]);
    }

    /// Invariant: sending one message turns an empty outbox non-empty
    #[test]
    fn send_to_empty_outbox() {
        let clock = SimClock::new(Timestamp::new(0));
        let cache = Cache::new();
        let s = seq(0);
        let mut outbox = Vec::new();

        assert!(outbox.is_empty());
        {
            let mut ctx = Context::new(&cache, &clock, &s, &mut outbox);
            ctx.send(Order { id: 1 });
        }
        assert_eq!(outbox.len(), 1);
    }

    /// Invariant: messages of different types coexist in the outbox
    #[test]
    fn send_mixed_message_types() {
        let clock = SimClock::new(Timestamp::new(0));
        let cache = Cache::new();
        let s = seq(0);
        let mut outbox = Vec::new();

        {
            let mut ctx = Context::new(&cache, &clock, &s, &mut outbox);
            ctx.send(Order { id: 1 });
            ctx.send(Fill { id: 99 });
        }

        assert_eq!(outbox.len(), 2);
        let first: &dyn Any = &*outbox[0].1;
        let second: &dyn Any = &*outbox[1].1;
        assert!(first.downcast_ref::<Order>().is_some());
        assert_eq!(second.downcast_ref::<Fill>().unwrap().id, 99);
    }

    // ------------------------------------------------------------------------
    // send_at()
    // ------------------------------------------------------------------------

    /// Invariant: send_at() uses the caller-supplied timestamp, not now()
    #[test]
    fn send_at_future_timestamp() {
        let clock = SimClock::new(Timestamp::new(1000));
        let cache = Cache::new();
        let s = seq(0);
        let mut outbox = Vec::new();

        {
            let mut ctx = Context::new(&cache, &clock, &s, &mut outbox);
            ctx.send_at(Timestamp::new(5000), Order { id: 1 });
        }

        assert_eq!(outbox.len(), 1);
        assert_eq!(outbox[0].0, Timestamp::new(5000));
    }

    /// Invariant: send_at(now(), msg) does not panic
    #[test]
    fn send_at_same_timestamp_does_not_panic() {
        let clock = SimClock::new(Timestamp::new(1000));
        let cache = Cache::new();
        let s = seq(0);
        let mut outbox = Vec::new();

        let mut ctx = Context::new(&cache, &clock, &s, &mut outbox);
        ctx.send_at(Timestamp::new(1000), Order { id: 1 });

        assert_eq!(outbox.len(), 1);
    }

    /// Invariant: send_at with a past timestamp panics
    #[test]
    #[should_panic(expected = "cannot send to the past")]
    fn send_at_past_timestamp_panics() {
        let clock = SimClock::new(Timestamp::new(1000));
        let cache = Cache::new();
        let s = seq(0);
        let mut outbox = Vec::new();

        let mut ctx = Context::new(&cache, &clock, &s, &mut outbox);
        ctx.send_at(Timestamp::new(999), Order { id: 1 });
    }

    /// Invariant: send_at with a far-past timestamp also panics
    #[test]
    #[should_panic(expected = "cannot send to the past")]
    fn send_at_far_past_panics() {
        let clock = SimClock::new(Timestamp::new(1_000_000));
        let cache = Cache::new();
        let s = seq(0);
        let mut outbox = Vec::new();

        let mut ctx = Context::new(&cache, &clock, &s, &mut outbox);
        ctx.send_at(Timestamp::new(0), Order { id: 1 });
    }

    /// Invariant: the message sent via send_at preserves its type and content
    #[test]
    fn send_at_future_message_type_preserved() {
        let clock = SimClock::new(Timestamp::new(0));
        let cache = Cache::new();
        let s = seq(0);
        let mut outbox = Vec::new();

        {
            let mut ctx = Context::new(&cache, &clock, &s, &mut outbox);
            ctx.send_at(Timestamp::new(100), Fill { id: 55 });
        }

        let payload: &dyn Any = &*outbox[0].1;
        assert_eq!(payload.downcast_ref::<Fill>().unwrap().id, 55);
    }

    /// Invariant: send() and send_at() preserve push order in the outbox;
    /// time-ordering is the scheduler's responsibility
    #[test]
    fn send_and_send_at_mixed_preserves_push_order() {
        let clock = SimClock::new(Timestamp::new(100));
        let cache = Cache::new();
        let s = seq(0);
        let mut outbox = Vec::new();

        {
            let mut ctx = Context::new(&cache, &clock, &s, &mut outbox);
            ctx.send(Order { id: 1 });
            ctx.send_at(Timestamp::new(9000), Order { id: 2 });
            ctx.send(Order { id: 3 });
        }

        assert_eq!(outbox.len(), 3);
        assert_eq!(outbox[0].0, Timestamp::new(100));
        assert_eq!(outbox[1].0, Timestamp::new(9000));
        assert_eq!(outbox[2].0, Timestamp::new(100));
    }

    // ------------------------------------------------------------------------
    // Composition
    // ------------------------------------------------------------------------

    /// Invariant: reading the cache and sending a message in sequence both work
    #[test]
    fn get_then_send_both_work() {
        let clock = SimClock::new(Timestamp::new(0));
        let mut cache = Cache::new();
        cache.insert(Account { id: 1, cash: 100 });
        let s = seq(0);
        let mut outbox = Vec::new();

        {
            let mut ctx = Context::new(&cache, &clock, &s, &mut outbox);
            let cash = ctx.get::<Account>(&1).unwrap().cash;
            assert_eq!(cash, 100);
            ctx.send(Order { id: cash as u64 });
        }

        assert_eq!(outbox.len(), 1);
        let payload: &dyn Any = &*outbox[0].1;
        assert_eq!(payload.downcast_ref::<Order>().unwrap().id, 100);
    }

    /// Invariant: sending a message does not mutate the cache
    #[test]
    fn send_does_not_mutate_cache() {
        let clock = SimClock::new(Timestamp::new(0));
        let mut cache = Cache::new();
        cache.insert(Account { id: 1, cash: 100 });
        let s = seq(0);
        let mut outbox = Vec::new();

        let mut ctx = Context::new(&cache, &clock, &s, &mut outbox);
        let cash_before = ctx.get::<Account>(&1).unwrap().cash;
        ctx.send(Order { id: 999 });
        let cash_after = ctx.get::<Account>(&1).unwrap().cash;
        assert_eq!(cash_before, cash_after);
    }

    /// Invariant: send_at does not change the value returned by now()
    #[test]
    fn send_at_does_not_affect_now() {
        let clock = SimClock::new(Timestamp::new(500));
        let cache = Cache::new();
        let s = seq(0);
        let mut outbox = Vec::new();

        let mut ctx = Context::new(&cache, &clock, &s, &mut outbox);
        let before = ctx.now();
        ctx.send_at(Timestamp::new(5000), Order { id: 1 });
        let after = ctx.now();
        assert_eq!(before, after);
        assert_eq!(after, Timestamp::new(500));
    }

    /// Invariant: the outbox survives context drop; entries pushed via send()
    /// and send_at() remain accessible
    #[test]
    fn outbox_survives_context_drop() {
        let clock = SimClock::new(Timestamp::new(0));
        let cache = Cache::new();
        let s = seq(0);
        let mut outbox = Vec::new();

        {
            let mut ctx = Context::new(&cache, &clock, &s, &mut outbox);
            ctx.send(Order { id: 1 });
            ctx.send_at(Timestamp::new(100), Fill { id: 2 });
        }

        assert_eq!(outbox.len(), 2);
        assert_eq!(outbox[0].0, Timestamp::new(0));
        assert_eq!(outbox[1].0, Timestamp::new(100));
    }

    /// Invariant: seq() is copy-by-value, not a reference that tracks external
    /// mutation
    #[test]
    fn seq_returns_snapshot_not_live_reference() {
        let clock = SimClock::new(Timestamp::new(0));
        let cache = Cache::new();
        let s = seq(5);
        let mut outbox = Vec::new();

        let ctx = Context::new(&cache, &clock, &s, &mut outbox);
        let snapshot = ctx.seq();
        assert_eq!(snapshot, s);
    }

    /// Invariant: get() returns None for a key whose value has been removed
    #[test]
    fn get_after_remove_returns_none() {
        let clock = SimClock::new(Timestamp::new(0));
        let mut cache = Cache::new();
        cache.insert(Account { id: 1, cash: 100 });
        cache.remove::<Account>(&1);
        let s = seq(0);
        let mut outbox = Vec::new();

        let ctx = Context::new(&cache, &clock, &s, &mut outbox);
        assert_eq!(ctx.get::<Account>(&1), None);
    }
}
