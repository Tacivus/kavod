use crate::{
    cache::{Cache, DuplicateState, State},
    time::timestamp::Timestamp,
};

#[derive(Debug)]
pub struct ReducerCtx<'a> {
    dispatch_time: Timestamp,
    cache: &'a mut Cache,
}

impl<'a> ReducerCtx<'a> {
    pub fn new(dispatch_time: Timestamp, cache: &'a mut Cache) -> Self {
        Self {
            dispatch_time,
            cache,
        }
    }

    pub fn dispatch_time(&self) -> Timestamp {
        self.dispatch_time
    }

    pub fn get<T: State>(&self, key: &T::Key) -> Option<&T> {
        self.cache.get(key)
    }

    pub fn get_mut<T: State>(&mut self, key: &T::Key) -> Option<&mut T> {
        self.cache.get_mut(key)
    }

    pub fn get_singleton<T: State<Key = ()>>(&self) -> Option<&T> {
        self.cache.get_singleton()
    }

    pub fn get_singleton_mut<T: State<Key = ()>>(&mut self) -> Option<&mut T> {
        self.cache.get_singleton_mut()
    }

    pub fn insert<T: State>(&mut self, value: T) -> Option<T> {
        self.cache.insert(value)
    }

    pub fn try_insert<T: State>(&mut self, value: T) -> Result<(), DuplicateState> {
        self.cache.try_insert(value)
    }

    pub fn remove<T: State>(&mut self, key: &T::Key) -> Option<T> {
        self.cache.remove(key)
    }

