use crate::time::TimeError;

pub const NANOS_PER_MICRO: u128 = 1_000;
pub const NANOS_PER_MILLI: u128 = 1_000_000;
pub const NANOS_PER_SEC: u128 = 1_000_000_000;
pub const NANOS_PER_MIN: u128 = 60 * NANOS_PER_SEC;
pub const NANOS_PER_HOUR: u128 = 60 * NANOS_PER_MIN;
pub const NANOS_PER_DAY: u128 = 24 * NANOS_PER_HOUR;
pub const NANOS_PER_WEEK: u128 = 7 * NANOS_PER_DAY;

/// Duration represents the core duration abstraction for Kavod.
/// Stored as a `u128` count of nanoseconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Duration(u128);

impl Duration {
    // Helper constants
    pub const ZERO: Self = Duration(0);
    pub const NANOSECOND: Self = Duration(1);
    pub const MICROSECOND: Self = Duration(NANOS_PER_MICRO);
    pub const MILLISECOND: Self = Duration(NANOS_PER_MILLI);
    pub const SECOND: Self = Duration(NANOS_PER_SEC);
    pub const MINUTE: Self = Duration(NANOS_PER_MIN);
    pub const HOUR: Self = Duration(NANOS_PER_HOUR);
    pub const DAY: Self = Duration(NANOS_PER_DAY);
    pub const WEEK: Self = Duration(NANOS_PER_WEEK);

    /// Creates a `Duration` from a number of nanoseconds.
    pub const fn from_nanos(val: u128) -> Self {
        Duration(val)
    }

    /// Creates a `Duration` from a number of whole seconds.
    ///
    /// Returns `Err(TimeError::Overflow)` if the nanosecond equivalent
    /// would overflow `u128`.
    pub const fn from_seconds(val: u128) -> Result<Self, TimeError> {
        match val.checked_mul(NANOS_PER_SEC) {
            Some(nanos) => Ok(Duration(nanos)),
            None => Err(TimeError::Overflow),
        }
    }

    /// Returns the total number of nanoseconds.
    pub const fn as_nanos(&self) -> u128 {
        self.0
    }

    /// Returns the number of whole seconds, truncating subsecond nanos.
    pub const fn as_secs(&self) -> u128 {
        self.0 / NANOS_PER_SEC
    }

    /// Returns `true` if this duration is zero.
    pub const fn is_zero(&self) -> bool {
        self.0 == 0
    }

    /// Adds two durations, returning a new `Duration`.
    ///
    /// Returns `Err(TimeError::Overflow)` if the result would overflow `u128`.
    pub const fn checked_add(&self, rhs: Self) -> Result<Self, TimeError> {
        match self.0.checked_add(rhs.0) {
            Some(nanos) => Ok(Duration(nanos)),
            None => Err(TimeError::Overflow),
        }
    }

    /// Subtracts `rhs` from this duration, returning a new `Duration`.
    ///
    /// Returns `Err(TimeError::Underflow)` if `rhs` is greater than `self`.
    pub const fn checked_sub(&self, rhs: Self) -> Result<Self, TimeError> {
        match self.0.checked_sub(rhs.0) {
            Some(nanos) => Ok(Duration(nanos)),
            None => Err(TimeError::Underflow),
        }
    }

    /// Returns this duration multiplied by `rhs`.
    ///
    /// Returns `Err(TimeError::Overflow)` if the result would overflow `u128`.
    pub const fn checked_mul(&self, rhs: u128) -> Result<Self, TimeError> {
        match self.0.checked_mul(rhs) {
            Some(nanos) => Ok(Duration(nanos)),
            None => Err(TimeError::Overflow),
        }
    }
}

impl Default for Duration {
    fn default() -> Self {
        Self::ZERO
    }
}

#[cfg(test)]
mod tests {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    use super::*;

    // ==================================================================
    // Constants
    // ==================================================================

    /// Invariant: ZERO is the only constant that is_zero
    #[test]
    fn test_constant_zero_is_only_zero() {
        assert!(Duration::ZERO.is_zero());
        assert!(!Duration::NANOSECOND.is_zero());
        assert!(!Duration::MICROSECOND.is_zero());
        assert!(!Duration::MILLISECOND.is_zero());
        assert!(!Duration::SECOND.is_zero());
        assert!(!Duration::MINUTE.is_zero());
        assert!(!Duration::HOUR.is_zero());
        assert!(!Duration::DAY.is_zero());
        assert!(!Duration::WEEK.is_zero());
    }

