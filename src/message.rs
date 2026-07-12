use std::{any::Any, fmt::Debug, sync::Arc};

/// Core message type that is passed around the engine
pub trait Message: Send + Sync + Debug + Any + 'static {}

pub(crate) type SharedMessage = Arc<dyn Message>;

#[cfg(test)]
mod tests {
    use super::*;
    use std::any::Any;
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };
    use std::thread;

    #[derive(Debug, PartialEq)]
    struct TestMsg(u64);

    impl Message for TestMsg {}

    #[derive(Debug, PartialEq)]
    struct OtherMsg(u64);

    impl Message for OtherMsg {}

    // ========================================================================
    // Shared ownership
    // ========================================================================

    /// Invariant: a concrete message can be stored as Arc<dyn Message>
    #[test]
    fn test_concrete_message_as_shared() {
        let shared: SharedMessage = Arc::new(TestMsg(42));
        let payload: &dyn Any = &*shared;
        assert_eq!(payload.downcast_ref::<TestMsg>(), Some(&TestMsg(42)));
    }

    /// Invariant: cloning SharedMessage clones the Arc, not the payload
    #[test]
    fn test_clone_shares_payload() {
        let shared: SharedMessage = Arc::new(TestMsg(7));
        let clone = Arc::clone(&shared);

        assert!(Arc::ptr_eq(&shared, &clone));
        assert_eq!(Arc::strong_count(&shared), 2);

        let a: &dyn Any = &*shared;
        let b: &dyn Any = &*clone;
        assert_eq!(a.downcast_ref::<TestMsg>(), Some(&TestMsg(7)));
        assert_eq!(b.downcast_ref::<TestMsg>(), Some(&TestMsg(7)));
    }

    /// Invariant: dropping one Arc clone leaves the payload alive
    #[test]
    fn test_clone_drop_retains_payload() {
        let shared: SharedMessage = Arc::new(TestMsg(99));
        let clone = Arc::clone(&shared);
        drop(clone);

        assert_eq!(Arc::strong_count(&shared), 1);
        let payload: &dyn Any = &*shared;
        assert_eq!(payload.downcast_ref::<TestMsg>(), Some(&TestMsg(99)));
    }

    // ========================================================================
    // Thread boundaries (Send + Sync)
    // ========================================================================

    /// Invariant: a shared message can cross a thread boundary
    #[test]
    fn test_shared_message_crosses_thread() {
        let shared: SharedMessage = Arc::new(TestMsg(11));
        let moved = Arc::clone(&shared);

        let handle = thread::spawn(move || {
            let payload: &dyn Any = &*moved;
            payload.downcast_ref::<TestMsg>().map(|m| m.0)
        });

        assert_eq!(handle.join().unwrap(), Some(11));
        assert_eq!(Arc::strong_count(&shared), 1);
    }

    /// Invariant: two threads can hold immutable clones of the same payload
    #[test]
    fn test_two_threads_hold_immutable_clones() {
        let shared: SharedMessage = Arc::new(TestMsg(55));
        let a = Arc::clone(&shared);
        let b = Arc::clone(&shared);
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_a = Arc::clone(&seen);
        let seen_b = Arc::clone(&seen);

        let t1 = thread::spawn(move || {
            let payload: &dyn Any = &*a;
            let v = payload.downcast_ref::<TestMsg>().unwrap().0;
            seen_a.lock().unwrap().push(v);
        });
        let t2 = thread::spawn(move || {
            let payload: &dyn Any = &*b;
            let v = payload.downcast_ref::<TestMsg>().unwrap().0;
            seen_b.lock().unwrap().push(v);
        });

        t1.join().unwrap();
        t2.join().unwrap();

        let values = seen.lock().unwrap();
        assert_eq!(values.len(), 2);
        assert!(values.iter().all(|&v| v == 55));
    }

    /// Invariant: Message is Sync — concurrent immutable access is sound
    #[test]
    fn test_concurrent_immutable_reads() {
        let shared: SharedMessage = Arc::new(TestMsg(3));
        let hits = Arc::new(AtomicUsize::new(0));
        let mut handles = Vec::new();

        for _ in 0..4 {
            let msg = Arc::clone(&shared);
            let hits = Arc::clone(&hits);
            handles.push(thread::spawn(move || {
                let payload: &dyn Any = &*msg;
                assert_eq!(payload.downcast_ref::<TestMsg>(), Some(&TestMsg(3)));
                hits.fetch_add(1, Ordering::SeqCst);
            }));
        }

        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(hits.load(Ordering::SeqCst), 4);
    }

    // ========================================================================
    // Downcast
    // ========================================================================

    /// Invariant: downcasting a shared message recovers the concrete type
    #[test]
    fn test_downcast_recovers_concrete_type() {
        let shared: SharedMessage = Arc::new(TestMsg(123));
        let payload: &dyn Any = &*shared;
        assert_eq!(payload.downcast_ref::<TestMsg>(), Some(&TestMsg(123)));
    }

    /// Invariant: downcasting to the wrong type returns None
    #[test]
    fn test_downcast_wrong_type_returns_none() {
        let shared: SharedMessage = Arc::new(TestMsg(1));
        let payload: &dyn Any = &*shared;
        assert!(payload.downcast_ref::<OtherMsg>().is_none());
    }
}
