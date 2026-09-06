use crate::journal::JournalError;
use crate::time::{EventIndex, Timestamp};
use serde::{Serialize, Serializer};
use std::marker::PhantomData;

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

#[allow(
    dead_code,
    reason = "used by the record commit helper in later grammar build steps"
)]
pub trait RecordPayload {
    const KIND: RecordKind;
}

/// Kind-typed zero-sized first field; `fn() -> P` keeps auto-traits clean.
#[allow(
    dead_code,
    reason = "constructed by record transitions in later grammar build steps"
)]
pub struct Kind<P>(PhantomData<fn() -> P>);

impl<P> Kind<P> {
    #[allow(
        dead_code,
        reason = "used to construct payloads in later grammar build steps"
    )]
    pub const fn new() -> Self {
        Self(PhantomData)
    }
}

impl<P: RecordPayload> Serialize for Kind<P> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(P::KIND.tag())
    }
}

#[derive(Serialize)]
#[allow(
    dead_code,
    reason = "constructed by the RunStarted transition in a later grammar build step"
)]
pub struct RunStartedRecord {
    pub record_kind: Kind<Self>,
    pub index: EventIndex,
    pub schema_version: u32,
    pub logical_time: Timestamp,
}

impl RecordPayload for RunStartedRecord {
    const KIND: RecordKind = RecordKind::RunStarted;
}

#[derive(Serialize)]
#[allow(
    dead_code,
    reason = "constructed by the EventAccepted transition in a later grammar build step"
)]
pub struct EventAcceptedRecord<'a, Ev> {
    pub record_kind: Kind<Self>,
    pub index: EventIndex,
    pub logical_time: Timestamp,
    pub event: &'a Ev,
}

impl<'a, Ev> RecordPayload for EventAcceptedRecord<'a, Ev> {
    const KIND: RecordKind = RecordKind::EventAccepted;
}

#[derive(Serialize)]
#[allow(
    dead_code,
    reason = "constructed by the batch transition in a later grammar build step"
)]
pub struct CommandsPreparedRecord<'a, C> {
    pub record_kind: Kind<Self>,
    pub index: EventIndex,
    pub commands: &'a [C],
}

impl<'a, C> RecordPayload for CommandsPreparedRecord<'a, C> {
    const KIND: RecordKind = RecordKind::CommandsPrepared;
}

#[derive(Serialize)]
#[allow(
    dead_code,
    reason = "constructed by the batch transition in a later grammar build step"
)]
pub struct CommandsDispatchedRecord {
    pub record_kind: Kind<Self>,
    pub index: EventIndex,
}

impl RecordPayload for CommandsDispatchedRecord {
    const KIND: RecordKind = RecordKind::CommandsDispatched;
}

#[derive(Serialize)]
#[allow(
    dead_code,
    reason = "constructed by the stop transition in a later grammar build step"
)]
pub struct StopRequestedRecord {
    pub record_kind: Kind<Self>,
    pub index: EventIndex,
}

impl RecordPayload for StopRequestedRecord {
    const KIND: RecordKind = RecordKind::StopRequested;
}

#[derive(Serialize)]
#[allow(
    dead_code,
    reason = "constructed by completion transitions in later grammar build steps"
)]
pub struct TurnCompletedRecord {
    pub record_kind: Kind<Self>,
    pub index: EventIndex,
    pub outcome: TurnOutcome,
}

impl RecordPayload for TurnCompletedRecord {
    const KIND: RecordKind = RecordKind::TurnCompleted;
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
    use std::cell::Cell;

    mod record_payload_wire {
        use super::*;

        #[derive(Serialize)]
        struct TestEvent {
            symbol: &'static str,
            quantity: u64,
        }

        #[derive(Serialize)]
        struct TestCommand {
            action: &'static str,
            quantity: u64,
        }

        struct FailsOnce {
            should_fail: Cell<bool>,
        }

        impl Serialize for FailsOnce {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                if self.should_fail.replace(false) {
                    return Err(serde::ser::Error::custom(
                        "intentional event serialization failure",
                    ));
                }

                serializer.serialize_str("recovered")
            }
        }

        struct FallibleCommand {
            action: &'static str,
            should_fail: Cell<bool>,
        }

        impl Serialize for FallibleCommand {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                if self.should_fail.replace(false) {
                    return Err(serde::ser::Error::custom(
                        "intentional command serialization failure",
                    ));
                }