    /// Invariant: all named constants are strictly ordered by magnitude
    #[test]
    fn test_constants_ascending_order() {
        let constants = vec![
            Duration::ZERO,
            Duration::NANOSECOND,
            Duration::MICROSECOND,
            Duration::MILLISECOND,
            Duration::SECOND,
            Duration::MINUTE,
            Duration::HOUR,
            Duration::DAY,
            Duration::WEEK,
        ];
        for i in 1..constants.len() {
            assert!(
                constants[i - 1] < constants[i],
                "{} >= {}",
                constants[i - 1].as_nanos(),
                constants[i].as_nanos(),
            );
        }
    }

    /// Invariant: SECOND * 60 == MINUTE
    #[test]
    fn test_constant_minute_relation() {
        assert_eq!(Duration::MINUTE, Duration::SECOND.checked_mul(60).unwrap());
    }

    /// Invariant: MINUTE * 60 == HOUR
    #[test]
    fn test_constant_hour_relation() {
        assert_eq!(Duration::HOUR, Duration::MINUTE.checked_mul(60).unwrap());
    }

    /// Invariant: HOUR * 24 == DAY
    #[test]
    fn test_constant_day_relation() {
        assert_eq!(Duration::DAY, Duration::HOUR.checked_mul(24).unwrap());
    }

    /// Invariant: DAY * 7 == WEEK
    #[test]
    fn test_constant_week_relation() {
        assert_eq!(Duration::WEEK, Duration::DAY.checked_mul(7).unwrap());
    }

    /// Invariant: ZERO has a nanosecond count of 0
    #[test]
    fn test_constant_zero_nanos() {
        assert_eq!(Duration::ZERO.as_nanos(), 0);
    }

    /// Invariant: NANOSECOND has a nanosecond count of 1
    #[test]
    fn test_constant_nanosecond_nanos() {
        assert_eq!(Duration::NANOSECOND.as_nanos(), 1);
    }

    /// Invariant: SECOND has a nanosecond count of NANOS_PER_SEC (anchors
    ///             the relation chain)
    #[test]
    fn test_constant_second_nanos() {
        assert_eq!(Duration::SECOND.as_nanos(), NANOS_PER_SEC);
    }

    // ==================================================================
    // Default
    // ==================================================================

    /// Invariant: Default::default() equals ZERO
    #[test]
    fn test_default_equals_zero() {
        assert_eq!(Duration::default(), Duration::ZERO);
    }

    // ==================================================================
    // Construction — from_nanos
    // ==================================================================

    /// Invariant: from_nanos(0) equals ZERO
    #[test]
    fn test_from_nanos_zero() {
        assert_eq!(Duration::from_nanos(0), Duration::ZERO);
    }

    /// Invariant: from_nanos with a small value stores and retrieves it
    #[test]
    fn test_from_nanos_small() {
        let dur = Duration::from_nanos(42);
        assert_eq!(dur.as_nanos(), 42);
        assert_eq!(dur.as_secs(), 0);
    }

    /// Invariant: from_nanos with exactly one second reports 1 second
    #[test]
    fn test_from_nanos_one_second() {
        let dur = Duration::from_nanos(NANOS_PER_SEC);
        assert_eq!(dur.as_nanos(), NANOS_PER_SEC);
        assert_eq!(dur.as_secs(), 1);
    }

    /// Invariant: from_nanos with seconds and subsec nanos correctly
    ///             reports both components
    #[test]
    fn test_from_nanos_seconds_and_subsec() {
        let dur = Duration::from_nanos(5 * NANOS_PER_SEC + 500_000_000);
        assert_eq!(dur.as_secs(), 5);
        assert_eq!(dur.as_nanos() - (5 * NANOS_PER_SEC), 500_000_000);
    }

    /// Invariant: from_nanos with a large value roundtrips
    #[test]
    fn test_from_nanos_large() {
        let val: u128 = 1_000_000_000_000_000_000;
        let dur = Duration::from_nanos(val);
        assert_eq!(dur.as_nanos(), val);
    }

