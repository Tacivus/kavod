use serde::Serialize;
use std::time::Duration;

/// The accepted turn's ordinal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct EventIndex(u64);

/// A logical timestamp represented as a nanosecond count.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Timestamp(u64);

impl EventIndex {
    #[allow(dead_code, reason = "used by the Run built in later steps")]
    pub(crate) fn new(index: u64) -> Self {
        Self(index)
    }

    /// Returns the accepted turn's ordinal: zero for the start turn and one onward
    /// for external events.
    pub fn as_u64(self) -> u64 {
        self.0
    }
}

impl Timestamp {
    /// Builds a timestamp from a nanosecond count.
    ///
    /// The count's origin and meaning belong to the stamping Environment.
    pub fn from_nanos(nanos: u64) -> Self {
        Self(nanos)
    }

    /// Returns the timestamp advanced by `elapsed`, or `None` if the duration or
    /// resulting sum exceeds the nanosecond domain.
    pub fn checked_add(self, elapsed: Duration) -> Option<Self> {
        let elapsed = u64::try_from(elapsed.as_nanos()).ok()?;
        self.0.checked_add(elapsed).map(Self)
    }

    /// Returns the timestamp's nanosecond count.
    pub fn as_nanos(self) -> u64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod timestamp_arithmetic {
        use super::*;

        /// Invariant: adding elapsed time that would exceed the timestamp domain
        /// returns no timestamp.
        /// Design Doc: A6
        #[test]
        fn overflowing_sum_returns_none() {
            let timestamp = Timestamp::from_nanos(u64::MAX);

            assert_eq!(
                timestamp.checked_add(Duration::from_nanos(1)),
                None,
                "timestamp addition must reject a sum beyond the u64 domain"
            );
        }

        /// Invariant: a duration outside the timestamp domain is rejected before
        /// it can be added.
        /// Design Doc: A6
        #[test]
        fn oversized_duration_returns_none() {
            let oversized = Duration::from_nanos(u64::MAX)
                .checked_add(Duration::from_nanos(1))
                .expect("the Duration domain must represent one nanosecond beyond u64");

            assert_eq!(
                Timestamp::from_nanos(0).checked_add(oversized),
                None,
                "timestamp addition must reject durations beyond the u64 domain"
            );
        }

        /// Invariant: advancing a timestamp by no elapsed time preserves the same
        /// valid timestamp.
        /// Design Doc: ENV-TIME
        #[test]
        fn equal_timestamp_is_valid() {
            let timestamp = Timestamp::from_nanos(42);

            assert_eq!(
                timestamp.checked_add(Duration::ZERO),
                Some(timestamp),
                "zero elapsed time must preserve an equal timestamp"
            );
        }

        /// Invariant: adding one nanosecond at the start of the timestamp domain
        /// advances the value by exactly one.
        #[test]
        fn one_nanosecond_addition_advances_by_one() {
            assert_eq!(
                Timestamp::from_nanos(0).checked_add(Duration::from_nanos(1)),
                Some(Timestamp::from_nanos(1)),
                "one-nanosecond timestamp addition must advance by exactly one"
            );
        }

        /// Invariant: an addition whose result is exactly the largest timestamp
        /// remains valid.
        #[test]
        fn exact_domain_maximum_sum_succeeds() {
            assert_eq!(
                Timestamp::from_nanos(0).checked_add(Duration::from_nanos(u64::MAX)),
                Some(Timestamp::from_nanos(u64::MAX)),
                "timestamp addition must accept a sum at the u64 domain maximum"
            );
        }

        /// Invariant: a failed addition does not prevent the original timestamp
        /// value from remaining usable and unchanged.
        #[test]
        fn failed_addition_leaves_original_timestamp_unchanged() {
            let timestamp = Timestamp::from_nanos(u64::MAX);

            assert_eq!(
                timestamp.checked_add(Duration::from_nanos(1)),
                None,
                "overflowing timestamp addition must fail"
            );
            assert_eq!(
                timestamp.as_nanos(),
                u64::MAX,
                "failed timestamp addition must leave the original value unchanged"
            );
        }
    }

    mod index_and_time_accessors {
        use super::*;

        /// Invariant: minting an event index preserves values at both ends of its
        /// numeric domain.
        #[test]
        fn event_index_round_trips_domain_boundaries() {
            assert_eq!(
                EventIndex::new(0).as_u64(),
                0,
                "the minimum event index must round-trip unchanged"
            );
            assert_eq!(
                EventIndex::new(u64::MAX).as_u64(),
                u64::MAX,
                "the maximum event index must round-trip unchanged"
            );
        }

        /// Invariant: constructing a timestamp preserves values at both ends of
        /// its numeric domain.
        #[test]
        fn timestamp_round_trips_domain_boundaries() {
            assert_eq!(
                Timestamp::from_nanos(0).as_nanos(),
                0,
                "the minimum timestamp must round-trip unchanged"
            );
            assert_eq!(
                Timestamp::from_nanos(u64::MAX).as_nanos(),
                u64::MAX,
                "the maximum timestamp must round-trip unchanged"
            );
        }
    }

    mod index_and_time_wire {
        use super::*;

        /// Invariant: event indexes and timestamps serialize directly as JSON
        /// numbers without an object or array wrapper.
        /// Design Doc: the EventIndex/Timestamp API block
        #[test]
        fn both_serialize_as_transparent_u64() {
            assert_eq!(
                serde_json::to_string(&EventIndex::new(17)).expect("an event index must serialize"),
                "17",
                "an event index must serialize as its bare u64 value"
            );
            assert_eq!(
                serde_json::to_string(&Timestamp::from_nanos(23))
                    .expect("a timestamp must serialize"),
                "23",
                "a timestamp must serialize as its bare u64 value"
            );
        }

        /// Invariant: values at the top of the unsigned domain serialize without
        /// sign changes or precision loss.
        #[test]
        fn maximum_values_serialize_without_loss() {
            let expected = u64::MAX.to_string();

            assert_eq!(
                serde_json::to_string(&EventIndex::new(u64::MAX))
                    .expect("the maximum event index must serialize"),
                expected,
                "the maximum event index must serialize without loss"
            );
            assert_eq!(
                serde_json::to_string(&Timestamp::from_nanos(u64::MAX))
                    .expect("the maximum timestamp must serialize"),
                expected,
                "the maximum timestamp must serialize without loss"
            );
        }
    }
}
