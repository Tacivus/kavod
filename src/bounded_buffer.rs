use core::{alloc::Layout, fmt, mem::MaybeUninit, slice};

pub(crate) struct BoundedBuffer<T> {
    buf: Box<[MaybeUninit<T>]>,
    len: usize,
}

impl<T> BoundedBuffer<T> {
    pub(crate) fn new(capacity: usize) -> Self {
        assert!(
            Layout::array::<MaybeUninit<T>>(capacity).is_ok(),
            "bounded buffer capacity exceeds the maximum allocation layout"
        );

        Self {
            buf: Box::<[T]>::new_uninit_slice(capacity),
            len: 0,
        }
    }

    pub(crate) fn capacity(&self) -> usize {
        self.assert_valid_len();
        self.buf.len()
    }

    pub(crate) fn len(&self) -> usize {
        self.assert_valid_len();
        self.len
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.assert_valid_len();
        self.len == 0
    }

    /// Pushes `value` when space remains.
    ///
    /// Returns `None` when the buffer consumed `value`, or returns `Some(value)`
    /// unchanged when the buffer is full.
    pub(crate) fn push(&mut self, value: T) -> Option<T> {
        self.assert_valid_len();

        if self.len == self.buf.len() {
            return Some(value);
        }

        self.buf[self.len].write(value);
        self.len += 1;
        None
    }

    pub(crate) fn clear(&mut self) {
        self.assert_valid_len();

        // Decrement before dropping so a panicking destructor cannot cause a
        // subsequent Drop implementation to drop the same element twice.
        while self.len != 0 {
            self.len -= 1;

            // `0..len` is initialized exclusively by `push`.
            unsafe {
                self.buf[self.len].assume_init_drop();
            }
        }
    }

    pub(crate) fn as_slice(&self) -> &[T] {
        self.assert_valid_len();

        // `push` initializes exactly the prefix `0..len`; `clear` and Drop
        // remove elements only from that initialized prefix.
        unsafe { slice::from_raw_parts(self.buf.as_ptr().cast::<T>(), self.len) }
    }

    fn assert_valid_len(&self) {
        assert!(
            self.len <= self.buf.len(),
            "bounded buffer length exceeds its capacity"
        );
    }
}

impl<T> Drop for BoundedBuffer<T> {
    fn drop(&mut self) {
        self.clear();
    }
}

impl<T> fmt::Debug for BoundedBuffer<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.assert_valid_len();

        formatter
            .debug_struct("BoundedBuffer")
            .field("capacity", &self.buf.len())
            .field("len", &self.len)
            .finish()
    }
}

