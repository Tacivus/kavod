use std::{
    any::{Any, TypeId},
    collections::HashMap,
    hash::{DefaultHasher, Hash, Hasher},
};

/// User-defined structs that persist across messages.
///
/// Two storage patterns:
/// - **Keyed** — multiple instances per type, indexed by `T::Key`.
/// - **Singleton** — one instance per type, `T::Key = ()`.
pub trait State: Clone + 'static {
    type Key: Hash + Eq;
    fn key(&self) -> Self::Key;
}

/// Typed, keyed global storage. Reducers write via `get_mut`/`insert`/`remove`;
/// handlers read via `get`. The internal map is `(TypeId, hashed_key) → value`.
pub struct Cache {
    storage: HashMap<(TypeId, u64), Box<dyn Any + Send + 'static>>,
}

impl Cache {
    pub fn new() -> Self {
        Self {
            storage: HashMap::new(),
        }
    }

    /// Upsert a value by its `State::key()`. Replaces any existing value
    /// for the same `(type, key)` pair.
    pub fn insert<T: State + Send>(&mut self, value: T) {
        let hashed = hash_key(&value.key());
        self.storage
            .insert((TypeId::of::<T>(), hashed), Box::new(value));
    }

    /// Read-only access. Used by handlers via `ctx.get::<T>(&key)`.
    pub fn get_keyed<T: State>(&self, key: &T::Key) -> Option<&T> {
        let hashed = hash_key(key);
        self.storage
            .get(&(TypeId::of::<T>(), hashed))
            .and_then(|boxed| boxed.downcast_ref::<T>())
    }

    /// Mutable access. Used by reducers. Requires `&mut self`, which
    /// enforces that reducers (which mutate) run before handlers (which
    /// only read) — the borrow checker guarantees no overlap.
    pub fn get_keyed_mut<T: State>(&mut self, key: &T::Key) -> Option<&mut T> {
        let hashed = hash_key(key);
        self.storage
            .get_mut(&(TypeId::of::<T>(), hashed))
            .and_then(|boxed| boxed.downcast_mut::<T>())
    }

    /// Remove and return a value by key.
    pub fn remove<T: State>(&mut self, key: &T::Key) -> Option<T> {
        let hashed = hash_key(key);
        self.storage
            .remove(&(TypeId::of::<T>(), hashed))
            .and_then(|boxed| boxed.downcast::<T>().ok())
            .map(|boxed| *boxed)
    }

    /// Read a singleton value. Sugar for `self.get(&())`.
    pub fn get<T: State<Key = ()>>(&self) -> Option<&T> {
        self.get_keyed(&())
    }

    /// Mutably read a singleton value. Sugar for `self.get_mut(&())`.
    pub fn get_mut<T: State<Key = ()>>(&mut self) -> Option<&mut T> {
        self.get_keyed_mut(&())
    }

    pub fn len(&self) -> usize {
        self.storage.len()
    }

    pub fn is_empty(&self) -> bool {
        self.storage.is_empty()
    }
}