    /// Invariant: from_nanos with u128::MAX roundtrips
    #[test]
    fn test_from_nanos_u128_max() {
        let dur = Duration::from_nanos(u128::MAX);
        assert_eq!(dur.as_nanos(), u128::MAX);
    }

    /// Invariant: from_nanos with a sub-second-only value has zero seconds
    #[test]
    fn test_from_nanos_sub_second() {
        let dur = Duration::from_nanos(999_999_999);
        assert_eq!(dur.as_nanos(), 999_999_999);
        assert_eq!(dur.as_secs(), 0);
    }

    /// Invariant: from_nanos(0).is_zero() is true
    #[test]
    fn test_from_nanos_zero_is_zero() {
        assert!(Duration::from_nanos(0).is_zero());
    }

    // ==================================================================
    // Construction — from_seconds
    // ==================================================================

    /// Invariant: from_seconds(0) returns Ok(ZERO)
    #[test]
    fn test_from_seconds_zero() {
        let dur = Duration::from_seconds(0).unwrap();
        assert_eq!(dur, Duration::ZERO);
    }

    /// Invariant: from_seconds(1) equals SECOND
    #[test]
    fn test_from_seconds_one() {
        let dur = Duration::from_seconds(1).unwrap();
        assert_eq!(dur, Duration::SECOND);
    }

    /// Invariant: from_seconds with a large value roundtrips through
    ///             as_secs()
    #[test]
    fn test_from_seconds_large() {
        let secs: u128 = 1_000_000_000;
        let dur = Duration::from_seconds(secs).unwrap();
        assert_eq!(dur.as_secs(), secs);
    }

    /// Invariant: from_seconds(u128::MAX) returns Overflow because the
    ///             multiplication by 1e9 exceeds u128 range
    #[test]
    fn test_from_seconds_u128_max_overflows() {
        let result = Duration::from_seconds(u128::MAX);
        assert!(result.is_err());
    }

    /// Invariant: from_seconds at the maximum valid value succeeds
    #[test]
    fn test_from_seconds_max_valid() {
        let max_secs = u128::MAX / NANOS_PER_SEC;
        let dur = Duration::from_seconds(max_secs).unwrap();
        assert_eq!(dur.as_secs(), max_secs);
    }

    /// Invariant: from_seconds just beyond the valid boundary returns
    ///             Overflow
    #[test]
    fn test_from_seconds_just_beyond_valid() {
        let max_secs = u128::MAX / NANOS_PER_SEC;
        let result = Duration::from_seconds(max_secs + 1);
        assert!(result.is_err());
    }

    // ==================================================================
    // Accessors — as_nanos
    // ==================================================================

    /// Invariant: from_nanos(val).as_nanos() == val for a range of values
    #[test]
    fn test_as_nanos_roundtrip() {
        for val in [
            0u128,
            1,
            NANOS_PER_SEC,
            5 * NANOS_PER_SEC + 500_000_000,
            999_999_999,
            1_000_000_000_000_000_000,
            u128::MAX,
        ] {
            let dur = Duration::from_nanos(val);
            assert_eq!(dur.as_nanos(), val, "as_nanos roundtrip failed for {val}");
        }
    }

    /// Invariant: from_seconds(val).unwrap().as_nanos() == val * 1e9
    #[test]
    fn test_from_seconds_as_nanos_roundtrip() {
        for secs in [0u128, 1, 42, 1_000, 1_000_000] {
            let dur = Duration::from_seconds(secs).unwrap();
            assert_eq!(
                dur.as_nanos(),
                secs * NANOS_PER_SEC,
                "from_seconds roundtrip failed for {secs}"
            );
        }
    }

    // ==================================================================
    // Accessors — as_secs
    // ==================================================================

    /// Invariant: ZERO.as_secs() returns 0
    #[test]
    fn test_as_secs_zero() {
        assert_eq!(Duration::ZERO.as_secs(), 0);
    }

    /// Invariant: as_secs truncates sub-second nanos (floor division)
    #[test]
    fn test_as_secs_truncates_subsec() {
        let dur = Duration::from_nanos(5 * NANOS_PER_SEC + 750_000_000);
        assert_eq!(dur.as_secs(), 5);
    }

    /// Invariant: as_secs returns 0 for sub-second-only durations
    #[test]
    fn test_as_secs_sub_second_only() {
        let dur = Duration::from_nanos(500_000_000);
        assert_eq!(dur.as_secs(), 0);
    }