impl std::io::Write for BoundedBuffer<u8> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.assert_valid_len();

        let remaining = self.buf.len() - self.len;
        if bytes.len() > remaining {
            return Err(std::io::Error::new(
                std::io::ErrorKind::WriteZero,
                "bounded buffer capacity exceeded",
            ));
        }

        let end = self
            .len
            .checked_add(bytes.len())
            .expect("bounded buffer length overflow");

        for (slot, byte) in self.buf[self.len..end].iter_mut().zip(bytes) {
            slot.write(*byte);
        }

        self.len = end;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{
        cell::Cell,
        io::{self, Write},
        mem::align_of,
        panic::{AssertUnwindSafe, catch_unwind},
        rc::Rc,
    };

    use super::*;

    struct DropProbe {
        drops: Rc<Cell<usize>>,
    }

    impl Drop for DropProbe {
        fn drop(&mut self) {
            self.drops.set(self.drops.get() + 1);
        }
    }

    fn drop_probe(drops: &Rc<Cell<usize>>) -> DropProbe {
        DropProbe {
            drops: Rc::clone(drops),
        }
    }

    fn assert_invalid_len_panics(operation: impl FnOnce(&mut BoundedBuffer<u8>)) {
        let mut buffer = BoundedBuffer::new(1);
        buffer.write_all(b"x").unwrap();

        let valid_len = buffer.len;
        buffer.len = buffer.buf.len() + 1;

        let panic = catch_unwind(AssertUnwindSafe(|| operation(&mut buffer)));
        assert!(panic.is_err(), "invalid cursor was not rejected");

        // Restore the invariant so the test fixture can be dropped safely.
        buffer.len = valid_len;
    }

    mod bounded_buffer_construction_and_empty_state {
        use super::*;

        /// Invariant: Construction preserves every requested logical capacity, including zero.
        #[test]
        fn new_buffer_reports_requested_capacity_and_empty_state() {
            for capacity in [0, 1, 2, 17, 1_024] {
                let buffer = BoundedBuffer::<u8>::new(capacity);

                assert_eq!(buffer.capacity(), capacity);
                assert_eq!(buffer.len(), 0);
                assert!(buffer.is_empty());
                assert_eq!(buffer.as_slice(), b"");
            }
        }

        /// Invariant: Capacities that cannot form a valid allocation layout fail before allocation.
        #[test]
        fn new_rejects_capacity_larger_than_a_valid_u8_layout() {
            let panic = catch_unwind(|| BoundedBuffer::<u8>::new(usize::MAX));

            assert!(panic.is_err());
        }
    }

    mod bounded_buffer_push_capacity_and_order {
        use super::*;

        /// Invariant: Accepted non-Copy values remain visible in insertion order.
        #[test]
        fn push_preserves_non_copy_value_order() {
            let mut buffer = BoundedBuffer::new(3);

            assert!(buffer.push(String::from("first")).is_none());
            assert!(buffer.push(String::from("second")).is_none());
            assert!(buffer.push(String::from("third")).is_none());

            let expected = [
                String::from("first"),
                String::from("second"),
                String::from("third"),
            ];
            assert_eq!(buffer.as_slice(), expected.as_slice());
            assert_eq!(buffer.len(), 3);
            assert!(!buffer.is_empty());
        }

        /// Invariant: A buffer accepts exactly its capacity and then returns each rejected value.
        #[test]
        fn push_capacity_sweep_accepts_exactly_capacity_values() {
            for capacity in 0..=64 {
                let mut buffer = BoundedBuffer::new(capacity);

                for value in 0..capacity {
                    assert_eq!(buffer.push(value), None);
                }

                assert_eq!(buffer.len(), capacity);
                for (actual, expected) in buffer.as_slice().iter().zip(0..capacity) {
                    assert_eq!(*actual, expected);
                }
                assert_eq!(buffer.push(capacity), Some(capacity));
                for (actual, expected) in buffer.as_slice().iter().zip(0..capacity) {
                    assert_eq!(*actual, expected);
                }
            }
        }

        /// Invariant: A zero-capacity buffer never consumes an input value.
        #[test]
        fn zero_capacity_push_returns_the_original_value() {
            let mut buffer = BoundedBuffer::new(0);
            let value = String::from("not consumed");

            assert_eq!(buffer.push(value), Some(String::from("not consumed")));
            assert_eq!(buffer.len(), 0);
            assert!(buffer.is_empty());
        }
    }

    mod bounded_buffer_clear_and_reuse {
        use super::*;

        /// Invariant: Clearing an empty buffer is idempotent and preserves its capacity.
        #[test]
        fn clear_empty_buffer_is_idempotent() {
            let mut buffer = BoundedBuffer::<u8>::new(3);

            buffer.clear();
            buffer.clear();

            assert_eq!(buffer.capacity(), 3);
            assert_eq!(buffer.len(), 0);
            assert!(buffer.is_empty());
            assert_eq!(buffer.as_slice(), b"");
        }

        /// Invariant: Clearing removes visible values and reuses the same bounded capacity.
        #[test]
        fn clear_removes_values_without_leaking_stale_bytes_into_reuse() {
            let mut buffer = BoundedBuffer::new(8);

            buffer.write_all(b"longer").unwrap();
            buffer.clear();
            buffer.write_all(b"new").unwrap();

            assert_eq!(buffer.capacity(), 8);
            assert_eq!(buffer.len(), 3);
            assert_eq!(buffer.as_slice(), b"new");
        }
    }

    mod bounded_buffer_element_lifecycle {
        use super::*;

        /// Invariant: Clearing drops each accepted element once and never drops it again.
        #[test]
        fn clear_drops_accepted_elements_once() {
            let drops = Rc::new(Cell::new(0));

            {
                let mut buffer = BoundedBuffer::new(3);
                for _ in 0..3 {
                    assert!(buffer.push(drop_probe(&drops)).is_none());
                }

                buffer.clear();
                assert_eq!(drops.get(), 3);

                buffer.clear();
                assert_eq!(drops.get(), 3);
            }

            assert_eq!(drops.get(), 3);
        }

        /// Invariant: Dropping a partially filled buffer drops only its accepted elements.
        #[test]
        fn drop_releases_partially_filled_buffer_elements() {
            let drops = Rc::new(Cell::new(0));

            {
                let mut buffer = BoundedBuffer::new(3);
                assert!(buffer.push(drop_probe(&drops)).is_none());
                assert!(buffer.push(drop_probe(&drops)).is_none());
            }

            assert_eq!(drops.get(), 2);
        }

        /// Invariant: A full buffer returns rejected ownership to its caller without dropping it.
        #[test]
        fn rejected_value_is_never_owned_by_the_buffer() {
            let drops = Rc::new(Cell::new(0));
            let mut buffer = BoundedBuffer::new(1);

            assert!(buffer.push(drop_probe(&drops)).is_none());
            let rejected = buffer
                .push(drop_probe(&drops))
                .expect("full buffer must return its rejected value");
            assert_eq!(drops.get(), 0);

            drop(rejected);
            assert_eq!(drops.get(), 1);

            buffer.clear();
            assert_eq!(drops.get(), 2);
        }

        /// Invariant: A panicking clear destructor is excluded from the cursor before unwinding.
        #[test]
        fn panicking_element_drop_does_not_leave_it_in_the_initialized_prefix() {
            struct PanicProbe {
                drops: Rc<Cell<usize>>,
                panic_on_drop: bool,
                has_panicked: Rc<Cell<bool>>,
            }

            impl Drop for PanicProbe {
                fn drop(&mut self) {
                    self.drops.set(self.drops.get() + 1);

                    if self.panic_on_drop && !self.has_panicked.replace(true) {
                        panic!("intentional destructor panic");
                    }
                }
            }

            let drops = Rc::new(Cell::new(0));
            let has_panicked = Rc::new(Cell::new(false));
            let mut buffer = BoundedBuffer::new(2);

            assert!(
                buffer
                    .push(PanicProbe {
                        drops: Rc::clone(&drops),
                        panic_on_drop: false,
                        has_panicked: Rc::clone(&has_panicked),
                    })
                    .is_none()
            );
            assert!(
                buffer
                    .push(PanicProbe {
                        drops: Rc::clone(&drops),
                        panic_on_drop: true,
                        has_panicked: Rc::clone(&has_panicked),
                    })
                    .is_none()
            );

            let panic = catch_unwind(AssertUnwindSafe(|| buffer.clear()));
            assert!(panic.is_err());
            assert_eq!(drops.get(), 1);
            assert_eq!(buffer.len(), 1);
            assert_eq!(buffer.as_slice().len(), 1);

            buffer.clear();
            assert_eq!(drops.get(), 2);
        }
    }

    mod bounded_buffer_slice_view {
        use super::*;

        /// Invariant: Zero-sized elements occupy logical capacity and remain visible in the slice.
        #[test]
        fn zero_sized_elements_respect_capacity_and_slice_length() {
            let mut buffer = BoundedBuffer::new(3);

            assert_eq!(buffer.push(()), None);
            assert_eq!(buffer.push(()), None);
            assert_eq!(buffer.push(()), None);
            assert_eq!(buffer.push(()), Some(()));
            assert_eq!(buffer.as_slice(), &[(), (), ()]);
        }

        /// Invariant: The initialized slice retains the alignment required by its element type.
        #[test]
        fn initialized_slice_is_aligned_for_high_alignment_element_types() {
            #[repr(align(64))]
            struct Aligned(u8);

            let mut buffer = BoundedBuffer::new(2);
            assert!(buffer.push(Aligned(7)).is_none());

            let values = buffer.as_slice();
            assert_eq!(values[0].0, 7);
            assert_eq!((values.as_ptr() as usize) % align_of::<Aligned>(), 0);
        }
    }

    mod bounded_buffer_write_protocol {
        use super::*;

        /// Invariant: Successful writes append their complete input and report exact progress.
        #[test]
        fn write_appends_multiple_inputs_in_order() {
            let mut buffer = BoundedBuffer::new(5);

            assert_eq!(buffer.write(b"ab").unwrap(), 2);
            assert_eq!(buffer.write(b"cde").unwrap(), 3);

            assert_eq!(buffer.len(), 5);
            assert_eq!(buffer.as_slice(), b"abcde");
        }

        /// Invariant: An oversized write fails atomically with WriteZero and preserves prior data.
        #[test]
        fn oversized_write_returns_write_zero_without_partial_append() {
            let mut buffer = BoundedBuffer::new(5);
            buffer.write_all(b"ab").unwrap();

            let error = buffer.write(b"cdef").unwrap_err();

            assert_eq!(error.kind(), io::ErrorKind::WriteZero);
            assert_eq!(buffer.len(), 2);
            assert_eq!(buffer.as_slice(), b"ab");
        }

        /// Invariant: A rejected write does not poison later fitting writes.
        #[test]
        fn rejected_write_leaves_remaining_capacity_usable() {
            let mut buffer = BoundedBuffer::new(3);
            buffer.write_all(b"a").unwrap();

            assert!(buffer.write(b"bcd").is_err());
            assert_eq!(buffer.write(b"bc").unwrap(), 2);
            assert_eq!(buffer.as_slice(), b"abc");
        }

        /// Invariant: Empty writes succeed regardless of remaining capacity and never mutate bytes.
        #[test]
        fn empty_write_succeeds_when_empty_and_when_full() {
            let mut buffer = BoundedBuffer::new(1);

            assert_eq!(buffer.write(b"").unwrap(), 0);
            assert_eq!(buffer.as_slice(), b"");

            buffer.write_all(b"x").unwrap();
            assert_eq!(buffer.write(b"").unwrap(), 0);
            assert_eq!(buffer.as_slice(), b"x");
        }

        /// Invariant: Standard Write helpers delegate to the bounded byte protocol.
        #[test]
        fn write_all_and_write_fmt_preserve_the_complete_formatted_output() {
            let mut buffer = BoundedBuffer::new(5);

            buffer.write_all(b"ab").unwrap();
            write!(&mut buffer, "{}{}", "c", "de").unwrap();

            assert_eq!(buffer.as_slice(), b"abcde");
        }

        /// Invariant: Flushing an in-memory buffer always succeeds without changing its bytes.
        #[test]
        fn flush_is_a_no_op() {
            let mut buffer = BoundedBuffer::new(3);
            buffer.write_all(b"abc").unwrap();

            buffer.flush().unwrap();

            assert_eq!(buffer.len(), 3);
            assert_eq!(buffer.as_slice(), b"abc");
        }
    }

    mod bounded_buffer_write_capacity_boundaries {
        use super::*;

        /// Invariant: Every capacity accepts exactly that many bytes and rejects the next byte.
        #[test]
        fn exact_capacity_write_sweep_preserves_complete_bytes() {
            for capacity in 0..=64 {
                let bytes = [42_u8; 64];
                let expected = &bytes[..capacity];
                let mut buffer = BoundedBuffer::new(capacity);

                assert_eq!(buffer.write(expected).unwrap(), capacity);
                assert_eq!(buffer.as_slice(), expected);

                let error = buffer.write(&[255]).unwrap_err();
                assert_eq!(error.kind(), io::ErrorKind::WriteZero);
                assert_eq!(buffer.as_slice(), expected);
            }
        }

        /// Invariant: A partial write leaves precisely its remaining capacity for a final write.
        #[test]
        fn partial_write_sweep_accepts_exact_remaining_bytes_only() {
            for capacity in 0..=64 {
                let bytes = [42_u8; 64];
                let expected = &bytes[..capacity];
                let split = capacity / 2;
                let (prefix, suffix) = expected.split_at(split);
                let mut buffer = BoundedBuffer::new(capacity);

                assert_eq!(buffer.write(prefix).unwrap(), prefix.len());
                assert_eq!(buffer.write(suffix).unwrap(), suffix.len());
                assert_eq!(buffer.as_slice(), expected);

                assert_eq!(
                    buffer.write(&[255]).unwrap_err().kind(),
                    io::ErrorKind::WriteZero
                );
                assert_eq!(buffer.as_slice(), expected);
            }
        }
    }

    mod bounded_buffer_internal_invariant_guards {
        use super::*;

        /// Invariant: Every public observer and mutator rejects a cursor beyond storage capacity.
        #[test]
        fn invalid_cursor_panics_at_every_public_entry_point() {
            assert_invalid_len_panics(|buffer| {
                let _ = buffer.capacity();
            });
            assert_invalid_len_panics(|buffer| {
                let _ = buffer.len();
            });
            assert_invalid_len_panics(|buffer| {
                let _ = buffer.is_empty();
            });
            assert_invalid_len_panics(|buffer| {
                let _ = buffer.as_slice();
            });
            assert_invalid_len_panics(|buffer| {
                let _ = format!("{buffer:?}");
            });
            assert_invalid_len_panics(|buffer| {
                let _ = buffer.push(b'y');
            });
            assert_invalid_len_panics(|buffer| {
                let _ = buffer.write(b"y");
            });
            assert_invalid_len_panics(|buffer| buffer.clear());
        }
    }

    mod bounded_buffer_debug_format {
        use super::*;

        /// Invariant: Debug output reports the type name, capacity, and initialized length.
        #[test]
        fn debug_format_reports_capacity_and_length_without_requiring_value_debug() {
            struct NotDebug;

            let mut buffer = BoundedBuffer::new(3);
            assert!(buffer.push(NotDebug).is_none());

            assert_eq!(
                format!("{buffer:?}"),
                "BoundedBuffer { capacity: 3, len: 1 }"
            );
        }
    }
}