    pub fn remove_singleton<T: State<Key = ()>>(&mut self) -> Option<T> {
        self.cache.remove_singleton()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // Test state types
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

    // ========================================================================
    // dispatch_time
    // ========================================================================

    /// Invariant: dispatch_time returns the value passed to the constructor
    #[test]
    fn test_dispatch_time_returns_constructor_value() {
        let ts = Timestamp::new(100);
        let mut cache = Cache::new();
        let ctx = ReducerCtx::new(ts, &mut cache);
        assert_eq!(ctx.dispatch_time(), ts);
    }

    /// Invariant: dispatch_time is a stable copy unaffected by cache mutations
    #[test]
    fn test_dispatch_time_stable_after_mutations() {
        let ts = Timestamp::new(42);
        let mut cache = Cache::new();
        let mut ctx = ReducerCtx::new(ts, &mut cache);
        ctx.insert(KeyedNum { key: 1, value: 10 });
        assert_eq!(ctx.dispatch_time(), ts);
    }

    // ========================================================================
    // Keyed reads
    // ========================================================================

    /// Invariant: get delegates to the cache and retrieves a stored value
    #[test]
    fn test_get_delegates_to_cache() {
        let mut cache = Cache::new();
        cache.insert(KeyedNum { key: 1, value: 77 });

        let ctx = ReducerCtx::new(Timestamp::new(0), &mut cache);
        let stored = ctx.get::<KeyedNum>(&1);
        assert!(stored.is_some());
        assert_eq!(stored.unwrap().value, 77);
    }

    /// Invariant: get for a missing key returns None
    #[test]
    fn test_get_missing_key_returns_none() {
        let mut cache = Cache::new();
        let ctx = ReducerCtx::new(Timestamp::new(0), &mut cache);
        assert!(ctx.get::<KeyedNum>(&99).is_none());
    }

    // ========================================================================
    // Mutable reads
    // ========================================================================

    /// Invariant: get_mut delegates to the cache and mutations persist
    #[test]
    fn test_get_mut_persists_changes() {
        let mut cache = Cache::new();
        cache.insert(KeyedNum { key: 1, value: 10 });

        {
            let mut ctx = ReducerCtx::new(Timestamp::new(0), &mut cache);
            let v = ctx.get_mut::<KeyedNum>(&1).unwrap();
            v.value = 99;
        }

        assert_eq!(cache.get::<KeyedNum>(&1).unwrap().value, 99);
    }

    /// Invariant: get_mut for a missing key returns None
    #[test]
    fn test_get_mut_missing_key_returns_none() {
        let mut cache = Cache::new();
        let mut ctx = ReducerCtx::new(Timestamp::new(0), &mut cache);
        assert!(ctx.get_mut::<KeyedNum>(&1).is_none());
    }

    // ========================================================================
    // Singleton reads and writes
    // ========================================================================

    /// Invariant: get_singleton delegates to the cache
    #[test]
    fn test_get_singleton_delegates() {
        let mut cache = Cache::new();
        cache.insert(SingletonNum { value: 42 });

        let ctx = ReducerCtx::new(Timestamp::new(0), &mut cache);
        assert_eq!(ctx.get_singleton::<SingletonNum>().unwrap().value, 42);
    }

    /// Invariant: get_singleton_mut delegates to the cache and mutations persist
    #[test]
    fn test_get_singleton_mut_persists() {
        let mut cache = Cache::new();
        cache.insert(SingletonNum { value: 0 });

        {
            let mut ctx = ReducerCtx::new(Timestamp::new(0), &mut cache);
            ctx.get_singleton_mut::<SingletonNum>().unwrap().value = 99;
        }

        assert_eq!(cache.get_singleton::<SingletonNum>().unwrap().value, 99);
    }

    /// Invariant: get_singleton for a missing value returns None
    #[test]
    fn test_get_singleton_missing_returns_none() {
        let mut cache = Cache::new();
        let ctx = ReducerCtx::new(Timestamp::new(0), &mut cache);
        assert!(ctx.get_singleton::<SingletonNum>().is_none());
    }

    // ========================================================================
    // Insertion
    // ========================================================================

    /// Invariant: insert delegates to the cache and returns None for a new key
    #[test]
    fn test_insert_new_key_returns_none() {
        let mut cache = Cache::new();
        let mut ctx = ReducerCtx::new(Timestamp::new(0), &mut cache);

        let prev = ctx.insert(KeyedNum { key: 1, value: 10 });
        assert!(prev.is_none());
        assert_eq!(ctx.get::<KeyedNum>(&1).unwrap().value, 10);
    }

    /// Invariant: insert on an existing key upserts and returns the old value
    #[test]
    fn test_insert_upsert_returns_old_value() {
        let mut cache = Cache::new();
        cache.insert(KeyedNum { key: 1, value: 10 });

        let mut ctx = ReducerCtx::new(Timestamp::new(0), &mut cache);
        let old = ctx.insert(KeyedNum { key: 1, value: 99 });
        assert_eq!(old, Some(KeyedNum { key: 1, value: 10 }));
        assert_eq!(ctx.get::<KeyedNum>(&1).unwrap().value, 99);
    }

    // ========================================================================
    // try_insert
    // ========================================================================

    /// Invariant: try_insert succeeds when the key is not already present
    #[test]
    fn test_try_insert_succeeds_for_new_key() {
        let mut cache = Cache::new();
        let mut ctx = ReducerCtx::new(Timestamp::new(0), &mut cache);
        assert!(ctx.try_insert(KeyedNum { key: 1, value: 10 }).is_ok());
        assert_eq!(ctx.get::<KeyedNum>(&1).unwrap().value, 10);
    }

    /// Invariant: try_insert rejects a duplicate key without replacing the
    /// existing value
    #[test]
    fn test_try_insert_rejects_duplicate() {
        let mut cache = Cache::new();
        cache.insert(KeyedNum { key: 1, value: 10 });

        let mut ctx = ReducerCtx::new(Timestamp::new(0), &mut cache);
        let result = ctx.try_insert(KeyedNum { key: 1, value: 99 });
        assert!(result.is_err());
        assert_eq!(ctx.get::<KeyedNum>(&1).unwrap().value, 10);
    }

    // ========================================================================
    // Removal
    // ========================================================================

    /// Invariant: remove delegates to the cache and returns the owned value
    #[test]
    fn test_remove_returns_owned_value() {
        let mut cache = Cache::new();
        cache.insert(KeyedNum { key: 1, value: 10 });

        let mut ctx = ReducerCtx::new(Timestamp::new(0), &mut cache);
        let removed = ctx.remove::<KeyedNum>(&1);
        assert_eq!(removed, Some(KeyedNum { key: 1, value: 10 }));
        assert!(ctx.get::<KeyedNum>(&1).is_none());
    }

    /// Invariant: remove on a missing key returns None
    #[test]
    fn test_remove_missing_key_returns_none() {
        let mut cache = Cache::new();
        let mut ctx = ReducerCtx::new(Timestamp::new(0), &mut cache);
        assert!(ctx.remove::<KeyedNum>(&99).is_none());
    }

    /// Invariant: remove_singleton delegates to the cache
    #[test]
    fn test_remove_singleton_delegates() {
        let mut cache = Cache::new();
        cache.insert(SingletonNum { value: 42 });

        let mut ctx = ReducerCtx::new(Timestamp::new(0), &mut cache);
        let removed = ctx.remove_singleton::<SingletonNum>();
        assert_eq!(removed.unwrap().value, 42);
        assert!(ctx.get_singleton::<SingletonNum>().is_none());
    }

    // ========================================================================
    // Multiple sequential mutations
    // ========================================================================

    /// Invariant: multiple sequential mutations through one context all persist
    #[test]
    fn test_multiple_sequential_mutations_persist() {
        let mut cache = Cache::new();
        cache.insert(KeyedNum { key: 1, value: 0 });
        cache.insert(KeyedNum { key: 2, value: 0 });

        {
            let mut ctx = ReducerCtx::new(Timestamp::new(0), &mut cache);
            ctx.get_mut::<KeyedNum>(&1).unwrap().value = 10;
            ctx.get_mut::<KeyedNum>(&2).unwrap().value = 20;
            ctx.insert(KeyedNum { key: 3, value: 30 });
        }

        assert_eq!(cache.get::<KeyedNum>(&1).unwrap().value, 10);
        assert_eq!(cache.get::<KeyedNum>(&2).unwrap().value, 20);
        assert_eq!(cache.get::<KeyedNum>(&3).unwrap().value, 30);
    }

    /// Invariant: multiple sequential mutations to the same entry accumulate
    #[test]
    fn test_multiple_mutations_to_same_entry_accumulate() {
        let mut cache = Cache::new();
        cache.insert(KeyedNum { key: 1, value: 0 });

        {
            let mut ctx = ReducerCtx::new(Timestamp::new(0), &mut cache);
            for _ in 0..5 {
                ctx.get_mut::<KeyedNum>(&1).unwrap().value += 1;
            }
        }

        assert_eq!(cache.get::<KeyedNum>(&1).unwrap().value, 5);
    }
}