/// Creates the key from the given value
fn hash_key<K: Hash>(key: &K) -> u64 {
    let mut hasher = DefaultHasher::new();
    key.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ==================================================================
    // Test types
    // ==================================================================

    #[derive(Clone, Debug, PartialEq)]
    struct Portfolio {
        account: u32,
        cash: u64,
    }
    impl State for Portfolio {
        type Key = u32;
        fn key(&self) -> u32 {
            self.account
        }
    }

    #[derive(Clone, Debug, PartialEq)]
    struct OrderBook {
        instrument: u32,
        orders: u64,
    }
    impl State for OrderBook {
        type Key = u32;
        fn key(&self) -> u32 {
            self.instrument
        }
    }

    #[derive(Clone, Debug, PartialEq)]
    struct GlobalConfig {
        max_pos_pct: u64,
        trading_enabled: bool,
    }
    impl State for GlobalConfig {
        type Key = ();
        fn key(&self) -> () {
            ()
        }
    }

    #[derive(Clone, Debug, PartialEq)]
    struct RiskLimits {
        max_drawdown: f64,
    }
    impl State for RiskLimits {
        type Key = ();
        fn key(&self) -> () {
            ()
        }
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    struct InstrumentId(u64);

    #[derive(Clone, Debug, PartialEq)]
    struct BarSeries {
        instrument: InstrumentId,
        bar_count: u64,
    }
    impl State for BarSeries {
        type Key = InstrumentId;
        fn key(&self) -> InstrumentId {
            self.instrument
        }
    }

    // ==================================================================
    // Construction
    // ==================================================================

    /// Invariant: a new Cache has zero entries.
    #[test]
    fn new_cache_is_empty() {
        let cache = Cache::new();
        assert!(cache.is_empty());
    }

    /// Invariant: a new Cache has len() == 0.
    #[test]
    fn new_cache_len_zero() {
        let cache = Cache::new();
        assert_eq!(cache.len(), 0);
    }

    // ==================================================================
    // Keyed: insert + get_keyed roundtrip
    // ==================================================================

    /// Invariant: inserting a keyed value and looking it up by the
    /// same key returns a reference to the inserted value.
    #[test]
    fn insert_and_get_keyed_roundtrip() {
        let mut cache = Cache::new();
        let portfolio = Portfolio {
            account: 1,
            cash: 50_000,
        };
        cache.insert(portfolio.clone());
        assert_eq!(cache.get_keyed::<Portfolio>(&1), Some(&portfolio));
    }

    /// Invariant: get_keyed with a key that was never inserted returns None.
    #[test]
    fn get_keyed_wrong_key_returns_none() {
        let mut cache = Cache::new();
        cache.insert(Portfolio {
            account: 1,
            cash: 50_000,
        });
        assert_eq!(cache.get_keyed::<Portfolio>(&2), None);
    }

    /// Invariant: get_keyed on a cache that has never seen type T returns None
    /// regardless of the key.
    #[test]
    fn get_keyed_on_empty_cache_returns_none() {
        let cache = Cache::new();
        assert_eq!(cache.get_keyed::<Portfolio>(&1), None);
        assert_eq!(cache.get_keyed::<Portfolio>(&0), None);
    }

    /// Invariant: inserting two instances of the same type with different
    /// keys allows independent retrieval of each.
    #[test]
    fn insert_multiple_same_type_different_keys_independent() {
        let mut cache = Cache::new();
        let p0 = Portfolio {
            account: 0,
            cash: 100_000,
        };
        let p1 = Portfolio {
            account: 1,
            cash: 50_000,
        };
        cache.insert(p0.clone());
        cache.insert(p1.clone());

        assert_eq!(cache.get_keyed::<Portfolio>(&0), Some(&p0));
        assert_eq!(cache.get_keyed::<Portfolio>(&1), Some(&p1));
        assert_eq!(cache.get_keyed::<Portfolio>(&2), None);
    }

    /// Invariant: inserting a value under the same (type, key) twice
    /// replaces the old value (upsert / last-write-wins).
    #[test]
    fn insert_same_key_upserts() {
        let mut cache = Cache::new();
        cache.insert(Portfolio {
            account: 1,
            cash: 50_000,
        });
        cache.insert(Portfolio {
            account: 1,
            cash: 75_000,
        });

        let found = cache.get_keyed::<Portfolio>(&1).unwrap();
        assert_eq!(found.cash, 75_000);
    }

    /// Invariant: upserting one key does not affect a different key's value.
    #[test]
    fn upsert_one_key_does_not_affect_another() {
        let mut cache = Cache::new();
        cache.insert(Portfolio {
            account: 0,
            cash: 100_000,
        });
        cache.insert(Portfolio {
            account: 1,
            cash: 50_000,
        });

        // upsert key=0
        cache.insert(Portfolio {
            account: 0,
            cash: 200_000,
        });

        assert_eq!(cache.get_keyed::<Portfolio>(&0).unwrap().cash, 200_000);
        assert_eq!(cache.get_keyed::<Portfolio>(&1).unwrap().cash, 50_000);
    }

    // ==================================================================
    // Keyed: get_keyed_mut
    // ==================================================================

    /// Invariant: get_keyed_mut returns a mutable reference that allows
    /// in-place mutation of the stored value.
    #[test]
    fn get_keyed_mut_allows_mutation() {
        let mut cache = Cache::new();
        cache.insert(Portfolio {
            account: 1,
            cash: 50_000,
        });

        let p = cache.get_keyed_mut::<Portfolio>(&1).unwrap();
        p.cash = 0;

        assert_eq!(cache.get_keyed::<Portfolio>(&1).unwrap().cash, 0);
    }

    /// Invariant: mutating via get_keyed_mut leaves other keys untouched.
    #[test]
    fn get_keyed_mut_does_not_affect_other_key() {
        let mut cache = Cache::new();
        cache.insert(Portfolio {
            account: 0,
            cash: 100,
        });
        cache.insert(Portfolio {
            account: 1,
            cash: 200,
        });

        let p = cache.get_keyed_mut::<Portfolio>(&0).unwrap();
        p.cash = 999;

        assert_eq!(cache.get_keyed::<Portfolio>(&0).unwrap().cash, 999);
        assert_eq!(cache.get_keyed::<Portfolio>(&1).unwrap().cash, 200);
    }

    /// Invariant: get_keyed_mut for a non-existent key returns None.
    #[test]
    fn get_keyed_mut_wrong_key_returns_none() {
        let mut cache = Cache::new();
        cache.insert(Portfolio {
            account: 1,
            cash: 50_000,
        });
        assert_eq!(cache.get_keyed_mut::<Portfolio>(&2), None);
    }

    /// Invariant: get_keyed_mut on a cache that has never seen type T
    /// returns None regardless of the key.
    #[test]
    fn get_keyed_mut_empty_cache_returns_none() {
        let mut cache = Cache::new();
        assert_eq!(cache.get_keyed_mut::<Portfolio>(&0), None);
    }

    // ==================================================================
    // Keyed: remove
    // ==================================================================

    /// Invariant: removing an existing key returns the value and removes
    /// it from the cache.
    #[test]
    fn remove_existing_returns_value_and_deletes() {
        let mut cache = Cache::new();
        let portfolio = Portfolio {
            account: 1,
            cash: 50_000,
        };
        cache.insert(portfolio.clone());

        let removed = cache.remove::<Portfolio>(&1);
        assert_eq!(removed, Some(portfolio));
        assert_eq!(cache.get_keyed::<Portfolio>(&1), None);
    }

    /// Invariant: removing a key that does not exist returns None.
    #[test]
    fn remove_nonexistent_returns_none() {
        let mut cache = Cache::new();
        cache.insert(Portfolio {
            account: 1,
            cash: 50_000,
        });

        assert_eq!(cache.remove::<Portfolio>(&2), None);
        // key 1 is still there
        assert!(cache.get_keyed::<Portfolio>(&1).is_some());
    }

    /// Invariant: removing one key does not affect another key of the same type.
    #[test]
    fn remove_one_key_does_not_affect_another() {
        let mut cache = Cache::new();
        cache.insert(Portfolio {
            account: 0,
            cash: 100,
        });
        cache.insert(Portfolio {
            account: 1,
            cash: 200,
        });

        let removed = cache.remove::<Portfolio>(&0);
        assert_eq!(removed.unwrap().cash, 100);
        assert_eq!(cache.get_keyed::<Portfolio>(&0), None);
        assert_eq!(cache.get_keyed::<Portfolio>(&1).unwrap().cash, 200);
    }

    /// Invariant: removing from an empty cache returns None.
    #[test]
    fn remove_from_empty_returns_none() {
        let mut cache = Cache::new();
        assert_eq!(cache.remove::<Portfolio>(&1), None);
    }

    /// Invariant: calling remove twice for the same key returns None the
    /// second time.
    #[test]
    fn remove_twice_second_is_none() {
        let mut cache = Cache::new();
        cache.insert(Portfolio {
            account: 1,
            cash: 50_000,
        });

        assert!(cache.remove::<Portfolio>(&1).is_some());
        assert_eq!(cache.remove::<Portfolio>(&1), None);
    }

    // ==================================================================
    // Singleton: insert + get + get_mut
    // ==================================================================

    /// Invariant: inserting a singleton and calling get() returns the value.
    #[test]
    fn singleton_insert_and_get_roundtrip() {
        let mut cache = Cache::new();
        let config = GlobalConfig {
            max_pos_pct: 5,
            trading_enabled: true,
        };
        cache.insert(config.clone());

        assert_eq!(cache.get::<GlobalConfig>(), Some(&config));
    }

    /// Invariant: get() on an empty cache returns None for any singleton type.
    #[test]
    fn singleton_get_on_empty_returns_none() {
        let cache = Cache::new();
        assert_eq!(cache.get::<GlobalConfig>(), None);
    }

    /// Invariant: get_mut() returns a mutable reference, mutations are
    /// visible through a subsequent get().
    #[test]
    fn singleton_get_mut_allows_mutation() {
        let mut cache = Cache::new();
        cache.insert(GlobalConfig {
            max_pos_pct: 5,
            trading_enabled: true,
        });

        let config = cache.get_mut::<GlobalConfig>().unwrap();
        config.trading_enabled = false;

        assert!(!cache.get::<GlobalConfig>().unwrap().trading_enabled);
    }

    /// Invariant: inserting a singleton twice replaces the old value.
    #[test]
    fn singleton_upsert_last_write_wins() {
        let mut cache = Cache::new();
        cache.insert(GlobalConfig {
            max_pos_pct: 5,
            trading_enabled: true,
        });
        cache.insert(GlobalConfig {
            max_pos_pct: 10,
            trading_enabled: false,
        });

        let config = cache.get::<GlobalConfig>().unwrap();
        assert_eq!(config.max_pos_pct, 10);
        assert!(!config.trading_enabled);
    }

    /// Invariant: get_keyed with &() on a singleton returns the same value
    /// as get(). They are the same lookup under the hood.
    #[test]
    fn singleton_get_keyed_with_unit_equals_get() {
        let mut cache = Cache::new();
        let config = GlobalConfig {
            max_pos_pct: 5,
            trading_enabled: true,
        };
        cache.insert(config.clone());

        let via_get = cache.get::<GlobalConfig>();
        let via_keyed = cache.get_keyed::<GlobalConfig>(&());

        assert_eq!(via_get, via_keyed);
    }

    // ==================================================================
    // Type isolation
    // ==================================================================

    /// Invariant: two different types (Portfolio, OrderBook) that share the
    /// same Key type (u32) and the same key value do not collide. TypeId
    /// prefixes the internal map key.
    #[test]
    fn different_types_same_key_value_no_collision() {
        let mut cache = Cache::new();
        let portfolio = Portfolio {
            account: 1,
            cash: 50_000,
        };
        let book = OrderBook {
            instrument: 1,
            orders: 42,
        };
        cache.insert(portfolio.clone());
        cache.insert(book.clone());

        // both key=1, different types — independently retrievable
        assert_eq!(cache.get_keyed::<Portfolio>(&1), Some(&portfolio));
        assert_eq!(cache.get_keyed::<OrderBook>(&1), Some(&book));
    }

    /// Invariant: removing a value of one type from a shared key does not
    /// affect the other type stored under the same key value.
    #[test]
    fn remove_one_type_does_not_affect_other_type_same_key() {
        let mut cache = Cache::new();
        cache.insert(Portfolio {
            account: 1,
            cash: 100,
        });
        cache.insert(OrderBook {
            instrument: 1,
            orders: 42,
        });

        cache.remove::<Portfolio>(&1);

        assert_eq!(cache.get_keyed::<Portfolio>(&1), None);
        assert_eq!(cache.get_keyed::<OrderBook>(&1).unwrap().orders, 42);
    }

    /// Invariant: two different singleton types are stored and retrieved
    /// independently, without cross-contamination.
    #[test]
    fn two_singletons_independent() {
        let mut cache = Cache::new();
        let config = GlobalConfig {
            max_pos_pct: 5,
            trading_enabled: true,
        };
        let limits = RiskLimits { max_drawdown: 0.25 };
        cache.insert(config.clone());
        cache.insert(limits.clone());

        assert_eq!(cache.get::<GlobalConfig>(), Some(&config));
        assert_eq!(cache.get::<RiskLimits>(), Some(&limits));

        // mutate one, verify the other is untouched
        cache.get_mut::<RiskLimits>().unwrap().max_drawdown = 0.50;

        assert_eq!(cache.get::<RiskLimits>().unwrap().max_drawdown, 0.50);
        assert_eq!(cache.get::<GlobalConfig>().unwrap().max_pos_pct, 5,);
    }

    /// Invariant: keyed types and singleton types coexist in the same cache
    /// without interference.
    #[test]
    fn keyed_and_singleton_coexist() {
        let mut cache = Cache::new();
        cache.insert(Portfolio {
            account: 0,
            cash: 100,
        });
        cache.insert(GlobalConfig {
            max_pos_pct: 5,
            trading_enabled: true,
        });

        assert_eq!(cache.get_keyed::<Portfolio>(&0).unwrap().cash, 100);
        assert!(cache.get::<GlobalConfig>().unwrap().trading_enabled);
    }

    // ==================================================================
    // len / is_empty
    // ==================================================================

    /// Invariant: len() increases by 1 for each unique (type, key) insert.
    #[test]
    fn len_increments_on_insert() {
        let mut cache = Cache::new();

        cache.insert(Portfolio {
            account: 0,
            cash: 100,
        });
        assert_eq!(cache.len(), 1);

        cache.insert(Portfolio {
            account: 1,
            cash: 200,
        });
        assert_eq!(cache.len(), 2);
    }

    /// Invariant: len() does not change when upserting an existing (type, key).
    #[test]
    fn len_unchanged_on_upsert() {
        let mut cache = Cache::new();
        cache.insert(Portfolio {
            account: 1,
            cash: 100,
        });
        assert_eq!(cache.len(), 1);

        cache.insert(Portfolio {
            account: 1,
            cash: 999,
        });
        assert_eq!(cache.len(), 1);
    }

    /// Invariant: len() decreases by 1 after a successful remove.
    #[test]
    fn len_decrements_on_remove() {
        let mut cache = Cache::new();
        cache.insert(Portfolio {
            account: 0,
            cash: 100,
        });
        cache.insert(Portfolio {
            account: 1,
            cash: 200,
        });

        cache.remove::<Portfolio>(&0);
        assert_eq!(cache.len(), 1);
    }

    /// Invariant: removing a non-existent key does not change len.
    #[test]
    fn len_unchanged_on_nonexistent_remove() {
        let mut cache = Cache::new();
        cache.insert(Portfolio {
            account: 1,
            cash: 100,
        });

        cache.remove::<Portfolio>(&999);
        assert_eq!(cache.len(), 1);
    }

    /// Invariant: is_empty() tracks len() correctly after insert and remove.
    #[test]
    fn is_empty_reflects_state() {
        let mut cache = Cache::new();
        assert!(cache.is_empty());

        cache.insert(Portfolio {
            account: 1,
            cash: 100,
        });
        assert!(!cache.is_empty());

        cache.remove::<Portfolio>(&1);
        assert!(cache.is_empty());
    }

    /// Invariant: len() counts entries across all types.
    #[test]
    fn len_across_multiple_types() {
        let mut cache = Cache::new();
        assert_eq!(cache.len(), 0);

        cache.insert(Portfolio {
            account: 0,
            cash: 100,
        });
        cache.insert(Portfolio {
            account: 1,
            cash: 200,
        });
        cache.insert(GlobalConfig {
            max_pos_pct: 5,
            trading_enabled: true,
        });
        cache.insert(OrderBook {
            instrument: 1,
            orders: 42,
        });

        assert_eq!(cache.len(), 4);

        cache.remove::<Portfolio>(&0);
        assert_eq!(cache.len(), 3);
    }

    // ==================================================================
    // Bulk / stress / edge cases
    // ==================================================================

    /// Invariant: inserting and retrieving a keyed type with a custom
    /// (non-primitive) Key type works correctly.
    #[test]
    fn custom_key_type_roundtrip() {
        let mut cache = Cache::new();
        let aapl = InstrumentId(0);
        let msft = InstrumentId(1);

        let series_aapl = BarSeries {
            instrument: aapl,
            bar_count: 100,
        };
        let series_msft = BarSeries {
            instrument: msft,
            bar_count: 250,
        };

        cache.insert(series_aapl.clone());
        cache.insert(series_msft.clone());

        assert_eq!(
            cache.get_keyed::<BarSeries>(&InstrumentId(0)),
            Some(&series_aapl)
        );
        assert_eq!(
            cache.get_keyed::<BarSeries>(&InstrumentId(1)),
            Some(&series_msft)
        );
    }

    /// Invariant: bulk insert of N keyed values, all independently retrievable.
    #[test]
    fn bulk_insert_keyed_retrieve_all() {
        let mut cache = Cache::new();
        let n = 100;
        for i in 0..n {
            cache.insert(Portfolio {
                account: i,
                cash: (i * 1000) as u64,
            });
        }
        assert_eq!(cache.len(), n as usize);

        for i in 0..n {
            let p = cache.get_keyed::<Portfolio>(&i).unwrap();
            assert_eq!(p.account, i);
            assert_eq!(p.cash, (i * 1000) as u64);
        }
    }

    /// Invariant: repeated insert/remove cycles leave the cache in the
    /// correct state (no stale entries, correct len).
    #[test]
    fn repeated_insert_remove_cycle() {
        let mut cache = Cache::new();

        for i in 0..10 {
            cache.insert(Portfolio {
                account: i,
                cash: 100,
            });
        }
        assert_eq!(cache.len(), 10);

        for i in 0..10 {
            assert!(cache.remove::<Portfolio>(&i).is_some());
        }
        assert_eq!(cache.len(), 0);
        assert!(cache.is_empty());

        // re-insert after full drain
        cache.insert(Portfolio {
            account: 42,
            cash: 999,
        });
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.get_keyed::<Portfolio>(&42).unwrap().cash, 999);
    }

    /// Invariant: a value returned from remove() outlives the cache.
    #[test]
    fn removed_value_outlives_cache() {
        let removed = {
            let mut cache = Cache::new();
            cache.insert(Portfolio {
                account: 1,
                cash: 50_000,
            });
            cache.remove::<Portfolio>(&1)
        };
        // cache is dropped; the removed value is still alive
        assert_eq!(removed.unwrap().cash, 50_000);
    }

    /// Invariant: get_keyed() returns None after the type's last entry is
    /// removed, even if other types remain in the cache.
    #[test]
    fn get_keyed_after_type_drained_returns_none() {
        let mut cache = Cache::new();
        cache.insert(Portfolio {
            account: 1,
            cash: 100,
        });
        cache.insert(GlobalConfig {
            max_pos_pct: 5,
            trading_enabled: true,
        });

        cache.remove::<Portfolio>(&1);

        assert_eq!(cache.get_keyed::<Portfolio>(&1), None);
        assert!(cache.get::<GlobalConfig>().is_some());
    }

    /// Invariant: calling get_keyed_mut on a key that was removed returns None.
    #[test]
    fn get_keyed_mut_after_remove_returns_none() {
        let mut cache = Cache::new();
        cache.insert(Portfolio {
            account: 1,
            cash: 100,
        });
        cache.remove::<Portfolio>(&1);

        assert_eq!(cache.get_keyed_mut::<Portfolio>(&1), None);
    }

    /// Invariant: insert, then get_keyed on the same key from a shared
    /// &Cache reference works (verifying get_keyed takes &self not &mut self).
    #[test]
    fn get_keyed_works_on_shared_reference() {
        let mut cache = Cache::new();
        cache.insert(Portfolio {
            account: 1,
            cash: 100,
        });

        let shared: &Cache = &cache;
        assert_eq!(shared.get_keyed::<Portfolio>(&1).unwrap().cash, 100);
    }
}
