use std::{
    any::{Any, TypeId},
    collections::HashMap,
    hash::Hash,
};
use thiserror::Error;

/// Error returned when a duplicate state value is inserted via
/// [`Cache::try_insert`].
#[derive(Debug, Error)]
#[error("duplicate state entry for type {type_name}")]
pub struct DuplicateState {
    /// The name of the state type whose key was duplicated.
    pub type_name: &'static str,
}

/// A value that has a stable key and can be stored in the [`Cache`].
///
/// # Key stability
///
/// A state's key is its identity while stored. Reducers must not mutate
/// fields in a way that changes the result of [`State::key`] while the
/// value is held in the cache.
pub trait State: Send + 'static {
    /// The key type used to identify this state value in the cache.
    type Key: Eq + Hash + Send + 'static;

    /// Returns the unique key for this state value.
    fn key(&self) -> Self::Key;
}

/// A global engine-wide cache of typed keyed state.
///
/// Each concrete [`State`] type is stored in its own `HashMap<Key, Value>`.
/// Equality (`Eq`) is used to resolve hash collisions — two unequal keys
/// with the same hash are stored independently.
///
/// # Collision safety
///
/// Unlike the previous implementation, `Cache` stores the full key alongside
/// the value. Hash collisions are resolved by the inner `HashMap` using
/// `Eq`. This guarantees that unequal keys with identical hashes do not
/// alias each other.
#[derive(Debug)]
pub struct Cache {
    stores: HashMap<TypeId, Box<dyn Any + Send>>,
    len: usize,
}

impl Cache {
    /// Returns an empty cache.
    pub fn new() -> Self {
        Self {
            stores: HashMap::new(),
            len: 0,
        }
    }

    /// Inserts a state value, replacing any existing value with the same
    /// type and key.
    ///
    /// Returns the previously stored value if one existed, otherwise `None`.
    pub fn insert<T: State>(&mut self, value: T) -> Option<T> {
        let key = value.key();
        let prev = self.store_mut::<T>().insert(key, value);
        if prev.is_none() {
            self.len += 1;
        }
        prev
    }

    /// Attempts to insert a state value, returning [`DuplicateState`] if a
    /// value with the same type and key already exists.
    ///
    /// This is used by `EngineBuilder::seed` and other initialisation paths
    /// where duplicate entries are a configuration error.
    pub fn try_insert<T: State>(&mut self, value: T) -> Result<(), DuplicateState> {
        let key = value.key();
        let store = self.store_mut::<T>();
        if store.contains_key(&key) {
            return Err(DuplicateState {
                type_name: std::any::type_name::<T>(),
            });
        }
        store.insert(key, value);
        self.len += 1;
        Ok(())
    }

    /// Returns a shared reference to the state value identified by `key`,
    /// or `None` if no such value exists.
    pub fn get<T: State>(&self, key: &T::Key) -> Option<&T> {
        self.store::<T>().and_then(|s| s.get(key))
    }

    /// Returns a mutable reference to the state value identified by `key`,
    /// or `None` if no such value exists.
    pub fn get_mut<T: State>(&mut self, key: &T::Key) -> Option<&mut T> {
        self.store_mut::<T>().get_mut(key)
    }

    /// Removes and returns the state value identified by `key`,
    /// or `None` if no such value exists.
    pub fn remove<T: State>(&mut self, key: &T::Key) -> Option<T> {
        let prev = self.store_mut::<T>().remove(key);
        if prev.is_some() {
            self.len -= 1;
        }
        prev
    }

    /// Returns `true` if a state value with the given key exists in the cache.
    pub fn contains<T: State>(&self, key: &T::Key) -> bool {
        self.store::<T>().is_some_and(|s| s.contains_key(key))
    }

    /// Returns a shared reference to the singleton state value of type `T`,
    /// or `None` if no such value exists.
    ///
    /// Only valid for [`State`] types with `Key = ()`.
    pub fn get_singleton<T: State<Key = ()>>(&self) -> Option<&T> {
        self.get(&())
    }

    /// Returns a mutable reference to the singleton state value of type `T`,
    /// or `None` if no such value exists.
    ///
    /// Only valid for [`State`] types with `Key = ()`.
    pub fn get_singleton_mut<T: State<Key = ()>>(&mut self) -> Option<&mut T> {
        self.get_mut(&())
    }

