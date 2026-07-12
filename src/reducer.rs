use std::{
    any::{Any, TypeId},
    collections::HashMap,
};

use crate::{context::reducer::ReducerCtx, message::Message, output::MessageType};

type ErasedReducer = Box<dyn Fn(&mut ReducerCtx<'_>, &dyn Message) + Send>;

fn erase_reducer<M: Message>(
    f: impl Fn(&mut ReducerCtx<'_>, &M) + Send + 'static,
) -> ErasedReducer {
    Box::new(move |ctx, msg| {
        let concrete = (msg as &dyn Any)
            .downcast_ref::<M>()
            .expect("ReducerRegistry invariant: message type mismatch");
        f(ctx, concrete);
    })
}

pub(crate) struct ReducerEntry {
    consumed_type_name: &'static str,
    invoke: ErasedReducer,
}

pub(crate) struct ReducerRegistry {
    by_type: HashMap<TypeId, Vec<ReducerEntry>>,
}

impl ReducerRegistry {
    pub(crate) fn new() -> Self {
        Self {
            by_type: HashMap::new(),
        }
    }

    pub(crate) fn register<M: Message>(
        &mut self,
        f: impl Fn(&mut ReducerCtx<'_>, &M) + Send + 'static,
    ) {
        let erased = erase_reducer(f);
        self.by_type
            .entry(TypeId::of::<M>())
            .or_default()
            .push(ReducerEntry {
                consumed_type_name: std::any::type_name::<M>(),
                invoke: erased,
            });
    }

    pub(crate) fn dispatch(&self, ctx: &mut ReducerCtx<'_>, msg: &dyn Message) {
        let type_id = msg.type_id();
        if let Some(entries) = self.by_type.get(&type_id) {
            for entry in entries {
                (entry.invoke)(ctx, msg);
            }
        }
    }

    /// One [`MessageType`] per reducer registration (registration order within
    /// each message type). Used by graph building without exposing entries.
    pub(crate) fn consumer_message_types(&self) -> Vec<MessageType> {
        let mut out = Vec::new();
        for (type_id, entries) in &self.by_type {
            for entry in entries {
                out.push(MessageType {
                    id: *type_id,
                    name: entry.consumed_type_name,
                });
            }
        }
        out
    }
}

#[cfg(test)]
impl ReducerRegistry {
    pub(crate) fn consumed_types(&self) -> Vec<TypeId> {
        self.by_type.keys().copied().collect()
    }

    pub(crate) fn len(&self) -> usize {
        self.by_type.values().map(|v| v.len()).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        cache::{Cache, State},
        time::Timestamp,
    };
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    };

    // ========================================================================
    // Test types
    // ========================================================================

    #[derive(Debug, Clone, PartialEq)]
    struct TestMsg(u64);

    impl Message for TestMsg {}

    #[derive(Debug, Clone, PartialEq)]
    struct OtherMsg(u64);

    impl Message for OtherMsg {}

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

    // ========================================================================
    // Matching reducer
    // ========================================================================

    /// Invariant: a reducer registered for a message type fires when that
    /// type is dispatched
    #[test]
    fn test_matching_reducer_fires() {
        let mut reg = ReducerRegistry::new();
        let called = Arc::new(AtomicBool::new(false));
        let called2 = called.clone();

        reg.register(move |_ctx: &mut ReducerCtx<'_>, _msg: &TestMsg| {
            called2.store(true, Ordering::SeqCst);
        });

        let mut cache = Cache::new();
        let ts = Timestamp::new(0);
        let mut ctx = ReducerCtx::new(ts, &mut cache);
        let msg = TestMsg(42);
        let msg_ref: &dyn Message = &msg;

        reg.dispatch(&mut ctx, msg_ref);

        assert!(called.load(Ordering::SeqCst));
    }

    // ========================================================================
    // Wrong message type
    // ========================================================================

    /// Invariant: a reducer does not fire for a message type it was not
    /// registered for
    #[test]
    fn test_wrong_message_type_does_not_fire() {
        let mut reg = ReducerRegistry::new();
        let called = Arc::new(AtomicBool::new(false));
        let called2 = called.clone();

        reg.register(move |_ctx: &mut ReducerCtx<'_>, _msg: &TestMsg| {
            called2.store(true, Ordering::SeqCst);
        });

        let mut cache = Cache::new();
        let ts = Timestamp::new(0);
        let mut ctx = ReducerCtx::new(ts, &mut cache);
        let msg = OtherMsg(99);
        let msg_ref: &dyn Message = &msg;

        reg.dispatch(&mut ctx, msg_ref);

        assert!(!called.load(Ordering::SeqCst));
    }

    // ========================================================================
    // Registration order
    // ========================================================================

    /// Invariant: multiple reducers for the same message type run in
    /// registration order
    #[test]
    fn test_multiple_reducers_run_in_registration_order() {
        let mut reg = ReducerRegistry::new();
        let order = Arc::new(Mutex::new(Vec::new()));

        {
            let order = order.clone();
            reg.register(move |_ctx: &mut ReducerCtx<'_>, _msg: &TestMsg| {
                order.lock().unwrap().push(1);
            });
        }
        {
            let order = order.clone();
            reg.register(move |_ctx: &mut ReducerCtx<'_>, _msg: &TestMsg| {
                order.lock().unwrap().push(2);
            });
        }
        {
            let order = order.clone();
            reg.register(move |_ctx: &mut ReducerCtx<'_>, _msg: &TestMsg| {
                order.lock().unwrap().push(3);
            });
        }

        let mut cache = Cache::new();
        let ts = Timestamp::new(0);
        let mut ctx = ReducerCtx::new(ts, &mut cache);
        let msg = TestMsg(7);
        let msg_ref: &dyn Message = &msg;

        reg.dispatch(&mut ctx, msg_ref);

        let order = order.lock().unwrap();
        assert_eq!(*order, vec![1, 2, 3]);
    }

    // ========================================================================
    // Prior reducer mutation visibility
    // ========================================================================

    /// Invariant: a reducer sees cache mutations performed by an
    /// earlier reducer for the same message
    #[test]
    fn test_prior_reducer_mutations_visible_to_next() {
        let mut reg = ReducerRegistry::new();

        reg.register(|ctx: &mut ReducerCtx<'_>, _msg: &TestMsg| {
            ctx.insert(KeyedNum { key: 1, value: 10 });
        });
        reg.register(|ctx: &mut ReducerCtx<'_>, _msg: &TestMsg| {
            let v = ctx.get::<KeyedNum>(&1).unwrap();
            assert_eq!(v.value, 10);
            ctx.get_mut::<KeyedNum>(&1).unwrap().value = 99;
        });

        let mut cache = Cache::new();
        let ts = Timestamp::new(0);
        let msg = TestMsg(1);
        let msg_ref: &dyn Message = &msg;

        {
            let mut ctx = ReducerCtx::new(ts, &mut cache);
            reg.dispatch(&mut ctx, msg_ref);
        }

        assert_eq!(cache.get::<KeyedNum>(&1).unwrap().value, 99);
    }

    // ========================================================================
    // Insert keyed state
    // ========================================================================

    /// Invariant: a reducer can insert keyed state that persists after
    /// dispatch
    #[test]
    fn test_reducer_can_insert_keyed_state() {
        let mut reg = ReducerRegistry::new();

        reg.register(|ctx: &mut ReducerCtx<'_>, _msg: &TestMsg| {
            ctx.insert(KeyedNum { key: 5, value: 42 });
        });

        let mut cache = Cache::new();
        let ts = Timestamp::new(0);
        let msg = TestMsg(1);
        let msg_ref: &dyn Message = &msg;

        {
            let mut ctx = ReducerCtx::new(ts, &mut cache);
            reg.dispatch(&mut ctx, msg_ref);
        }

        let stored = cache.get::<KeyedNum>(&5);
        assert!(stored.is_some());
        assert_eq!(stored.unwrap().value, 42);
    }

    // ========================================================================
    // Mutate singleton state
    // ========================================================================

    /// Invariant: a reducer can mutate singleton state and the mutation
    /// persists
    #[test]
    fn test_reducer_can_mutate_singleton_state() {
        let mut reg = ReducerRegistry::new();

        reg.register(|ctx: &mut ReducerCtx<'_>, _msg: &TestMsg| {
            let s = ctx.get_singleton_mut::<SingletonNum>().unwrap();
            s.value += 1;
        });

        let mut cache = Cache::new();
        cache.insert(SingletonNum { value: 0 });

        let ts = Timestamp::new(0);
        let msg = TestMsg(1);
        let msg_ref: &dyn Message = &msg;

        {
            let mut ctx = ReducerCtx::new(ts, &mut cache);
            reg.dispatch(&mut ctx, msg_ref);
        }

        assert_eq!(cache.get_singleton::<SingletonNum>().unwrap().value, 1);
    }

    // ========================================================================
    // Dispatch time
    // ========================================================================

    /// Invariant: the reducer receives the dispatch time supplied to the
    /// context
    #[test]
    fn test_reducer_receives_dispatch_time() {
        let mut reg = ReducerRegistry::new();
        let captured = Arc::new(Mutex::new(Timestamp::new(0)));
        let captured2 = captured.clone();

        reg.register(move |ctx: &mut ReducerCtx<'_>, _msg: &TestMsg| {
            *captured2.lock().unwrap() = ctx.dispatch_time();
        });

        let mut cache = Cache::new();
        let ts = Timestamp::new(9_600);
        let msg = TestMsg(1);
        let msg_ref: &dyn Message = &msg;

        {
            let mut ctx = ReducerCtx::new(ts, &mut cache);
            reg.dispatch(&mut ctx, msg_ref);
        }

        assert_eq!(*captured.lock().unwrap(), ts);
    }

    // ========================================================================
    // Consumed types metadata
    // ========================================================================

    /// Invariant: the registry reports TypeIds for which at least one
    /// reducer is registered
    #[test]
    fn test_registry_reports_consumed_types() {
        let mut reg = ReducerRegistry::new();

        reg.register(|_ctx: &mut ReducerCtx<'_>, _msg: &TestMsg| {});
        reg.register(|_ctx: &mut ReducerCtx<'_>, _msg: &OtherMsg| {});

        let types = reg.consumed_types();
        assert!(types.contains(&TypeId::of::<TestMsg>()));
        assert!(types.contains(&TypeId::of::<OtherMsg>()));
        assert_eq!(types.len(), 2);
    }

    // ========================================================================
    // Empty registry
    // ========================================================================

    /// Invariant: dispatch against an empty registry performs no callback
    /// and leaves the cache unmodified
    #[test]
    fn test_empty_registry_performs_no_callback() {
        let reg = ReducerRegistry::new();

        let mut cache = Cache::new();
        cache.insert(KeyedNum { key: 1, value: 99 });

        let ts = Timestamp::new(0);
        let msg = TestMsg(1);
        let msg_ref: &dyn Message = &msg;

        {
            let mut ctx = ReducerCtx::new(ts, &mut cache);
            reg.dispatch(&mut ctx, msg_ref);
        }

        assert_eq!(cache.get::<KeyedNum>(&1).unwrap().value, 99);
        assert_eq!(cache.len(), 1);
    }

    // ========================================================================
    // Len
    // ========================================================================

    /// Invariant: len returns the total number of registered reducers
    /// across all message types
    #[test]
    fn test_len_counts_all_registered_reducers() {
        let mut reg = ReducerRegistry::new();
        assert_eq!(reg.len(), 0);

        reg.register(|_ctx: &mut ReducerCtx<'_>, _msg: &TestMsg| {});
        assert_eq!(reg.len(), 1);

        reg.register(|_ctx: &mut ReducerCtx<'_>, _msg: &TestMsg| {});
        assert_eq!(reg.len(), 2);

        reg.register(|_ctx: &mut ReducerCtx<'_>, _msg: &OtherMsg| {});
        assert_eq!(reg.len(), 3);
    }

    // ========================================================================
    // Downcast mismatch
    // ========================================================================

    /// Invariant: a downcast mismatch in the erased reducer wrapper
    /// panics rather than silently producing no effect
    #[test]
    #[should_panic(expected = "ReducerRegistry invariant: message type mismatch")]
    fn test_erased_reducer_downcast_mismatch_panics() {
        let mut cache = Cache::new();
        let mut ctx = ReducerCtx::new(Timestamp::new(0), &mut cache);

        let erased = erase_reducer(|_ctx: &mut ReducerCtx<'_>, _msg: &TestMsg| {});
        let msg = OtherMsg(1);
        erased(&mut ctx, &msg);
    }
}
