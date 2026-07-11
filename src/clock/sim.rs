use crate::{
    clock::Clock,
    time::{TimeError, duration::Duration, timestamp::Timestamp},
};

/// The main clock that is used in non-live contexts. It can be arbitrarily
/// created/set/advanced programatically.
pub struct SimClock(Timestamp);

impl SimClock {
    pub fn new(start: Timestamp) -> Self {
        SimClock(start)
    }

    pub fn set(&mut self, ts: Timestamp) {
        self.0 = ts
    }

    /// Advances the clock 1 nanosecond.
    ///
    /// Returns an error if adding a nanonsecond would overflow the clock.
    pub fn tick(&mut self) -> Result<(), TimeError> {
        self.0 = self.0.checked_add(Duration::NANOSECOND)?;
        Ok(())
    }

    /// Advances the clock `count` nanoseconds.
    ///
    /// Reutrns an error if adding `duration` would overflow the clock.
    pub fn advance(&mut self, duration: Duration) -> Result<(), TimeError> {
        self.0 = self.0.checked_add(duration)?;
        Ok(())
    }
}

impl Clock for SimClock {
    fn now(&self) -> Timestamp {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ==================================================================
    // Construction — new / now roundtrip
    // ==================================================================

    /// Invariant: new(t).now().raw() == t for positive values
    #[test]
    fn test_new_now_roundtrip_positive() {
        for t in [0i128, 1, 1_000_000_000, i128::MAX] {
            let clock = SimClock::new(Timestamp::new(t));
            assert_eq!(clock.now().raw(), t, "roundtrip failed for {t}");
        }
    }

    /// Invariant: new(t).now().raw() == t for negative values
    #[test]
    fn test_new_now_roundtrip_negative() {
        for t in [-1i128, -1_000_000_000, i128::MIN] {
            let clock = SimClock::new(Timestamp::new(t));
            assert_eq!(clock.now().raw(), t, "roundtrip failed for {t}");
        }
    }

    /// Invariant: new(epoch).now() == epoch
    #[test]
    fn test_new_now_epoch() {
        let clock = SimClock::new(Timestamp::new(0));
        assert_eq!(clock.now().raw(), 0);
    }

    // ==================================================================
    // set
    // ==================================================================

    /// Invariant: set(t).now().raw() == t for a range of timestamps
    #[test]
    fn test_set_then_now() {
        for t in [0i128, -50, 100, i128::MAX, i128::MIN] {
            let mut clock = SimClock::new(Timestamp::new(0));
            clock.set(Timestamp::new(t));
            assert_eq!(clock.now().raw(), t, "set failed for {t}");
        }
    }

    /// Invariant: set to the same value is idempotent
    #[test]
    fn test_set_idempotent() {
        let mut clock = SimClock::new(Timestamp::new(42));
        clock.set(Timestamp::new(99));
        clock.set(Timestamp::new(99));
        assert_eq!(clock.now().raw(), 99);
    }

    /// Invariant: set overwrites previous set
    #[test]
    fn test_set_overwrites() {
        let mut clock = SimClock::new(Timestamp::new(0));
        clock.set(Timestamp::new(100));
        clock.set(Timestamp::new(-50));
        clock.set(Timestamp::new(7));
        assert_eq!(clock.now().raw(), 7);
    }

    // ==================================================================
    // tick
    // ==================================================================

    /// Invariant: tick advances now() by exactly 1 nanosecond
    #[test]
    fn test_tick_advances_one_nanosecond() {
        let mut clock = SimClock::new(Timestamp::new(0));
        clock.tick().unwrap();
        assert_eq!(clock.now().raw(), 1);
    }

    /// Invariant: tick from a non-zero positive timestamp
    #[test]
    fn test_tick_from_positive() {
        let mut clock = SimClock::new(Timestamp::new(1_000_000_000));
        clock.tick().unwrap();
        assert_eq!(clock.now().raw(), 1_000_000_001);
    }

    /// Invariant: tick from a negative timestamp advances toward zero
    #[test]
    fn test_tick_from_negative() {
        let mut clock = SimClock::new(Timestamp::new(-100));
        clock.tick().unwrap();
        assert_eq!(clock.now().raw(), -99);
    }

    /// Invariant: tick multiple times cumulatively advances the clock
    #[test]
    fn test_tick_multiple_cumulative() {
        let mut clock = SimClock::new(Timestamp::new(0));
        for expected in 1i128..=5 {
            clock.tick().unwrap();
            assert_eq!(clock.now().raw(), expected);
        }
    }

    /// Invariant: tick across the epoch (negative → 0 → positive)
    #[test]
    fn test_tick_crosses_epoch() {
        let mut clock = SimClock::new(Timestamp::new(-2));
        clock.tick().unwrap();
        assert_eq!(clock.now().raw(), -1);
        clock.tick().unwrap();
        assert_eq!(clock.now().raw(), 0);
        clock.tick().unwrap();
        assert_eq!(clock.now().raw(), 1);
    }

    /// Invariant: tick at i128::MAX returns Overflow
    #[test]
    fn test_tick_overflow_at_i128_max() {
        let mut clock = SimClock::new(Timestamp::new(i128::MAX));
        let result = clock.tick();
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), TimeError::Overflow);
    }

    /// Invariant: tick at i128::MAX - 1 succeeds; then tick overflows
    #[test]
    fn test_tick_one_before_max_then_overflow() {
        let mut clock = SimClock::new(Timestamp::new(i128::MAX - 1));
        clock.tick().unwrap();
        assert_eq!(clock.now().raw(), i128::MAX);
        let result = clock.tick();
        assert!(result.is_err());
    }

    /// Invariant: after a failed tick, the clock is unchanged
    #[test]
    fn test_tick_overflow_leaves_clock_unchanged() {
        let mut clock = SimClock::new(Timestamp::new(i128::MAX));
        let _ = clock.tick();
        assert_eq!(clock.now().raw(), i128::MAX);
    }

    /// Invariant: tick at i128::MIN succeeds (no underflow — moves toward zero)
    #[test]
    fn test_tick_from_i128_min() {
        let mut clock = SimClock::new(Timestamp::new(i128::MIN));
        clock.tick().unwrap();
        assert_eq!(clock.now().raw(), i128::MIN + 1);
    }

    // ==================================================================
    // advance
    // ==================================================================

    /// Invariant: advance(ZERO) leaves the clock unchanged
    #[test]
    fn test_advance_zero_identity() {
        for t in [0i128, -50, 100, i128::MAX] {
            let mut clock = SimClock::new(Timestamp::new(t));
            clock.advance(Duration::ZERO).unwrap();
            assert_eq!(clock.now().raw(), t);
        }
    }

    /// Invariant: advance(NANOSECOND) behaves identically to tick
    #[test]
    fn test_advance_nanosecond_equals_tick() {
        let ts = Timestamp::new(0);
        let mut a = SimClock::new(ts);
        let mut b = SimClock::new(ts);
        a.tick().unwrap();
        b.advance(Duration::NANOSECOND).unwrap();
        assert_eq!(a.now(), b.now());
    }

    /// Invariant: advance by a named Duration constant produces the
    ///             expected raw() delta
    #[test]
    fn test_advance_constants() {
        let tests = [
            (Duration::MICROSECOND, 1_000i128),
            (Duration::MILLISECOND, 1_000_000),
            (Duration::SECOND, 1_000_000_000),
            (Duration::MINUTE, 60_000_000_000),
        ];
        for (dur, expected_delta) in tests {
            let mut clock = SimClock::new(Timestamp::new(0));
            clock.advance(dur).unwrap();
            assert_eq!(
                clock.now().raw(),
                expected_delta,
                "advance failed for dur={dur:?}"
            );
        }
    }

    /// Invariant: advance from a negative timestamp
    #[test]
    fn test_advance_from_negative() {
        let mut clock = SimClock::new(Timestamp::new(-500));
        clock.advance(Duration::from_nanos(200)).unwrap();
        assert_eq!(clock.now().raw(), -300);
    }

    /// Invariant: advance across the epoch
    #[test]
    fn test_advance_crosses_epoch() {
        let mut clock = SimClock::new(Timestamp::new(-50));
        clock.advance(Duration::from_nanos(100)).unwrap();
        assert_eq!(clock.now().raw(), 50);
    }

    /// Invariant: multiple advances are cumulative
    #[test]
    fn test_advance_cumulative() {
        let mut clock = SimClock::new(Timestamp::new(0));
        clock.advance(Duration::MILLISECOND).unwrap();
        clock.advance(Duration::SECOND).unwrap();
        clock.advance(Duration::MINUTE).unwrap();
        assert_eq!(
            clock.now().raw(),
            1_000_000 + 1_000_000_000 + 60_000_000_000
        );
    }

    /// Invariant: advance + tick mixed in sequence
    #[test]
    fn test_advance_and_tick_mixed() {
        let mut clock = SimClock::new(Timestamp::new(0));
        clock.advance(Duration::SECOND).unwrap();
        clock.tick().unwrap();
        clock.tick().unwrap();
        clock.advance(Duration::from_nanos(500)).unwrap();
        assert_eq!(clock.now().raw(), 1_000_000_000 + 2 + 500);
    }

    /// Invariant: advance at i128::MAX with any positive duration
    ///             returns Overflow
    #[test]
    fn test_advance_overflow_at_i128_max() {
        let mut clock = SimClock::new(Timestamp::new(i128::MAX));
        let result = clock.advance(Duration::NANOSECOND);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), TimeError::Overflow);
    }

    /// Invariant: advance at i128::MAX with ZERO succeeds (identity)
    #[test]
    fn test_advance_zero_at_max_succeeds() {
        let mut clock = SimClock::new(Timestamp::new(i128::MAX));
        let result = clock.advance(Duration::ZERO);
        assert!(result.is_ok());
        assert_eq!(clock.now().raw(), i128::MAX);
    }

    /// Invariant: advance with a Duration exceeding i128::MAX fails
    #[test]
    fn test_advance_duration_exceeds_i128_max() {
        let huge = Duration::from_nanos(i128::MAX as u128 + 1);
        let mut clock = SimClock::new(Timestamp::new(0));
        let result = clock.advance(huge);
        assert!(result.is_err());
    }

    /// Invariant: advance near the boundary — fits, then overflows
    #[test]
    fn test_advance_boundary_fits_then_overflows() {
        let mut clock = SimClock::new(Timestamp::new(i128::MAX - 1000));
        clock.advance(Duration::from_nanos(1000)).unwrap();
        assert_eq!(clock.now().raw(), i128::MAX);
        let result = clock.advance(Duration::NANOSECOND);
        assert!(result.is_err());
    }

    /// Invariant: advance fits exactly at the boundary
    #[test]
    fn test_advance_exact_boundary() {
        let dur = Duration::from_nanos(i128::MAX as u128);
        let mut clock = SimClock::new(Timestamp::new(0));
        clock.advance(dur).unwrap();
        assert_eq!(clock.now().raw(), i128::MAX);
    }

    /// Invariant: after a failed advance, the clock is unchanged
    #[test]
    fn test_advance_overflow_leaves_clock_unchanged() {
        let mut clock = SimClock::new(Timestamp::new(100));
        let _ = clock.advance(Duration::from_nanos(i128::MAX as u128));
        assert_eq!(clock.now().raw(), 100);
    }

    /// Invariant: advance from i128::MIN by a small amount
    #[test]
    fn test_advance_from_i128_min() {
        let mut clock = SimClock::new(Timestamp::new(i128::MIN));
        clock.advance(Duration::NANOSECOND).unwrap();
        assert_eq!(clock.now().raw(), i128::MIN + 1);
    }

    /// Invariant: advance by HOUR, DAY, WEEK constants
    #[test]
    fn test_advance_large_constants() {
        let starts = [Timestamp::new(0), Timestamp::new(i128::MIN)];
        for start in starts {
            for (dur, expected_delta) in [
                (Duration::HOUR, 3_600_000_000_000i128),
                (Duration::DAY, 86_400_000_000_000i128),
            ] {
                let mut clock = SimClock::new(start);
                // only run if the addition won't overflow
                if start.raw().checked_add(expected_delta).is_some() {
                    clock.advance(dur).unwrap();
                    assert_eq!(clock.now().raw(), start.raw() + expected_delta);
                }
            }
        }
    }

    // ==================================================================
    // Combined / sequence
    // ==================================================================

    /// Invariant: set → tick → advance → tick → now reflects correct
    ///             accumulated time
    #[test]
    fn test_full_sequence() {
        let mut clock = SimClock::new(Timestamp::new(10_000));
        clock.tick().unwrap();
        clock.advance(Duration::SECOND).unwrap();
        clock.tick().unwrap();
        clock.advance(Duration::MILLISECOND).unwrap();
        clock.set(Timestamp::new(0));
        clock.advance(Duration::from_nanos(3)).unwrap();
        clock.tick().unwrap();
        assert_eq!(clock.now().raw(), 4);
    }

    /// Invariant: now() is stable across repeated calls with no mutation
    #[test]
    fn test_now_idempotent() {
        let clock = SimClock::new(Timestamp::new(42));
        assert_eq!(clock.now(), clock.now());
        assert_eq!(clock.now(), clock.now());
    }

    /// Invariant: clock is usable after an overflow — set + tick succeeds
    #[test]
    fn test_recovery_after_tick_overflow() {
        let mut clock = SimClock::new(Timestamp::new(i128::MAX));
        let _ = clock.tick(); // overflow
        clock.set(Timestamp::new(0));
        clock.tick().unwrap();
        assert_eq!(clock.now().raw(), 1);
    }

    /// Invariant: clock is usable after an advance overflow — set + advance succeeds
    #[test]
    fn test_recovery_after_advance_overflow() {
        let mut clock = SimClock::new(Timestamp::new(i128::MAX));
        let _ = clock.advance(Duration::NANOSECOND); // overflow
        clock.set(Timestamp::new(0));
        clock.advance(Duration::SECOND).unwrap();
        assert_eq!(clock.now().raw(), 1_000_000_000);
    }
}
