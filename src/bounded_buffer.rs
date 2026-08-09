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
        self.buf.len()
    }

    pub(crate) fn len(&self) -> usize {
        self.len
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Pushes `value` when space remains.
    ///
    /// Returns `Ok(())` when the buffer consumed `value`, or returns `Err(value)`
    /// unchanged when the buffer is full.
    pub(crate) fn push(&mut self, value: T) -> Result<(), T> {
        self.assert_valid_len();

        if self.len == self.buf.len() {
            return Err(value);
        }

        self.buf[self.len].write(value);
        self.len += 1;
        Ok(())
    }

    /// Pushes values from `iter` until the buffer is full.
    ///
    /// Returns `None` when every value from `iter` was pushed, or `Some`
    /// holding the values that did not fit, in order, when the buffer filled
    /// up first — starting with the value that was rejected and continuing
    /// with whatever `iter` had left. As with `push`, a value that does not
    /// fit is handed back to the caller rather than dropped.
    pub(crate) fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) -> Option<Vec<T>> {
        let mut iter = iter.into_iter();

        for value in iter.by_ref() {
            if let Err(value) = self.push(value) {
                let mut leftover = vec![value];
                leftover.extend(iter);
                return Some(leftover);
            }
        }

        None
    }

    /// Removes and returns the most recently pushed value, if any.
    pub(crate) fn pop(&mut self) -> Option<T> {
        self.assert_valid_len();

        if self.len == 0 {
            return None;
        }

        self.len -= 1;

        // `buf[len]` was initialized by `push` and is now excluded from the
        // initialized prefix `0..len`, so reading it out here cannot double-drop.
        Some(unsafe { self.buf[self.len].assume_init_read() })
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
        self
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
        formatter
            .debug_struct("BoundedBuffer")
            .field("capacity", &self.buf.len())
            .field("len", &self.len)
            .finish()
    }
}

impl<T> core::ops::Deref for BoundedBuffer<T> {
    type Target = [T];

    fn deref(&self) -> &[T] {
        self.assert_valid_len();

        // `push` initializes exactly the prefix `0..len`; `clear` and Drop
        // remove elements only from that initialized prefix.
        unsafe { slice::from_raw_parts(self.buf.as_ptr().cast::<T>(), self.len) }
    }
}

impl<'a, T> IntoIterator for &'a BoundedBuffer<T> {
    type Item = &'a T;
    type IntoIter = slice::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// A consuming iterator over a [`BoundedBuffer`], yielding elements in the
/// reverse of push order (LIFO) — the same order as repeated
/// [`BoundedBuffer::pop`] calls.
pub(crate) struct IntoIter<T> {
    buffer: BoundedBuffer<T>,
}

impl<T> Iterator for IntoIter<T> {
    type Item = T;

    fn next(&mut self) -> Option<T> {
        self.buffer.pop()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.buffer.len(), Some(self.buffer.len()))
    }
}

impl<T> ExactSizeIterator for IntoIter<T> {}

impl<T> IntoIterator for BoundedBuffer<T> {
    type Item = T;
    type IntoIter = IntoIter<T>;

    /// Consumes the buffer, yielding elements in the reverse of push order (LIFO).
    fn into_iter(self) -> Self::IntoIter {
        IntoIter { buffer: self }
    }
}

impl std::io::Write for BoundedBuffer<u8> {
    /// Writes as many leading bytes of `bytes` as remaining capacity allows.
    ///
    /// Per the `Write::write` contract, it is not an error for fewer bytes to
    /// be written than requested; this returns `Ok(0)` once the buffer is
    /// full rather than erroring. Callers that need all-or-nothing behavior
    /// should use `write_all`, whose default implementation turns a `write`
    /// that makes no progress into a `WriteZero` error.
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.assert_valid_len();

        // `remaining` bounds `written` above, so `end` cannot exceed
        // `self.buf.len()` and cannot overflow.
        let remaining = self.buf.len() - self.len;
        let written = bytes.len().min(remaining);
        let end = self.len + written;

        for (slot, byte) in self.buf[self.len..end].iter_mut().zip(bytes) {
            slot.write(*byte);
        }

        self.len = end;
        Ok(written)
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

    /// Invariant: `BoundedBuffer<T>` is `Send`/`Sync` whenever `T` is; this compiles or it doesn't.
    #[allow(dead_code)]
    fn assert_send_sync<T: Send + Sync>() {
        fn is_send_sync<U: Send + Sync>() {}
        is_send_sync::<BoundedBuffer<T>>();
    }

