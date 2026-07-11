use std::{
    any::{Any, TypeId},
    collections::HashMap,
};

use crate::{cache::Cache, message::Message};

type ErasedReducer = Box<dyn Fn(&mut Cache, &dyn Message)>;

/// Registry of cache-mutation functions keyed by message type.
///
/// Reducers run before handlers for the same message and have no access
/// to the clock, sequence number, or message-production — pure mutation
/// via `(&mut Cache, &M)`. Multiple reducers for the same message type
/// fire in registration order.
pub(crate) struct ReducerRegistry {
    reducers: HashMap<TypeId, Vec<ErasedReducer>>,
}

impl ReducerRegistry {
    pub(crate) fn new() -> Self {
        Self {
            reducers: HashMap::new(),
        }
    }

    /// Register a reducer for message type `M`.
    ///
    /// The closure receives `(&mut Cache, &M)` — read or write any entry
    /// in the cache, but no `Context` access (no clock, no `send`).
    pub(crate) fn register<M: Message>(&mut self, reduce: impl Fn(&mut Cache, &M) + 'static) {
        let erased: ErasedReducer = Box::new(move |cache, msg| {
            let typed: &M = (msg as &dyn Any)
                .downcast_ref()
                .expect("Reducer invoked with wrong message type");
            reduce(cache, typed);
        });
        self.reducers
            .entry(TypeId::of::<M>())
            .or_default()
            .push(erased);
    }

    /// Run every reducer registered for the given message's type,
    /// in registration order. No-op if no reducers are registered.
    pub(crate) fn reduce(&self, cache: &mut Cache, msg: &dyn Message) {
        let type_id = (msg as &dyn Any).type_id();
        if let Some(reducers) = self.reducers.get(&type_id) {
            for reduce in reducers {
                reduce(cache, msg);
            }
        }
    }

    /// Whether any reducer is registered for message type `M`.
    /// Used by graph validation.
    pub(crate) fn has_reducer_for<M: Message>(&self) -> bool {
        self.reducers.contains_key(&TypeId::of::<M>())
    }

