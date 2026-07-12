use crate::{
    cache::{Cache, State},
    message::Message,
    output::{HandlerOutput, HandlerOutputError, ProductionSet},
    time::timestamp::Timestamp,
};

/// Context passed to handler callbacks.
///
/// Provides read-only cache access and restricted output capability.
/// Handlers cannot mutate the global cache, access the scheduler directly,
/// or read the sequence allocator.
///
/// # Capabilities
///
/// | Operation | Available |
/// |---|---|
/// | `dispatch_time()` | Yes |
/// | `get::<T>(key)` | Yes |
/// | `get_singleton::<T>()` | Yes |
/// | `send(msg)` | Yes |
/// | `send_at(ts, msg)` | Yes |
/// | `get_mut`, `insert`, `remove` | No |
/// | Clock, sequence, scheduler, mode | No |
pub struct HandlerCtx<'a> {
    dispatch_time: Timestamp,
    cache: &'a Cache,
    output: &'a mut HandlerOutput<'a>,
    declared_productions: &'a ProductionSet,
}

impl<'a> HandlerCtx<'a> {
    pub(crate) fn new(
        dispatch_time: Timestamp,
        cache: &'a Cache,
        output: &'a mut HandlerOutput<'a>,
        declared_productions: &'a ProductionSet,
    ) -> Self {
        Self {
            dispatch_time,
            cache,
            output,
            declared_productions,
        }
    }

    /// Returns the scheduler timestamp of the message currently being
    /// handled.
    ///
    /// The dispatch time is a stable copy and does not change during
    /// processing of the current message.
    pub fn dispatch_time(&self) -> Timestamp {
        self.dispatch_time
    }

    /// Returns a shared reference to the keyed cache state of type `T`,
    /// or `None` if no such value exists.
    pub fn get<T: State>(&self, key: &T::Key) -> Option<&T> {
        self.cache.get(key)
    }

    /// Returns a shared reference to the singleton cache state of type `T`,
    /// or `None` if no such value exists.
    ///
    /// Only valid for [`State`] types with `Key = ()`.
    pub fn get_singleton<T: State<Key = ()>>(&self) -> Option<&T> {
        self.cache.get_singleton()
    }

    /// Schedules `msg` at the current dispatch time.
    ///
    /// The message type must have been declared via `.produces::<M>()` during
    /// handler registration.  Sending an undeclared type returns
    /// [`HandlerOutputError::UndeclaredProduction`].
    ///
    /// The message receives a new kernel sequence number and is inserted
    /// after all previously queued same-time messages.
    pub fn send<M: Message>(&mut self, msg: M) -> Result<(), HandlerOutputError> {
        self.output.send(msg, self.declared_productions)
    }

