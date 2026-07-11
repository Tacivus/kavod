use std::{fmt, time::SystemTime};

use crate::time::{TimeError, duration::Duration};

/// Timestamp represents the core time abstraction for Kavod.
/// Stored as an `i128` count of nanoseconds since the Unix epoch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(i128);

impl Timestamp {
    pub fn new(ts: i128) -> Self {
        Timestamp(ts)
    }

    pub fn raw(&self) -> i128 {
        self.0
    }

    /// Adds a `Duration`, returning a new `Timestamp`.
    ///
    /// Returns `Err(TimeError::Overflow)` if the addition would overflow
    /// `i128` or if the `Duration`'s nanosecond count exceeds `i128::MAX`.
    pub fn checked_add(&self, dur: Duration) -> Result<Self, TimeError> {
        let dur_nanos: i128 = dur.as_nanos().try_into().map_err(|_| TimeError::Overflow)?;
        self.0
            .checked_add(dur_nanos)
            .map(Timestamp)
            .ok_or(TimeError::Overflow)
    }

    /// Subtracts a `Duration`, returning a new `Timestamp`.
    ///
    /// Returns `Err(TimeError::Underflow)` if the subtraction would
    /// underflow `i128` or if the `Duration` exceeds `i128::MAX`.
    pub fn checked_sub(&self, dur: Duration) -> Result<Self, TimeError> {
        let dur_nanos: i128 = dur
            .as_nanos()
            .try_into()
            .map_err(|_| TimeError::Underflow)?;
        self.0
            .checked_sub(dur_nanos)
            .map(Timestamp)
            .ok_or(TimeError::Underflow)
    }
}

impl From<SystemTime> for Timestamp {
    fn from(value: SystemTime) -> Self {
        let nanos: i128 = match value.duration_since(std::time::UNIX_EPOCH) {
            Ok(dur) => dur.as_nanos() as i128,
            Err(e) => -(e.duration().as_nanos() as i128),
        };
        Timestamp(nanos)
    }
}

impl TryFrom<Timestamp> for SystemTime {
    type Error = TimeError;

    fn try_from(value: Timestamp) -> Result<Self, Self::Error> {
        let nanos = value.raw();
        if nanos >= 0 {
            let dur = std::time::Duration::from_nanos(
                nanos.try_into().map_err(|_| TimeError::OutOfRange)?,
            );
            std::time::UNIX_EPOCH
                .checked_add(dur)
                .ok_or(TimeError::Overflow)
        } else {
            let pos_val = nanos.checked_neg().ok_or(TimeError::OutOfRange)?;
            let pos: u64 = pos_val.try_into().map_err(|_| TimeError::OutOfRange)?;
            std::time::UNIX_EPOCH
                .checked_sub(std::time::Duration::from_nanos(pos))
                .ok_or(TimeError::Underflow)
        }
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}ns", self.0)
    }
}

#[cfg(test)]
mod tests {
    use std::{
        hash::{DefaultHasher, Hash, Hasher},
        time::SystemTime,
    };

    use super::*;

    // ==================================================================
    // Construction — new / raw roundtrip
    // ==================================================================

    /// Invariant: new(t).raw() == t for positive values
    #[test]
    fn test_new_raw_roundtrip_positive() {
        for t in [0i128, 1, 1_000_000_000, i128::MAX] {
            let ts = Timestamp::new(t);
            assert_eq!(ts.raw(), t, "roundtrip failed for {t}");
        }
    }

    /// Invariant: new(t).raw() == t for negative values
    #[test]
    fn test_new_raw_roundtrip_negative() {
        for t in [-1i128, -1_000_000_000, i128::MIN] {
            let ts = Timestamp::new(t);
            assert_eq!(ts.raw(), t, "roundtrip failed for {t}");
        }
    }

    // ==================================================================
    // checked_add
    // ==================================================================

    /// Invariant: adding ZERO to any timestamp returns the same timestamp
    #[test]
    fn test_checked_add_zero_identity() {
        for t in [0i128, -50, 100] {
            let ts = Timestamp::new(t);
            let result = ts.checked_add(Duration::ZERO).unwrap();
            assert_eq!(result, ts);
        }
    }

