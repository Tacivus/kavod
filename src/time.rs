use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct Timestamp(u64);

impl Timestamp {
    pub(crate) const fn new(ts: u64) -> Self {
        Timestamp(ts)
    }

    pub fn checked_add(self, elapsed: std::time::Duration) -> Option<Self> {
        let nanos = u64::try_from(elapsed.as_nanos()).ok()?;
        self.0.checked_add(nanos).map(Timestamp)
    }

    pub const fn as_nanos(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct EventIndex(u64);

impl EventIndex {
    pub(crate) const START: Self = EventIndex(0);

    pub const fn as_u64(self) -> u64 {
        self.0
    }

    pub(crate) fn checked_next(self) -> Option<Self> {
        self.0.checked_add(1).map(EventIndex)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    mod timestamp_addition {
        use super::*;

        /// Invariant: Adding zero elapsed time preserves every timestamp.
        #[test]
        fn zero_duration_preserves_timestamp() {
            assert_eq!(
                Timestamp::new(u64::MAX).checked_add(Duration::ZERO),
                Some(Timestamp::new(u64::MAX))
            );
        }

        /// Invariant: Timestamp addition uses the full nanosecond duration.
        #[test]
        fn duration_is_added_in_nanoseconds() {
            assert_eq!(
                Timestamp::new(1).checked_add(Duration::new(2, 345_678_901)),
                Some(Timestamp::new(2_345_678_902))
            );
        }

        /// Invariant: A timestamp may advance exactly to the u64 boundary.
        #[test]
        fn addition_can_reach_u64_max_exactly() {
            assert_eq!(
                Timestamp::new(u64::MAX - 1).checked_add(Duration::from_nanos(1)),
                Some(Timestamp::new(u64::MAX))
            );
        }

        /// Invariant: Timestamp arithmetic never wraps or silently saturates.
        #[test]
        fn addition_overflow_returns_none() {
            assert_eq!(
                Timestamp::new(u64::MAX).checked_add(Duration::from_nanos(1)),
                None
            );
        }

        /// Invariant: Durations not representable as u64 nanoseconds are rejected.
        #[test]
        fn unrepresentable_duration_returns_none() {
            assert_eq!(
                Timestamp::new(0).checked_add(Duration::from_secs(u64::MAX)),
                None
            );
        }

        /// Invariant: A duration whose nanosecond count is exactly u64::MAX is
        /// representable, so only the subsequent sum can overflow, not the conversion.
        #[test]
        fn duration_at_exact_u64_nanos_boundary_is_representable() {
            assert_eq!(
                Timestamp::new(0).checked_add(Duration::from_nanos(u64::MAX)),
                Some(Timestamp::new(u64::MAX))
            );
        }

        /// Invariant: A duration one nanosecond past the u64 boundary is rejected by
        /// the conversion itself, distinct from a checked_add overflow.
        #[test]
        fn duration_one_nanos_past_u64_boundary_returns_none() {
            let just_over = Duration::from_nanos(u64::MAX) + Duration::from_nanos(1);
            assert_eq!(Timestamp::new(0).checked_add(just_over), None);
        }
    }

    mod timestamp_accessors {
        use super::*;

        /// Invariant: as_nanos returns exactly the value the timestamp was built from.
        #[test]
        fn as_nanos_round_trips_the_constructed_value() {
            assert_eq!(Timestamp::new(0).as_nanos(), 0);
            assert_eq!(Timestamp::new(42).as_nanos(), 42);
            assert_eq!(Timestamp::new(u64::MAX).as_nanos(), u64::MAX);
        }
    }

    mod timestamp_ordering {
        use super::*;

        /// Invariant: Equal timestamps are valid and values sort by nanoseconds.
        #[test]
        fn ordering_and_equality_follow_nanosecond_values() {
            assert_eq!(Timestamp::new(7), Timestamp::new(7));
            assert!(Timestamp::new(7) < Timestamp::new(8));
        }
    }

    mod timestamp_serialization {
        use super::*;

        /// Invariant: Timestamp serializes as a transparent u64 JSON value.
        #[test]
        fn serializes_as_a_json_u64() {
            assert_eq!(serde_json::to_string(&Timestamp::new(0)).unwrap(), "0");
            assert_eq!(serde_json::to_string(&Timestamp::new(42)).unwrap(), "42");
            assert_eq!(
                serde_json::to_string(&Timestamp::new(u64::MAX)).unwrap(),
                "18446744073709551615"
            );
        }
    }

    mod event_index_advancement {
        use super::*;

        /// Invariant: The start turn has accepted-event index zero.
        #[test]
        fn start_is_zero() {
            assert_eq!(EventIndex::START.as_u64(), 0);
        }

        /// Invariant: Each accepted external event advances the index by one.
        #[test]
        fn checked_next_advances_by_one() {
            assert_eq!(EventIndex::START.checked_next(), Some(EventIndex(1)));
            assert_eq!(EventIndex(41).checked_next(), Some(EventIndex(42)));
        }

        /// Invariant: An index may advance exactly to the u64 boundary.
        #[test]
        fn checked_next_can_reach_u64_max_exactly() {
            assert_eq!(
                EventIndex(u64::MAX - 1).checked_next(),
                Some(EventIndex(u64::MAX))
            );
        }

        /// Invariant: Event indices never wrap.
        #[test]
        fn checked_next_returns_none_at_u64_max() {
            assert_eq!(EventIndex(u64::MAX).checked_next(), None);
        }
    }

    mod event_index_ordering {
        use super::*;

        /// Invariant: Event-index order is accepted-turn order.
        #[test]
        fn ordering_follows_turn_number() {
            assert!(EventIndex::START < EventIndex(1));
            assert!(EventIndex(1) < EventIndex(u64::MAX));
        }
    }

    mod event_index_serialization {
        use super::*;

        /// Invariant: EventIndex serializes as a transparent u64 JSON value.
        #[test]
        fn serializes_as_a_json_u64() {
            assert_eq!(serde_json::to_string(&EventIndex(0)).unwrap(), "0");
            assert_eq!(serde_json::to_string(&EventIndex(42)).unwrap(), "42");
            assert_eq!(
                serde_json::to_string(&EventIndex(u64::MAX)).unwrap(),
                "18446744073709551615"
            );
        }
    }
}