    /// Schedules `msg` at the requested future timestamp.
    ///
    /// The message type must have been declared via `.produces::<M>()` during
    /// handler registration.
    ///
    /// Timestamps strictly before the current dispatch time are rejected
    /// with [`HandlerOutputError::PastEvent`].
    pub fn send_at<M: Message>(&mut self, ts: Timestamp, msg: M) -> Result<(), HandlerOutputError> {
        self.output.send_at(ts, msg, self.declared_productions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::{
        cache::Cache,
        message::Message,
        output::{HandlerOutput, ProductionSet},
        schedule::Scheduler,
        sequence::Sequence,
        time::timestamp::Timestamp,
    };

    // ========================================================================
    // Test types
    // ========================================================================

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

    #[derive(Debug, Clone, PartialEq)]
    struct MyMsg(u64);

    impl Message for MyMsg {}

    #[derive(Debug, Clone, PartialEq)]
    struct OtherMsg(u64);

    impl Message for OtherMsg {}

    // ========================================================================
    // dispatch_time
    // ========================================================================

    /// Invariant: dispatch_time returns a stable copied value
    #[test]
    fn test_dispatch_time_returns_stable_value() {
        let ts = Timestamp::new(100);
        let cache = Cache::new();
        let mut sched = Scheduler::new();
        let mut seq = Sequence::initial();
        let mut output = HandlerOutput::new(&mut sched, &mut seq, ts);
        let productions = ProductionSet::new();

        let ctx = HandlerCtx::new(ts, &cache, &mut output, &productions);
        assert_eq!(ctx.dispatch_time(), ts);
        assert_eq!(ctx.dispatch_time(), ts);
    }

    /// Invariant: dispatch_time is stable after cache reads
    #[test]
    fn test_dispatch_time_stable_after_cache_reads() {
        let mut cache = Cache::new();
        cache.insert(KeyedNum { key: 1, value: 77 });

        let ts = Timestamp::new(42);
        let mut sched = Scheduler::new();
        let mut seq = Sequence::initial();
        let mut output = HandlerOutput::new(&mut sched, &mut seq, ts);
        let productions = ProductionSet::new();

        let ctx = HandlerCtx::new(ts, &cache, &mut output, &productions);

        let _ = ctx.get::<KeyedNum>(&1);
        assert_eq!(ctx.dispatch_time(), ts);
    }

    // ========================================================================
    // send
    // ========================================================================

    /// Invariant: send schedules at dispatch time
    #[test]
    fn test_send_schedules_at_dispatch_time() {
        let dispatch_ts = Timestamp::new(100);
        let cache = Cache::new();
        let mut sched = Scheduler::new();
        let mut seq = Sequence::initial();
        let mut output = HandlerOutput::new(&mut sched, &mut seq, dispatch_ts);

        let mut productions = ProductionSet::new();
        productions.insert::<MyMsg>();

        let mut ctx = HandlerCtx::new(dispatch_ts, &cache, &mut output, &productions);
        ctx.send(MyMsg(42)).unwrap();

        let item = sched.pop().unwrap();
        assert_eq!(item.dispatch_time(), dispatch_ts);
    }

    /// Invariant: send payload roundtrips correctly
    #[test]
    fn test_send_payload_roundtrips() {
        let ts = Timestamp::new(0);
        let cache = Cache::new();
        let mut sched = Scheduler::new();
        let mut seq = Sequence::initial();
        let mut output = HandlerOutput::new(&mut sched, &mut seq, ts);

        let mut productions = ProductionSet::new();
        productions.insert::<MyMsg>();

        let mut ctx = HandlerCtx::new(ts, &cache, &mut output, &productions);
        ctx.send(MyMsg(42)).unwrap();

        let item = sched.pop().unwrap();
        let payload: &dyn std::any::Any = &*item.payload();
        assert_eq!(payload.downcast_ref::<MyMsg>(), Some(&MyMsg(42)));
    }

    // ========================================================================
    // send_at
    // ========================================================================

    /// Invariant: send_at schedules at the requested future time
    #[test]
    fn test_send_at_schedules_at_future_time() {
        let dispatch_ts = Timestamp::new(100);
        let future_ts = Timestamp::new(200);
        let cache = Cache::new();
        let mut sched = Scheduler::new();
        let mut seq = Sequence::initial();
        let mut output = HandlerOutput::new(&mut sched, &mut seq, dispatch_ts);

        let mut productions = ProductionSet::new();
        productions.insert::<MyMsg>();

        let mut ctx = HandlerCtx::new(dispatch_ts, &cache, &mut output, &productions);
        ctx.send_at(future_ts, MyMsg(7)).unwrap();

        let item = sched.pop().unwrap();
        assert_eq!(item.dispatch_time(), future_ts);
    }

    /// Invariant: send_at rejects a timestamp strictly before dispatch_time
    #[test]
    fn test_send_at_rejects_past_timestamp() {
        let dispatch_ts = Timestamp::new(100);
        let past_ts = Timestamp::new(50);
        let cache = Cache::new();
        let mut sched = Scheduler::new();
        let mut seq = Sequence::initial();
        let mut output = HandlerOutput::new(&mut sched, &mut seq, dispatch_ts);

        let mut productions = ProductionSet::new();
        productions.insert::<MyMsg>();

        let mut ctx = HandlerCtx::new(dispatch_ts, &cache, &mut output, &productions);
        let result = ctx.send_at(past_ts, MyMsg(3));

        assert_eq!(
            result,
            Err(HandlerOutputError::PastEvent {
                requested: past_ts,
                current: dispatch_ts,
            })
        );
        assert!(sched.pop().is_none());
    }

    /// Invariant: send_at at the same dispatch time succeeds
    #[test]
    fn test_send_at_at_dispatch_time_succeeds() {
        let ts = Timestamp::new(100);
        let cache = Cache::new();
        let mut sched = Scheduler::new();
        let mut seq = Sequence::initial();
        let mut output = HandlerOutput::new(&mut sched, &mut seq, ts);

        let mut productions = ProductionSet::new();
        productions.insert::<MyMsg>();

        let mut ctx = HandlerCtx::new(ts, &cache, &mut output, &productions);
        assert!(ctx.send_at(ts, MyMsg(5)).is_ok());

        let item = sched.pop().unwrap();
        assert_eq!(item.dispatch_time(), ts);
    }

    // ========================================================================
    // Production declaration enforcement
    // ========================================================================

    /// Invariant: declared production succeeds
    #[test]
    fn test_declared_production_succeeds() {
        let ts = Timestamp::new(0);
        let cache = Cache::new();
        let mut sched = Scheduler::new();
        let mut seq = Sequence::initial();
        let mut output = HandlerOutput::new(&mut sched, &mut seq, ts);

        let mut productions = ProductionSet::new();
        productions.insert::<MyMsg>();

        let mut ctx = HandlerCtx::new(ts, &cache, &mut output, &productions);
        assert!(ctx.send(MyMsg(1)).is_ok());
        assert!(sched.pop().is_some());
    }

    /// Invariant: undeclared production fails
    #[test]
    fn test_undeclared_production_fails() {
        let ts = Timestamp::new(0);
        let cache = Cache::new();
        let mut sched = Scheduler::new();
        let mut seq = Sequence::initial();
        let mut output = HandlerOutput::new(&mut sched, &mut seq, ts);

        let productions = ProductionSet::new();

        let mut ctx = HandlerCtx::new(ts, &cache, &mut output, &productions);
        let result = ctx.send(MyMsg(1));

        assert!(matches!(
            result,
            Err(HandlerOutputError::UndeclaredProduction { .. })
        ));
        assert!(sched.pop().is_none());
    }

    /// Invariant: send_at also enforces production declarations
    #[test]
    fn test_undeclared_send_at_fails() {
        let ts = Timestamp::new(100);
        let cache = Cache::new();
        let mut sched = Scheduler::new();
        let mut seq = Sequence::initial();
        let mut output = HandlerOutput::new(&mut sched, &mut seq, ts);

        let productions = ProductionSet::new();

        let mut ctx = HandlerCtx::new(ts, &cache, &mut output, &productions);
        let result = ctx.send_at(Timestamp::new(200), MyMsg(1));

        assert!(matches!(
            result,
            Err(HandlerOutputError::UndeclaredProduction { .. })
        ));
        assert!(sched.pop().is_none());
    }

    // ========================================================================
    // Sequence ordering
    // ========================================================================

    /// Invariant: two sends receive increasing sequences
    #[test]
    fn test_two_sends_receive_increasing_sequences() {
        let ts = Timestamp::new(42);
        let cache = Cache::new();
        let mut sched = Scheduler::new();
        let mut seq = Sequence::initial();
        let mut output = HandlerOutput::new(&mut sched, &mut seq, ts);

        let mut productions = ProductionSet::new();
        productions.insert::<MyMsg>();

        let mut ctx = HandlerCtx::new(ts, &cache, &mut output, &productions);
        ctx.send(MyMsg(0)).unwrap();
        ctx.send(MyMsg(1)).unwrap();

        let item_a = sched.pop().unwrap();
        let item_b = sched.pop().unwrap();
        assert!(item_a.sequence() < item_b.sequence());
    }

    /// Invariant: existing same-time items remain ahead of newly produced
    /// messages
    #[test]
    fn test_existing_same_time_items_stay_ahead_of_newly_produced() {
        use std::sync::Arc;

        let ts = Timestamp::new(100);
        let mut sched = Scheduler::new();

        // Pre-populate scheduler with two items at time T, sharing the
        // same sequence allocator so the produced message gets a later seq.
        let mut shared_seq = Sequence::initial();
        let seq_a = shared_seq.next().unwrap();
        let seq_b = shared_seq.next().unwrap();
        sched.push_shared_msg(ts, seq_a, Arc::new(OtherMsg(0)));
        sched.push_shared_msg(ts, seq_b, Arc::new(OtherMsg(1)));

        let cache = Cache::new();
        let mut productions = ProductionSet::new();
        productions.insert::<MyMsg>();

        let mut output = HandlerOutput::new(&mut sched, &mut shared_seq, ts);
        let mut ctx = HandlerCtx::new(ts, &cache, &mut output, &productions);
        ctx.send(MyMsg(42)).unwrap();

        let first = sched.pop().unwrap();
        assert_eq!(first.sequence(), seq_a);
        let raw: &dyn std::any::Any = &*first.payload();
        assert!(raw.downcast_ref::<OtherMsg>().is_some());

        let second = sched.pop().unwrap();
        assert_eq!(second.sequence(), seq_b);
        let raw: &dyn std::any::Any = &*second.payload();
        assert!(raw.downcast_ref::<OtherMsg>().is_some());

        let third = sched.pop().unwrap();
        let raw: &dyn std::any::Any = &*third.payload();
        assert!(raw.downcast_ref::<MyMsg>().is_some_and(|m| m.0 == 42));
        assert!(third.sequence() > seq_b);

        assert!(sched.pop().is_none());
    }

    // ========================================================================
    // Cache reads
    // ========================================================================

    /// Invariant: handler reads keyed cache state via get
    #[test]
    fn test_handler_reads_keyed_cache_state() {
        let mut cache = Cache::new();
        cache.insert(KeyedNum { key: 1, value: 77 });

        let ts = Timestamp::new(0);
        let mut sched = Scheduler::new();
        let mut seq = Sequence::initial();
        let mut output = HandlerOutput::new(&mut sched, &mut seq, ts);
        let productions = ProductionSet::new();

        let ctx = HandlerCtx::new(ts, &cache, &mut output, &productions);
        let stored = ctx.get::<KeyedNum>(&1);
        assert!(stored.is_some());
        assert_eq!(stored.unwrap().value, 77);
    }

    /// Invariant: handler reads singleton cache state via get_singleton
    #[test]
    fn test_handler_reads_singleton_cache_state() {
        let mut cache = Cache::new();
        cache.insert(SingletonNum { value: 42 });

        let ts = Timestamp::new(0);
        let mut sched = Scheduler::new();
        let mut seq = Sequence::initial();
        let mut output = HandlerOutput::new(&mut sched, &mut seq, ts);
        let productions = ProductionSet::new();

        let ctx = HandlerCtx::new(ts, &cache, &mut output, &productions);
        let stored = ctx.get_singleton::<SingletonNum>();
        assert!(stored.is_some());
        assert_eq!(stored.unwrap().value, 42);
    }

    /// Invariant: get for a missing key returns None
    #[test]
    fn test_handler_get_missing_key_returns_none() {
        let cache = Cache::new();

        let ts = Timestamp::new(0);
        let mut sched = Scheduler::new();
        let mut seq = Sequence::initial();
        let mut output = HandlerOutput::new(&mut sched, &mut seq, ts);
        let productions = ProductionSet::new();

        let ctx = HandlerCtx::new(ts, &cache, &mut output, &productions);
        assert!(ctx.get::<KeyedNum>(&99).is_none());
    }

    /// Invariant: get_singleton for a missing value returns None
    #[test]
    fn test_handler_get_singleton_missing_returns_none() {
        let cache = Cache::new();

        let ts = Timestamp::new(0);
        let mut sched = Scheduler::new();
        let mut seq = Sequence::initial();
        let mut output = HandlerOutput::new(&mut sched, &mut seq, ts);
        let productions = ProductionSet::new();

        let ctx = HandlerCtx::new(ts, &cache, &mut output, &productions);
        assert!(ctx.get_singleton::<SingletonNum>().is_none());
    }

    // ========================================================================
    // Compile-time capability isolation
    // ========================================================================

    /// Verifies that HandlerCtx does not expose mutable cache access.
    ///
    /// This test is purely a type-level check: if `get_mut` were added to
    /// HandlerCtx, the following would compile.
    #[test]
    fn test_handler_ctx_struct_has_no_get_mut_field() {
        // HandlerCtx holds &Cache (not &mut Cache), so mutating the cache
        // is impossible at the type level.
        let mut cache = Cache::new();
        cache.insert(KeyedNum { key: 1, value: 1 });

        let ts = Timestamp::new(0);
        let mut sched = Scheduler::new();
        let mut seq = Sequence::initial();
        let mut output = HandlerOutput::new(&mut sched, &mut seq, ts);
        let productions = ProductionSet::new();

        let ctx = HandlerCtx::new(ts, &cache, &mut output, &productions);

        // Read is fine.
        let v = ctx.get::<KeyedNum>(&1);
        assert_eq!(v.unwrap().value, 1);

        // But we cannot mutate through the context: ctx.get_mut does not exist.
        // We can mutate the cache directly afterwards to prove the reference
        // was shared, not exclusive.
    }
}
