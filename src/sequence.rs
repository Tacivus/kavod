use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub(crate) enum SequenceError {
    #[error("sequence overflowed")]
    Overflow,
}

/// `SeqNo` is the value type used for deterministic scheduler ordering.
/// It is opaque — callers outside this module cannot construct one directly.
/// Every scheduled message receives a unique `SeqNo` allocated by the kernels
/// `Sequencer`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct SeqNo(u64);

#[cfg(test)]
impl SeqNo {
    /// Test-only: construct a `SeqNo` at an arbitrary value.
    pub(crate) fn from_raw(n: u64) -> Self {
        Self(n)
    }
}

/// A monotonically increasing kernel-owned sequencer that allocates `SeqNo`
#[derive(Debug)]
pub(crate) struct Sequencer {
    current: SeqNo,
}

impl Sequencer {
    pub(crate) const fn initial() -> Self {
        Sequencer { current: SeqNo(0) }
    }

    /// Advance to the next sequence value and return it.
    ///
    /// Uses `checked_add` — returns `SequenceError::Overflow` instead of
    /// wrapping when the `u64` address space is exhausted.
    pub(crate) fn next(&mut self) -> Result<SeqNo, SequenceError> {
        let next = SeqNo(
            self.current
                .0
                .checked_add(1)
                .ok_or(SequenceError::Overflow)?,
        );
        self.current = next;

        Ok(next)
    }
}

#[cfg(test)]
impl Sequencer {
    /// Test-only: construct a sequencer at an arbitrary raw value.
    pub(crate) fn from_raw(n: u64) -> Self {
        Self { current: SeqNo(n) }
    }

    /// Gets the current `SeqNo`
    pub(crate) fn get(&self) -> SeqNo {
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
        assert_eq!(Sequencer::initial().get(), SeqNo(0));
    }

    /// Invariant: Sequence can only be constructed through initial()
    #[test]
    fn test_no_unrestricted_constructor() {
        let s = Sequencer::initial();
        assert_eq!(s.get(), SeqNo(0));
    }

    // ========================================================================
    // Advancement
    // ========================================================================

    /// Invariant: next() returns the value after advancing by exactly 1
    #[test]
    fn test_next_returns_larger_value() {
        let mut seq = Sequencer::initial();
        assert_eq!(seq.next().unwrap(), SeqNo(1));
    }

    /// Invariant: repeated next() calls produce strictly increasing values
    #[test]
    fn test_next_is_strictly_monotonic() {
        let mut seq = Sequencer::initial();
        let mut prev = seq.get();
        for _ in 0..1000 {
            let cur = seq.next().unwrap();
            assert!(cur > prev);
            prev = cur;
        }
    }

    /// Invariant: get() reflects the current value after each next() call
    #[test]
    fn test_get_reflects_state() {
        let mut seq = Sequencer::initial();
        assert_eq!(seq.get(), SeqNo(0));
        seq.next().unwrap();
        assert_eq!(seq.get(), SeqNo(1));
        seq.next().unwrap();
        assert_eq!(seq.get(), SeqNo(2));
    }

    // ========================================================================
    // Ordering and copy
    // ========================================================================

    /// Invariant: distinct sequences compare correctly via PartialOrd
    #[test]
    fn test_seq_nos_are_ordered() {
        let mut s = Sequencer::initial();
        let a = s.get();
        let b = s.next().unwrap();
        let c = s.next().unwrap();
        assert!(a < b);
        assert!(b < c);
        assert!(a < c);
        assert!(c > a);
    }

    /// Invariant: equal sequences compare equal via PartialEq
    #[test]
    fn test_equal_seq_nos() {
        let mut s = Sequencer::initial();
        let _ = s.next().unwrap();
        let a = s.next().unwrap();
        let b = s.next().unwrap();
        assert_eq!(a, a);
        assert_ne!(a, b);
    }

    /// Invariant: Sequence is Copy — copies are independently valid
    #[test]
    fn test_seq_no_is_copyable() {
        let mut s = Sequencer::initial();
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
        let mut seq = Sequencer {
            current: SeqNo(u64::MAX),
        };
        let result = seq.next();
        assert!(matches!(result, Err(SequenceError::Overflow)));
    }

    /// Invariant: failed overflow does not mutate the current value
    #[test]
    fn test_overflow_preserves_current() {
        let mut seq = Sequencer {
            current: SeqNo(u64::MAX),
        };
        let _ = seq.next();
        assert_eq!(seq.get(), SeqNo(u64::MAX));
    }
}