                serializer.serialize_str(self.action)
            }
        }

        /// Invariant: the run's first record serializes its kind, index, schema
        /// version, and logical time in that order as one JSON object.
        /// Design Doc: RUN-RECORDS
        #[test]
        fn run_started_matches_the_documented_example_line() {
            let record = RunStartedRecord {
                record_kind: Kind::new(),
                index: EventIndex::new(0),
                schema_version: 1,
                logical_time: Timestamp::from_nanos(100),
            };

            assert_eq!(
                serde_json::to_string(&record).expect("a RunStarted record must serialize"),
                r#"{"record_kind":"RunStarted","index":0,"schema_version":1,"logical_time":100}"#,
                "a RunStarted record must serialize kind, index, schema version, and logical time in order"
            );
        }

        /// Invariant: an accepted event record serializes its kind, newly accepted
        /// index, logical time, and borrowed event in that order without consuming it.
        /// Design Doc: RUN-RECORDS
        #[test]
        fn event_accepted_serializes_index_time_and_borrowed_event() {
            let event = TestEvent {
                symbol: "KVD",
                quantity: 2,
            };
            let record = EventAcceptedRecord {
                record_kind: Kind::new(),
                index: EventIndex::new(1),
                logical_time: Timestamp::from_nanos(0),
                event: &event,
            };

            assert_eq!(
                serde_json::to_string(&record).expect("an EventAccepted record must serialize"),
                r#"{"record_kind":"EventAccepted","index":1,"logical_time":0,"event":{"symbol":"KVD","quantity":2}}"#,
                "an EventAccepted record must serialize kind, index, logical time, and event in order"
            );
            assert_eq!(
                event.quantity, 2,
                "serializing an EventAccepted record must not consume its borrowed event"
            );
        }

        /// Invariant: each payload's serialized kind marker is derived from the same
        /// declared kind used to identify that payload internally.
        /// Design Doc: RUN-GRAMMAR
        #[test]
        fn payload_tag_and_kind_share_one_source() {
            assert_eq!(
                RunStartedRecord::KIND,
                RecordKind::RunStarted,
                "RunStarted payload metadata must declare the RunStarted kind"
            );
            assert_eq!(
                serde_json::to_string(&Kind::<RunStartedRecord>::new())
                    .expect("a RunStarted kind marker must serialize"),
                r#""RunStarted""#,
                "a RunStarted kind marker must serialize from its payload's declared kind"
            );
            assert_eq!(
                <EventAcceptedRecord<'static, TestEvent> as RecordPayload>::KIND,
                RecordKind::EventAccepted,
                "EventAccepted payload metadata must declare the EventAccepted kind"
            );
            assert_eq!(
                serde_json::to_string(&Kind::<EventAcceptedRecord<'static, TestEvent>>::new())
                    .expect("an EventAccepted kind marker must serialize"),
                r#""EventAccepted""#,
                "an EventAccepted kind marker must serialize from its payload's declared kind"
            );
        }

        /// Invariant: a prepared command record preserves the exact order of the
        /// borrowed command batch in its serialized bytes.
        /// Design Doc: RUN-RECORDS
        #[test]
        fn commands_prepared_keeps_batch_order_in_bytes() {
            let commands = [
                TestCommand {
                    action: "buy",
                    quantity: 2,
                },
                TestCommand {
                    action: "sell",
                    quantity: 1,
                },
            ];
            let record = CommandsPreparedRecord {
                record_kind: Kind::new(),
                index: EventIndex::new(4),
                commands: &commands,
            };

            assert_eq!(
                serde_json::to_string(&record).expect("a CommandsPrepared record must serialize"),
                r#"{"record_kind":"CommandsPrepared","index":4,"commands":[{"action":"buy","quantity":2},{"action":"sell","quantity":1}]}"#,
                "a CommandsPrepared record must preserve command order in its bytes"
            );
            assert_eq!(
                commands[0].action, "buy",
                "serializing a CommandsPrepared record must not consume its borrowed batch"
            );
        }

        /// Invariant: every record payload serializes only its documented fields,
        /// beginning with its matching kind and preserving field order.
        /// Design Doc: RUN-RECORDS
        #[test]
        fn every_payload_leads_with_its_kind_in_table_order() {
            let event = TestEvent {
                symbol: "KVD",
                quantity: 3,
            };
            let commands = [TestCommand {
                action: "hold",
                quantity: 3,
            }];
            let cases = [
                (
                    serde_json::to_string(&RunStartedRecord {
                        record_kind: Kind::new(),
                        index: EventIndex::new(0),
                        schema_version: 1,
                        logical_time: Timestamp::from_nanos(100),
                    })
                    .expect("a RunStarted record must serialize"),
                    r#"{"record_kind":"RunStarted","index":0,"schema_version":1,"logical_time":100}"#,
                ),
                (
                    serde_json::to_string(&EventAcceptedRecord {
                        record_kind: Kind::new(),
                        index: EventIndex::new(1),
                        logical_time: Timestamp::from_nanos(101),
                        event: &event,
                    })
                    .expect("an EventAccepted record must serialize"),
                    r#"{"record_kind":"EventAccepted","index":1,"logical_time":101,"event":{"symbol":"KVD","quantity":3}}"#,
                ),
                (
                    serde_json::to_string(&CommandsPreparedRecord {
                        record_kind: Kind::new(),
                        index: EventIndex::new(1),
                        commands: &commands,
                    })
                    .expect("a CommandsPrepared record must serialize"),
                    r#"{"record_kind":"CommandsPrepared","index":1,"commands":[{"action":"hold","quantity":3}]}"#,
                ),
                (
                    serde_json::to_string(&CommandsDispatchedRecord {
                        record_kind: Kind::new(),
                        index: EventIndex::new(1),
                    })
                    .expect("a CommandsDispatched record must serialize"),
                    r#"{"record_kind":"CommandsDispatched","index":1}"#,
                ),
                (
                    serde_json::to_string(&StopRequestedRecord {
                        record_kind: Kind::new(),
                        index: EventIndex::new(1),
                    })
                    .expect("a StopRequested record must serialize"),
                    r#"{"record_kind":"StopRequested","index":1}"#,
                ),
                (
                    serde_json::to_string(&TurnCompletedRecord {
                        record_kind: Kind::new(),
                        index: EventIndex::new(1),
                        outcome: TurnOutcome::Continue,
                    })
                    .expect("a TurnCompleted record must serialize"),
                    r#"{"record_kind":"TurnCompleted","index":1,"outcome":"Continue"}"#,
                ),
            ];

            for (actual, expected) in cases {
                assert_eq!(
                    actual.as_str(),
                    expected,
                    "every record payload must serialize its exact fields in table order"
                );
            }
        }

        /// Invariant: completion records serialize either possible turn outcome as
        /// a direct string value rather than a nested object.
        /// Design Doc: RUN-RECORDS
        #[test]
        fn turn_completed_outcome_is_a_bare_tag_for_both_answers() {
            for (outcome, expected) in [
                (
                    TurnOutcome::Continue,
                    r#"{"record_kind":"TurnCompleted","index":8,"outcome":"Continue"}"#,
                ),
                (
                    TurnOutcome::Stop,
                    r#"{"record_kind":"TurnCompleted","index":8,"outcome":"Stop"}"#,
                ),
            ] {
                let record = TurnCompletedRecord {
                    record_kind: Kind::new(),
                    index: EventIndex::new(8),
                    outcome,
                };

                assert_eq!(
                    serde_json::to_string(&record).expect("a TurnCompleted record must serialize"),
                    expected,
                    "a TurnCompleted outcome must serialize as its bare tag"
                );
            }
        }

        /// Invariant: prepared command records serialize borrowed slices correctly
        /// when they contain no commands or exactly one command.
        #[test]
        fn commands_prepared_serializes_zero_and_one_command_boundaries() {
            let no_commands: [TestCommand; 0] = [];
            let one_command = [TestCommand {
                action: "buy",
                quantity: 1,
            }];

            for (commands, expected) in [
                (
                    no_commands.as_slice(),
                    r#"{"record_kind":"CommandsPrepared","index":0,"commands":[]}"#,
                ),
                (
                    one_command.as_slice(),
                    r#"{"record_kind":"CommandsPrepared","index":0,"commands":[{"action":"buy","quantity":1}]}"#,
                ),
            ] {
                let record = CommandsPreparedRecord {
                    record_kind: Kind::new(),
                    index: EventIndex::new(0),
                    commands,
                };

                assert_eq!(
                    serde_json::to_string(&record)
                        .expect("a boundary-sized command batch must serialize"),
                    expected,
                    "a CommandsPrepared record must preserve a zero- or one-command slice"
                );
            }
        }

        /// Invariant: every remaining payload preserves an event index at the
        /// largest value in its unsigned numeric domain.
        #[test]
        fn remaining_payloads_preserve_maximum_index() {
            let commands = [0_u8];
            let maximum = u64::MAX;
            let cases = [
                (
                    serde_json::to_string(&CommandsPreparedRecord {
                        record_kind: Kind::new(),
                        index: EventIndex::new(maximum),
                        commands: &commands,
                    })
                    .expect("a maximum-index CommandsPrepared record must serialize"),
                    format!(
                        "{{\"record_kind\":\"CommandsPrepared\",\"index\":{maximum},\"commands\":[0]}}"
                    ),
                ),
                (
                    serde_json::to_string(&CommandsDispatchedRecord {
                        record_kind: Kind::new(),
                        index: EventIndex::new(maximum),
                    })
                    .expect("a maximum-index CommandsDispatched record must serialize"),
                    format!("{{\"record_kind\":\"CommandsDispatched\",\"index\":{maximum}}}"),
                ),
                (
                    serde_json::to_string(&StopRequestedRecord {
                        record_kind: Kind::new(),
                        index: EventIndex::new(maximum),
                    })
                    .expect("a maximum-index StopRequested record must serialize"),
                    format!("{{\"record_kind\":\"StopRequested\",\"index\":{maximum}}}"),
                ),
                (
                    serde_json::to_string(&TurnCompletedRecord {
                        record_kind: Kind::new(),
                        index: EventIndex::new(maximum),
                        outcome: TurnOutcome::Stop,
                    })
                    .expect("a maximum-index TurnCompleted record must serialize"),
                    format!(
                        "{{\"record_kind\":\"TurnCompleted\",\"index\":{maximum},\"outcome\":\"Stop\"}}"
                    ),
                ),
            ];

            for (actual, expected) in cases {
                assert_eq!(
                    actual, expected,
                    "a remaining record payload must preserve the maximum event index"
                );
            }
        }

        /// Invariant: if a borrowed command fails during serialization, the same
        /// batch and record remain intact for a later serialization attempt.
        #[test]
        fn commands_prepared_serialization_failure_leaves_batch_and_record_reusable() {
            let commands = [
                FallibleCommand {
                    action: "first",
                    should_fail: Cell::new(false),
                },
                FallibleCommand {
                    action: "second",
                    should_fail: Cell::new(true),
                },
            ];
            let record = CommandsPreparedRecord {
                record_kind: Kind::new(),
                index: EventIndex::new(2),
                commands: &commands,
            };

            let error = serde_json::to_string(&record)
                .expect_err("the command's first serialization failure must propagate");
            assert!(
                error
                    .to_string()
                    .contains("intentional command serialization failure"),
                "a CommandsPrepared record must preserve its command's serialization error"
            );
            assert_eq!(
                record.commands.len(),
                2,
                "a command serialization failure must leave the borrowed batch intact"
            );
            assert_eq!(
                serde_json::to_string(&record)
                    .expect("the same borrowed command record must remain serializable"),
                r#"{"record_kind":"CommandsPrepared","index":2,"commands":["first","second"]}"#,
                "a CommandsPrepared record must remain reusable after a command serialization failure"
            );
        }

        /// Invariant: the typed record-kind marker occupies zero bytes for each
        /// payload type.
        #[test]
        fn kind_marker_is_zero_sized_for_both_payload_types() {
            assert_eq!(
                std::mem::size_of::<Kind<RunStartedRecord>>(),
                0,
                "a RunStarted kind marker must occupy zero bytes"
            );
            assert_eq!(
                std::mem::size_of::<Kind<EventAcceptedRecord<'static, TestEvent>>>(),
                0,
                "an EventAccepted kind marker must occupy zero bytes"
            );
        }

        /// Invariant: whether a typed record-kind marker can be sent or shared
        /// between threads does not depend on its payload type.
        #[test]
        fn kind_marker_auto_traits_do_not_depend_on_payload_type() {
            fn require_send_sync<T: Send + Sync>() {}

            require_send_sync::<Kind<EventAcceptedRecord<'static, std::rc::Rc<u8>>>>();
        }

        /// Invariant: accepted-event records preserve indexes and logical times at
        /// the largest values in their unsigned numeric domains.
        #[test]
        fn event_accepted_serializes_maximum_index_and_time_without_loss() {
            let event = ();
            let record = EventAcceptedRecord {
                record_kind: Kind::new(),
                index: EventIndex::new(u64::MAX),
                logical_time: Timestamp::from_nanos(u64::MAX),
                event: &event,
            };
            let maximum = u64::MAX;

            assert_eq!(
                serde_json::to_string(&record).expect("maximum record values must serialize"),
                format!(
                    "{{\"record_kind\":\"EventAccepted\",\"index\":{maximum},\"logical_time\":{maximum},\"event\":null}}"
                ),
                "an EventAccepted record must serialize maximum index and time values without loss"
            );
        }

        /// Invariant: if a borrowed event fails during serialization, its record can
        /// be serialized again without rebuilding or replacing the event.
        #[test]
        fn event_accepted_serializer_failure_leaves_the_record_reusable() {
            let event = FailsOnce {
                should_fail: Cell::new(true),
            };
            let record = EventAcceptedRecord {
                record_kind: Kind::new(),
                index: EventIndex::new(1),
                logical_time: Timestamp::from_nanos(1),
                event: &event,
            };

            let error = serde_json::to_string(&record)
                .expect_err("the event's first serialization failure must propagate");
            assert!(
                error
                    .to_string()
                    .contains("intentional event serialization failure"),
                "an EventAccepted record must preserve its event's serialization error"
            );
            assert_eq!(
                serde_json::to_string(&record).expect(
                    "the same borrowed event record must remain serializable after failure"
                ),
                r#"{"record_kind":"EventAccepted","index":1,"logical_time":1,"event":"recovered"}"#,
                "an EventAccepted record must remain reusable after an event serialization failure"
            );
        }
    }

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
