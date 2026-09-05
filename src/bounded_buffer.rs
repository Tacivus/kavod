use std::collections::TryReserveError;
use std::vec::Drain;

#[allow(
    dead_code,
    reason = "the Journal and Engine use this buffer in later build steps"
)]
pub(crate) struct BoundedBuffer<T> {
    items: Vec<T>,
    capacity: usize,
}

#[allow(
    dead_code,
    reason = "the Journal and Engine use this buffer in later build steps"
)]
impl<T> BoundedBuffer<T> {
    pub(crate) fn new(capacity: usize) -> Result<Self, TryReserveError> {
        let mut items = Vec::new();
        items.try_reserve_exact(capacity)?;
        assert!(
            items.capacity() >= capacity,
            "a successful bounded-buffer reservation must cover its full logical capacity"
        );

        Ok(Self { items, capacity })
    }

    pub(crate) fn try_push(&mut self, value: T) -> Result<(), T> {
        if self.items.len() == self.capacity {
            return Err(value);
        }
        assert!(
            self.items.len() < self.capacity,
            "bounded-buffer length must never exceed its logical capacity"
        );
        self.items.push(value);
        Ok(())
    }

    pub(crate) fn len(&self) -> usize {
        self.items.len()
    }

    pub(crate) fn capacity(&self) -> usize {
        self.capacity
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub(crate) fn as_slice(&self) -> &[T] {
        self.items.as_slice()
    }

    pub(crate) fn clear(&mut self) {
        self.items.clear();
    }

    pub(crate) fn drain(&mut self) -> Drain<'_, T> {
        self.items.drain(..)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod bounded_buffer_capacity {
        use super::*;

        /// Invariant: once the logical capacity is full, another push returns its
        /// value without changing the contents or growing the allocation.
        /// Design Doc: A6
        #[test]
        fn push_beyond_capacity_is_refused_without_growth() {
            let mut buffer = BoundedBuffer::new(2).expect("capacity two must be reservable");
            buffer.try_push(10).expect("the first value must fit");
            buffer.try_push(20).expect("the second value must fit");
            let allocation_capacity = buffer.items.capacity();

            assert_eq!(
                buffer.try_push(30),
                Err(30),
                "a push beyond logical capacity must return the refused value"
            );
            assert_eq!(
                buffer.as_slice(),
                &[10, 20],
                "a refused push must leave existing values unchanged"
            );
            assert_eq!(
                buffer.items.capacity(),
                allocation_capacity,
                "a refused push must not grow the backing allocation"
            );
        }

        /// Invariant: an unreservable logical capacity is reported as an error
        /// instead of constructing a buffer without its promised storage.
        /// Design Doc: A6
        #[test]
        fn construction_failure_reports_the_reservation_error() {
            assert!(
                BoundedBuffer::<u8>::new(usize::MAX).is_err(),
                "an impossible reservation must return its reservation error"
            );
        }

        /// Invariant: a zero-capacity buffer starts empty and returns the first
        /// value offered to it.
        #[test]
        fn zero_capacity_refuses_first_push_and_returns_value() {
            let mut buffer = BoundedBuffer::new(0).expect("zero capacity must be reservable");

            assert!(buffer.is_empty(), "a zero-capacity buffer must start empty");
            assert_eq!(
                buffer.capacity(),
                0,
                "a zero-capacity buffer must report zero logical capacity"
            );
            assert_eq!(
                buffer.try_push(String::from("returned")),
                Err(String::from("returned")),
                "a zero-capacity buffer must return the refused value"
            );
            assert_eq!(
                buffer.len(),
                0,
                "a refused zero-capacity push must leave the buffer empty"
            );
        }

        /// Invariant: pushes through the exact logical capacity remain observable
        /// in insertion order.
        #[test]
        fn pushes_up_to_capacity_preserve_order() {
            let mut buffer = BoundedBuffer::new(3).expect("capacity three must be reservable");

            assert_eq!(buffer.len(), 0, "a new buffer must have length zero");
            assert!(buffer.is_empty(), "a new buffer must report itself empty");
            for value in [1, 2, 3] {
                buffer
                    .try_push(value)
                    .expect("every value through logical capacity must fit");
            }

            assert_eq!(
                buffer.len(),
                buffer.capacity(),
                "a buffer filled to capacity must report matching length and capacity"
            );
            assert_eq!(
                buffer.as_slice(),
                &[1, 2, 3],
                "successful pushes must preserve insertion order"
            );
        }

        /// Invariant: refusing a push leaves all observations of the full buffer
        /// unchanged.
        #[test]
        fn refused_push_preserves_full_buffer_state() {
            let mut buffer = BoundedBuffer::new(1).expect("capacity one must be reservable");
            buffer.try_push(7).expect("the first value must fit");

            assert_eq!(
                buffer.try_push(8),
                Err(8),
                "the one-past-capacity value must be returned"
            );
            assert_eq!(buffer.len(), 1, "a refused push must preserve the length");
            assert_eq!(
                buffer.capacity(),
                1,
                "a refused push must preserve the logical capacity"
            );
            assert_eq!(
                buffer.as_slice(),
                &[7],
                "a refused push must preserve the existing value"
            );
        }
    }

    mod bounded_buffer_reuse {
        use super::*;

        /// Invariant: clearing and draining remove values while preserving both
        /// the logical capacity and the reserved allocation for reuse.
        /// Design Doc: A6
        #[test]
        fn clear_and_drain_retain_capacity() {
            let mut buffer = BoundedBuffer::new(3).expect("capacity three must be reservable");
            buffer.try_push(1).expect("the first value must fit");
            buffer.try_push(2).expect("the second value must fit");
            let allocation_capacity = buffer.items.capacity();

            buffer.clear();
            assert_eq!(
                buffer.capacity(),
                3,
                "clearing must preserve logical capacity"
            );
            assert_eq!(
                buffer.items.capacity(),
                allocation_capacity,
                "clearing must preserve the backing allocation"
            );

            buffer.try_push(3).expect("a value must fit after clearing");
            let drained: Vec<_> = buffer.drain().collect();
            assert_eq!(drained, [3], "draining must yield the value after reuse");
            assert_eq!(
                buffer.capacity(),
                3,
                "draining must preserve logical capacity"
            );
            assert_eq!(
                buffer.items.capacity(),
                allocation_capacity,
                "draining must preserve the backing allocation"
            );
        }

        /// Invariant: clearing a full buffer makes every logical slot available
        /// for subsequent pushes.
        #[test]
        fn clear_empties_buffer_for_reuse() {
            let mut buffer = BoundedBuffer::new(2).expect("capacity two must be reservable");
            buffer.try_push(1).expect("the first value must fit");
            buffer.try_push(2).expect("the second value must fit");

            buffer.clear();
            assert!(buffer.is_empty(), "clearing must leave the buffer empty");
            assert!(
                buffer.as_slice().is_empty(),
                "clearing must remove every prior value"
            );
            buffer.try_push(3).expect("the first reused slot must fit");
            buffer.try_push(4).expect("the second reused slot must fit");
            assert_eq!(
                buffer.as_slice(),
                &[3, 4],
                "all slots must be reusable after clearing"
            );
        }

        /// Invariant: draining transfers owned values in insertion order and
        /// leaves all logical slots reusable.
        #[test]
        fn drain_yields_owned_values_in_order_and_allows_refill() {
            let mut buffer = BoundedBuffer::new(2).expect("capacity two must be reservable");
            buffer
                .try_push(String::from("first"))
                .expect("the first value must fit");
            buffer
                .try_push(String::from("second"))
                .expect("the second value must fit");

            let drained: Vec<_> = buffer.drain().collect();
            assert_eq!(
                drained,
                [String::from("first"), String::from("second")],
                "draining must transfer owned values in insertion order"
            );
            assert!(buffer.is_empty(), "draining must leave the buffer empty");
            buffer
                .try_push(String::from("replacement"))
                .expect("a drained slot must be reusable");
            assert_eq!(
                buffer.as_slice(),
                &[String::from("replacement")],
                "a buffer must accept new values after draining"
            );
        }

        /// Invariant: dropping a partially consumed drain still empties the
        /// buffer and leaves it reusable.
        #[test]
        fn dropped_partial_drain_removes_remaining_values() {
            let mut buffer = BoundedBuffer::new(3).expect("capacity three must be reservable");
            for value in [1, 2, 3] {
                buffer.try_push(value).expect("each value must fit");
            }

            {
                let mut drain = buffer.drain();
                assert_eq!(
                    drain.next(),
                    Some(1),
                    "a drain must yield the first value before it is dropped"
                );
            }

            assert!(
                buffer.is_empty(),
                "dropping a full-range drain must remove its remaining values"
            );
            buffer
                .try_push(4)
                .expect("the drained buffer must be reusable");
            assert_eq!(
                buffer.as_slice(),
                &[4],
                "a buffer must remain usable after a partial drain is dropped"
            );
        }
    }
}
