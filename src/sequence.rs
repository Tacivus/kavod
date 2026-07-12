use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum SequenceError {
    #[error("sequence overflowed")]
    Overflow,
}

/// A monotonically increasing kernel-owned sequence number.
///
/// `Sequence` is the value type used for deterministic scheduler ordering.
/// It is opaque — callers outside this module cannot construct one directly.
/// Every scheduled message receives a unique `Sequence` allocated by the kernel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Sequence {
    current: u64,
}

impl Sequence {
    pub(crate) const fn initial() -> Self {
        Sequence { current: 0 }
    }

    /// Advance to the next sequence value and return it.
    ///
    /// Uses `checked_add` — returns `SequenceError::Overflow` instead of
    /// wrapping when the `u64` address space is exhausted.
    pub(crate) fn next(&mut self) -> Result<Self, SequenceError> {
        let next = self.current.checked_add(1).ok_or(SequenceError::Overflow)?;
        self.current = next;
        Ok(Sequence { current: next })
    }

    pub(crate) fn get(&self) -> u64 {
        self.current
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // Construction
    // ========================================================================

    /// Invariant: initial sequence value is 0
    #[test]
    fn test_initial_is_zero() {
        assert_eq!(Sequence::initial().get(), 0);
    }

    /// Invariant: Sequence can only be constructed through initial()
    #[test]
    fn test_no_unrestricted_constructor() {
        let s = Sequence::initial();
        assert_eq!(s.get(), 0);
    }

    // ========================================================================
    // Advancement
    // ========================================================================

    /// Invariant: next() returns the value after advancing by exactly 1
    #[test]
    fn test_next_returns_larger_value() {
        let mut seq = Sequence::initial();
        assert_eq!(seq.next().unwrap().get(), 1);
    }

    /// Invariant: repeated next() calls produce strictly increasing values
    #[test]
    fn test_next_is_strictly_monotonic() {
        let mut seq = Sequence::initial();
        let mut prev = seq.get();
        for _ in 0..1000 {
            seq = seq.next().unwrap();
            assert!(seq.get() > prev);
            prev = seq.get();
        }
    }

    /// Invariant: get() reflects the current value after each next() call
    #[test]
    fn test_get_reflects_state() {
        let mut seq = Sequence::initial();
        assert_eq!(seq.get(), 0);
        seq.next().unwrap();
        assert_eq!(seq.get(), 1);
        seq.next().unwrap();
        assert_eq!(seq.get(), 2);
    }

    // ========================================================================
    // Ordering and copy
    // ========================================================================

    /// Invariant: distinct sequences compare correctly via PartialOrd
    #[test]
    fn test_sequences_are_ordered() {
        let mut s = Sequence::initial();
        let a = s;
        let b = s.next().unwrap();
        let c = s.next().unwrap();
        assert!(a < b);
        assert!(b < c);
        assert!(a < c);
        assert!(c > a);
    }

    /// Invariant: equal sequences compare equal via PartialEq
    #[test]
    fn test_equal_sequences() {
        let mut s = Sequence::initial();
        let _ = s.next().unwrap();
        let a = s.next().unwrap();
        let b = s.next().unwrap();
        assert_eq!(a, a);
        assert_ne!(a, b);
    }

    /// Invariant: Sequence is Copy — copies are independently valid
    #[test]
    fn test_sequence_is_copyable() {
        let mut s = Sequence::initial();
        let a = s.next().unwrap();
        let b = a;
        assert_eq!(a, b);
    }

    // ========================================================================
    // Overflow safety
    // ========================================================================

    /// Invariant: overflowing u64::MAX returns Overflow error, does not wrap
    #[test]
    fn test_overflow_is_checked() {
        let mut seq = Sequence { current: u64::MAX };
        let result = seq.next();
        assert!(matches!(result, Err(SequenceError::Overflow)));
    }

    /// Invariant: failed overflow does not mutate the current value
    #[test]
    fn test_overflow_preserves_current() {
        let mut seq = Sequence { current: u64::MAX };
        let _ = seq.next();
        assert_eq!(seq.get(), u64::MAX);
    }
}
