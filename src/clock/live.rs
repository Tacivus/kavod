use std::time::SystemTime;

use crate::{clock::Clock, time::timestamp::Timestamp};

/// The clock that will be used in live contexts. It gets its time from the
/// system clock.
pub struct LiveClock;

impl LiveClock {
    /// Returns a new LiveClock instance
    pub fn new() -> Self {
        LiveClock
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

    /// Invariant: now() gives a non-zero timestamp
    #[test]
    fn test_non_zero_timestamp() {
        let clock = LiveClock::new();
        let ts = clock.now();
        assert_ne!(ts.raw(), 0)
    }
}
