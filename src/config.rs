use crate::{error::BuildError, time::timestamp::Timestamp};

/// Selects the engine execution infrastructure.
///
/// `Mode` is not exposed through callback contexts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Scheduler-driven logical time with inline deterministic actors.
    Backtest,
    /// Kernel receipt time with runtime-owned actor threads.
    Live,
    /// Recorded ingress order with no external IO.
    Replay,
}

/// Global engine configuration containing values and policies.
///
/// Contains no clocks, channels, locks, schedulers, or executors.
/// The runtime constructs mechanisms from this declarative configuration.
#[derive(Debug, Clone)]
pub struct EngineConfig {
    mode: Mode,
    initial_dispatch_time: Timestamp,
    max_events_per_instant: usize,
}

impl EngineConfig {
    /// Creates a backtest configuration starting at `initial_dispatch_time`
    /// with a default same-instant limit of 1 000 events.
    ///
    /// The backtest runtime creates its internal simulated clock from this
    /// timestamp and begins dispatching from that point.
    pub fn backtest(initial_dispatch_time: Timestamp) -> Self {
        Self {
            initial_dispatch_time,
            mode: Mode::Backtest,
            max_events_per_instant: 1_000,
        }
    }

    /// Creates a live configuration.
    ///
    /// Live mode is not yet implemented. This constructor will succeed
    /// once the live runtime is built.
    pub fn live(_initial_dispatch_time: Timestamp) -> Result<Self, BuildError> {
        Err(BuildError::UnsupportedMode { mode: "Live" })
    }

    /// Creates a replay configuration.
    ///
    /// Replay mode is not yet implemented. This constructor will succeed
    /// once the replay runtime is built.
    pub fn replay(_initial_dispatch_time: Timestamp) -> Result<Self, BuildError> {
        Err(BuildError::UnsupportedMode { mode: "Replay" })
    }

    /// Returns the engine execution mode.
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// Returns the initial dispatch time for backtest mode.
    pub fn initial_dispatch_time(&self) -> Timestamp {
        self.initial_dispatch_time
    }

    /// Returns the maximum number of events processed at a single
    /// dispatch timestamp before raising a same-instant limit error.
    pub fn max_events_per_instant(&self) -> usize {
        self.max_events_per_instant
    }

    /// Sets the maximum events per instant.
    ///
    /// # Panics
    ///
    /// Panics if `value` is zero.
    pub fn with_max_events_per_instant(mut self, value: usize) -> Self {
        assert!(
            value > 0,
            "max_events_per_instant must be greater than zero"
        );
        self.max_events_per_instant = value;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // Backtest construction
    // ========================================================================

    /// Invariant: a backtest configuration can be constructed.
    #[test]
    fn test_backtest_config_can_be_constructed() {
        let config = EngineConfig::backtest(Timestamp::new(0));
        assert_eq!(config.mode(), Mode::Backtest);
    }

    /// Invariant: a backtest configuration preserves its initial dispatch time.
    #[test]
    fn test_backtest_config_preserves_initial_dispatch_time() {
        let ts = Timestamp::new(9_600);
        let config = EngineConfig::backtest(ts);
        assert_eq!(config.initial_dispatch_time(), ts);
    }

    /// Invariant: a backtest configuration defaults to 1000 max events per instant.
    #[test]
    fn test_backtest_config_default_max_events() {
        let config = EngineConfig::backtest(Timestamp::new(0));
        assert_eq!(config.max_events_per_instant(), 1_000);
    }

    // ========================================================================
    // Builder method
    // ========================================================================

    /// Invariant: with_max_events_per_instant overrides the default.
    #[test]
    fn test_with_max_events_overrides_default() {
        let config = EngineConfig::backtest(Timestamp::new(0)).with_max_events_per_instant(500);
        assert_eq!(config.max_events_per_instant(), 500);
    }

    /// Invariant: with_max_events_per_instant rejects zero.
    #[test]
    #[should_panic(expected = "max_events_per_instant must be greater than zero")]
    fn test_max_events_rejects_zero() {
        EngineConfig::backtest(Timestamp::new(0)).with_max_events_per_instant(0);
    }

    // ========================================================================
    // Unsupported modes
    // ========================================================================

    /// Invariant: live mode fails explicitly with UnsupportedMode until
    /// the live runtime is implemented.
    #[test]
    fn test_live_mode_returns_unsupported_error() {
        let result = EngineConfig::live(Timestamp::new(0));
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, BuildError::UnsupportedMode { mode: "Live" }));
    }

    /// Invariant: replay mode fails explicitly with UnsupportedMode until
    /// the replay runtime is implemented.
    #[test]
    fn test_replay_mode_returns_unsupported_error() {
        let result = EngineConfig::replay(Timestamp::new(0));
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(
            err,
            BuildError::UnsupportedMode { mode: "Replay" }
        ));
    }

    /// Invariant: the UnsupportedMode error message is human-readable.
    #[test]
    fn test_unsupported_mode_error_is_readable() {
        let err = EngineConfig::live(Timestamp::new(0)).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("Live"),
            "error message should name the mode, got: {msg}"
        );
    }
}