    /// Invariant: adding a positive Duration increases raw()
    #[test]
    fn test_checked_add_positive_from_epoch() {
        let ts = Timestamp::new(0);
        let result = ts.checked_add(Duration::from_nanos(50)).unwrap();
        assert_eq!(result.raw(), 50);
    }

    /// Invariant: adding a positive Duration to a negative timestamp
    ///             moves it toward zero
    #[test]
    fn test_checked_add_to_negative() {
        let ts = Timestamp::new(-100);
        let result = ts.checked_add(Duration::from_nanos(30)).unwrap();
        assert_eq!(result.raw(), -70);
    }

    /// Invariant: adding a Duration that crosses the epoch produces a
    ///             positive timestamp
    #[test]
    fn test_checked_add_crosses_epoch() {
        let ts = Timestamp::new(-50);
        let result = ts.checked_add(Duration::from_nanos(100)).unwrap();
        assert_eq!(result.raw(), 50);
    }

    /// Invariant: adding a Duration that lands exactly on the epoch
    #[test]
    fn test_checked_add_exactly_to_epoch() {
        let ts = Timestamp::new(-100);
        let result = ts.checked_add(Duration::from_nanos(100)).unwrap();
        assert_eq!(result.raw(), 0);
    }

    /// Invariant: adding a large Duration (using SECOND constant) works
    #[test]
    fn test_checked_add_seconds() {
        let ts = Timestamp::new(1_000_000_000);
        let result = ts.checked_add(Duration::SECOND).unwrap();
        assert_eq!(result.raw(), 2_000_000_000);
    }

    /// Invariant: adding 1 nanosecond to i128::MAX returns Overflow
    #[test]
    fn test_checked_add_overflow_at_i128_max() {
        let ts = Timestamp::new(i128::MAX);
        let result = ts.checked_add(Duration::NANOSECOND);
        assert!(result.is_err());
    }

    /// Invariant: adding a Duration whose nanosecond count exceeds
    ///             i128::MAX returns Overflow (conversion fails)
    #[test]
    fn test_checked_add_duration_too_large_for_i128() {
        let huge = Duration::from_nanos(i128::MAX as u128 + 1);
        let ts = Timestamp::new(0);
        let result = ts.checked_add(huge);
        assert!(result.is_err());
    }

    /// Invariant: (ts + dur) - dur == ts (add-then-sub roundtrip)
    #[test]
    fn test_checked_add_sub_roundtrip() {
        for (ts_val, dur_ns) in [
            (0i128, 0u128),
            (0, 50),
            (100, 30),
            (-50, 20),
            (1_000_000_000, 500_000_000),
        ] {
            let ts = Timestamp::new(ts_val);
            let dur = Duration::from_nanos(dur_ns);
            let sum = ts.checked_add(dur).unwrap();
            let result = sum.checked_sub(dur).unwrap();
            assert_eq!(
                result, ts,
                "add-sub roundtrip failed for ts={ts_val}, dur={dur_ns}"
            );
        }
    }

    /// Invariant: checked_add with a Duration exactly at the i128::MAX
    ///             boundary succeeds when the timestamp is 0
    #[test]
    fn test_checked_add_duration_exact_i128_max() {
        let dur = Duration::from_nanos(i128::MAX as u128);
        let ts = Timestamp::new(0);
        let result = ts.checked_add(dur).unwrap();
        assert_eq!(result.raw(), i128::MAX);
    }

    /// Invariant: adding ZERO to i128::MIN succeeds (identity at negative
    ///             boundary)
    #[test]
    fn test_checked_add_zero_to_min() {
        let ts = Timestamp::new(i128::MIN);
        let result = ts.checked_add(Duration::ZERO).unwrap();
        assert_eq!(result, ts);
    }

    /// Invariant: i128::MAX + ZERO == i128::MAX (identity at positive
    ///             boundary)
    #[test]
    fn test_checked_add_zero_to_max() {
        let ts = Timestamp::new(i128::MAX);
        let result = ts.checked_add(Duration::ZERO).unwrap();
        assert_eq!(result, ts);
    }

    // ==================================================================
    // checked_sub
    // ==================================================================