    pub(crate) fn len(&self) -> usize {
        self.reducers.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.reducers.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // Test types
    // ========================================================================

    #[derive(Clone, Debug, PartialEq)]
    struct Portfolio {
        account: u32,
        cash: i64,
    }
    impl crate::cache::State for Portfolio {
        type Key = u32;
        fn key(&self) -> u32 {
            self.account
        }
    }

    #[derive(Clone, Debug, PartialEq)]
    struct Position {
        instrument: u32,
        quantity: i64,
    }
    impl crate::cache::State for Position {
        type Key = u32;
        fn key(&self) -> u32 {
            self.instrument
        }
    }

    #[derive(Clone, Debug, PartialEq)]
    struct TradeCounter {
        count: u64,
    }
    impl crate::cache::State for TradeCounter {
        type Key = ();
        fn key(&self) {}
    }

    #[derive(Clone, Debug, PartialEq)]
    struct Fill {
        account: u32,
        instrument: u32,
        quantity: i64,
        price: i64,
    }
    impl Message for Fill {}

    #[derive(Clone, Debug, PartialEq)]
    struct NewOrder {
        id: u64,
        instrument: u32,
    }
    impl Message for NewOrder {}

    #[derive(Clone, Debug, PartialEq)]
    struct Cancel {
        id: u64,
    }
    impl Message for Cancel {}

    // ========================================================================
    // Basic mutation
    // ========================================================================

    /// Invariant: a registered reducer mutates the cache when its
    /// message type is dispatched.
    #[test]
    fn reducer_mutates_cache_on_message() {
        let mut reg = ReducerRegistry::new();
        let mut cache = Cache::new();

        cache.insert(Portfolio {
            account: 1,
            cash: 1000,
        });

        reg.register::<Fill>(|cache, fill| {
            if let Some(p) = cache.get_keyed_mut::<Portfolio>(&fill.account) {
                p.cash -= fill.quantity * fill.price;
            }
        });

        let fill = Fill {
            account: 1,
            instrument: 100,
            quantity: 10,
            price: 50,
        };

        reg.reduce(&mut cache, &fill);

        let portfolio = cache.get_keyed::<Portfolio>(&1).unwrap();
        assert_eq!(portfolio.cash, 500); // 1000 - (10 * 50)
    }

    /// Invariant: a reducer can insert a new value into the cache.
    #[test]
    fn reducer_can_insert_new_cache_entry() {
        let mut reg = ReducerRegistry::new();
        let mut cache = Cache::new();

        reg.register::<Fill>(|cache, fill| {
            cache.insert(Position {
                instrument: fill.instrument,
                quantity: fill.quantity,
            });
        });

        let fill = Fill {
            account: 1,
            instrument: 42,
            quantity: 100,
            price: 10,
        };

        reg.reduce(&mut cache, &fill);

        let pos = cache.get_keyed::<Position>(&42).unwrap();
        assert_eq!(pos.quantity, 100);
    }

    // ========================================================================
    // Ordering
    // ========================================================================

    /// Invariant: multiple reducers for the same message type run
    /// in registration order.
    #[test]
    fn multiple_reducers_run_in_registration_order() {
        let mut reg = ReducerRegistry::new();
        let mut cache = Cache::new();

        cache.insert(TradeCounter { count: 0 });

        // First reducer — increments counter to 1
        reg.register::<Fill>(|cache, _fill| {
            if let Some(c) = cache.get_mut::<TradeCounter>() {
                assert_eq!(c.count, 0, "first reducer should see initial value");
                c.count += 1;
            }
        });

        // Second reducer — sees counter = 1, increments to 2
        reg.register::<Fill>(|cache, _fill| {
            if let Some(c) = cache.get_mut::<TradeCounter>() {
                assert_eq!(
                    c.count, 1,
                    "second reducer should see first reducer's change"
                );
                c.count += 1;
            }
        });

        // Third reducer — sees counter = 2
        reg.register::<Fill>(|cache, _fill| {
            if let Some(c) = cache.get::<TradeCounter>() {
                assert_eq!(c.count, 2);
            }
        });

        let fill = Fill {
            account: 1,
            instrument: 1,
            quantity: 1,
            price: 1,
        };

        reg.reduce(&mut cache, &fill);

        assert_eq!(cache.get::<TradeCounter>().unwrap().count, 2);
    }

    /// Invariant: registration order is honored independently for
    /// different message types.
    #[test]
    fn reducers_dispatch_by_message_type() {
        let mut reg = ReducerRegistry::new();
        let mut cache = Cache::new();

        reg.register::<NewOrder>(|cache, order| {
            cache.insert(Position {
                instrument: order.instrument,
                quantity: 0,
            });
        });

        reg.register::<Fill>(|cache, fill| {
            if let Some(p) = cache.get_keyed_mut::<Position>(&fill.instrument) {
                p.quantity += fill.quantity;
            }
        });

        let order = NewOrder {
            id: 1,
            instrument: 7,
        };
        reg.reduce(&mut cache, &order);
        assert!(cache.get_keyed::<Position>(&7).is_some());

        let fill = Fill {
            account: 1,
            instrument: 7,
            quantity: 50,
            price: 10,
        };
        reg.reduce(&mut cache, &fill);
        assert_eq!(cache.get_keyed::<Position>(&7).unwrap().quantity, 50);
    }

    // ========================================================================
    // No-op paths
    // ========================================================================

    /// Invariant: reduce() is a no-op when no reducers are registered
    /// for the message type — no panics, no side effects.
    #[test]
    fn no_reducer_for_type_is_noop() {
        let reg = ReducerRegistry::new();
        let mut cache = Cache::new();

        cache.insert(Portfolio {
            account: 1,
            cash: 1000,
        });

        let fill = Fill {
            account: 1,
            instrument: 1,
            quantity: 1,
            price: 1,
        };

        reg.reduce(&mut cache, &fill);

        // Cache untouched
        assert_eq!(cache.get_keyed::<Portfolio>(&1).unwrap().cash, 1000);
        assert_eq!(cache.len(), 1);
    }

    /// Invariant: reduce() is a no-op when a different message type
    /// is dispatched than what reducers are registered for.
    #[test]
    fn reduce_only_fires_on_matching_type() {
        let mut reg = ReducerRegistry::new();
        let mut cache = Cache::new();

        cache.insert(TradeCounter { count: 0 });

        reg.register::<Fill>(|cache, _fill| {
            if let Some(c) = cache.get_mut::<TradeCounter>() {
                c.count += 1;
            }
        });

        let cancel = Cancel { id: 1 };
        reg.reduce(&mut cache, &cancel);
        assert_eq!(cache.get::<TradeCounter>().unwrap().count, 0);

        let fill = Fill {
            account: 1,
            instrument: 1,
            quantity: 1,
            price: 1,
        };
        reg.reduce(&mut cache, &fill);
        assert_eq!(cache.get::<TradeCounter>().unwrap().count, 1);
    }

    // ========================================================================
    // Plumbing
    // ========================================================================

    /// Invariant: a new registry is empty.
    #[test]
    fn new_registry_is_empty() {
        let reg = ReducerRegistry::new();
        assert!(reg.is_empty());
        assert_eq!(reg.len(), 0);
    }

    /// Invariant: has_reducer_for returns true only for registered types.
    #[test]
    fn has_reducer_for_reflects_registration() {
        let mut reg = ReducerRegistry::new();
        assert!(!reg.has_reducer_for::<Fill>());

        reg.register::<Fill>(|_, _| {});
        assert!(reg.has_reducer_for::<Fill>());
        assert!(!reg.has_reducer_for::<NewOrder>());
    }
}