    /// Removes and returns the singleton state value of type `T`,
    /// or `None` if no such value exists.
    ///
    /// Only valid for [`State`] types with `Key = ()`.
    pub fn remove_singleton<T: State<Key = ()>>(&mut self) -> Option<T> {
        self.remove(&())
    }

    /// Returns the total number of state values stored across all types in
    /// the cache.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Returns `true` if the cache contains no state values.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    // ── private helpers ────────────────────────────────────────────────

    /// Downcasts the type-erased store for `T` to a shared `HashMap`
    /// reference, returning `None` if no store exists for that type.
    fn store<T: State>(&self) -> Option<&HashMap<T::Key, T>> {
        self.stores
            .get(&TypeId::of::<T>())
            .and_then(|boxed| boxed.downcast_ref::<HashMap<T::Key, T>>())
    }

    /// Downcasts the type-erased store for `T` to a mutable `HashMap`
    /// reference, creating an empty store if one does not yet exist.
    fn store_mut<T: State>(&mut self) -> &mut HashMap<T::Key, T> {
        self.stores
            .entry(TypeId::of::<T>())
            .or_insert_with(|| Box::new(HashMap::<T::Key, T>::new()))
            .downcast_mut::<HashMap<T::Key, T>>()
            .expect("TypeId guarantees correct HashMap type")
    }
}