    /// Invariant: subtracting ZERO from any timestamp returns the same
    ///             timestamp
    #[test]
    fn test_checked_sub_zero_identity() {
        for t in [0i128, -50, 100] {
            let ts = Timestamp::new(t);
            let result = ts.checked_sub(Duration::ZERO).unwrap();
            assert_eq!(result, ts);
        }
    }

    /// Invariant: subtracting a positive Duration decreases raw()
    #[test]
    fn test_checked_sub_positive_from_positive() {
        let ts = Timestamp::new(100);
        let result = ts.checked_sub(Duration::from_nanos(30)).unwrap();
        assert_eq!(result.raw(), 70);
    }

    /// Invariant: subtracting a Duration that crosses the epoch produces
    ///             a negative timestamp
    #[test]
    fn test_checked_sub_crosses_epoch() {
        let ts = Timestamp::new(50);
        let result = ts.checked_sub(Duration::from_nanos(100)).unwrap();
        assert_eq!(result.raw(), -50);
    }

    /// Invariant: subtracting a Duration that lands exactly on the epoch
    #[test]
    fn test_checked_sub_exactly_to_epoch() {
        let ts = Timestamp::new(100);
        let result = ts.checked_sub(Duration::from_nanos(100)).unwrap();
        assert_eq!(result.raw(), 0);
    }

    /// Invariant: subtracting a Duration from a negative timestamp makes
    ///             it more negative
    #[test]
    fn test_checked_sub_from_negative() {
        let ts = Timestamp::new(-50);
        let result = ts.checked_sub(Duration::from_nanos(50)).unwrap();
        assert_eq!(result.raw(), -100);
    }

    /// Invariant: subtracting a large Duration (using MINUTE constant)
    ///             works
    #[test]
    fn test_checked_sub_large() {
        let ts = Timestamp::new(120_000_000_000);
        let result = ts.checked_sub(Duration::MINUTE).unwrap();
        assert_eq!(result.raw(), 60_000_000_000);
    }

    /// Invariant: subtracting 1 nanosecond from i128::MIN returns
    ///             Underflow
    #[test]
    fn test_checked_sub_underflow_at_i128_min() {
        let ts = Timestamp::new(i128::MIN);
        let result = ts.checked_sub(Duration::NANOSECOND);
        assert!(result.is_err());
    }

    /// Invariant: subtracting a Duration whose nanosecond count exceeds
    ///             i128::MAX returns Underflow (conversion fails)
    #[test]
    fn test_checked_sub_duration_too_large_for_i128() {
        let huge = Duration::from_nanos(i128::MAX as u128 + 1);
        let ts = Timestamp::new(0);
        let result = ts.checked_sub(huge);
        assert!(result.is_err());
    }

    /// Invariant: (ts - dur) + dur == ts (sub-then-add roundtrip)
    #[test]
    fn test_checked_sub_add_roundtrip() {
        for (ts_val, dur_ns) in [(200i128, 30u128), (1_000, 999), (-10, 50)] {
            let ts = Timestamp::new(ts_val);
            let dur = Duration::from_nanos(dur_ns);
            let diff = ts.checked_sub(dur).unwrap();
            let result = diff.checked_add(dur).unwrap();
            assert_eq!(
                result, ts,
                "sub-add roundtrip failed for ts={ts_val}, dur={dur_ns}"
            );
        }
    }

    /// Invariant: subtracting ZERO from i128::MAX succeeds (identity at
    ///             positive boundary)
    #[test]
    fn test_checked_sub_zero_from_max() {
        let ts = Timestamp::new(i128::MAX);
        let result = ts.checked_sub(Duration::ZERO).unwrap();
        assert_eq!(result, ts);
    }

    /// Invariant: i128::MIN - ZERO == i128::MIN (identity at negative
    ///             boundary)
    #[test]
    fn test_checked_sub_zero_from_min() {
        let ts = Timestamp::new(i128::MIN);
        let result = ts.checked_sub(Duration::ZERO).unwrap();
        assert_eq!(result, ts);
    }

    // ==================================================================
    // From<SystemTime>
    // ==================================================================

    /// Invariant: SystemTime::now() converts to a positive Timestamp
    #[test]
    fn test_from_system_time_now_positive() {
        let ts = Timestamp::from(SystemTime::now());
        assert!(ts.raw() > 0);
    }

