use std::num::NonZeroUsize;

/// Declarative per-actor configuration.
///
/// Contains values and policies only — no mailboxes, channels, threads,
/// or executors. The runtime constructs those later from this config.
#[derive(Debug, Clone, Default)]
pub(crate) struct ActorConfig {
    inbox_capacity: Option<NonZeroUsize>,
}

impl ActorConfig {
    pub(crate) fn new() -> Self {
        Self {
            inbox_capacity: None,
        }
    }

    /// Effective inbox capacity, if configured.
    ///
    /// Unused by the runtime until live actor construction.
    #[allow(dead_code)]
    pub(crate) fn inbox_capacity(&self) -> Option<NonZeroUsize> {
        self.inbox_capacity
    }

    pub(crate) fn set_inbox_capacity(&mut self, capacity: NonZeroUsize) {
        self.inbox_capacity = Some(capacity);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // Defaults
    // ========================================================================

    /// Invariant: default actor config has no inbox capacity set
    #[test]
    fn test_default_config_has_no_capacity() {
        let config = ActorConfig::new();
        assert!(config.inbox_capacity().is_none());
    }

    // ========================================================================
    // Capacity
    // ========================================================================

    /// Invariant: non-zero capacity is stored as NonZeroUsize
    #[test]
    fn test_inbox_capacity_stores_nonzero() {
        let mut config = ActorConfig::new();
        config.set_inbox_capacity(NonZeroUsize::new(4_096).unwrap());
        assert_eq!(config.inbox_capacity().map(|n| n.get()), Some(4_096));
    }

    /// Invariant: ActorConfig contains only declarative values (no channel types)
    #[test]
    fn test_config_is_values_only() {
        // Compile-time shape check: only Option<NonZeroUsize>.
        let config = ActorConfig::new();
        let _: Option<NonZeroUsize> = config.inbox_capacity();
        let _ = config.clone();
    }
}
