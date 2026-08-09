use core::{fmt, ops, slice};
use std::collections::TryReserveError;
use std::io;

/// Reusable, fixed-capacity storage for an ordered batch of values.
pub(crate) struct BoundedBuffer<T> {
    values: Vec<T>,
    capacity: usize,
}

impl<T> BoundedBuffer<T> {
    /// Creates an empty buffer with the given logical capacity.
    ///
    /// Allocates enough storage to hold `capacity` values. Returns an error if
    /// that allocation cannot be reserved.
    pub(crate) fn new(capacity: usize) -> Result<Self, TryReserveError> {
        let mut values = Vec::new();
        values.try_reserve_exact(capacity)?;

        Ok(Self { values, capacity })
    }

    /// Returns the maximum number of values the buffer can hold.
    #[must_use]
    pub(crate) fn capacity(&self) -> usize {
        self.capacity
    }

    /// Returns the number of values currently stored in the buffer.
    #[must_use]
    pub(crate) fn len(&self) -> usize {
        self.values.len()
    }

    /// Returns `true` if the buffer contains no values.
    #[must_use]
    pub(crate) fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Returns `true` if the buffer has no remaining capacity.
    #[must_use]
    pub(crate) fn is_full(&self) -> bool {
        self.remaining_capacity() == 0
    }

    /// Returns the number of values the buffer can still accept.
    #[must_use]
    pub(crate) fn remaining_capacity(&self) -> usize {
        self.capacity
            .checked_sub(self.values.len())
            .expect("bounded buffer length exceeds its capacity")
    }

    /// Appends `value` if the buffer has remaining capacity.
    ///
    /// Returns `Ok(())` when the buffer consumed `value`, or returns `Err(value)`
    /// unchanged when the buffer is full.
    pub(crate) fn push(&mut self, value: T) -> Result<(), T> {
        if self.remaining_capacity() == 0 {
            return Err(value);
        }

        self.values.push(value);
        Ok(())
    }

    /// Removes all values while retaining the allocation for reuse.
    pub(crate) fn clear(&mut self) {
        self.values.clear();
    }

    /// Returns the stored values as a slice in insertion order.
    #[must_use]
    pub(crate) fn as_slice(&self) -> &[T] {
        self.values.as_slice()
    }

    /// Returns an iterator over the stored values in insertion order.
    pub(crate) fn iter(&self) -> slice::Iter<'_, T> {
        self.values.iter()
    }
}

impl<T> fmt::Debug for BoundedBuffer<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BoundedBuffer")
            .field("capacity", &self.capacity)
            .field("len", &self.values.len())
            .finish()
    }
}

impl<'a, T> IntoIterator for &'a BoundedBuffer<T> {
    type Item = &'a T;
    type IntoIter = slice::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.values.iter()
    }
}

impl<T> ops::Deref for BoundedBuffer<T> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        self.values.as_slice()
    }
}