    /// Invariant: two consecutive SystemTime conversions are monotonic
    #[test]
    fn test_from_system_time_monotonic() {
        let a = Timestamp::from(SystemTime::now());
        let b = Timestamp::from(SystemTime::now());
        assert!(b.raw() >= a.raw());
    }

    /// Invariant: UNIX_EPOCH converts to raw() == 0
    #[test]
    fn test_from_system_time_unix_epoch() {
        let ts = Timestamp::from(std::time::UNIX_EPOCH);
        assert_eq!(ts.raw(), 0);
    }

    // ==================================================================
    // TryFrom<Timestamp> for SystemTime
    // ==================================================================

    /// Invariant: an epoch Timestamp converts to UNIX_EPOCH SystemTime
    #[test]
    fn test_try_into_system_time_epoch() {
        let ts = Timestamp::new(0);
        let st: SystemTime = ts.try_into().unwrap();
        let dur = st.duration_since(std::time::UNIX_EPOCH).unwrap();
        assert_eq!(dur.as_nanos(), 0);
    }

    /// Invariant: a positive Timestamp converts to the correct SystemTime
    #[test]
    fn test_try_into_system_time_positive() {
        let ts = Timestamp::new(5_000_000_000);
        let st: SystemTime = ts.try_into().unwrap();
        let dur = st.duration_since(std::time::UNIX_EPOCH).unwrap();
        assert_eq!(dur.as_nanos(), 5_000_000_000);
    }

    /// Invariant: Timestamp → SystemTime → Timestamp roundtrip is identity
    #[test]
    fn test_system_time_roundtrip() {
        let ts = Timestamp::new(1_000_000_000_000);
        let st: SystemTime = ts.try_into().unwrap();
        let back = Timestamp::from(st);
        assert_eq!(back, ts);
    }

    /// Invariant: a pre-epoch Timestamp roundtrips through SystemTime
    #[test]
    fn test_try_into_system_time_pre_epoch() {
        let ts = Timestamp::new(-1);
        let st: SystemTime = ts.try_into().unwrap();
        let back = Timestamp::from(st);
        assert_eq!(back, ts);
    }

    /// Invariant: i128::MAX cannot convert to SystemTime (exceeds u64)
    #[test]
    fn test_try_into_system_time_positive_out_of_range() {
        let ts = Timestamp::new(i128::MAX);
        let result: Result<SystemTime, _> = ts.try_into();
        assert!(result.is_err());
    }

    /// Invariant: a negative timestamp whose magnitude exceeds u64::MAX
    ///             returns OutOfRange
    #[test]
    fn test_try_into_system_time_negative_out_of_range() {
        let ts = Timestamp::new(-(u64::MAX as i128) - 1);
        let result: Result<SystemTime, _> = ts.try_into();
        assert!(result.is_err());
    }

    /// Invariant: a significantly negative timestamp roundtrips through
    ///             SystemTime
    #[test]
    fn test_try_into_system_time_pre_epoch_significant() {
        let ts = Timestamp::new(-5_000_000_000);
        let st: SystemTime = ts.try_into().unwrap();
        let back = Timestamp::from(st);
        assert_eq!(back, ts);
    }

    /// Invariant: i128::MIN cannot convert to SystemTime (negation
    ///             overflows i128)
    #[test]
    fn test_try_into_system_time_i128_min() {
        let ts = Timestamp::new(i128::MIN);
        let result: Result<SystemTime, _> = ts.try_into();
        assert!(result.is_err());
    }

    /// Invariant: a pre-epoch SystemTime converts to a negative Timestamp
    ///             with the correct magnitude
    #[test]
    fn test_from_system_time_pre_epoch() {
        let pre_epoch_st = std::time::UNIX_EPOCH
            .checked_sub(std::time::Duration::from_nanos(5_000_000_000))
            .unwrap();
        let ts = Timestamp::from(pre_epoch_st);
        assert_eq!(ts.raw(), -5_000_000_000);
    }

