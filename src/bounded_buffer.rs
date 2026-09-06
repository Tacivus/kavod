use std::collections::TryReserveError;
use std::vec::Drain;

pub(crate) struct BoundedBuffer<T> {
    items: Vec<T>,
    capacity: usize,
}

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

impl std::io::Write for BoundedBuffer<u8> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        assert!(
            self.items.len() <= self.capacity,
            "bounded-buffer length must never exceed its logical capacity"
        );
        let remaining = self.capacity - self.items.len();
        if remaining == 0 {
            return Err(std::io::ErrorKind::WriteZero.into());
        }

        let accepted = remaining.min(bytes.len());
        let allocation_capacity = self.items.capacity();
        self.items.extend_from_slice(&bytes[..accepted]);
        assert_eq!(
            self.items.capacity(),
            allocation_capacity,
            "bounded-buffer writes must never grow the backing allocation"
        );
        Ok(accepted)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
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

    mod encode_buffer_write {
        use super::*;
        use std::io::{ErrorKind, Write as _};

        /// Invariant: writing after the encode buffer is full reports zero
        /// progress without changing its bytes or allocation.
        /// Design Doc: JRN-ENCODE
        #[test]
        fn full_buffer_returns_write_zero() {
            let mut buffer = BoundedBuffer::new(2).expect("capacity two must be reservable");
            buffer
                .write_all(b"ab")
                .expect("bytes through exact capacity must fit");
            let allocation_capacity = buffer.items.capacity();

            let error = buffer
                .write(b"c")
                .expect_err("a full buffer must reject another write");

            assert_eq!(
                error.kind(),
                ErrorKind::WriteZero,
                "a write to a full buffer must report zero progress"
            );
            assert_eq!(
                buffer.as_slice(),
                b"ab",
                "a rejected write must preserve the accepted bytes"
            );
            assert_eq!(
                buffer.items.capacity(),
                allocation_capacity,
                "a rejected write must not grow the backing allocation"
            );
        }

        /// Invariant: successive writes append every accepted byte in order,
        /// including when the final write is accepted only in part.
        /// Design Doc: JRN-ENCODE
        #[test]
        fn partial_writes_accumulate_without_loss() {
            let mut buffer = BoundedBuffer::new(5).expect("capacity five must be reservable");
            let allocation_capacity = buffer.items.capacity();

            assert_eq!(
                buffer.write(b"ab").expect("the first write must fit"),
                2,
                "a fitting write must accept every byte"
            );
            assert_eq!(
                buffer
                    .write(b"cdef")
                    .expect("the remaining capacity must be accepted"),
                3,
                "a write larger than the remaining capacity must return a short count"
            );
            assert_eq!(
                buffer.as_slice(),
                b"abcde",
                "partial writes must accumulate all accepted bytes in order"
            );
            assert_eq!(
                buffer.items.capacity(),
                allocation_capacity,
                "partial writes must not grow the backing allocation"
            );
        }

        /// Invariant: JSON encoding succeeds when its complete byte sequence fits
        /// exactly in the bounded encode region.
        /// Design Doc: JRN-ENCODE
        #[test]
        fn serde_json_encode_completes_at_exact_region_size() {
            let value = serde_json::json!({"answer": 42});
            let expected = br#"{"answer":42}"#;
            let mut buffer =
                BoundedBuffer::new(expected.len()).expect("the exact region must be reservable");

            serde_json::to_writer(&mut buffer, &value)
                .expect("encoding at the exact region size must complete");

            assert_eq!(
                buffer.as_slice(),
                expected,
                "exact-size encoding must retain the complete JSON value"
            );
            assert_eq!(
                buffer.len(),
                buffer.capacity(),
                "exact-size encoding must fill the logical region"
            );
        }

        /// Invariant: a zero-capacity encode buffer rejects every write without
        /// retaining bytes.
        #[test]
        fn zero_capacity_rejects_all_writes() {
            let mut buffer = BoundedBuffer::new(0).expect("zero capacity must be reservable");

            for bytes in [b"x".as_slice(), b"".as_slice()] {
                let error = buffer
                    .write(bytes)
                    .expect_err("a zero-capacity buffer must reject every write");
                assert_eq!(
                    error.kind(),
                    ErrorKind::WriteZero,
                    "a zero-capacity write must report zero progress"
                );
            }
            assert!(
                buffer.is_empty(),
                "rejected zero-capacity writes must leave the buffer empty"
            );
        }

        /// Invariant: a one-byte encode buffer accepts its sole byte without
        /// allocating more storage.
        #[test]
        fn one_byte_capacity_accepts_exactly_one_byte() {
            let mut buffer = BoundedBuffer::new(1).expect("capacity one must be reservable");
            let allocation_capacity = buffer.items.capacity();

            assert_eq!(
                buffer.write(b"x").expect("the sole byte must fit"),
                1,
                "a one-byte buffer must accept one byte"
            );
            assert_eq!(
                buffer.as_slice(),
                b"x",
                "the accepted byte must be retained"
            );
            assert_eq!(
                buffer.items.capacity(),
                allocation_capacity,
                "an exact one-byte write must not grow the backing allocation"
            );
        }

        /// Invariant: an empty write makes zero progress while space remains, but
        /// the same write is rejected once no progress can ever be made.
        #[test]
        fn empty_write_returns_zero_only_while_capacity_remains() {
            let mut buffer = BoundedBuffer::new(1).expect("capacity one must be reservable");

            assert_eq!(
                buffer
                    .write(b"")
                    .expect("an empty write with room must succeed"),
                0,
                "an empty write must accept zero bytes"
            );
            assert!(
                buffer.is_empty(),
                "an empty write must not change the buffer"
            );
            buffer.write_all(b"x").expect("the sole byte must fit");
            let error = buffer
                .write(b"")
                .expect_err("an empty write to a full buffer must be rejected");
            assert_eq!(
                error.kind(),
                ErrorKind::WriteZero,
                "a full buffer must report zero progress even for empty input"
            );
        }

        /// Invariant: writing one byte beyond capacity reports failure only after
        /// retaining the prefix that fit.
        #[test]
        fn write_all_one_past_capacity_retains_accepted_prefix() {
            let mut buffer = BoundedBuffer::new(3).expect("capacity three must be reservable");

            let error = buffer
                .write_all(b"abcd")
                .expect_err("a write one past capacity must fail");

            assert_eq!(
                error.kind(),
                ErrorKind::WriteZero,
                "write_all must surface the buffer's zero-progress failure"
            );
            assert_eq!(
                buffer.as_slice(),
                b"abc",
                "write_all failure must retain exactly the accepted prefix"
            );
        }

        /// Invariant: clearing a buffer after a zero-progress write restores every
        /// logical slot for subsequent writes.
        #[test]
        fn clear_after_write_zero_restores_writes() {
            let mut buffer = BoundedBuffer::new(2).expect("capacity two must be reservable");
            buffer.write_all(b"ab").expect("the initial bytes must fit");
            buffer
                .write(b"c")
                .expect_err("a write to the full buffer must fail");

            buffer.clear();
            buffer
                .write_all(b"cd")
                .expect("all slots must be writable after clearing");

            assert_eq!(
                buffer.as_slice(),
                b"cd",
                "clearing after failure must permit complete buffer reuse"
            );
        }
    }

    mod encode_buffer_flush {
        use super::*;
        use std::io::Write as _;

        /// Invariant: flushing before, between, and after writes always succeeds
        /// without changing the buffered bytes.
        #[test]
        fn flush_succeeds_without_mutating_any_fill_state() {
            let mut buffer = BoundedBuffer::new(2).expect("capacity two must be reservable");

            buffer
                .flush()
                .expect("flushing an empty buffer must succeed");
            assert!(
                buffer.is_empty(),
                "flushing an empty buffer must leave it empty"
            );

            buffer.write_all(b"a").expect("the first byte must fit");
            buffer
                .flush()
                .expect("flushing a partially filled buffer must succeed");
            assert_eq!(
                buffer.as_slice(),
                b"a",
                "flushing a partial buffer must preserve its bytes"
            );

            buffer.write_all(b"b").expect("the final byte must fit");
            buffer.flush().expect("flushing a full buffer must succeed");
            assert_eq!(
                buffer.as_slice(),
                b"ab",
                "flushing a full buffer must preserve its bytes"
            );
        }
    }
}