impl io::Write for BoundedBuffer<u8> {
    /// Writes as many leading bytes of `buf` as remaining capacity allows.
    ///
    /// Returns `Ok(n)` where `n` is the number of bytes accepted, which may be
    /// fewer than `buf.len()` (including zero) when the buffer is full.
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let accepted = buf.len().min(self.remaining_capacity());
        self.values.extend_from_slice(&buf[..accepted]);
        Ok(accepted)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, rc::Rc};

    use super::*;

    #[derive(Debug)]
    struct DropProbe(Rc<Cell<usize>>);

    impl Drop for DropProbe {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }

    mod bounded_buffer_construction {
        use super::*;

        /// Invariant: Construction records every requested logical capacity and starts empty.
        #[test]
        fn records_capacity_and_empty_state() {
            for capacity in [0, 1, 2, 17, 1_024] {
                let buffer = BoundedBuffer::<u8>::new(capacity).unwrap();

                assert_eq!(buffer.capacity(), capacity);
                assert_eq!(buffer.len(), 0);
                assert!(buffer.is_empty());
                assert_eq!(buffer.is_full(), capacity == 0);
                assert_eq!(buffer.remaining_capacity(), capacity);
                assert_eq!(buffer.as_slice(), b"");
                assert!(buffer.values.capacity() >= capacity);
            }
        }

        /// Invariant: An unrepresentable allocation fails without panicking.
        #[test]
        fn rejects_capacity_larger_than_a_valid_u8_layout() {
            assert!(BoundedBuffer::<u8>::new(usize::MAX).is_err());
        }

        /// Invariant: Multi-byte element layouts are checked before allocation.
        #[test]
        fn rejects_capacity_that_overflows_a_multi_byte_layout() {
            assert!(BoundedBuffer::<u64>::new(usize::MAX / 4).is_err());
        }

        /// Invariant: Zero-sized values can use the largest logical capacity.
        #[test]
        fn accepts_maximum_capacity_for_zero_sized_values() {
            let buffer = BoundedBuffer::<()>::new(usize::MAX).unwrap();

            assert_eq!(buffer.capacity(), usize::MAX);
            assert_eq!(buffer.remaining_capacity(), usize::MAX);
        }

        /// Invariant: Zero-sized values consume logical capacity just like stored values.
        #[test]
        fn zero_sized_values_respect_logical_capacity() {
            let mut buffer = BoundedBuffer::new(2).unwrap();

            assert_eq!(buffer.push(()), Ok(()));
            assert_eq!(buffer.push(()), Ok(()));
            assert_eq!(buffer.push(()), Err(()));
            assert_eq!(buffer.as_slice(), &[(), ()]);
        }
    }

    mod bounded_buffer_push {
        use super::*;

        /// Invariant: A buffer accepts exactly its capacity in insertion order.
        #[test]
        fn accepts_exactly_capacity_values() {
            for capacity in 0..=64 {
                let mut buffer = BoundedBuffer::new(capacity).unwrap();

                for value in 0..capacity {
                    assert_eq!(buffer.push(value), Ok(()));
                    assert_eq!(buffer.remaining_capacity(), capacity - value - 1);
                }

                assert_eq!(buffer.push(capacity), Err(capacity));
                assert_eq!(buffer.as_slice(), &(0..capacity).collect::<Vec<_>>());
                assert_eq!(buffer.len(), capacity);
                assert_eq!(buffer.remaining_capacity(), 0);
            }
        }

        /// Invariant: Rejection transfers ownership back to the caller without mutation.
        #[test]
        fn full_buffer_returns_the_original_value() {
            let mut buffer = BoundedBuffer::new(1).unwrap();
            buffer.push(String::from("stored")).unwrap();

            assert_eq!(
                buffer.push(String::from("rejected")),
                Err(String::from("rejected"))
            );
            assert_eq!(buffer.as_slice(), [String::from("stored")]);
        }

        /// Invariant: A rejected push leaves the existing allocation and contents untouched.
        #[test]
        fn rejected_push_does_not_allocate_or_mutate() {
            let mut buffer = BoundedBuffer::new(1).unwrap();
            buffer.push(b'a').unwrap();
            let allocation = buffer.values.as_ptr();
            let allocation_capacity = buffer.values.capacity();

            assert_eq!(buffer.push(b'z'), Err(b'z'));

            assert_eq!(buffer.as_slice(), b"a");
            assert_eq!(buffer.values.as_ptr(), allocation);
            assert_eq!(buffer.values.capacity(), allocation_capacity);
        }

        /// Invariant: Clearing drops the current batch exactly once and restores all capacity.
        #[test]
        fn clear_drops_values_and_restores_capacity() {
            let drops = Rc::new(Cell::new(0));
            let mut buffer = BoundedBuffer::new(2).unwrap();
            buffer.push(DropProbe(Rc::clone(&drops))).unwrap();
            buffer.push(DropProbe(Rc::clone(&drops))).unwrap();

            buffer.clear();

            assert_eq!(drops.get(), 2);
            assert!(buffer.is_empty());
            assert_eq!(buffer.remaining_capacity(), 2);
        }

        /// Invariant: Rejected values remain owned by the caller until it drops them.
        #[test]
        fn rejected_value_is_not_dropped_by_the_buffer() {
            let drops = Rc::new(Cell::new(0));
            let mut buffer = BoundedBuffer::new(1).unwrap();
            buffer.push(DropProbe(Rc::clone(&drops))).unwrap();

            let rejected = buffer.push(DropProbe(Rc::clone(&drops))).unwrap_err();

            assert_eq!(drops.get(), 0);
            drop(rejected);
            assert_eq!(drops.get(), 1);

            drop(buffer);
            assert_eq!(drops.get(), 2);
        }
    }

    mod bounded_buffer_full_state {
        use super::*;

        /// Invariant: A zero-capacity buffer starts full without ever accepting a value.
        #[test]
        fn zero_capacity_buffer_starts_full() {
            let buffer = BoundedBuffer::<u8>::new(0).unwrap();

            assert!(buffer.is_full());
        }

        /// Invariant: A buffer with remaining capacity reports itself as not full.
        #[test]
        fn buffer_with_remaining_capacity_is_not_full() {
            let mut buffer = BoundedBuffer::new(2).unwrap();
            buffer.push(1).unwrap();

            assert!(!buffer.is_full());
        }

        /// Invariant: A buffer reports full exactly when it has consumed its capacity.
        #[test]
        fn buffer_at_capacity_is_full() {
            let mut buffer = BoundedBuffer::new(2).unwrap();
            buffer.push(1).unwrap();
            buffer.push(2).unwrap();

            assert!(buffer.is_full());
        }
    }

    mod bounded_buffer_iteration {
        use super::*;

        /// Invariant: All accepted values remain visible in insertion order through iteration.
        #[test]
        fn borrowed_iteration_preserves_insertion_order() {
            let mut buffer = BoundedBuffer::new(3).unwrap();
            buffer.push(1).unwrap();
            buffer.push(2).unwrap();
            buffer.push(3).unwrap();

            assert_eq!(
                (&buffer).into_iter().copied().collect::<Vec<_>>(),
                vec![1, 2, 3]
            );
            assert_eq!(buffer.iter().copied().collect::<Vec<_>>(), vec![1, 2, 3]);
        }

        /// Invariant: An empty buffer iterates to no values.
        #[test]
        fn empty_buffer_iterates_to_no_values() {
            let buffer = BoundedBuffer::<u8>::new(3).unwrap();

            assert_eq!((&buffer).into_iter().next(), None);
            assert_eq!(buffer.iter().next(), None);
        }

        /// Invariant: Iteration reports an exact remaining count equal to the buffer's length.
        #[test]
        fn iterator_length_matches_buffer_length() {
            let mut buffer = BoundedBuffer::new(3).unwrap();
            buffer.push(1).unwrap();
            buffer.push(2).unwrap();

            assert_eq!(buffer.iter().len(), 2);
            assert_eq!((&buffer).into_iter().len(), 2);
        }

        /// Invariant: Iteration can be reversed to yield values in reverse insertion order.
        #[test]
        fn iterator_can_be_reversed() {
            let mut buffer = BoundedBuffer::new(3).unwrap();
            buffer.push(1).unwrap();
            buffer.push(2).unwrap();
            buffer.push(3).unwrap();

            assert_eq!(
                buffer.iter().rev().copied().collect::<Vec<_>>(),
                vec![3, 2, 1]
            );
            assert_eq!(
                (&buffer).into_iter().rev().copied().collect::<Vec<_>>(),
                vec![3, 2, 1]
            );
        }
    }

    mod bounded_buffer_invariant_enforcement {
        use super::*;

        /// Invariant: A length exceeding capacity is an unreachable state that panics when queried.
        #[test]
        #[should_panic(expected = "bounded buffer length exceeds its capacity")]
        fn length_exceeding_capacity_panics_on_query() {
            let mut buffer = BoundedBuffer::new(1).unwrap();
            buffer.values.push(b'a');
            buffer.values.push(b'b');

            let _ = buffer.remaining_capacity();
        }
    }

    mod bounded_buffer_deref {
        use super::*;

        /// Invariant: Dereferencing exposes accepted values as a slice in insertion order.
        #[test]
        fn dereferences_to_a_slice_of_accepted_values() {
            let mut buffer = BoundedBuffer::new(3).unwrap();
            buffer.push(1).unwrap();
            buffer.push(2).unwrap();

            assert_eq!(&*buffer, [1, 2]);
        }

        /// Invariant: Slice methods are reachable directly on the buffer through Deref.
        #[test]
        fn slice_methods_are_reachable_through_deref() {
            let mut buffer = BoundedBuffer::new(3).unwrap();
            buffer.push(1).unwrap();
            buffer.push(2).unwrap();
            buffer.push(3).unwrap();

            assert_eq!(buffer.first(), Some(&1));
            assert_eq!(buffer.last(), Some(&3));
            assert_eq!(buffer.get(1), Some(&2));
        }

        /// Invariant: An empty buffer dereferences to an empty slice.
        #[test]
        fn empty_buffer_dereferences_to_an_empty_slice() {
            let buffer = BoundedBuffer::<u8>::new(3).unwrap();

            assert_eq!(&*buffer, b"");
        }
    }

    mod bounded_buffer_reuse {
        use super::*;

        /// Invariant: Clearing is idempotent and makes a full buffer accept values again.
        #[test]
        fn clear_empty_or_full_buffer_restores_the_empty_state() {
            let mut buffer = BoundedBuffer::new(2).unwrap();

            buffer.clear();
            buffer.clear();
            assert!(buffer.is_empty());
            assert!(!buffer.is_full());
            assert_eq!(buffer.remaining_capacity(), 2);

            buffer.push(b'a').unwrap();
            buffer.push(b'b').unwrap();
            assert!(buffer.is_full());

            buffer.clear();
            assert!(buffer.is_empty());
            assert!(!buffer.is_full());
            assert_eq!(buffer.remaining_capacity(), 2);
            assert_eq!(buffer.push(b'c'), Ok(()));
        }

        /// Invariant: Dropping a partially filled buffer drops every accepted value once.
        #[test]
        fn dropping_a_partially_filled_buffer_drops_accepted_values_once() {
            let drops = Rc::new(Cell::new(0));

            {
                let mut buffer = BoundedBuffer::new(3).unwrap();
                buffer.push(DropProbe(Rc::clone(&drops))).unwrap();
                buffer.push(DropProbe(Rc::clone(&drops))).unwrap();
            }

            assert_eq!(drops.get(), 2);
        }

        /// Invariant: Zero-sized values with destructors follow the same ownership rules.
        #[test]
        fn zero_sized_values_with_destructors_are_dropped_once() {
            thread_local! {
                static DROPS: Cell<usize> = const { Cell::new(0) };
            }

            struct ZstDropProbe;

            impl Drop for ZstDropProbe {
                fn drop(&mut self) {
                    DROPS.with(|drops| drops.set(drops.get() + 1));
                }
            }

            DROPS.with(|drops| drops.set(0));
            let mut buffer = BoundedBuffer::new(2).unwrap();
            assert!(buffer.push(ZstDropProbe).is_ok());
            assert!(buffer.push(ZstDropProbe).is_ok());
            let rejected = buffer.push(ZstDropProbe).unwrap_err();

            drop(rejected);
            buffer.clear();
            buffer.clear();

            DROPS.with(|drops| assert_eq!(drops.get(), 3));
        }

        /// Invariant: Accepted pushes and clears retain the allocation established at construction.
        #[test]
        fn mutations_do_not_allocate_after_construction() {
            let mut buffer = BoundedBuffer::new(4).unwrap();
            let allocation = buffer.values.as_ptr();
            let allocation_capacity = buffer.values.capacity();

            buffer.push(b'a').unwrap();
            buffer.push(b'b').unwrap();
            buffer.clear();
            buffer.push(b'c').unwrap();
            buffer.push(b'd').unwrap();
            buffer.push(b'e').unwrap();
            buffer.push(b'f').unwrap();

            assert_eq!(buffer.as_slice(), b"cdef");
            assert_eq!(buffer.values.as_ptr(), allocation);
            assert_eq!(buffer.values.capacity(), allocation_capacity);
        }

        /// Invariant: Mixed bounded operations agree with a reference batch at every step.
        #[test]
        fn mixed_push_and_clear_operations_preserve_the_batch_model() {
            let mut buffer = BoundedBuffer::new(3).unwrap();
            let mut expected = Vec::new();

            for value in 0..32 {
                if value % 5 == 0 {
                    buffer.clear();
                    expected.clear();
                }

                let result = buffer.push(value);
                if expected.len() < 3 {
                    expected.push(value);
                    assert_eq!(result, Ok(()));
                } else {
                    assert_eq!(result, Err(value));
                }

                assert_eq!(buffer.as_slice(), expected.as_slice());
                assert_eq!(buffer.len(), expected.len());
                assert_eq!(buffer.remaining_capacity(), 3 - expected.len());
            }
        }
    }

    mod bounded_buffer_io_write {
        use std::io::Write;

        use super::*;

        /// Invariant: A write that fits within remaining capacity is accepted in full.
        #[test]
        fn write_within_capacity_accepts_all_bytes() {
            let mut buffer = BoundedBuffer::new(5).unwrap();

            let written = buffer.write(b"abc").unwrap();

            assert_eq!(written, 3);
            assert_eq!(buffer.as_slice(), b"abc");
        }

        /// Invariant: A write exceeding remaining capacity is truncated to what fits.
        #[test]
        fn write_exceeding_capacity_accepts_only_remaining_capacity() {
            let mut buffer = BoundedBuffer::new(3).unwrap();

            let written = buffer.write(b"abcdef").unwrap();

            assert_eq!(written, 3);
            assert_eq!(buffer.as_slice(), b"abc");
        }

        /// Invariant: Writing to a full buffer accepts no bytes and reports zero.
        #[test]
        fn write_to_full_buffer_accepts_no_bytes() {
            let mut buffer = BoundedBuffer::new(2).unwrap();
            buffer.write_all(b"ab").unwrap();

            let written = buffer.write(b"cd").unwrap();

            assert_eq!(written, 0);
            assert_eq!(buffer.as_slice(), b"ab");
        }

        /// Invariant: Writing an empty slice accepts zero bytes without mutating the buffer.
        #[test]
        fn write_empty_slice_accepts_no_bytes_and_does_not_mutate() {
            let mut buffer = BoundedBuffer::new(3).unwrap();
            buffer.write_all(b"a").unwrap();

            let written = buffer.write(b"").unwrap();

            assert_eq!(written, 0);
            assert_eq!(buffer.as_slice(), b"a");
        }

        /// Invariant: Successive writes append in call order up to capacity.
        #[test]
        fn successive_writes_append_in_order() {
            let mut buffer = BoundedBuffer::new(6).unwrap();

            buffer.write(b"ab").unwrap();
            buffer.write(b"cd").unwrap();
            buffer.write(b"ef").unwrap();

            assert_eq!(buffer.as_slice(), b"abcdef");
        }

        /// Invariant: `write_all` succeeds when the full payload fits within capacity.
        #[test]
        fn write_all_succeeds_when_payload_fits() {
            let mut buffer = BoundedBuffer::new(5).unwrap();

            assert!(buffer.write_all(b"hello").is_ok());
            assert_eq!(buffer.as_slice(), b"hello");
        }

        /// Invariant: `write_all` reports `WriteZero` once the buffer fills, after
        /// writing every byte that fit.
        #[test]
        fn write_all_fails_with_write_zero_when_payload_overflows_capacity() {
            let mut buffer = BoundedBuffer::new(3).unwrap();

            let error = buffer.write_all(b"abcdef").unwrap_err();

            assert_eq!(error.kind(), std::io::ErrorKind::WriteZero);
            assert_eq!(buffer.as_slice(), b"abc");
        }

        /// Invariant: Flushing never fails and leaves buffered contents unchanged.
        #[test]
        fn flush_is_a_no_op() {
            let mut buffer = BoundedBuffer::new(3).unwrap();
            buffer.write_all(b"ab").unwrap();

            assert!(buffer.flush().is_ok());
            assert_eq!(buffer.as_slice(), b"ab");
        }

        /// Invariant: The `write!` formatting macro can target the buffer through `io::Write`.
        #[test]
        fn write_macro_formats_into_the_buffer() {
            let mut buffer = BoundedBuffer::new(16).unwrap();

            write!(buffer, "{}-{}", 1, "two").unwrap();

            assert_eq!(buffer.as_slice(), b"1-two");
        }

        /// Invariant: A zero-capacity buffer accepts no bytes from a nonempty write.
        #[test]
        fn zero_capacity_buffer_accepts_no_bytes() {
            let mut buffer = BoundedBuffer::new(0).unwrap();

            let written = buffer.write(b"a").unwrap();

            assert_eq!(written, 0);
            assert_eq!(buffer.as_slice(), b"");
        }

        /// Invariant: A write updates length, fullness, and remaining capacity consistently.
        #[test]
        fn write_updates_len_and_capacity_state() {
            let mut buffer = BoundedBuffer::new(4).unwrap();

            buffer.write_all(b"ab").unwrap();

            assert_eq!(buffer.len(), 2);
            assert!(!buffer.is_empty());
            assert!(!buffer.is_full());
            assert_eq!(buffer.remaining_capacity(), 2);

            buffer.write_all(b"cd").unwrap();

            assert_eq!(buffer.len(), 4);
            assert!(buffer.is_full());
            assert_eq!(buffer.remaining_capacity(), 0);
        }

        /// Invariant: Iteration and dereferencing agree with `as_slice` after a write.
        #[test]
        fn write_result_is_visible_through_iteration_and_deref() {
            let mut buffer = BoundedBuffer::new(3).unwrap();

            buffer.write_all(b"xy").unwrap();

            assert_eq!(buffer.iter().copied().collect::<Vec<_>>(), b"xy");
            assert_eq!(&*buffer, b"xy");
        }

        /// Invariant: `write_all` with an empty payload succeeds even when the buffer is full.
        #[test]
        fn write_all_empty_payload_succeeds_on_a_full_buffer() {
            let mut buffer = BoundedBuffer::new(2).unwrap();
            buffer.write_all(b"ab").unwrap();

            assert!(buffer.write_all(b"").is_ok());
            assert_eq!(buffer.as_slice(), b"ab");
        }
    }

    mod bounded_buffer_write_push_interop {
        use std::io::Write;

        use super::*;

        /// Invariant: Bytes appended via `push` count against the capacity a later `write` consumes.
        #[test]
        fn write_after_push_only_accepts_remaining_capacity() {
            let mut buffer = BoundedBuffer::new(5).unwrap();
            buffer.push(b'a').unwrap();
            buffer.push(b'b').unwrap();

            let written = buffer.write(b"cdefg").unwrap();

            assert_eq!(written, 3);
            assert_eq!(buffer.as_slice(), b"abcde");
            assert!(buffer.is_full());
        }

        /// Invariant: Bytes appended via `write` count against the capacity a later `push` consumes.
        #[test]
        fn push_after_write_only_accepts_remaining_capacity() {
            let mut buffer = BoundedBuffer::new(3).unwrap();
            buffer.write_all(b"ab").unwrap();

            assert_eq!(buffer.push(b'c'), Ok(()));
            assert_eq!(buffer.push(b'd'), Err(b'd'));
            assert_eq!(buffer.as_slice(), b"abc");
        }

        /// Invariant: A buffer filled entirely via `push` reports full and rejects further writes.
        #[test]
        fn write_to_buffer_filled_by_push_accepts_no_bytes() {
            let mut buffer = BoundedBuffer::new(2).unwrap();
            buffer.push(b'a').unwrap();
            buffer.push(b'b').unwrap();

            let written = buffer.write(b"cd").unwrap();

            assert_eq!(written, 0);
            assert_eq!(buffer.as_slice(), b"ab");
        }
    }

    mod bounded_buffer_debug_format {
        use super::*;

        /// Invariant: Debug reports metadata without requiring values to implement Debug.
        #[test]
        fn reports_capacity_and_length_without_value_debug() {
            struct NotDebug;

            let mut buffer = BoundedBuffer::new(3).unwrap();
            assert!(buffer.push(NotDebug).is_ok());

            assert_eq!(
                format!("{buffer:?}"),
                "BoundedBuffer { capacity: 3, len: 1 }"
            );
        }

        /// Invariant: Debug reports a zero length for an empty buffer.
        #[test]
        fn reports_zero_length_at_zero_occupancy() {
            let buffer = BoundedBuffer::<u8>::new(3).unwrap();

            assert_eq!(
                format!("{buffer:?}"),
                "BoundedBuffer { capacity: 3, len: 0 }"
            );
        }

        /// Invariant: Debug reports a length equal to capacity for a full buffer.
        #[test]
        fn reports_length_equal_to_capacity_at_full_occupancy() {
            let mut buffer = BoundedBuffer::new(2).unwrap();
            buffer.push(b'a').unwrap();
            buffer.push(b'b').unwrap();

            assert_eq!(
                format!("{buffer:?}"),
                "BoundedBuffer { capacity: 2, len: 2 }"
            );
        }
    }

    mod bounded_buffer_zero_capacity {
        use super::*;

        /// Invariant: Clearing a zero-capacity buffer is a no-op that remains both empty and full.
        #[test]
        fn clear_on_zero_capacity_buffer_preserves_full_and_empty_state() {
            let mut buffer = BoundedBuffer::<u8>::new(0).unwrap();

            buffer.clear();

            assert!(buffer.is_empty());
            assert!(buffer.is_full());
            assert_eq!(buffer.remaining_capacity(), 0);
        }

        /// Invariant: A zero-sized value type at zero capacity is simultaneously empty and full.
        #[test]
        fn zero_sized_values_at_zero_capacity_are_empty_and_full() {
            let mut buffer = BoundedBuffer::<()>::new(0).unwrap();

            assert!(buffer.is_empty());
            assert!(buffer.is_full());
            assert_eq!(buffer.push(()), Err(()));
        }
    }
}