    /// Invariant: a pre-epoch SystemTime roundtrips through Timestamp
    ///             and back
    #[test]
    fn test_from_system_time_pre_epoch_roundtrip() {
        let pre_epoch_st = std::time::UNIX_EPOCH
            .checked_sub(std::time::Duration::from_nanos(1_000))
            .unwrap();
        let ts = Timestamp::from(pre_epoch_st);
        let st: SystemTime = ts.try_into().unwrap();
        let back = Timestamp::from(st);
        assert_eq!(back, ts);
    }

    // ==================================================================
    // Display
    // ==================================================================

    /// Invariant: Display output is non-empty
    #[test]
    fn test_display_nonempty() {
        let ts = Timestamp::new(0);
        assert!(!format!("{ts}").is_empty());
    }

    /// Invariant: different timestamps produce different Display output
    #[test]
    fn test_display_distinguishes_values() {
        let a = Timestamp::new(0);
        let b = Timestamp::new(1);
        assert_ne!(format!("{a}"), format!("{b}"));
    }

    /// Invariant: Display is deterministic — same timestamp produces the
    ///             same string every time
    #[test]
    fn test_display_deterministic() {
        let ts = Timestamp::new(1_000_000_000);
        assert_eq!(format!("{ts}"), format!("{ts}"));
    }

    // ==================================================================
    // Ord / PartialOrd
    // ==================================================================

    /// Invariant: smaller timestamp is less than larger
    #[test]
    fn test_ord_positive() {
        let a = Timestamp::new(0);
        let b = Timestamp::new(100);
        assert!(a < b);
        assert!(b > a);
    }

    /// Invariant: negative timestamps are less than epoch, positive
    ///             are greater
    #[test]
    fn test_ord_crosses_epoch() {
        let a = Timestamp::new(-100);
        let b = Timestamp::new(0);
        let c = Timestamp::new(100);
        assert!(a < b);
        assert!(b < c);
        assert!(a < c);
    }

    /// Invariant: cmp returns Equal for identical timestamps
    #[test]
    fn test_ord_equal_cmp() {
        let a = Timestamp::new(42);
        let b = Timestamp::new(42);
        assert_eq!(a.cmp(&b), std::cmp::Ordering::Equal);
    }

    /// Invariant: Ord is transitive
    #[test]
    fn test_ord_transitive() {
        let a = Timestamp::new(-100);
        let b = Timestamp::new(0);
        let c = Timestamp::new(100);
        assert!(a < b && b < c && a < c);
    }

    /// Invariant: a more negative timestamp is less than a less negative one
    #[test]
    fn test_ord_negative() {
        let a = Timestamp::new(-200);
        let b = Timestamp::new(-50);
        assert!(a < b);
        assert!(b > a);
        assert_eq!(a.cmp(&b), std::cmp::Ordering::Less);
    }

    // ==================================================================
    // Hash
    // ==================================================================

    /// Invariant: two equal timestamps produce the same hash
    #[test]
    fn test_hash_consistency() {
        let a = Timestamp::new(42);
        let b = Timestamp::new(42);
        let mut ha = DefaultHasher::new();
        let mut hb = DefaultHasher::new();
        a.hash(&mut ha);
        b.hash(&mut hb);
        assert_eq!(ha.finish(), hb.finish());
    }

    // ==================================================================
    // Eq / PartialEq
    // ==================================================================

    /// Invariant: two timestamps with the same raw value are equal
    #[test]
    fn test_eq_same_value() {
        let a = Timestamp::new(42);
        let b = Timestamp::new(42);
        assert_eq!(a, b);
    }

    /// Invariant: two timestamps with different raw values are not equal
    #[test]
    fn test_eq_different_value() {
        assert_ne!(Timestamp::new(0), Timestamp::new(1));
        assert_ne!(Timestamp::new(-1), Timestamp::new(1));
    }

    // ==================================================================
    // Debug
    // ==================================================================

    /// Invariant: Debug output is non-empty
    #[test]
    fn test_debug_nonempty() {
        let ts = Timestamp::new(0);
        assert!(!format!("{ts:?}").is_empty());
    }

    /// Invariant: Debug output distinguishes different timestamps
    #[test]
    fn test_debug_distinguishes_values() {
        let a = Timestamp::new(0);
        let b = Timestamp::new(1);
        assert_ne!(format!("{a:?}"), format!("{b:?}"));
    }
}