    #[derive(Debug)]
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

        /// Invariant: The overflow check also applies to multi-byte element types, not
        /// just the `usize::MAX`/`u8` edge case.
        #[test]
        fn new_rejects_capacity_that_overflows_layout_for_a_multi_byte_element() {
            let panic = catch_unwind(|| BoundedBuffer::<u64>::new(usize::MAX / 4));

            assert!(panic.is_err());
        }

        /// Invariant: Zero-sized elements never overflow the allocation layout, so even the
        /// largest capacity succeeds.
        #[test]
        fn new_accepts_maximum_capacity_for_zero_sized_elements() {
            let buffer = BoundedBuffer::<()>::new(usize::MAX);

            assert_eq!(buffer.capacity(), usize::MAX);
            assert_eq!(buffer.len(), 0);
        }
    }

    mod bounded_buffer_push_capacity_and_order {
        use super::*;

        /// Invariant: Accepted non-Copy values remain visible in insertion order.
        #[test]
        fn push_preserves_non_copy_value_order() {
            let mut buffer = BoundedBuffer::new(3);

            assert!(buffer.push(String::from("first")).is_ok());
            assert!(buffer.push(String::from("second")).is_ok());
            assert!(buffer.push(String::from("third")).is_ok());

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
                    assert_eq!(buffer.push(value), Ok(()));
                }

                assert_eq!(buffer.len(), capacity);
                for (actual, expected) in buffer.as_slice().iter().zip(0..capacity) {
                    assert_eq!(*actual, expected);
                }
                assert_eq!(buffer.push(capacity), Err(capacity));
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

            assert_eq!(buffer.push(value), Err(String::from("not consumed")));
            assert_eq!(buffer.len(), 0);
            assert!(buffer.is_empty());
        }
    }

    mod bounded_buffer_extend {
        use super::*;

        /// Invariant: When every value fits, `extend` consumes all of `iter`
        /// and reports no leftover.
        #[test]
        fn extend_appends_every_value_when_it_all_fits() {
            let mut buffer = BoundedBuffer::new(5);

            let leftover = buffer.extend([1, 2, 3]);

            assert_eq!(leftover, None);
            assert_eq!(buffer.as_slice(), &[1, 2, 3]);
        }

        /// Invariant: When the buffer fills up partway through, `extend`
        /// pushes what fits and hands back every value that did not, in
        /// order, rather than dropping any of them.
        #[test]
        fn extend_returns_leftover_values_in_order_once_full() {
            let mut buffer = BoundedBuffer::new(2);

            let leftover = buffer.extend([1, 2, 3, 4, 5]);

            assert_eq!(buffer.as_slice(), &[1, 2]);
            assert_eq!(leftover, Some(vec![3, 4, 5]));
        }

        /// Invariant: Extending an already-full buffer returns every value
        /// from `iter` as leftover without mutating the buffer.
        #[test]
        fn extend_on_full_buffer_returns_the_entire_iterator() {
            let mut buffer = BoundedBuffer::new(1);
            buffer.push(1).unwrap();

            let leftover = buffer.extend([2, 3, 4]);

            assert_eq!(buffer.as_slice(), &[1]);
            assert_eq!(leftover, Some(vec![2, 3, 4]));
        }

        /// Invariant: Extending with an empty iterator is a no-op that
        /// reports no leftover.
        #[test]
        fn extend_with_empty_iterator_is_a_no_op() {
            let mut buffer = BoundedBuffer::<u8>::new(3);

            let leftover = buffer.extend(Vec::new());

            assert_eq!(leftover, None);
            assert!(buffer.is_empty());
        }

        /// Invariant: A value that does not fit is handed back to the caller
        /// through the leftover list rather than dropped, just as `push`
        /// hands back a rejected value.
        #[test]
        fn extend_never_drops_values_that_do_not_fit() {
            let drops = Rc::new(Cell::new(0));
            let mut buffer = BoundedBuffer::new(1);

            let leftover = buffer
                .extend([drop_probe(&drops), drop_probe(&drops)])
                .unwrap();
            assert_eq!(drops.get(), 0);
            assert_eq!(leftover.len(), 1);

            drop(leftover);
            assert_eq!(drops.get(), 1);
        }
    }

    mod bounded_buffer_pop {
        use super::*;

        /// Invariant: `pop` removes values in the reverse of push order (LIFO).
        #[test]
        fn pop_returns_values_in_reverse_push_order() {
            let mut buffer = BoundedBuffer::new(3);
            buffer.push(1).unwrap();
            buffer.push(2).unwrap();
            buffer.push(3).unwrap();

            assert_eq!(buffer.pop(), Some(3));
            assert_eq!(buffer.pop(), Some(2));
            assert_eq!(buffer.pop(), Some(1));
            assert_eq!(buffer.pop(), None);
            assert_eq!(buffer.len(), 0);
        }

        /// Invariant: Popping an empty buffer returns `None` and never panics.
        #[test]
        fn pop_on_empty_buffer_returns_none() {
            let mut buffer = BoundedBuffer::<u8>::new(2);

            assert_eq!(buffer.pop(), None);
            assert_eq!(buffer.pop(), None);
        }

        /// Invariant: A slot freed by `pop` is reusable by a subsequent `push`.
        #[test]
        fn push_after_pop_reuses_the_freed_slot() {
            let mut buffer = BoundedBuffer::new(2);
            buffer.push(1).unwrap();
            buffer.push(2).unwrap();

            assert_eq!(buffer.pop(), Some(2));
            assert_eq!(buffer.push(3), Ok(()));
            assert_eq!(buffer.push(4), Err(4));
            assert_eq!(buffer.as_slice(), &[1, 3]);
        }

        /// Invariant: `pop` transfers ownership out exactly once; the buffer never drops a
        /// popped value on its own destruction.
        #[test]
        fn pop_transfers_ownership_without_a_later_double_drop() {
            let drops = Rc::new(Cell::new(0));

            {
                let mut buffer = BoundedBuffer::new(2);
                buffer.push(drop_probe(&drops)).unwrap();
                buffer.push(drop_probe(&drops)).unwrap();

                let popped = buffer.pop().unwrap();
                assert_eq!(drops.get(), 0);

                drop(popped);
                assert_eq!(drops.get(), 1);
            }

            assert_eq!(drops.get(), 2);
        }

        /// Invariant: `pop` operates on the same cursor as the `Write` impl, so it can remove
        /// the most recently written byte.
        #[test]
        fn pop_removes_the_most_recently_written_byte() {
            let mut buffer = BoundedBuffer::new(3);
            buffer.write_all(b"ab").unwrap();

            assert_eq!(buffer.pop(), Some(b'b'));
            assert_eq!(buffer.as_slice(), b"a");
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
                    assert!(buffer.push(drop_probe(&drops)).is_ok());
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
                assert!(buffer.push(drop_probe(&drops)).is_ok());
                assert!(buffer.push(drop_probe(&drops)).is_ok());
            }

            assert_eq!(drops.get(), 2);
        }

        /// Invariant: A full buffer returns rejected ownership to its caller without dropping it.
        #[test]
        fn rejected_value_is_never_owned_by_the_buffer() {
            let drops = Rc::new(Cell::new(0));
            let mut buffer = BoundedBuffer::new(1);

            assert!(buffer.push(drop_probe(&drops)).is_ok());
            let rejected = buffer.push(drop_probe(&drops)).unwrap_err();
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
                    .is_ok()
            );
            assert!(
                buffer
                    .push(PanicProbe {
                        drops: Rc::clone(&drops),
                        panic_on_drop: true,
                        has_panicked: Rc::clone(&has_panicked),
                    })
                    .is_ok()
            );

            let panic = catch_unwind(AssertUnwindSafe(|| buffer.clear()));
            assert!(panic.is_err());
            assert_eq!(drops.get(), 1);
            assert_eq!(buffer.len(), 1);
            assert_eq!(buffer.as_slice().len(), 1);

            buffer.clear();
            assert_eq!(drops.get(), 2);
        }

        /// Invariant: Zero-sized elements are still dropped by `pop` and `clear`,
        /// exactly once, even though they occupy no storage.
        #[test]
        fn zero_sized_element_with_drop_is_dropped_exactly_once() {
            thread_local! {
                static DROPS: Cell<usize> = const { Cell::new(0) };
            }

            #[derive(Debug)]
            struct ZstDropProbe;

            impl Drop for ZstDropProbe {
                fn drop(&mut self) {
                    DROPS.with(|drops| drops.set(drops.get() + 1));
                }
            }

            let mut buffer = BoundedBuffer::new(3);
            buffer.push(ZstDropProbe).unwrap();
            buffer.push(ZstDropProbe).unwrap();
            buffer.push(ZstDropProbe).unwrap();

            let popped = buffer.pop().unwrap();
            DROPS.with(|drops| assert_eq!(drops.get(), 0));
            drop(popped);
            DROPS.with(|drops| assert_eq!(drops.get(), 1));

            buffer.clear();
            DROPS.with(|drops| assert_eq!(drops.get(), 3));
        }
    }

    mod bounded_buffer_slice_view {
        use super::*;

        /// Invariant: Zero-sized elements occupy logical capacity and remain visible in the slice.
        #[test]
        fn zero_sized_elements_respect_capacity_and_slice_length() {
            let mut buffer = BoundedBuffer::new(3);

            assert_eq!(buffer.push(()), Ok(()));
            assert_eq!(buffer.push(()), Ok(()));
            assert_eq!(buffer.push(()), Ok(()));
            assert_eq!(buffer.push(()), Err(()));
            assert_eq!(buffer.as_slice(), &[(), (), ()]);
        }

        /// Invariant: The initialized slice retains the alignment required by its element type.
        #[test]
        fn initialized_slice_is_aligned_for_high_alignment_element_types() {
            #[repr(align(64))]
            struct Aligned(u8);

            let mut buffer = BoundedBuffer::new(2);
            assert!(buffer.push(Aligned(7)).is_ok());

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

        /// Invariant: A write beyond remaining capacity writes only what fits and reports the
        /// partial count, per the `Write::write` contract.
        #[test]
        fn write_beyond_remaining_capacity_writes_only_what_fits() {
            let mut buffer = BoundedBuffer::new(5);
            buffer.write_all(b"ab").unwrap();

            let written = buffer.write(b"cdef").unwrap();

            assert_eq!(written, 3);
            assert_eq!(buffer.len(), 5);
            assert_eq!(buffer.as_slice(), b"abcde");
        }

        /// Invariant: A partial write consumes only the bytes that fit, and a further write
        /// against an exhausted buffer reports zero progress rather than erroring.
        #[test]
        fn partial_write_consumes_only_what_fits_and_reports_remaining_state() {
            let mut buffer = BoundedBuffer::new(3);
            buffer.write_all(b"a").unwrap();

            assert_eq!(buffer.write(b"bcd").unwrap(), 2);
            assert_eq!(buffer.as_slice(), b"abc");

            assert_eq!(buffer.write(b"e").unwrap(), 0);
            assert_eq!(buffer.as_slice(), b"abc");
        }

        /// Invariant: `write_all` still fails with `WriteZero` once the buffer can no longer
        /// make progress, even though the underlying `write` is not itself all-or-nothing; any
        /// progress made before the failure is not rolled back.
        #[test]
        fn write_all_fails_with_write_zero_once_the_buffer_is_full() {
            let mut buffer = BoundedBuffer::new(3);

            let error = buffer.write_all(b"abcdef").unwrap_err();

            assert_eq!(error.kind(), io::ErrorKind::WriteZero);
            assert_eq!(buffer.as_slice(), b"abc");
        }

        /// Invariant: A zero-capacity buffer accepts empty writes and reports zero progress for
        /// any non-empty write, without erroring.
        #[test]
        fn zero_capacity_buffer_write_reports_zero_progress_without_erroring() {
            let mut buffer = BoundedBuffer::<u8>::new(0);

            assert_eq!(buffer.write(b"").unwrap(), 0);
            assert_eq!(buffer.write(b"x").unwrap(), 0);
            assert_eq!(buffer.as_slice(), b"");
        }

        /// Invariant: `push` and `write` operate on the same cursor and interoperate correctly
        /// on a `BoundedBuffer<u8>`.
        #[test]
        fn push_and_write_interleave_on_the_same_cursor() {
            let mut buffer = BoundedBuffer::new(4);

            assert_eq!(buffer.push(b'a'), Ok(()));
            assert_eq!(buffer.write(b"bc").unwrap(), 2);
            assert_eq!(buffer.push(b'd'), Ok(()));
            assert_eq!(buffer.push(b'e'), Err(b'e'));

            assert_eq!(buffer.as_slice(), b"abcd");
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

                assert_eq!(buffer.write(&[255]).unwrap(), 0);
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

                assert_eq!(buffer.write(&[255]).unwrap(), 0);
                assert_eq!(buffer.as_slice(), expected);
            }
        }
    }

    mod bounded_buffer_internal_invariant_guards {
        use super::*;

        /// Invariant: Every entry point that reaches unsafe code rejects a cursor beyond capacity.
        #[test]
        fn invalid_cursor_panics_at_every_unsafe_entry_point() {
            assert_invalid_len_panics(|buffer| {
                let _ = buffer.as_slice();
            });
            assert_invalid_len_panics(|buffer| {
                let _ = buffer.push(b'y');
            });
            assert_invalid_len_panics(|buffer| {
                let _ = buffer.write(b"y");
            });
            assert_invalid_len_panics(|buffer| {
                let _ = buffer.pop();
            });
            assert_invalid_len_panics(|buffer| buffer.clear());
        }

        /// Invariant: Getters that never touch unsafe code report the corrupted cursor as-is
        /// instead of paying for a redundant guard.
        #[test]
        fn invalid_cursor_does_not_panic_at_plain_getters() {
            let mut buffer = BoundedBuffer::new(1);
            buffer.write_all(b"x").unwrap();
            buffer.len = buffer.buf.len() + 1;

            assert_eq!(buffer.capacity(), 1);
            assert_eq!(buffer.len(), 2);
            assert!(!buffer.is_empty());
            assert_eq!(
                format!("{buffer:?}"),
                "BoundedBuffer { capacity: 1, len: 2 }"
            );

            buffer.len = 1;
        }
    }

    mod bounded_buffer_into_iter {
        use super::*;

        /// Invariant: Consuming iteration yields values in the reverse of push
        /// order (LIFO), the same order as repeated `pop` calls.
        #[test]
        fn into_iter_yields_values_in_reverse_push_order() {
            let mut buffer = BoundedBuffer::new(3);
            buffer.push(1).unwrap();
            buffer.push(2).unwrap();
            buffer.push(3).unwrap();

            let collected: Vec<_> = buffer.into_iter().collect();

            assert_eq!(collected, vec![3, 2, 1]);
        }

        /// Invariant: Consuming an empty buffer yields no values.
        #[test]
        fn into_iter_on_empty_buffer_yields_nothing() {
            let buffer = BoundedBuffer::<u8>::new(3);

            let mut iter = buffer.into_iter();

            assert_eq!(iter.next(), None);
        }

        /// Invariant: The iterator reports an exact remaining count that
        /// decreases as values are yielded.
        #[test]
        fn into_iter_size_hint_matches_remaining_len() {
            let mut buffer = BoundedBuffer::new(2);
            buffer.push(1).unwrap();
            buffer.push(2).unwrap();

            let mut iter = buffer.into_iter();
            assert_eq!(iter.len(), 2);

            iter.next();
            assert_eq!(iter.len(), 1);

            iter.next();
            assert_eq!(iter.len(), 0);
        }

        /// Invariant: Dropping the iterator before it is exhausted still drops
        /// every remaining element exactly once.
        #[test]
        fn dropping_partially_consumed_iter_drops_remaining_elements_once() {
            let drops = Rc::new(Cell::new(0));
            let mut buffer = BoundedBuffer::new(3);
            buffer.push(drop_probe(&drops)).unwrap();
            buffer.push(drop_probe(&drops)).unwrap();
            buffer.push(drop_probe(&drops)).unwrap();

            {
                let mut iter = buffer.into_iter();
                let first = iter.next().unwrap();
                assert_eq!(drops.get(), 0);
                drop(first);
                assert_eq!(drops.get(), 1);
            }

            assert_eq!(drops.get(), 3);
        }

        /// Invariant: A `BoundedBuffer` can be consumed directly by a `for` loop.
        #[test]
        fn for_loop_consumes_buffer_by_value() {
            let mut buffer = BoundedBuffer::new(3);
            buffer.push('a').unwrap();
            buffer.push('b').unwrap();

            let mut collected = Vec::new();
            for value in buffer {
                collected.push(value);
            }

            assert_eq!(collected, vec!['b', 'a']);
        }
    }

    mod bounded_buffer_debug_format {
        use super::*;

        /// Invariant: Debug output reports the type name, capacity, and initialized length.
        #[test]
        fn debug_format_reports_capacity_and_length_without_requiring_value_debug() {
            struct NotDebug;

            let mut buffer = BoundedBuffer::new(3);
            assert!(buffer.push(NotDebug).is_ok());

            assert_eq!(
                format!("{buffer:?}"),
                "BoundedBuffer { capacity: 3, len: 1 }"
            );
        }
    }
}
