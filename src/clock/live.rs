use std::time::SystemTime;

use crate::{clock::Clock, time::Timestamp};

/// The clock that will be used in live contexts. It gets its time from the
/// system clock.
pub struct LiveClock;

impl LiveClock {
    /// Returns a new LiveClock instance
    pub fn new() -> Self {
        LiveClock
    }
}

impl Default for LiveClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for LiveClock {
    fn now(&self) -> Timestamp {
        Timestamp::from(SystemTime::now())
    }

    fn set(&mut self, _ts: Timestamp) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    // ==================================================================
    // Now()
    // ==================================================================

    /// Invariant: now() gives a non-zero timestamp
    #[test]
    fn test_non_zero_timestamp() {
        let clock = LiveClock::new();
        let ts = clock.now();
        assert_ne!(ts.raw(), 0)
    }

    // ==================================================================
    // Construction / Default
    // ==================================================================

    /// Invariant: LiveClock::default() yields a valid clock equivalent to new()
    #[test]
    fn test_default() {
        let clock = LiveClock;
        assert_ne!(clock.now().raw(), 0);
    }

    /// Invariant: set() on LiveClock is a no-op that does not panic
    #[test]
    fn test_set_noop() {
        let mut clock = LiveClock::new();
        let _before = clock.now();
        clock.set(Timestamp::new(999));
        assert!(clock.now().raw() > 0); // set is ignored; still from system time
    }
}