    /// Invariant: as_secs returns 0 for 999,999,999 nanoseconds
    #[test]
    fn test_as_secs_edge_below_one_second() {
        let dur = Duration::from_nanos(999_999_999);
        assert_eq!(dur.as_secs(), 0);
    }

    /// Invariant: as_secs returns 1 for exactly 1,000,000,000 nanoseconds
    #[test]
    fn test_as_secs_exactly_one_second() {
        let dur = Duration::from_nanos(NANOS_PER_SEC);
        assert_eq!(dur.as_secs(), 1);
    }

    /// Invariant: as_secs for a large value reports the correct number of
    ///             whole seconds
    #[test]
    fn test_as_secs_large_value() {
        let dur = Duration::from_nanos(1_000_000_000 * NANOS_PER_SEC + 500_000_000);
        assert_eq!(dur.as_secs(), 1_000_000_000);
    }

    /// Invariant: as_secs at u128::MAX does not overflow
    #[test]
    fn test_as_secs_u128_max() {
        let dur = Duration::from_nanos(u128::MAX);
        let expected = u128::MAX / NANOS_PER_SEC;
        assert_eq!(dur.as_secs(), expected);
    }

    // ==================================================================
    // is_zero
    // ==================================================================

    /// Invariant: ZERO.is_zero() returns true
    #[test]
    fn test_is_zero_on_zero() {
        assert!(Duration::ZERO.is_zero());
    }

    /// Invariant: is_zero returns false for a small positive duration
    #[test]
    fn test_is_zero_on_small() {
        assert!(!Duration::from_nanos(1).is_zero());
    }

    /// Invariant: is_zero returns false for a large duration
    #[test]
    fn test_is_zero_on_large() {
        assert!(!Duration::from_seconds(100).unwrap().is_zero());
    }

    // ==================================================================
    // checked_mul
    // ==================================================================

    /// Invariant: multiplying any duration by 0 returns ZERO
    #[test]
    fn test_checked_mul_by_zero() {
        for dur in [Duration::NANOSECOND, Duration::SECOND, Duration::DAY] {
            let result = dur.checked_mul(0).unwrap();
            assert_eq!(result, Duration::ZERO);
        }
    }

    /// Invariant: multiplying any duration by 1 returns the same duration
    #[test]
    fn test_checked_mul_by_one() {
        for dur in [
            Duration::ZERO,
            Duration::NANOSECOND,
            Duration::SECOND,
            Duration::WEEK,
        ] {
            let result = dur.checked_mul(1).unwrap();
            assert_eq!(result, dur);
        }
    }

    /// Invariant: checked_mul with a small factor produces the expected
    ///             nanosecond count
    #[test]
    fn test_checked_mul_basic() {
        let dur = Duration::SECOND.checked_mul(5).unwrap();
        assert_eq!(dur, Duration::from_seconds(5).unwrap());
        assert_eq!(dur.as_nanos(), 5 * NANOS_PER_SEC);
    }

    /// Invariant: checked_mul with a large factor produces the expected
    ///             value
    #[test]
    fn test_checked_mul_large() {
        let dur = Duration::MILLISECOND.checked_mul(3_600_000).unwrap();
        assert_eq!(dur, Duration::HOUR);
    }

    /// Invariant: checked_mul returns Overflow when the product exceeds
    ///             u128::MAX
    #[test]
    fn test_checked_mul_overflow() {
        let result = Duration::SECOND.checked_mul(u128::MAX);
        assert!(result.is_err());
    }

    /// Invariant: checked_mul on a value near u128::MAX overflows
    #[test]
    fn test_checked_mul_near_limit_overflow() {
        let dur = Duration::from_nanos(u128::MAX / 2 + 1);
        let result = dur.checked_mul(2);
        assert!(result.is_err());
    }

    /// Invariant: checked_mul on a value near u128::MAX with factor 2
    ///             succeeds when below the limit
    #[test]
    fn test_checked_mul_near_limit_succeeds() {
        let dur = Duration::from_nanos(u128::MAX / 2);
        let result = dur.checked_mul(2).unwrap();
        assert_eq!(result.as_nanos(), (u128::MAX / 2) * 2);
    }

    // ==================================================================
    // checked_add
    // ==================================================================