impl Default for Cache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::hash::Hash;
    use std::hash::Hasher;

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

    #[derive(Debug, Clone, PartialEq)]
    struct TaggedValue {
        tag: String,
        value: u64,
    }

    impl State for TaggedValue {
        type Key = String;

        fn key(&self) -> Self::Key {
            self.tag.clone()
        }
    }

    // ========================================================================
    // Construction
    // ========================================================================

    /// Invariant: a new cache has no stored values
    #[test]
    fn test_new_cache_is_empty() {
        let cache = Cache::new();
        assert_eq!(cache.len(), 0);
        assert!(cache.is_empty());
    }

    /// Invariant: a new cache reports 0 length
    #[test]
    fn test_new_cache_len_zero() {
        let cache = Cache::new();
        assert_eq!(cache.len(), 0);
    }

    /// Invariant: default is empty
    #[test]
    fn test_default_cache_len_zero() {
        let cache = Cache::default();
        assert_eq!(cache.len(), 0)
    }

    // ========================================================================
    // Keyed insert and retrieve
    // ========================================================================

    /// Invariant: an inserted keyed value can be retrieved by its key
    #[test]
    fn test_insert_and_get_one_value() {
        let mut cache = Cache::new();
        cache.insert(KeyedNum {
            key: 42,
            value: 100,
        });

        let stored = cache.get::<KeyedNum>(&42);
        assert!(stored.is_some());
        assert_eq!(stored.unwrap().value, 100);
    }

    /// Invariant: multiple values of the same state type are stored under
    /// their distinct keys
    #[test]
    fn test_insert_multiple_values_same_type() {
        let mut cache = Cache::new();
        cache.insert(KeyedNum { key: 1, value: 10 });
        cache.insert(KeyedNum { key: 2, value: 20 });
        cache.insert(KeyedNum { key: 3, value: 30 });

        assert_eq!(cache.get::<KeyedNum>(&1).unwrap().value, 10);
        assert_eq!(cache.get::<KeyedNum>(&2).unwrap().value, 20);
        assert_eq!(cache.get::<KeyedNum>(&3).unwrap().value, 30);
        assert_eq!(cache.len(), 3);
    }

    /// Invariant: equal key values used with different state types do not
    /// collide across types
    #[test]
    fn test_equal_keys_across_different_types_do_not_collide() {
        let mut cache = Cache::new();
        cache.insert(KeyedNum { key: 1, value: 10 });
        cache.insert(TaggedValue {
            tag: "1".to_string(),
            value: 20,
        });

        assert_eq!(cache.get::<KeyedNum>(&1).unwrap().value, 10);
        assert_eq!(
            cache.get::<TaggedValue>(&"1".to_string()).unwrap().value,
            20
        );
        assert_eq!(cache.len(), 2);
    }

    // ========================================================================
    // Upsert semantics
    // ========================================================================

    /// Invariant: insert on an existing key replaces the value and returns the
    /// old value
    #[test]
    fn test_upsert_returns_old_value() {
        let mut cache = Cache::new();
        cache.insert(KeyedNum { key: 1, value: 10 });

        let old = cache.insert(KeyedNum { key: 1, value: 99 });
        assert_eq!(old, Some(KeyedNum { key: 1, value: 10 }));

        let current = cache.get::<KeyedNum>(&1);
        assert_eq!(current.unwrap().value, 99);
    }

    /// Invariant: upsert with the same key does not increase length
    #[test]
    fn test_upsert_preserves_length() {
        let mut cache = Cache::new();
        cache.insert(KeyedNum { key: 1, value: 10 });
        assert_eq!(cache.len(), 1);

        cache.insert(KeyedNum { key: 1, value: 20 });
        assert_eq!(cache.len(), 1);

        cache.insert(KeyedNum { key: 2, value: 30 });
        assert_eq!(cache.len(), 2);
    }

    // ========================================================================
    // try_insert duplicate rejection
    // ========================================================================

    /// Invariant: try_insert succeeds when the key is not already present
    #[test]
    fn test_try_insert_succeeds_for_new_key() {
        let mut cache = Cache::new();
        assert!(cache.try_insert(KeyedNum { key: 1, value: 10 }).is_ok());
        assert_eq!(cache.get::<KeyedNum>(&1).unwrap().value, 10);
    }

    /// Invariant: try_insert rejects a duplicate key
    #[test]
    fn test_try_insert_rejects_duplicate_key() {
        let mut cache = Cache::new();
        cache.try_insert(KeyedNum { key: 1, value: 10 }).unwrap();

        let result = cache.try_insert(KeyedNum { key: 1, value: 99 });
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().type_name,
            std::any::type_name::<KeyedNum>()
        );
    }

    /// Invariant: a failed try_insert does not replace the existing value
    #[test]
    fn test_try_insert_does_not_replace_on_failure() {
        let mut cache = Cache::new();
        cache.try_insert(KeyedNum { key: 1, value: 10 }).unwrap();

        let _ = cache.try_insert(KeyedNum { key: 1, value: 99 });
        assert_eq!(cache.get::<KeyedNum>(&1).unwrap().value, 10);
    }

    /// Invariant: a failed try_insert does not increase length
    #[test]
    fn test_try_insert_rejection_preserves_length() {
        let mut cache = Cache::new();
        cache.try_insert(KeyedNum { key: 1, value: 10 }).unwrap();
        assert_eq!(cache.len(), 1);

        let _ = cache.try_insert(KeyedNum { key: 1, value: 99 });
        assert_eq!(cache.len(), 1);
    }

    // ========================================================================
    // Missing reads
    // ========================================================================

    /// Invariant: get for a missing key returns None
    #[test]
    fn test_get_missing_key_returns_none() {
        let cache = Cache::new();
        assert!(cache.get::<KeyedNum>(&1).is_none());
    }

    /// Invariant: get_mut for a missing key returns None
    #[test]
    fn test_get_mut_missing_key_returns_none() {
        let mut cache = Cache::new();
        assert!(cache.get_mut::<KeyedNum>(&1).is_none());
    }

    /// Invariant: get for a type that was never stored returns None
    #[test]
    fn test_get_unstored_type_returns_none() {
        let mut cache = Cache::new();
        cache.insert(KeyedNum { key: 1, value: 10 });

        assert!(cache.get::<TaggedValue>(&"x".to_string()).is_none());
    }

    // ========================================================================
    // Mutable read persistence
    // ========================================================================

    /// Invariant: mutations through get_mut persist across subsequent reads
    #[test]
    fn test_get_mut_persists_changes() {
        let mut cache = Cache::new();
        cache.insert(KeyedNum { key: 1, value: 10 });

        {
            let v = cache.get_mut::<KeyedNum>(&1).unwrap();
            v.value = 42;
        }

        assert_eq!(cache.get::<KeyedNum>(&1).unwrap().value, 42);
    }

    /// Invariant: multiple sequential get_mut mutations are visible
    #[test]
    fn test_multiple_get_mut_mutations() {
        let mut cache = Cache::new();
        cache.insert(KeyedNum { key: 1, value: 0 });

        for i in 1..=5 {
            cache.get_mut::<KeyedNum>(&1).unwrap().value += 1;
            assert_eq!(cache.get::<KeyedNum>(&1).unwrap().value, i);
        }
    }

    // ========================================================================
    // Removal
    // ========================================================================

    /// Invariant: remove returns the owned value and it is no longer present
    #[test]
    fn test_remove_returns_owned_value() {
        let mut cache = Cache::new();
        cache.insert(KeyedNum { key: 1, value: 10 });

        let removed = cache.remove::<KeyedNum>(&1);
        assert_eq!(removed, Some(KeyedNum { key: 1, value: 10 }));
        assert!(cache.get::<KeyedNum>(&1).is_none());
    }

    /// Invariant: remove on a missing key returns None
    #[test]
    fn test_remove_missing_key_returns_none() {
        let mut cache = Cache::new();
        assert!(cache.remove::<KeyedNum>(&1).is_none());
    }

    /// Invariant: removal decrements length
    #[test]
    fn test_remove_decrements_length() {
        let mut cache = Cache::new();
        cache.insert(KeyedNum { key: 1, value: 10 });
        cache.insert(KeyedNum { key: 2, value: 20 });
        assert_eq!(cache.len(), 2);

        cache.remove::<KeyedNum>(&1);
        assert_eq!(cache.len(), 1);

        cache.remove::<KeyedNum>(&2);
        assert_eq!(cache.len(), 0);
    }

    /// Invariant: after removing the last value, there is no stale state
    #[test]
    fn test_remove_last_value_leaves_no_stale_state() {
        let mut cache = Cache::new();
        cache.insert(KeyedNum { key: 1, value: 10 });
        cache.remove::<KeyedNum>(&1);

        assert_eq!(cache.len(), 0);
        assert!(cache.is_empty());
        assert!(cache.get::<KeyedNum>(&1).is_none());
    }

    // ========================================================================
    // Contains check
    // ========================================================================

    /// Invariant: contains returns true for a stored key
    #[test]
    fn test_contains_existing_key_returns_true() {
        let mut cache = Cache::new();
        cache.insert(KeyedNum { key: 1, value: 10 });
        assert!(cache.contains::<KeyedNum>(&1));
    }

    /// Invariant: contains returns false for a missing key
    #[test]
    fn test_contains_missing_key_returns_false() {
        let cache = Cache::new();
        assert!(!cache.contains::<KeyedNum>(&1));
    }

    /// Invariant: contains returns false for an unstored type
    #[test]
    fn test_contains_unstored_type_returns_false() {
        let mut cache = Cache::new();
        cache.insert(KeyedNum { key: 1, value: 10 });
        assert!(!cache.contains::<TaggedValue>(&"x".to_string()));
    }

    // ========================================================================
    // Singleton helpers
    // ========================================================================

    /// Invariant: a singleton value can be inserted and retrieved
    #[test]
    fn test_singleton_insert_and_get() {
        let mut cache = Cache::new();
        cache.insert(SingletonNum { value: 42 });

        assert_eq!(cache.get_singleton::<SingletonNum>().unwrap().value, 42);
    }

    /// Invariant: get_singleton_mut produces persistent mutations
    #[test]
    fn test_singleton_get_mut_persists() {
        let mut cache = Cache::new();
        cache.insert(SingletonNum { value: 0 });

        cache.get_singleton_mut::<SingletonNum>().unwrap().value = 99;
        assert_eq!(cache.get_singleton::<SingletonNum>().unwrap().value, 99);
    }

    /// Invariant: remove_singleton returns the owned singleton
    #[test]
    fn test_singleton_remove() {
        let mut cache = Cache::new();
        cache.insert(SingletonNum { value: 42 });

        let removed = cache.remove_singleton::<SingletonNum>();
        assert_eq!(removed.unwrap().value, 42);
        assert!(cache.get_singleton::<SingletonNum>().is_none());
    }

    /// Invariant: singleton get on a missing value returns None
    #[test]
    fn test_singleton_get_missing_returns_none() {
        let cache = Cache::new();
        assert!(cache.get_singleton::<SingletonNum>().is_none());
    }

    /// Invariant: singleton upsert replaces the value and returns the old one
    #[test]
    fn test_singleton_upsert_returns_old() {
        let mut cache = Cache::new();
        cache.insert(SingletonNum { value: 1 });
        let old = cache.insert(SingletonNum { value: 2 });
        assert_eq!(old.unwrap().value, 1);
        assert_eq!(cache.get_singleton::<SingletonNum>().unwrap().value, 2);
    }

    // ========================================================================
    // Length tracking
    // ========================================================================

    /// Invariant: len counts the total number of state values across all
    /// type stores
    #[test]
    fn test_len_counts_across_type_stores() {
        let mut cache = Cache::new();
        cache.insert(KeyedNum { key: 1, value: 10 });
        cache.insert(KeyedNum { key: 2, value: 20 });
        cache.insert(TaggedValue {
            tag: "a".to_string(),
            value: 30,
        });
        cache.insert(SingletonNum { value: 40 });

        assert_eq!(cache.len(), 4);
    }

    /// Invariant: is_empty is the negation of len > 0
    #[test]
    fn test_is_empty_matches_len_zero() {
        let mut cache = Cache::new();
        assert!(cache.is_empty());

        cache.insert(KeyedNum { key: 1, value: 10 });
        assert!(!cache.is_empty());

        cache.remove::<KeyedNum>(&1);
        assert!(cache.is_empty());
    }

    // ========================================================================
    // Collision safety — mandatory
    // ========================================================================

    /// A key whose Hash implementation always produces the same value,
    /// while Eq distinguishes identifiers. This forces the cache to rely
    /// on Eq (not just the hash) for collision resolution.
    #[derive(Debug, Clone)]
    struct CollidingKey(u32);

    impl PartialEq for CollidingKey {
        fn eq(&self, other: &Self) -> bool {
            self.0 == other.0
        }
    }

    impl Eq for CollidingKey {}

    impl Hash for CollidingKey {
        fn hash<H: Hasher>(&self, state: &mut H) {
            // Every key writes the same constant — the hash carries zero
            // discriminating power. All equality must be resolved by Eq.
            state.write_u32(0);
        }
    }

    #[derive(Debug, Clone, PartialEq)]
    struct CollidingState {
        key: CollidingKey,
        value: u64,
    }

    impl State for CollidingState {
        type Key = CollidingKey;

        fn key(&self) -> Self::Key {
            CollidingKey(self.key.0)
        }
    }

    /// Invariant: two unequal keys with identical hashes are stored independently
    #[test]
    fn test_colliding_hashes_stored_independently() {
        let mut cache = Cache::new();
        cache.insert(CollidingState {
            key: CollidingKey(1),
            value: 100,
        });
        cache.insert(CollidingState {
            key: CollidingKey(2),
            value: 200,
        });

        assert_eq!(cache.len(), 2);
        assert_eq!(
            cache.get::<CollidingState>(&CollidingKey(1)).unwrap().value,
            100
        );
        assert_eq!(
            cache.get::<CollidingState>(&CollidingKey(2)).unwrap().value,
            200
        );
    }

    /// Invariant: mutating one colliding-hash entry does not affect the other
    #[test]
    fn test_colliding_hashes_mutation_is_isolated() {
        let mut cache = Cache::new();
        cache.insert(CollidingState {
            key: CollidingKey(1),
            value: 100,
        });
        cache.insert(CollidingState {
            key: CollidingKey(2),
            value: 200,
        });

        cache
            .get_mut::<CollidingState>(&CollidingKey(1))
            .unwrap()
            .value = 999;

        assert_eq!(
            cache.get::<CollidingState>(&CollidingKey(1)).unwrap().value,
            999
        );
        assert_eq!(
            cache.get::<CollidingState>(&CollidingKey(2)).unwrap().value,
            200
        );
    }

    /// Invariant: removing one colliding-hash entry does not remove the other
    #[test]
    fn test_colliding_hashes_removal_is_isolated() {
        let mut cache = Cache::new();
        cache.insert(CollidingState {
            key: CollidingKey(1),
            value: 100,
        });
        cache.insert(CollidingState {
            key: CollidingKey(2),
            value: 200,
        });

        cache.remove::<CollidingState>(&CollidingKey(1));
        assert_eq!(cache.len(), 1);
        assert!(cache.get::<CollidingState>(&CollidingKey(1)).is_none());
        assert_eq!(
            cache.get::<CollidingState>(&CollidingKey(2)).unwrap().value,
            200
        );
    }

    /// Invariant: equal keys with colliding hashes correctly report as duplicates
    #[test]
    fn test_colliding_hashes_equal_keys_are_duplicates() {
        let mut cache = Cache::new();
        cache
            .try_insert(CollidingState {
                key: CollidingKey(1),
                value: 100,
            })
            .unwrap();

        let result = cache.try_insert(CollidingState {
            key: CollidingKey(1),
            value: 999,
        });
        assert!(result.is_err());
        assert_eq!(
            cache.get::<CollidingState>(&CollidingKey(1)).unwrap().value,
            100
        );
    }
}
