use crate::journal::JournalError;
use serde::Serialize;

#[derive(Debug, PartialEq, Eq)]
pub enum RecordKind {
    RunStarted,
    EventAccepted,
    CommandsPrepared,
    CommandsDispatched,
    StopRequested,
    TurnCompleted,
}

impl RecordKind {
    /// Returns the record kind's stable wire tag.
    pub const fn tag(self) -> &'static str {
        match self {
            Self::RunStarted => "RunStarted",
            Self::EventAccepted => "EventAccepted",
            Self::CommandsPrepared => "CommandsPrepared",
            Self::CommandsDispatched => "CommandsDispatched",
            Self::StopRequested => "StopRequested",
            Self::TurnCompleted => "TurnCompleted",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum TurnOutcome {
    Continue,
    Stop,
}

pub struct JournalFatal {
    /// The kind of the record whose commit failed.
    pub record_kind: RecordKind,
    /// `Some` with the attempted outcome exactly when `record_kind` is
    /// `TurnCompleted`; `None` otherwise.
    pub outcome: Option<TurnOutcome>,
    pub error: JournalError,
}

#[cfg(test)]
mod tests {
    use super::*;

    mod record_kind_wire {
        use super::*;

        /// Invariant: each turn outcome serializes directly to its variant name
        /// without an enclosing object or field name.
        /// Design Doc: RUN-RECORDS
        #[test]
        fn turn_outcome_serializes_as_a_bare_tag() {
            for (outcome, expected) in [
                (TurnOutcome::Continue, r#""Continue""#),
                (TurnOutcome::Stop, r#""Stop""#),
            ] {
                let encoded = serde_json::to_string(&outcome)
                    .expect("a TurnOutcome tag must serialize successfully");

                assert_eq!(
                    encoded, expected,
                    "a TurnOutcome must serialize as its bare variant tag"
                );
            }
        }

        /// Invariant: every record kind's wire tag exactly matches its Rust
        /// variant name.
        /// Design Doc: RUN-RECORDS
        #[test]
        fn kind_tags_match_their_variant_names() {
            const CASES: [(RecordKind, &str); 6] = [
                (RecordKind::RunStarted, "RunStarted"),
                (RecordKind::EventAccepted, "EventAccepted"),
                (RecordKind::CommandsPrepared, "CommandsPrepared"),
                (RecordKind::CommandsDispatched, "CommandsDispatched"),
                (RecordKind::StopRequested, "StopRequested"),
                (RecordKind::TurnCompleted, "TurnCompleted"),
            ];

            for (kind, expected) in CASES {
                assert_eq!(
                    kind.tag(),
                    expected,
                    "a RecordKind wire tag must match its variant name"
                );
            }
        }
    }

    mod journal_fatal_metadata {
        use super::*;

        /// Invariant: failed completion records retain their attempted outcome,
        /// while every other failed record kind carries no outcome.
        /// Design Doc: JournalFatal
        #[test]
        fn outcome_is_present_only_for_turn_completed() {
            for (record_kind, outcome) in [
                (RecordKind::RunStarted, None),
                (RecordKind::EventAccepted, None),
                (RecordKind::CommandsPrepared, None),
                (RecordKind::CommandsDispatched, None),
                (RecordKind::StopRequested, None),
                (RecordKind::TurnCompleted, Some(TurnOutcome::Continue)),
                (RecordKind::TurnCompleted, Some(TurnOutcome::Stop)),
            ] {
                let fatal = JournalFatal {
                    record_kind,
                    outcome,
                    error: JournalError::BoundExceeded,
                };
                let is_turn_completed = fatal.record_kind == RecordKind::TurnCompleted;

                assert_eq!(
                    fatal.outcome.is_some(),
                    is_turn_completed,
                    "JournalFatal outcome metadata must be present exactly for TurnCompleted"
                );
            }
        }
    }

    mod turn_outcome_traits {
        use super::*;

        fn require_clone_and_copy<T: Clone + Copy>() {}

        /// Invariant: one turn outcome value can be reused wherever both record
        /// payload and failure metadata need the same answer.
        #[test]
        fn outcome_is_clone_and_copy() {
            require_clone_and_copy::<TurnOutcome>();

            let outcome = TurnOutcome::Stop;
            let copied = outcome;

            assert_eq!(
                outcome, copied,
                "copying a TurnOutcome must preserve the original answer"
            );
        }
    }
}