    /// Invariant: adding ZERO to any duration returns the same duration
    #[test]
    fn test_checked_add_zero_identity() {
        for dur in [Duration::ZERO, Duration::NANOSECOND, Duration::SECOND] {
            let result = dur.checked_add(Duration::ZERO).unwrap();
            assert_eq!(result, dur);
        }
    }

    /// Invariant: ZERO + dur == dur
    #[test]
    fn test_checked_add_zero_left_identity() {
        let dur = Duration::MINUTE;
        let result = Duration::ZERO.checked_add(dur).unwrap();
        assert_eq!(result, dur);
    }

    /// Invariant: adding two durations produces the expected sum
    #[test]
    fn test_checked_add_basic() {
        let a = Duration::from_nanos(100);
        let b = Duration::from_nanos(200);
        let result = a.checked_add(b).unwrap();
        assert_eq!(result.as_nanos(), 300);
    }

    /// Invariant: adding constants that fit within u128::MAX succeeds
    #[test]
    fn test_checked_add_constants() {
        let result = Duration::SECOND.checked_add(Duration::MINUTE).unwrap();
        assert_eq!(result.as_nanos(), NANOS_PER_SEC + NANOS_PER_MIN);
    }

    /// Invariant: checked_add at u128::MAX overflows
    #[test]
    fn test_checked_add_overflow() {
        let dur = Duration::from_nanos(u128::MAX);
        let result = dur.checked_add(Duration::NANOSECOND);
        assert!(result.is_err());
    }

    /// Invariant: add-then-sub roundtrip: (a + b) - b == a
    #[test]
    fn test_checked_add_sub_roundtrip() {
        for (a_ns, b_ns) in [(0u128, 50), (100, 30), (1000, 1), (1, 999)] {
            let a = Duration::from_nanos(a_ns);
            let b = Duration::from_nanos(b_ns);
            let sum = a.checked_add(b).unwrap();
            let result = sum.checked_sub(b).unwrap();
            assert_eq!(result, a, "add-sub roundtrip failed for a={a_ns}, b={b_ns}");
        }
    }

    /// Invariant: adding ZERO to u128::MAX succeeds (identity at
    ///             boundary)
    #[test]
    fn test_checked_add_zero_to_max() {
        let dur = Duration::from_nanos(u128::MAX);
        let result = dur.checked_add(Duration::ZERO).unwrap();
        assert_eq!(result, dur);
    }

    /// Invariant: checked_add is commutative: a + b == b + a
    #[test]
    fn test_checked_add_commutative() {
        for a_ns in [0u128, 1, 100, NANOS_PER_SEC, u128::MAX / 2] {
            for b_ns in [0u128, 1, 50, NANOS_PER_SEC] {
                let a = Duration::from_nanos(a_ns);
                let b = Duration::from_nanos(b_ns);
                let ab = a.checked_add(b);
                let ba = b.checked_add(a);
                assert_eq!(ab, ba, "commutativity failed for a={a_ns}, b={b_ns}");
            }
        }
    }

    // ==================================================================
    // checked_sub
    // ==================================================================

    /// Invariant: subtracting ZERO from any duration returns the same
    ///             duration
    #[test]
    fn test_checked_sub_zero_identity() {
        for dur in [Duration::ZERO, Duration::NANOSECOND, Duration::SECOND] {
            let result = dur.checked_sub(Duration::ZERO).unwrap();
            assert_eq!(result, dur);
        }
    }

    /// Invariant: subtracting a duration from itself returns ZERO
    #[test]
    fn test_checked_sub_self_yields_zero() {
        for dur in [Duration::ZERO, Duration::NANOSECOND, Duration::SECOND] {
            let result = dur.checked_sub(dur).unwrap();
            assert_eq!(result, Duration::ZERO);
        }
    }

    /// Invariant: subtracting a smaller duration from a larger one produces
    ///             the expected difference
    #[test]
    fn test_checked_sub_basic() {
        let a = Duration::from_nanos(500);
        let b = Duration::from_nanos(200);
        let result = a.checked_sub(b).unwrap();
        assert_eq!(result.as_nanos(), 300);
    }

    /// Invariant: subtracting constants produces the expected result
    #[test]
    fn test_checked_sub_constants() {
        let result = Duration::MINUTE.checked_sub(Duration::SECOND).unwrap();
        assert_eq!(result.as_nanos(), NANOS_PER_MIN - NANOS_PER_SEC);
    }

    /// Invariant: subtracting a larger duration from a smaller one
    ///             returns Underflow
    #[test]
    fn test_checked_sub_underflow() {
        let result = Duration::SECOND.checked_sub(Duration::MINUTE);
        assert!(result.is_err());
    }

    /// Invariant: sub-then-add roundtrip: (a - b) + b == a
    #[test]
    fn test_checked_sub_add_roundtrip() {
        for (a_ns, b_ns) in [(200u128, 30), (1000, 999), (50, 0)] {
            let a = Duration::from_nanos(a_ns);
            let b = Duration::from_nanos(b_ns);
            let diff = a.checked_sub(b).unwrap();
            let result = diff.checked_add(b).unwrap();
            assert_eq!(result, a, "sub-add roundtrip failed for a={a_ns}, b={b_ns}");
        }
    }

    /// Invariant: subtracting from ZERO underflows (even by 1 nanosecond)
    #[test]
    fn test_checked_sub_zero_minus_nanosecond_underflows() {
        let result = Duration::ZERO.checked_sub(Duration::NANOSECOND);
        assert!(result.is_err());
    }

    /// Invariant: u128::MAX - u128::MAX == ZERO
    #[test]
    fn test_checked_sub_self_at_max_yields_zero() {
        let dur = Duration::from_nanos(u128::MAX);
        let result = dur.checked_sub(dur).unwrap();
        assert_eq!(result, Duration::ZERO);
    }

    /// Invariant: u128::MAX - ZERO == u128::MAX (identity at positive
    ///             boundary)
    #[test]
    fn test_checked_sub_zero_from_max() {
        let dur = Duration::from_nanos(u128::MAX);
        let result = dur.checked_sub(Duration::ZERO).unwrap();
        assert_eq!(result, dur);
    }

    // ==================================================================
    // Ord / PartialOrd
    // ==================================================================

    /// Invariant: smaller nanosecond count is less than larger
    #[test]
    fn test_ord_by_nanos() {
        let a = Duration::from_nanos(100);
        let b = Duration::from_nanos(200);
        assert!(a < b);
        assert!(b > a);
        assert!(a <= b);
        assert!(b >= a);
    }

    /// Invariant: cmp returns Equal for identical durations
    #[test]
    fn test_ord_equal_cmp() {
        let a = Duration::from_nanos(100);
        let b = Duration::from_nanos(100);
        assert_eq!(a.cmp(&b), std::cmp::Ordering::Equal);
    }

    /// Invariant: Ord is transitive
    #[test]
    fn test_ord_transitive() {
        let a = Duration::from_nanos(10);
        let b = Duration::from_nanos(20);
        let c = Duration::from_nanos(30);
        assert!(a < b && b < c && a < c);
    }

    // ==================================================================
    // Hash
    // ==================================================================

    /// Invariant: two equal durations produce the same hash
    #[test]
    fn test_hash_consistency() {
        let a = Duration::from_nanos(42);
        let b = Duration::from_nanos(42);
        let mut ha = DefaultHasher::new();
        let mut hb = DefaultHasher::new();
        a.hash(&mut ha);
        b.hash(&mut hb);
        assert_eq!(ha.finish(), hb.finish());
    }

    // ==================================================================
    // Eq / PartialEq
    // ==================================================================

    /// Invariant: two durations with the same nanosecond count are equal
    #[test]
    fn test_eq_same_value() {
        let a = Duration::from_nanos(42);
        let b = Duration::from_nanos(42);
        assert_eq!(a, b);
    }

    /// Invariant: two durations with different nanosecond counts are not
    ///             equal
    #[test]
    fn test_eq_different_value() {
        let a = Duration::from_nanos(1);
        let b = Duration::from_nanos(2);
        assert_ne!(a, b);
    }

    // ==================================================================
    // Debug
    // ==================================================================

    /// Invariant: Debug output is non-empty
    #[test]
    fn test_debug_nonempty() {
        let dur = Duration::from_nanos(100);
        assert!(!format!("{dur:?}").is_empty());
    }

    /// Invariant: Debug output changes with the nanosecond count
    #[test]
    fn test_debug_distinguishes_values() {
        let a = Duration::from_nanos(0);
        let b = Duration::from_nanos(1);
        assert_ne!(format!("{a:?}"), format!("{b:?}"));
    }
}
