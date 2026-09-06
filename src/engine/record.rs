use crate::bounded_buffer::BoundedBuffer;
use crate::environment::{Environment, Quiescence};
use crate::journal::{Journal, JournalError};
use crate::time::{EventIndex, Timestamp};
use serde::{Serialize, Serializer};
use std::{io, marker::PhantomData};

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
    pub(crate) const fn tag(self) -> &'static str {
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

pub trait RecordPayload {
    const KIND: RecordKind;
}

/// Kind-typed zero-sized first field; `fn() -> P` keeps auto-traits clean.
pub struct Kind<P>(PhantomData<fn() -> P>);

impl<P> Kind<P> {
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
pub struct CommandsPreparedRecord<'a, C> {
    pub record_kind: Kind<Self>,
    pub index: EventIndex,
    pub commands: &'a [C],
}

impl<'a, C> RecordPayload for CommandsPreparedRecord<'a, C> {
    const KIND: RecordKind = RecordKind::CommandsPrepared;
}

#[derive(Serialize)]
pub struct CommandsDispatchedRecord {
    pub record_kind: Kind<Self>,
    pub index: EventIndex,
}

impl RecordPayload for CommandsDispatchedRecord {
    const KIND: RecordKind = RecordKind::CommandsDispatched;
}

#[derive(Serialize)]
pub struct StopRequestedRecord {
    pub record_kind: Kind<Self>,
    pub index: EventIndex,
}

impl RecordPayload for StopRequestedRecord {
    const KIND: RecordKind = RecordKind::StopRequested;
}

#[derive(Serialize)]
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

pub(super) struct Unclassified;

pub(super) mod answer {
    mod sealed {
        pub trait Sealed {}
        impl Sealed for super::Continue {}
        impl Sealed for super::Stop {}
    }

    /// A classified turn's fixed answer: `Continue` or `Stop`, and nothing else.
    /// The supertrait is private to this module, so no other marker can
    /// implement it.
    pub(in crate::engine) trait Answer: sealed::Sealed {}

    pub(in crate::engine) struct Continue;
    pub(in crate::engine) struct Stop;

    impl Answer for Continue {}
    impl Answer for Stop {}
}

pub(super) struct Initial;

pub(super) struct TurnOpen<A = Unclassified>(PhantomData<fn() -> A>);

pub(super) struct EffectsComplete<A: answer::Answer>(PhantomData<fn() -> A>);

pub(super) struct Checkpointed<A: answer::Answer>(PhantomData<fn() -> A>);

pub(super) struct BetweenTurns;

pub(super) struct StopPending;

pub(super) struct Closed;

pub(super) struct Certificate<W: io::Write, P> {
    journal: Journal<W>,
    index: EventIndex,
    last_time: Timestamp,
    _phase: PhantomData<fn() -> P>,
}

pub(super) enum ClassifiedTurn<W: io::Write> {
    Continue(Certificate<W, TurnOpen<answer::Continue>>),
    Stop(Certificate<W, TurnOpen<answer::Stop>>),
}

impl<W: io::Write, P> Certificate<W, P> {
    fn advance<Q>(self) -> Certificate<W, Q> {
        Certificate {
            journal: self.journal,
            index: self.index,
            last_time: self.last_time,
            _phase: PhantomData,
        }
    }

    fn commit<R: RecordPayload + Serialize>(
        &mut self,
        payload: &R,
        outcome: Option<TurnOutcome>,
    ) -> Result<(), JournalFatal> {
        self.journal.commit(payload).map_err(|error| JournalFatal {
            record_kind: R::KIND,
            outcome,
            error,
        })
    }
}

impl<W: io::Write> Certificate<W, TurnOpen> {
    pub(super) fn index(&self) -> EventIndex {
        self.index
    }

    pub(super) fn logical_time(&self) -> Timestamp {
        self.last_time
    }

    pub(super) fn classify(self, answer: TurnOutcome) -> ClassifiedTurn<W> {
        match answer {
            TurnOutcome::Continue => ClassifiedTurn::Continue(self.advance()),
            TurnOutcome::Stop => ClassifiedTurn::Stop(self.advance()),
        }
    }
}

impl<W: io::Write, A: answer::Answer> Certificate<W, TurnOpen<A>> {
    pub(super) fn no_commands<C>(
        self,
        commands: &BoundedBuffer<C>,
    ) -> Certificate<W, EffectsComplete<A>> {
        assert!(
            commands.is_empty(),
            "ASSERT-INVARIANTS: the recordless batch edge requires an empty command buffer"
        );
        self.advance()
    }

    pub(super) fn dispatch_batch<C, E, AE>(
        mut self,
        environment: &mut E,
        commands: &mut BoundedBuffer<C>,
    ) -> Result<Certificate<W, EffectsComplete<A>>, super::FatalCause<AE, E::Error>>
    where
        C: Serialize,
        E: Environment<Command = C>,
    {
        assert!(
            !commands.is_empty(),
            "ASSERT-INVARIANTS: the dispatch batch transition requires a nonempty command buffer"
        );

        self.commit(
            &CommandsPreparedRecord {
                record_kind: Kind::new(),
                index: self.index,
                commands: commands.as_slice(),
            },
            None,
        )
        .map_err(super::FatalCause::Journal)?;

        for (position, command) in commands.drain().enumerate() {
            environment.dispatch(command).map_err(|error| {
                super::FatalCause::Environment(super::EnvironmentFatal {
                    error,
                    operation: super::EnvironmentOperation::Dispatch { position },
                })
            })?;
        }

        self.commit(
            &CommandsDispatchedRecord {
                record_kind: Kind::new(),
                index: self.index,
            },
            None,
        )
        .map_err(super::FatalCause::Journal)?;

        Ok(self.advance())
    }
}

impl<W: io::Write, A: answer::Answer> Certificate<W, EffectsComplete<A>> {
    pub(super) fn checkpoint<E: Environment, AE>(
        self,
        environment: &mut E,
    ) -> Result<Certificate<W, Checkpointed<A>>, super::FatalCause<AE, E::Error>> {
        match environment.take_error() {
            Some(error) => Err(super::FatalCause::Environment(super::EnvironmentFatal {
                error,
                operation: super::EnvironmentOperation::Checkpoint,
            })),
            None => Ok(self.advance()),
        }
    }
}

impl<W: io::Write> Certificate<W, Checkpointed<answer::Continue>> {
    pub(super) fn complete_continue(
        mut self,
    ) -> Result<Certificate<W, BetweenTurns>, JournalFatal> {
        self.commit(
            &TurnCompletedRecord {
                record_kind: Kind::new(),
                index: self.index,
                outcome: TurnOutcome::Continue,
            },
            Some(TurnOutcome::Continue),
        )?;
        Ok(self.advance())
    }
}

impl<W: io::Write> Certificate<W, BetweenTurns> {
    #[allow(
        clippy::type_complexity,
        reason = "the transition returns its typed successor and owned Event or the shared fatal cause"
    )]
    pub(super) fn accept_event<E: Environment, AE>(
        mut self,
        environment: &mut E,
    ) -> Result<(Certificate<W, TurnOpen>, E::Event), super::FatalCause<AE, E::Error>>
    where
        E::Event: Serialize,
    {
        if self.index.as_u64() == u64::MAX {
            return Err(super::FatalCause::Core(super::CoreError::IndexExhausted));
        }

        let (event, offered) = environment.next_event().map_err(|error| {
            super::FatalCause::Environment(super::EnvironmentFatal {
                error,
                operation: super::EnvironmentOperation::NextEvent,
            })
        })?;
        let next_index = self
            .index
            .as_u64()
            .checked_add(1)
            .expect("RUN-INDEX: overflow past the index domain check");
        if offered < self.last_time {
            return Err(super::FatalCause::Core(super::CoreError::TimeRegression {
                previous: self.last_time,
                offered,
            }));
        }

        self.commit(
            &EventAcceptedRecord {
                record_kind: Kind::new(),
                index: EventIndex::new(next_index),
                logical_time: offered,
                event: &event,
            },
            None,
        )
        .map_err(super::FatalCause::Journal)?;
        self.index = EventIndex::new(next_index);
        self.last_time = offered;
        Ok((self.advance(), event))
    }
}

impl<W: io::Write> Certificate<W, Checkpointed<answer::Stop>> {
    pub(super) fn request_stop(mut self) -> Result<Certificate<W, StopPending>, JournalFatal> {
        self.commit(
            &StopRequestedRecord {
                record_kind: Kind::new(),
                index: self.index,
            },
            None,
        )?;
        Ok(self.advance())
    }
}

impl<W: io::Write> Certificate<W, StopPending> {
    #[allow(
        clippy::type_complexity,
        reason = "the close failure carries its typed cause and retained shutdown quiescence"
    )]
    pub(super) fn close<E: Environment, AE>(
        mut self,
        environment: E,
    ) -> Result<Certificate<W, Closed>, (super::FatalCause<AE, E::Error>, Quiescence)> {
        let report = environment.shutdown();
        let retained_quiescence = report.quiescence;

        if let Some(error) = report.error {
            return Err((
                super::FatalCause::Environment(super::EnvironmentFatal {
                    error,
                    operation: super::EnvironmentOperation::Shutdown,
                }),
                retained_quiescence,
            ));
        }
        if retained_quiescence == Quiescence::Incomplete {
            return Err((
                super::FatalCause::Core(super::CoreError::ShutdownIncomplete),
                retained_quiescence,
            ));
        }

        self.commit(
            &TurnCompletedRecord {
                record_kind: Kind::new(),
                index: self.index,
                outcome: TurnOutcome::Stop,
            },
            Some(TurnOutcome::Stop),
        )
        .map_err(|fatal| (super::FatalCause::Journal(fatal), retained_quiescence))?;
        Ok(self.advance())
    }
}

impl<W: io::Write> Certificate<W, Initial> {
    pub(super) fn mint(journal: Journal<W>, start_time: Timestamp) -> Self {
        let certificate = Self {
            journal,
            index: EventIndex::new(0),
            last_time: start_time,
            _phase: PhantomData,
        };
        assert_eq!(
            certificate.index.as_u64(),
            0,
            "RUN-ENFORCEMENT: an Initial certificate's prospective index must be zero"
        );
        certificate
    }

    pub(super) fn run_started(mut self) -> Result<Certificate<W, TurnOpen>, JournalFatal> {
        let payload = RunStartedRecord {
            record_kind: Kind::new(),
            index: self.index,
            schema_version: 1,
            logical_time: self.last_time,
        };
        self.commit(&payload, None)?;
        Ok(self.advance())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        cell::{Cell, RefCell},
        num::NonZeroUsize,
        rc::Rc,
    };

    fn certificate_journal<W: io::Write>(writer: W, max_record_bytes: usize) -> Journal<W> {
        Journal::new(
            writer,
            NonZeroUsize::new(max_record_bytes)
                .expect("a certificate test record bound must be nonzero"),
        )
        .expect("a small certificate test Journal must reserve its record buffer")
    }

    #[derive(Debug, PartialEq, Eq)]
    enum ScriptedCall<C> {
        RecordCommitted(Vec<u8>),
        Start,
        NextEvent,
        Dispatch(C),
        DispatchFailed(C),
        TakeError,
        Shutdown,
    }

    type ScriptedCalls<C> = Rc<RefCell<Vec<ScriptedCall<C>>>>;

    struct ScriptedEnvironment<C> {
        calls: ScriptedCalls<C>,
        dispatch_position: usize,
        fail_dispatch_at: Option<usize>,
        next_event: Option<Result<(u8, Timestamp), &'static str>>,
        pending_error: Option<&'static str>,
        shutdown_report: crate::environment::ShutdownReport<&'static str>,
    }

    impl<C> ScriptedEnvironment<C> {
        fn new(calls: ScriptedCalls<C>, fail_dispatch_at: Option<usize>) -> Self {
            Self {
                calls,
                dispatch_position: 0,
                fail_dispatch_at,
                next_event: Some(Ok((1, Timestamp::from_nanos(1)))),
                pending_error: None,
                shutdown_report: crate::environment::ShutdownReport {
                    quiescence: crate::environment::Quiescence::Quiesced,
                    error: None,
                },
            }
        }
    }

    impl<C> Environment for ScriptedEnvironment<C> {
        type Event = u8;
        type Command = C;
        type Error = &'static str;

        fn start(&mut self) -> Result<Timestamp, Self::Error> {
            self.calls.borrow_mut().push(ScriptedCall::Start);
            Ok(Timestamp::from_nanos(0))
        }

        fn next_event(&mut self) -> Result<(Self::Event, Timestamp), Self::Error> {
            self.calls.borrow_mut().push(ScriptedCall::NextEvent);
            self.next_event
                .take()
                .expect("a scripted Environment must have one next-event result")
        }

        fn dispatch(&mut self, command: Self::Command) -> Result<(), Self::Error> {
            let position = self.dispatch_position;
            self.dispatch_position += 1;
            if self.fail_dispatch_at == Some(position) {
                self.calls
                    .borrow_mut()
                    .push(ScriptedCall::DispatchFailed(command));
                return Err("scripted dispatch failure");
            }

            self.calls
                .borrow_mut()
                .push(ScriptedCall::Dispatch(command));
            Ok(())
        }

        fn take_error(&mut self) -> Option<Self::Error> {
            self.calls.borrow_mut().push(ScriptedCall::TakeError);
            self.pending_error.take()
        }

        fn shutdown(self) -> crate::environment::ShutdownReport<Self::Error> {
            self.calls.borrow_mut().push(ScriptedCall::Shutdown);
            self.shutdown_report
        }
    }

    struct ScriptedWriter<C> {
        calls: ScriptedCalls<C>,
        pending_record: Vec<u8>,
        flush_position: usize,
        fail_flush_at: Option<usize>,
    }

    impl<C> ScriptedWriter<C> {
        fn new(calls: ScriptedCalls<C>, fail_flush_at: Option<usize>) -> Self {
            Self {
                calls,
                pending_record: Vec::new(),
                flush_position: 0,
                fail_flush_at,
            }
        }
    }

    impl<C> io::Write for ScriptedWriter<C> {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.pending_record.extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            let position = self.flush_position;
            self.flush_position += 1;
            if self.fail_flush_at == Some(position) {
                return Err(io::Error::other("scripted record flush failure"));
            }

            let record = std::mem::take(&mut self.pending_record);
            self.calls
                .borrow_mut()
                .push(ScriptedCall::RecordCommitted(record));
            Ok(())
        }
    }

    fn record_calls<C>() -> ScriptedCalls<C> {
        Rc::new(RefCell::new(Vec::new()))
    }

    fn scripted_turn_open<C>(
        calls: ScriptedCalls<C>,
        start_time: Timestamp,
    ) -> Certificate<ScriptedWriter<C>, TurnOpen> {
        let certificate = Certificate::mint(
            certificate_journal(ScriptedWriter::new(calls, None), 512),
            start_time,
        );
        match certificate.run_started() {
            Ok(certificate) => certificate,
            Err(_) => panic!("a C21 transition fixture must commit RunStarted"),
        }
    }

    fn scripted_continue_effects<C>(
        calls: ScriptedCalls<C>,
        start_time: Timestamp,
    ) -> Certificate<ScriptedWriter<C>, EffectsComplete<answer::Continue>> {
        let turn_open = match scripted_turn_open(calls, start_time).classify(TurnOutcome::Continue)
        {
            ClassifiedTurn::Continue(certificate) => certificate,
            ClassifiedTurn::Stop(_) => {
                panic!("a Continue answer must produce the Continue-typed phase")
            }
        };
        let commands = BoundedBuffer::<C>::new(0).expect("a zero-capacity C21 batch must reserve");
        turn_open.no_commands(&commands)
    }

    fn scripted_stop_effects<C>(
        calls: ScriptedCalls<C>,
        start_time: Timestamp,
    ) -> Certificate<ScriptedWriter<C>, EffectsComplete<answer::Stop>> {
        let turn_open = match scripted_turn_open(calls, start_time).classify(TurnOutcome::Stop) {
            ClassifiedTurn::Stop(certificate) => certificate,
            ClassifiedTurn::Continue(_) => {
                panic!("a Stop answer must produce the Stop-typed phase")
            }
        };
        let commands = BoundedBuffer::<C>::new(0).expect("a zero-capacity C21 batch must reserve");
        turn_open.no_commands(&commands)
    }

    mod certificate_minting {
        use super::*;

        /// Invariant: every newly minted certificate begins at prospective event
        /// index zero, regardless of its frozen start time.
        /// Design Doc: RUN-ENFORCEMENT
        #[test]
        fn minting_asserts_the_prospective_index_base() {
            let certificate: Certificate<_, Initial> = Certificate::mint(
                certificate_journal(Vec::new(), 256),
                Timestamp::from_nanos(73),
            );

            assert_eq!(
                certificate.index.as_u64(),
                0,
                "a newly minted Initial certificate must store prospective index zero"
            );
        }

        /// Invariant: starting a run commits its versioned record first, using the
        /// certificate's prospective index and frozen logical time.
        /// Design Doc: RUN-RECORDS
        #[test]
        fn run_started_commits_the_versioned_first_record() {
            let mut bytes = Vec::new();
            let certificate = Certificate::mint(
                certificate_journal(&mut bytes, 256),
                Timestamp::from_nanos(100),
            );
            let turn_open = match certificate.run_started() {
                Ok(certificate) => certificate,
                Err(_) => panic!("a valid RunStarted record must commit successfully"),
            };

            assert_eq!(
                turn_open.index.as_u64(),
                0,
                "the RunStarted transition must preserve accepted start-turn index zero"
            );
            assert_eq!(
                turn_open.last_time,
                Timestamp::from_nanos(100),
                "the RunStarted transition must preserve the frozen start time"
            );
            drop(turn_open);
            assert_eq!(
                bytes,
                br#"{"record_kind":"RunStarted","index":0,"schema_version":1,"logical_time":100}
"#,
                "RunStarted must be the versioned first Journal record with exact field order"
            );
        }

        /// Invariant: the RunStarted transition preserves logical times at zero,
        /// one, and the largest representable nanosecond value without truncation.
        #[test]
        fn run_started_preserves_frozen_time_boundaries() {
            for nanos in [0, 1, u64::MAX] {
                let mut bytes = Vec::new();
                let certificate = Certificate::mint(
                    certificate_journal(&mut bytes, 256),
                    Timestamp::from_nanos(nanos),
                );
                let turn_open = match certificate.run_started() {
                    Ok(certificate) => certificate,
                    Err(_) => panic!("a boundary-valued start time must commit successfully"),
                };

                assert_eq!(
                    turn_open.last_time,
                    Timestamp::from_nanos(nanos),
                    "the RunStarted successor must retain the exact frozen start time"
                );
                drop(turn_open);
                assert_eq!(
                    bytes,
                    format!(
                        "{{\"record_kind\":\"RunStarted\",\"index\":0,\"schema_version\":1,\"logical_time\":{nanos}}}\n"
                    )
                    .into_bytes(),
                    "RunStarted bytes must preserve a boundary-valued frozen start time"
                );
            }
        }
    }

    mod certificate_fatal_path {
        use super::*;

        struct FailingWriter {
            dropped: Rc<Cell<bool>>,
        }

        impl io::Write for FailingWriter {
            fn write(&mut self, _bytes: &[u8]) -> io::Result<usize> {
                Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "intentional RunStarted write failure",
                ))
            }

            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }

        impl Drop for FailingWriter {
            fn drop(&mut self) {
                self.dropped.set(true);
            }
        }

        /// Invariant: if the first record cannot commit, the failure identifies
        /// RunStarted and the consumed certificate destroys its Journal.
        /// Design Doc: RUN-GRAMMAR
        #[test]
        fn commit_failure_names_run_started_and_destroys_the_journal() {
            let dropped = Rc::new(Cell::new(false));
            let certificate = Certificate::mint(
                certificate_journal(
                    FailingWriter {
                        dropped: Rc::clone(&dropped),
                    },
                    256,
                ),
                Timestamp::from_nanos(0),
            );
            assert!(
                !dropped.get(),
                "minting must transfer the live Journal into the certificate"
            );

            let fatal = match certificate.run_started() {
                Ok(_) => panic!("a failing writer must prevent the RunStarted transition"),
                Err(fatal) => fatal,
            };

            assert_eq!(
                fatal.record_kind,
                RecordKind::RunStarted,
                "a failed RunStarted commit must retain the RunStarted record kind"
            );
            assert_eq!(
                fatal.outcome, None,
                "a RunStarted commit failure must not carry a turn outcome"
            );
            match fatal.error {
                JournalError::Sink { operation, error } => {
                    assert_eq!(
                        operation,
                        crate::journal::SinkOperation::Write,
                        "the RunStarted fatal must preserve the failed sink operation"
                    );
                    assert_eq!(
                        error.kind(),
                        io::ErrorKind::BrokenPipe,
                        "the RunStarted fatal must preserve the writer's error kind"
                    );
                }
                _ => panic!("a RunStarted writer failure must retain its typed sink error"),
            }
            assert!(
                dropped.get(),
                "a failed RunStarted transition must destroy its consumed Journal"
            );
        }
    }

    mod certificate_bounds {
        use super::*;

        const RUN_STARTED_ZERO: &[u8] =
            b"{\"record_kind\":\"RunStarted\",\"index\":0,\"schema_version\":1,\"logical_time\":0}\n";

        /// Invariant: a RunStarted record that exactly fills the configured record
        /// capacity commits successfully, including its reserved newline.
        #[test]
        fn run_started_succeeds_at_exact_record_capacity() {
            let mut bytes = Vec::new();
            let record_capacity = RUN_STARTED_ZERO.len() - 1;
            let certificate = Certificate::mint(
                certificate_journal(&mut bytes, record_capacity),
                Timestamp::from_nanos(0),
            );
            let turn_open = match certificate.run_started() {
                Ok(certificate) => certificate,
                Err(_) => panic!("a RunStarted record at exact capacity must commit"),
            };

            drop(turn_open);
            assert_eq!(
                bytes, RUN_STARTED_ZERO,
                "an exact-capacity RunStarted record must commit its complete JSON line"
            );
        }

        /// Invariant: a RunStarted record one byte beyond capacity fails before
        /// writing any partial bytes to its sink.
        #[test]
        fn one_byte_past_record_capacity_fails_without_output() {
            let mut bytes = Vec::new();
            let record_capacity = RUN_STARTED_ZERO.len() - 2;
            let certificate = Certificate::mint(
                certificate_journal(&mut bytes, record_capacity),
                Timestamp::from_nanos(0),
            );

            let fatal = match certificate.run_started() {
                Ok(_) => panic!("a RunStarted record beyond capacity must fail"),
                Err(fatal) => fatal,
            };

            assert_eq!(
                fatal.record_kind,
                RecordKind::RunStarted,
                "an oversized RunStarted record must retain its record kind"
            );
            assert_eq!(
                fatal.outcome, None,
                "an oversized RunStarted record must not carry a turn outcome"
            );
            assert!(
                matches!(fatal.error, JournalError::BoundExceeded),
                "a RunStarted record one byte past capacity must report BoundExceeded"
            );
            assert!(
                bytes.is_empty(),
                "an oversized RunStarted record must fail before writing any sink bytes"
            );
        }
    }

    mod certificate_auto_traits {
        use super::*;

        /// Invariant: a certificate remains sendable and shareable when its phase
        /// marker itself is neither sendable nor shareable.
        #[test]
        fn phase_marker_does_not_control_send_or_sync() {
            fn require_send_sync<T: Send + Sync>() {}

            require_send_sync::<Certificate<Vec<u8>, Rc<()>>>();
        }
    }

    mod turn_classification {
        use super::*;

        const RUN_STARTED_AT_ZERO: &[u8] =
            b"{\"record_kind\":\"RunStarted\",\"index\":0,\"schema_version\":1,\"logical_time\":0}\n";

        fn turn_open<W: io::Write>(writer: W, start_time: Timestamp) -> Certificate<W, TurnOpen> {
            let certificate = Certificate::mint(certificate_journal(writer, 256), start_time);
            match certificate.run_started() {
                Ok(certificate) => certificate,
                Err(_) => panic!("a turn-classification fixture must commit RunStarted"),
            }
        }

        /// Invariant: classifying either non-fatal answer consumes the unclassified
        /// turn and returns a certificate whose phase type permanently names that
        /// answer.
        /// Design Doc: RUN-ENFORCEMENT
        #[test]
        fn classify_fixes_the_answer_in_the_phase_type() {
            fn require_continue<W: io::Write>(
                _certificate: Certificate<W, TurnOpen<answer::Continue>>,
            ) {
            }
            fn require_stop<W: io::Write>(_certificate: Certificate<W, TurnOpen<answer::Stop>>) {}

            match turn_open(Vec::new(), Timestamp::from_nanos(0)).classify(TurnOutcome::Continue) {
                ClassifiedTurn::Continue(certificate) => require_continue(certificate),
                ClassifiedTurn::Stop(_) => {
                    panic!("a Continue answer must produce the Continue-typed phase")
                }
            }
            match turn_open(Vec::new(), Timestamp::from_nanos(0)).classify(TurnOutcome::Stop) {
                ClassifiedTurn::Stop(certificate) => require_stop(certificate),
                ClassifiedTurn::Continue(_) => {
                    panic!("a Stop answer must produce the Stop-typed phase")
                }
            }
        }

        /// Invariant: advancing an empty command batch to effects-complete writes no
        /// record and leaves the existing Journal bytes unchanged.
        /// Design Doc: the Edges table's recordless row, by name
        #[test]
        fn the_empty_batch_edge_commits_nothing() {
            let mut bytes = Vec::new();
            let commands =
                BoundedBuffer::<u8>::new(2).expect("a two-command batch must be reservable");
            let classified =
                turn_open(&mut bytes, Timestamp::from_nanos(0)).classify(TurnOutcome::Continue);
            let turn_open = match classified {
                ClassifiedTurn::Continue(certificate) => certificate,
                ClassifiedTurn::Stop(_) => {
                    panic!("a Continue answer must retain its phase during the empty edge")
                }
            };

            let effects_complete = turn_open.no_commands(&commands);

            drop(effects_complete);
            assert_eq!(
                bytes, RUN_STARTED_AT_ZERO,
                "the empty-batch edge must not append a Journal record"
            );
        }

        /// Invariant: the recordless command-batch edge rejects a nonempty batch
        /// instead of silently advancing past undispatched commands.
        /// Design Doc: ASSERT-INVARIANTS
        #[test]
        #[should_panic(expected = "ASSERT-INVARIANTS")]
        fn no_commands_panics_on_a_nonempty_buffer() {
            let mut commands =
                BoundedBuffer::new(2).expect("a two-command batch must be reservable");
            commands
                .try_push("pending")
                .expect("the first command must fit");
            let classified =
                turn_open(Vec::new(), Timestamp::from_nanos(0)).classify(TurnOutcome::Continue);
            let turn_open = match classified {
                ClassifiedTurn::Continue(certificate) => certificate,
                ClassifiedTurn::Stop(_) => {
                    panic!("a Continue answer must produce the Continue-typed phase")
                }
            };

            let _effects_complete = turn_open.no_commands(&commands);
        }

        /// Invariant: classifying either answer changes no Journal bytes before the
        /// chosen effects transition runs.
        #[test]
        fn classify_commits_nothing_for_either_answer() {
            for answer in [TurnOutcome::Continue, TurnOutcome::Stop] {
                let mut bytes = Vec::new();
                let classified = turn_open(&mut bytes, Timestamp::from_nanos(0)).classify(answer);

                drop(classified);
                assert_eq!(
                    bytes, RUN_STARTED_AT_ZERO,
                    "classification must not append a Journal record for either answer"
                );
            }
        }

        /// Invariant: answer classification preserves the accepted index and exact
        /// logical time, including both time-domain boundaries.
        #[test]
        fn classify_preserves_certificate_state_for_both_answers() {
            fn assert_state<W: io::Write, A>(
                certificate: Certificate<W, TurnOpen<A>>,
                expected_time: Timestamp,
            ) {
                assert_eq!(
                    certificate.index.as_u64(),
                    0,
                    "classification must preserve the accepted turn index"
                );
                assert_eq!(
                    certificate.last_time, expected_time,
                    "classification must preserve the exact accepted logical time"
                );
            }

            for (answer, nanos) in [(TurnOutcome::Continue, 0), (TurnOutcome::Stop, u64::MAX)] {
                let classified =
                    turn_open(Vec::new(), Timestamp::from_nanos(nanos)).classify(answer);

                match (answer, classified) {
                    (TurnOutcome::Continue, ClassifiedTurn::Continue(certificate)) => {
                        assert_state(certificate, Timestamp::from_nanos(nanos));
                    }
                    (TurnOutcome::Stop, ClassifiedTurn::Stop(certificate)) => {
                        assert_state(certificate, Timestamp::from_nanos(nanos));
                    }
                    _ => panic!("classification must preserve the selected answer variant"),
                }
            }
        }

        /// Invariant: taking the empty-batch edge preserves the certificate's
        /// accepted index and last accepted logical time in the effects-complete
        /// phase.
        #[test]
        fn no_commands_preserves_certificate_state() {
            let commands =
                BoundedBuffer::<u8>::new(1).expect("a one-command batch must be reservable");
            let classified =
                turn_open(Vec::new(), Timestamp::from_nanos(u64::MAX)).classify(TurnOutcome::Stop);
            let turn_open = match classified {
                ClassifiedTurn::Stop(certificate) => certificate,
                ClassifiedTurn::Continue(_) => {
                    panic!("a Stop answer must retain its phase during the empty edge")
                }
            };
            let expected_index = turn_open.index;
            let expected_time = turn_open.last_time;

            let effects_complete = turn_open.no_commands(&commands);

            assert_eq!(
                effects_complete.index, expected_index,
                "the empty-batch edge must preserve the certificate's accepted index"
            );
            assert_eq!(
                effects_complete.last_time, expected_time,
                "the empty-batch edge must preserve the certificate's last accepted logical time"
            );
            assert_eq!(
                effects_complete.last_time.as_nanos(),
                u64::MAX,
                "the empty-batch edge fixture must exercise the maximum nonzero logical time"
            );
        }

        /// Invariant: an empty zero-capacity command batch can take the recordless
        /// edge without writing or requiring a storage slot.
        #[test]
        fn zero_capacity_empty_batch_advances_without_a_record() {
            let mut bytes = Vec::new();
            let commands =
                BoundedBuffer::<u8>::new(0).expect("a zero-capacity batch must be reservable");
            let classified =
                turn_open(&mut bytes, Timestamp::from_nanos(0)).classify(TurnOutcome::Stop);
            let turn_open = match classified {
                ClassifiedTurn::Stop(certificate) => certificate,
                ClassifiedTurn::Continue(_) => {
                    panic!("a Stop answer must retain its phase during the empty edge")
                }
            };

            let effects_complete = turn_open.no_commands(&commands);

            assert!(
                commands.is_empty(),
                "taking the recordless edge must leave the zero-capacity batch empty"
            );
            drop(effects_complete);
            assert_eq!(
                bytes, RUN_STARTED_AT_ZERO,
                "a zero-capacity empty batch must not append a Journal record"
            );
        }

        /// Invariant: rejecting a full command batch leaves every command and all
        /// previously committed Journal bytes unchanged after the panic.
        #[test]
        fn full_batch_rejection_preserves_buffer_and_journal() {
            let mut bytes = Vec::new();
            let mut commands =
                BoundedBuffer::new(2).expect("a two-command batch must be reservable");
            commands
                .try_push("first")
                .expect("the first command must fit");
            commands
                .try_push("second")
                .expect("the second command must fit");
            let classified =
                turn_open(&mut bytes, Timestamp::from_nanos(0)).classify(TurnOutcome::Continue);
            let turn_open = match classified {
                ClassifiedTurn::Continue(certificate) => certificate,
                ClassifiedTurn::Stop(_) => {
                    panic!("a Continue answer must produce the Continue-typed phase")
                }
            };

            let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _effects_complete = turn_open.no_commands(&commands);
            }));

            assert!(
                panic.is_err(),
                "the recordless edge must panic when the command batch is full"
            );
            assert_eq!(
                commands.as_slice(),
                &["first", "second"],
                "rejecting a full batch must preserve every pending command"
            );
            assert_eq!(
                bytes, RUN_STARTED_AT_ZERO,
                "rejecting a full batch must not append a Journal record"
            );
        }
    }

    mod batch_dispatch {
        use super::*;
        use crate::{EnvironmentOperation, FatalCause};

        const RUN_STARTED_AT_ZERO: &[u8] =
            b"{\"record_kind\":\"RunStarted\",\"index\":0,\"schema_version\":1,\"logical_time\":0}\n";
        const COMMANDS_PREPARED_AT_ZERO: &[u8] =
            b"{\"record_kind\":\"CommandsPrepared\",\"index\":0,\"commands\":[10,20]}\n";
        const COMMANDS_DISPATCHED_AT_ZERO: &[u8] =
            b"{\"record_kind\":\"CommandsDispatched\",\"index\":0}\n";
        type DispatchResult<C, A> = Result<
            Certificate<ScriptedWriter<C>, EffectsComplete<A>>,
            FatalCause<(), &'static str>,
        >;

        fn scripted_calls<C>() -> ScriptedCalls<C> {
            Rc::new(RefCell::new(Vec::new()))
        }

        fn turn_open<C>(
            calls: ScriptedCalls<C>,
            fail_flush_at: Option<usize>,
            start_time: Timestamp,
        ) -> Certificate<ScriptedWriter<C>, TurnOpen> {
            let certificate = Certificate::mint(
                certificate_journal(ScriptedWriter::new(calls, fail_flush_at), 512),
                start_time,
            );
            match certificate.run_started() {
                Ok(certificate) => certificate,
                Err(_) => panic!("a batch-dispatch fixture must commit RunStarted"),
            }
        }

        fn dispatch<A: answer::Answer, C: Serialize>(
            certificate: Certificate<ScriptedWriter<C>, TurnOpen<A>>,
            environment: &mut ScriptedEnvironment<C>,
            commands: &mut BoundedBuffer<C>,
        ) -> DispatchResult<C, A> {
            certificate.dispatch_batch(environment, commands)
        }

        fn continue_turn<C>(
            calls: ScriptedCalls<C>,
            fail_flush_at: Option<usize>,
        ) -> Certificate<ScriptedWriter<C>, TurnOpen<answer::Continue>> {
            match turn_open(calls, fail_flush_at, Timestamp::from_nanos(0))
                .classify(TurnOutcome::Continue)
            {
                ClassifiedTurn::Continue(certificate) => certificate,
                ClassifiedTurn::Stop(_) => {
                    panic!("a Continue answer must produce the Continue-typed phase")
                }
            }
        }

        fn expect_journal_fatal<A: answer::Answer, C>(
            result: DispatchResult<C, A>,
        ) -> JournalFatal {
            match result {
                Err(FatalCause::Journal(fatal)) => fatal,
                Err(_) => panic!("a scripted Journal failure must remain the fatal cause"),
                Ok(_) => panic!("a scripted Journal failure must prevent phase advancement"),
            }
        }

        fn expect_environment_fatal<A: answer::Answer, C>(
            result: DispatchResult<C, A>,
        ) -> crate::EnvironmentFatal<&'static str> {
            match result {
                Err(FatalCause::Environment(fatal)) => fatal,
                Err(_) => panic!("a scripted dispatch failure must remain the fatal cause"),
                Ok(_) => panic!("a scripted dispatch failure must prevent phase advancement"),
            }
        }

        fn direct_continue_turn<W: io::Write>(
            writer: W,
            max_record_bytes: usize,
        ) -> Certificate<W, TurnOpen<answer::Continue>> {
            Certificate {
                journal: certificate_journal(writer, max_record_bytes),
                index: EventIndex::new(0),
                last_time: Timestamp::from_nanos(0),
                _phase: PhantomData,
            }
        }

        /// Invariant: a nonempty batch is durably recorded before its commands are
        /// handed off in order, and completion is recorded only after every handoff.
        /// Design Doc: A5
        #[test]
        fn prepared_then_each_handoff_in_order_then_dispatched() {
            let calls = scripted_calls();
            let certificate = continue_turn(Rc::clone(&calls), None);
            let mut environment = ScriptedEnvironment::new(Rc::clone(&calls), None);
            let mut commands = BoundedBuffer::new(3).expect("three command slots must reserve");
            for command in [10, 20, 30] {
                commands
                    .try_push(command)
                    .expect("each command through exact capacity must fit");
            }

            let effects_complete = dispatch(certificate, &mut environment, &mut commands);

            assert!(
                effects_complete.is_ok(),
                "a successful full-batch handoff must reach effects-complete"
            );
            assert!(
                commands.is_empty(),
                "a successful batch handoff must drain every command"
            );
            assert_eq!(
                &*calls.borrow(),
                &[
                    ScriptedCall::RecordCommitted(RUN_STARTED_AT_ZERO.to_vec()),
                    ScriptedCall::RecordCommitted(
                        b"{\"record_kind\":\"CommandsPrepared\",\"index\":0,\"commands\":[10,20,30]}\n"
                            .to_vec(),
                    ),
                    ScriptedCall::Dispatch(10),
                    ScriptedCall::Dispatch(20),
                    ScriptedCall::Dispatch(30),
                    ScriptedCall::RecordCommitted(
                        b"{\"record_kind\":\"CommandsDispatched\",\"index\":0}\n".to_vec(),
                    ),
                ],
                "the committed intent, ordered handoffs, and committed completion must be bracketed exactly"
            );
        }

        /// Invariant: when command handoff fails at a batch position, only the
        /// successful prefix remains handed off and every later command is discarded.
        /// Design Doc: the Prepared phase row, by name
        #[test]
        fn error_at_position_k_keeps_the_prefix_and_discards_the_suffix() {
            let calls = scripted_calls();
            let certificate = continue_turn(Rc::clone(&calls), None);
            let mut environment = ScriptedEnvironment::new(Rc::clone(&calls), Some(1));
            let mut commands = BoundedBuffer::new(3).expect("three command slots must reserve");
            for command in [10, 20, 30] {
                commands
                    .try_push(command)
                    .expect("each scripted command must fit");
            }

            let fatal =
                expect_environment_fatal(dispatch(certificate, &mut environment, &mut commands));

            assert_eq!(
                fatal.operation,
                EnvironmentOperation::Dispatch { position: 1 },
                "a dispatch failure must identify its zero-based batch position"
            );
            assert_eq!(
                fatal.error, "scripted dispatch failure",
                "a dispatch failure must preserve the Environment error"
            );
            assert!(
                commands.is_empty(),
                "a dispatch failure must discard the failed command and undelivered suffix"
            );
            assert_eq!(
                &*calls.borrow(),
                &[
                    ScriptedCall::RecordCommitted(RUN_STARTED_AT_ZERO.to_vec()),
                    ScriptedCall::RecordCommitted(
                        b"{\"record_kind\":\"CommandsPrepared\",\"index\":0,\"commands\":[10,20,30]}\n"
                            .to_vec(),
                    ),
                    ScriptedCall::Dispatch(10),
                    ScriptedCall::DispatchFailed(20),
                ],
                "a failed middle handoff must retain only the handed-off prefix and never attempt the suffix"
            );
        }

        /// Invariant: if the prepared-command record cannot commit, no command is
        /// handed off and the complete batch remains available for fatal cleanup.
        /// Design Doc: RUN-GRAMMAR
        #[test]
        fn prepared_commit_failure_precedes_any_handoff() {
            let calls = scripted_calls();
            let certificate = continue_turn(Rc::clone(&calls), Some(1));
            let mut environment = ScriptedEnvironment::new(Rc::clone(&calls), None);
            let mut commands = BoundedBuffer::new(1).expect("one command slot must reserve");
            commands.try_push(7).expect("the scripted command must fit");

            let fatal =
                expect_journal_fatal(dispatch(certificate, &mut environment, &mut commands));

            assert_eq!(
                fatal.record_kind,
                RecordKind::CommandsPrepared,
                "a failed intent commit must identify CommandsPrepared"
            );
            assert_eq!(
                fatal.outcome, None,
                "a failed CommandsPrepared record must carry no turn outcome"
            );
            assert!(
                matches!(
                    fatal.error,
                    JournalError::Sink {
                        operation: crate::journal::SinkOperation::Flush,
                        ..
                    }
                ),
                "the scripted prepared-record flush failure must preserve its typed Journal error"
            );
            assert_eq!(
                commands.as_slice(),
                &[7],
                "a prepared-record failure must leave the complete undrained batch intact"
            );
            assert_eq!(
                &*calls.borrow(),
                &[ScriptedCall::RecordCommitted(RUN_STARTED_AT_ZERO.to_vec())],
                "a prepared-record failure must occur before every Environment handoff"
            );
        }

        /// Invariant: if the dispatched-command record cannot commit, every command
        /// has already been handed off and the drained batch remains empty.
        /// Design Doc: the Edges table, by name
        #[test]
        fn dispatched_commit_failure_follows_every_handoff() {
            let calls = scripted_calls();
            let certificate = continue_turn(Rc::clone(&calls), Some(2));
            let mut environment = ScriptedEnvironment::new(Rc::clone(&calls), None);
            let mut commands = BoundedBuffer::new(2).expect("two command slots must reserve");
            commands.try_push(4).expect("the first command must fit");
            commands.try_push(5).expect("the second command must fit");

            let fatal =
                expect_journal_fatal(dispatch(certificate, &mut environment, &mut commands));

            assert_eq!(
                fatal.record_kind,
                RecordKind::CommandsDispatched,
                "a failed completion commit must identify CommandsDispatched"
            );
            assert_eq!(
                fatal.outcome, None,
                "a failed CommandsDispatched record must carry no turn outcome"
            );
            assert!(
                commands.is_empty(),
                "a dispatched-record failure must leave the already handed-off batch empty"
            );
            assert_eq!(
                &*calls.borrow(),
                &[
                    ScriptedCall::RecordCommitted(RUN_STARTED_AT_ZERO.to_vec()),
                    ScriptedCall::RecordCommitted(
                        b"{\"record_kind\":\"CommandsPrepared\",\"index\":0,\"commands\":[4,5]}\n"
                            .to_vec(),
                    ),
                    ScriptedCall::Dispatch(4),
                    ScriptedCall::Dispatch(5),
                ],
                "a dispatched-record failure must follow every ordered handoff without committing completion"
            );
        }

        /// Invariant: the dispatch transition rejects an empty command batch rather
        /// than recording intent for work that does not exist.
        /// Design Doc: ASSERT-INVARIANTS
        #[test]
        #[should_panic(expected = "ASSERT-INVARIANTS")]
        fn an_empty_buffer_is_an_invariant_panic() {
            let calls = scripted_calls();
            let certificate = continue_turn(Rc::clone(&calls), None);
            let mut environment = ScriptedEnvironment::new(calls, None);
            let mut commands =
                BoundedBuffer::<u8>::new(0).expect("a zero-capacity command batch must reserve");

            let _effects_complete = dispatch(certificate, &mut environment, &mut commands);
        }

        /// Invariant: handing off the sole command in a one-slot batch returns a
        /// certificate that still encodes the Stop answer, preserves its accepted
        /// index and logical time, and leaves the command slot reusable.
        #[test]
        fn one_command_batch_preserves_phase_state_and_reusable_capacity() {
            let calls = scripted_calls();
            let certificate =
                match turn_open(Rc::clone(&calls), None, Timestamp::from_nanos(u64::MAX))
                    .classify(TurnOutcome::Stop)
                {
                    ClassifiedTurn::Stop(certificate) => certificate,
                    ClassifiedTurn::Continue(_) => {
                        panic!("a Stop answer must produce the Stop-typed phase")
                    }
                };
            let mut environment = ScriptedEnvironment::new(calls, None);
            let mut commands = BoundedBuffer::new(1).expect("one command slot must reserve");
            commands
                .try_push(9)
                .expect("the sole command must fit exactly");

            let effects_complete = match dispatch(certificate, &mut environment, &mut commands) {
                Ok(certificate) => certificate,
                Err(_) => panic!("a one-command batch must dispatch successfully"),
            };

            fn require_stop<W: io::Write>(
                _certificate: &Certificate<W, EffectsComplete<answer::Stop>>,
            ) {
            }
            require_stop(&effects_complete);
            assert_eq!(
                effects_complete.index.as_u64(),
                0,
                "batch dispatch must preserve the accepted turn index"
            );
            assert_eq!(
                effects_complete.last_time.as_nanos(),
                u64::MAX,
                "batch dispatch must preserve the exact last accepted logical time"
            );
            assert_eq!(
                commands.capacity(),
                1,
                "draining the sole command must preserve the batch's logical capacity"
            );
            commands
                .try_push(10)
                .expect("the drained command slot must remain reusable");
            assert_eq!(
                commands.as_slice(),
                &[10],
                "the batch must accept a replacement command after successful dispatch"
            );
        }

        /// Invariant: failure at the first command hands off no prefix and discards
        /// the failed command together with the entire remaining batch.
        #[test]
        fn first_position_failure_hands_off_nothing_and_discards_all_commands() {
            let calls = scripted_calls();
            let certificate = continue_turn(Rc::clone(&calls), None);
            let mut environment = ScriptedEnvironment::new(Rc::clone(&calls), Some(0));
            let mut commands = BoundedBuffer::new(3).expect("three command slots must reserve");
            for command in [1, 2, 3] {
                commands
                    .try_push(command)
                    .expect("each scripted command must fit");
            }

            let fatal =
                expect_environment_fatal(dispatch(certificate, &mut environment, &mut commands));

            assert_eq!(
                fatal.operation,
                EnvironmentOperation::Dispatch { position: 0 },
                "a first-command failure must report position zero"
            );
            assert!(
                commands.is_empty(),
                "a first-command failure must discard the entire drained batch"
            );
            assert!(
                matches!(calls.borrow().last(), Some(ScriptedCall::DispatchFailed(1))),
                "a first-command failure must attempt only the first command"
            );
        }

        /// Invariant: failure at the last command preserves every earlier handoff,
        /// does not hand off the failing command, and leaves no batch residue.
        #[test]
        fn last_position_failure_keeps_the_full_prefix_and_discards_the_failed_command() {
            let calls = scripted_calls();
            let certificate = continue_turn(Rc::clone(&calls), None);
            let mut environment = ScriptedEnvironment::new(Rc::clone(&calls), Some(2));
            let mut commands = BoundedBuffer::new(3).expect("three command slots must reserve");
            for command in [1, 2, 3] {
                commands
                    .try_push(command)
                    .expect("each scripted command must fit");
            }

            let fatal =
                expect_environment_fatal(dispatch(certificate, &mut environment, &mut commands));

            assert_eq!(
                fatal.operation,
                EnvironmentOperation::Dispatch { position: 2 },
                "a last-command failure must report the final zero-based position"
            );
            assert!(
                commands.is_empty(),
                "a last-command failure must leave the drained batch empty"
            );
            let calls = calls.borrow();
            assert!(
                calls.ends_with(&[
                    ScriptedCall::Dispatch(1),
                    ScriptedCall::Dispatch(2),
                    ScriptedCall::DispatchFailed(3),
                ]),
                "a last-command failure must retain every successful prefix handoff in order"
            );
        }

        /// Invariant: a command serialization failure occurs while the batch is
        /// still borrowed, leaving every command intact and the Environment untouched.
        #[test]
        fn command_serialization_failure_leaves_the_batch_undrained() {
            struct Unserializable;

            impl Serialize for Unserializable {
                fn serialize<S: Serializer>(&self, _serializer: S) -> Result<S::Ok, S::Error> {
                    Err(serde::ser::Error::custom(
                        "scripted command serialization failure",
                    ))
                }
            }

            let calls = scripted_calls();
            let certificate = continue_turn(Rc::clone(&calls), None);
            let mut environment = ScriptedEnvironment::new(Rc::clone(&calls), None);
            let mut commands =
                BoundedBuffer::new(1).expect("one unserializable command slot must reserve");
            commands
                .try_push(Unserializable)
                .unwrap_or_else(|_| panic!("the unserializable command must fit"));

            let fatal =
                expect_journal_fatal(dispatch(certificate, &mut environment, &mut commands));

            assert_eq!(
                fatal.record_kind,
                RecordKind::CommandsPrepared,
                "a command encoding failure must identify CommandsPrepared"
            );
            assert!(
                matches!(fatal.error, JournalError::Encode(_)),
                "a command serializer failure must retain the Journal Encode error"
            );
            assert_eq!(
                commands.len(),
                1,
                "a command serializer failure must leave the complete batch undrained"
            );
            assert_eq!(
                environment.dispatch_position, 0,
                "a command serializer failure must precede every Environment handoff"
            );
            assert_eq!(
                calls.borrow().len(),
                1,
                "a command serializer failure must commit no record after RunStarted"
            );
        }

        /// Invariant: rejecting an empty batch changes neither Journal history nor
        /// Environment state and leaves the empty buffer reusable after the panic.
        #[test]
        fn empty_batch_panic_has_no_record_or_environment_side_effect() {
            let calls = scripted_calls();
            let certificate = continue_turn(Rc::clone(&calls), None);
            let mut environment = ScriptedEnvironment::new(Rc::clone(&calls), None);
            let mut commands = BoundedBuffer::<u8>::new(1).expect("one command slot must reserve");

            let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _effects_complete = dispatch(certificate, &mut environment, &mut commands);
            }));

            assert!(
                panic.is_err(),
                "an empty command batch must panic before phase advancement"
            );
            assert_eq!(
                &*calls.borrow(),
                &[ScriptedCall::RecordCommitted(RUN_STARTED_AT_ZERO.to_vec())],
                "empty-batch rejection must append no Journal record or Environment call"
            );
            assert!(
                commands.is_empty(),
                "empty-batch rejection must leave the command buffer empty"
            );
            commands
                .try_push(1)
                .expect("the rejected empty batch must remain reusable");
        }

        /// Invariant: a prepared-command record that exactly fills the configured
        /// record capacity commits before the first handoff, and the shorter
        /// dispatched-command record then commits after the last.
        #[test]
        fn prepared_record_succeeds_at_exact_record_capacity() {
            let calls = scripted_calls();
            let mut bytes = Vec::new();
            let certificate = direct_continue_turn(&mut bytes, COMMANDS_PREPARED_AT_ZERO.len() - 1);
            let mut environment = ScriptedEnvironment::new(Rc::clone(&calls), None);
            let mut commands = BoundedBuffer::new(2).expect("two command slots must reserve");
            for command in [10, 20] {
                commands
                    .try_push(command)
                    .expect("each command through exact capacity must fit");
            }

            let effects_complete =
                match certificate.dispatch_batch::<_, _, ()>(&mut environment, &mut commands) {
                    Ok(certificate) => certificate,
                    Err(_) => panic!("a CommandsPrepared record at exact capacity must commit"),
                };

            drop(effects_complete);
            assert_eq!(
                bytes,
                [COMMANDS_PREPARED_AT_ZERO, COMMANDS_DISPATCHED_AT_ZERO].concat(),
                "an exact-capacity CommandsPrepared record must commit its complete line and leave room for the shorter CommandsDispatched record"
            );
            assert_eq!(
                &*calls.borrow(),
                &[ScriptedCall::Dispatch(10), ScriptedCall::Dispatch(20)],
                "an exact-capacity intent record must be followed by every handoff in order"
            );
            assert!(
                commands.is_empty(),
                "a successful exact-capacity batch must drain every command"
            );
        }

        /// Invariant: a prepared-command record one byte beyond capacity fails
        /// without output, hands off no command, and leaves the complete batch
        /// intact.
        /// Design Doc: JRN-ENCODE
        #[test]
        fn prepared_record_one_byte_past_capacity_hands_off_nothing() {
            let calls = scripted_calls();
            let mut bytes = Vec::new();
            let certificate = direct_continue_turn(&mut bytes, COMMANDS_PREPARED_AT_ZERO.len() - 2);
            let mut environment = ScriptedEnvironment::new(Rc::clone(&calls), None);
            let mut commands = BoundedBuffer::new(2).expect("two command slots must reserve");
            for command in [10, 20] {
                commands
                    .try_push(command)
                    .expect("each command through exact capacity must fit");
            }

            let fatal = match certificate
                .dispatch_batch::<_, _, ()>(&mut environment, &mut commands)
            {
                Err(FatalCause::Journal(fatal)) => fatal,
                Err(_) => panic!("an oversized CommandsPrepared record must be a Journal fatal"),
                Ok(_) => panic!("a CommandsPrepared record beyond capacity must fail"),
            };

            assert_eq!(
                fatal.record_kind,
                RecordKind::CommandsPrepared,
                "an oversized CommandsPrepared record must retain its record kind"
            );
            assert_eq!(
                fatal.outcome, None,
                "an oversized CommandsPrepared record must not carry a turn outcome"
            );
            assert!(
                matches!(fatal.error, JournalError::BoundExceeded),
                "a CommandsPrepared record one byte past capacity must report BoundExceeded"
            );
            assert!(
                bytes.is_empty(),
                "an oversized CommandsPrepared record must fail before writing any sink bytes"
            );
            assert!(
                calls.borrow().is_empty(),
                "an oversized CommandsPrepared record must precede every Environment handoff"
            );
            assert_eq!(
                commands.as_slice(),
                &[10, 20],
                "an oversized CommandsPrepared record must leave the complete batch intact"
            );
        }
    }

    mod turn_checkpoint {
        use super::*;
        use crate::{EnvironmentOperation, FatalCause};

        const RUN_STARTED_AT_ZERO: &[u8] =
            b"{\"record_kind\":\"RunStarted\",\"index\":0,\"schema_version\":1,\"logical_time\":0}\n";
        const TURN_COMPLETED_CONTINUE_AT_ZERO: &[u8] =
            b"{\"record_kind\":\"TurnCompleted\",\"index\":0,\"outcome\":\"Continue\"}\n";

        /// Invariant: every effects-complete turn observes the Environment error
        /// latch once after effects finish and before its completion record commits.
        /// Design Doc: RUN-CHECKPOINT
        #[test]
        fn the_snapshot_is_taken_exactly_once() {
            let calls = record_calls();
            let effects_complete =
                scripted_continue_effects(Rc::clone(&calls), Timestamp::from_nanos(0));
            let mut environment = ScriptedEnvironment::<u8>::new(Rc::clone(&calls), None);

            let checkpointed = match effects_complete.checkpoint::<_, ()>(&mut environment) {
                Ok(certificate) => certificate,
                Err(_) => panic!("an empty error latch must permit checkpointing"),
            };
            let between_turns = match checkpointed.complete_continue() {
                Ok(certificate) => certificate,
                Err(_) => panic!("the completion record must commit after checkpointing"),
            };
            drop(between_turns);

            assert_eq!(
                &*calls.borrow(),
                &[
                    ScriptedCall::RecordCommitted(RUN_STARTED_AT_ZERO.to_vec()),
                    ScriptedCall::TakeError,
                    ScriptedCall::RecordCommitted(TURN_COMPLETED_CONTINUE_AT_ZERO.to_vec()),
                ],
                "checkpointing must take exactly one snapshot between effects and completion"
            );
        }

        /// Invariant: after a nonempty command batch, the error-latch snapshot
        /// occurs after every handoff and its dispatched record, but before turn
        /// completion is recorded.
        #[test]
        fn a_dispatched_batch_checkpoints_after_the_last_handoff() {
            let calls = record_calls();
            let turn_open = match scripted_turn_open(Rc::clone(&calls), Timestamp::from_nanos(0))
                .classify(TurnOutcome::Continue)
            {
                ClassifiedTurn::Continue(certificate) => certificate,
                ClassifiedTurn::Stop(_) => {
                    panic!("a Continue answer must produce the Continue-typed phase")
                }
            };
            let mut environment = ScriptedEnvironment::<u8>::new(Rc::clone(&calls), None);
            let mut commands = BoundedBuffer::new(2).expect("two command slots must reserve");
            commands.try_push(4).expect("the first command must fit");
            commands.try_push(5).expect("the second command must fit");

            let effects_complete =
                match turn_open.dispatch_batch::<_, _, ()>(&mut environment, &mut commands) {
                    Ok(certificate) => certificate,
                    Err(_) => panic!("both commands must dispatch successfully"),
                };
            let checkpointed = match effects_complete.checkpoint::<_, ()>(&mut environment) {
                Ok(certificate) => certificate,
                Err(_) => panic!("an empty error latch must permit checkpointing"),
            };
            let between_turns = match checkpointed.complete_continue() {
                Ok(certificate) => certificate,
                Err(_) => panic!("completion must commit after the dispatched checkpoint"),
            };
            drop(between_turns);

            assert!(
                commands.is_empty(),
                "the dispatched checkpoint path must leave the command batch drained"
            );
            assert_eq!(
                &*calls.borrow(),
                &[
                    ScriptedCall::RecordCommitted(RUN_STARTED_AT_ZERO.to_vec()),
                    ScriptedCall::RecordCommitted(
                        b"{\"record_kind\":\"CommandsPrepared\",\"index\":0,\"commands\":[4,5]}\n"
                            .to_vec(),
                    ),
                    ScriptedCall::Dispatch(4),
                    ScriptedCall::Dispatch(5),
                    ScriptedCall::RecordCommitted(
                        b"{\"record_kind\":\"CommandsDispatched\",\"index\":0}\n".to_vec(),
                    ),
                    ScriptedCall::TakeError,
                    ScriptedCall::RecordCommitted(TURN_COMPLETED_CONTINUE_AT_ZERO.to_vec()),
                ],
                "the checkpoint snapshot must follow the final handoff and dispatched record before completion"
            );
        }

        /// Invariant: a pending Environment error at the checkpoint ends the turn
        /// without returning a certificate that could commit a completion record.
        /// Design Doc: RUN-CHECKPOINT
        #[test]
        fn a_pending_error_is_checkpoint_fatal_and_consumes_the_certificate() {
            let calls = record_calls();
            let effects_complete =
                scripted_continue_effects(Rc::clone(&calls), Timestamp::from_nanos(0));
            let mut environment = ScriptedEnvironment::<u8>::new(Rc::clone(&calls), None);
            environment.pending_error = Some("pending checkpoint failure");

            let fatal = match effects_complete.checkpoint::<_, ()>(&mut environment) {
                Err(FatalCause::Environment(fatal)) => fatal,
                Err(_) => panic!("a pending latch error must remain an Environment fatal"),
                Ok(_) => panic!("a pending latch error must prevent phase advancement"),
            };

            assert_eq!(
                fatal.operation,
                EnvironmentOperation::Checkpoint,
                "a pending checkpoint error must identify the checkpoint operation"
            );
            assert_eq!(
                fatal.error, "pending checkpoint failure",
                "a checkpoint fatal must preserve the pending Environment error"
            );
            assert_eq!(
                environment.pending_error, None,
                "a checkpoint fatal must consume the Environment's pending error"
            );
            assert_eq!(
                &*calls.borrow(),
                &[
                    ScriptedCall::RecordCommitted(RUN_STARTED_AT_ZERO.to_vec()),
                    ScriptedCall::TakeError,
                ],
                "a pending checkpoint error must append no completion record"
            );
        }

        /// Invariant: a clean latch snapshot preserves the certificate's accepted
        /// index and logical time for either fixed turn answer.
        #[test]
        fn a_clean_snapshot_preserves_state_for_both_answers() {
            let continue_calls = record_calls();
            let continue_effects =
                scripted_continue_effects(Rc::clone(&continue_calls), Timestamp::from_nanos(0));
            let mut continue_environment =
                ScriptedEnvironment::<u8>::new(Rc::clone(&continue_calls), None);
            let continue_checkpointed =
                match continue_effects.checkpoint::<_, ()>(&mut continue_environment) {
                    Ok(certificate) => certificate,
                    Err(_) => panic!("a clean Continue checkpoint must advance"),
                };

            assert_eq!(
                continue_checkpointed.index.as_u64(),
                0,
                "a Continue checkpoint must preserve event index zero"
            );
            assert_eq!(
                continue_checkpointed.last_time,
                Timestamp::from_nanos(0),
                "a Continue checkpoint must preserve logical time zero"
            );
            assert_eq!(
                &*continue_calls.borrow(),
                &[
                    ScriptedCall::RecordCommitted(RUN_STARTED_AT_ZERO.to_vec()),
                    ScriptedCall::TakeError,
                ],
                "a clean Continue checkpoint must commit no record"
            );

            let stop_calls = record_calls();
            let stop_effects =
                scripted_stop_effects(Rc::clone(&stop_calls), Timestamp::from_nanos(u64::MAX));
            let mut stop_environment = ScriptedEnvironment::<u8>::new(Rc::clone(&stop_calls), None);
            let stop_checkpointed = match stop_effects.checkpoint::<_, ()>(&mut stop_environment) {
                Ok(certificate) => certificate,
                Err(_) => panic!("a clean Stop checkpoint must advance"),
            };

            assert_eq!(
                stop_checkpointed.index.as_u64(),
                0,
                "a Stop checkpoint must preserve event index zero"
            );
            assert_eq!(
                stop_checkpointed.last_time,
                Timestamp::from_nanos(u64::MAX),
                "a Stop checkpoint must preserve the maximum logical time"
            );
            assert_eq!(
                stop_calls
                    .borrow()
                    .iter()
                    .filter(|call| matches!(call, ScriptedCall::TakeError))
                    .count(),
                1,
                "a clean Stop checkpoint must take exactly one latch snapshot"
            );
        }

        /// Invariant: a pending error on a Stop turn is consumed without writing a
        /// completion or shutdown-intent record.
        #[test]
        fn a_stop_path_pending_error_commits_nothing() {
            let calls = record_calls();
            let effects_complete =
                scripted_stop_effects(Rc::clone(&calls), Timestamp::from_nanos(1));
            let mut environment = ScriptedEnvironment::<u8>::new(Rc::clone(&calls), None);
            environment.pending_error = Some("pending stop checkpoint failure");

            let fatal = match effects_complete.checkpoint::<_, ()>(&mut environment) {
                Err(FatalCause::Environment(fatal)) => fatal,
                Err(_) => panic!("a Stop checkpoint error must remain an Environment fatal"),
                Ok(_) => panic!("a pending Stop checkpoint error must prevent advancement"),
            };

            assert_eq!(
                fatal.operation,
                EnvironmentOperation::Checkpoint,
                "a Stop checkpoint error must identify the checkpoint operation"
            );
            assert_eq!(
                fatal.error, "pending stop checkpoint failure",
                "a Stop checkpoint fatal must preserve the pending error"
            );
            assert_eq!(
                calls
                    .borrow()
                    .iter()
                    .filter(|call| matches!(call, ScriptedCall::RecordCommitted(_)))
                    .count(),
                1,
                "a failed Stop checkpoint must leave RunStarted as the only record"
            );
        }
    }

    mod turn_completion {
        use super::*;

        const RUN_STARTED_AT_ZERO: &[u8] =
            b"{\"record_kind\":\"RunStarted\",\"index\":0,\"schema_version\":1,\"logical_time\":0}\n";
        const TURN_COMPLETED_CONTINUE_AT_ZERO: &[u8] =
            b"{\"record_kind\":\"TurnCompleted\",\"index\":0,\"outcome\":\"Continue\"}\n";
        const STOP_REQUESTED_AT_ZERO: &[u8] = b"{\"record_kind\":\"StopRequested\",\"index\":0}\n";

        fn continue_checkpointed(
            calls: ScriptedCalls<u8>,
        ) -> Certificate<ScriptedWriter<u8>, Checkpointed<answer::Continue>> {
            let effects_complete =
                scripted_continue_effects(Rc::clone(&calls), Timestamp::from_nanos(0));
            let mut environment = ScriptedEnvironment::<u8>::new(calls, None);
            match effects_complete.checkpoint::<_, ()>(&mut environment) {
                Ok(certificate) => certificate,
                Err(_) => panic!("a clean Continue completion fixture must checkpoint"),
            }
        }

        fn stop_checkpointed(
            calls: ScriptedCalls<u8>,
        ) -> Certificate<ScriptedWriter<u8>, Checkpointed<answer::Stop>> {
            let effects_complete =
                scripted_stop_effects(Rc::clone(&calls), Timestamp::from_nanos(0));
            let mut environment = ScriptedEnvironment::<u8>::new(calls, None);
            match effects_complete.checkpoint::<_, ()>(&mut environment) {
                Ok(certificate) => certificate,
                Err(_) => panic!("a clean Stop completion fixture must checkpoint"),
            }
        }

        fn direct_checkpointed<W: io::Write, A: answer::Answer>(
            writer: W,
            max_record_bytes: usize,
            index: u64,
            last_time: u64,
        ) -> Certificate<W, Checkpointed<A>> {
            Certificate {
                journal: certificate_journal(writer, max_record_bytes),
                index: EventIndex::new(index),
                last_time: Timestamp::from_nanos(last_time),
                _phase: PhantomData,
            }
        }

        /// Invariant: completing a Continue turn commits exactly one completion
        /// record carrying the Continue outcome before entering the next-turn phase.
        /// Design Doc: the Edges table, by name
        #[test]
        fn continue_commits_turn_completed_continue() {
            let calls = record_calls();
            let checkpointed = continue_checkpointed(Rc::clone(&calls));

            let between_turns = match checkpointed.complete_continue() {
                Ok(certificate) => certificate,
                Err(_) => panic!("a Continue completion record must commit"),
            };
            fn require_between_turns<W: io::Write>(_certificate: &Certificate<W, BetweenTurns>) {}
            require_between_turns(&between_turns);
            drop(between_turns);

            assert_eq!(
                &*calls.borrow(),
                &[
                    ScriptedCall::RecordCommitted(RUN_STARTED_AT_ZERO.to_vec()),
                    ScriptedCall::TakeError,
                    ScriptedCall::RecordCommitted(TURN_COMPLETED_CONTINUE_AT_ZERO.to_vec()),
                ],
                "a Continue completion must append exactly its fixed completion record"
            );
        }

        /// Invariant: completing a Stop turn commits shutdown intent before entering
        /// the stop-pending phase and does not initiate Environment shutdown itself.
        /// Design Doc: the Edges table, by name
        #[test]
        fn stop_commits_stop_requested() {
            let calls = record_calls();
            let checkpointed = stop_checkpointed(Rc::clone(&calls));

            let stop_pending = match checkpointed.request_stop() {
                Ok(certificate) => certificate,
                Err(_) => panic!("a StopRequested record must commit"),
            };
            fn require_stop_pending<W: io::Write>(_certificate: &Certificate<W, StopPending>) {}
            require_stop_pending(&stop_pending);
            drop(stop_pending);

            assert_eq!(
                &*calls.borrow(),
                &[
                    ScriptedCall::RecordCommitted(RUN_STARTED_AT_ZERO.to_vec()),
                    ScriptedCall::TakeError,
                    ScriptedCall::RecordCommitted(STOP_REQUESTED_AT_ZERO.to_vec()),
                ],
                "requesting Stop must commit intent without invoking shutdown"
            );
        }

        /// Invariant: each completion method derives its record solely from the
        /// certificate's fixed answer, without accepting a caller-supplied outcome.
        /// Design Doc: RUN-ENFORCEMENT
        #[test]
        fn the_committed_outcome_is_the_phase_marker_not_a_caller_value() {
            let mut continue_bytes = Vec::new();
            let continue_certificate: Certificate<_, Checkpointed<answer::Continue>> =
                direct_checkpointed(&mut continue_bytes, 128, 7, 11);
            let continue_successor = match continue_certificate.complete_continue() {
                Ok(certificate) => certificate,
                Err(_) => panic!("the Continue marker must commit its fixed outcome"),
            };
            drop(continue_successor);

            let mut stop_bytes = Vec::new();
            let stop_certificate: Certificate<_, Checkpointed<answer::Stop>> =
                direct_checkpointed(&mut stop_bytes, 128, 7, 11);
            let stop_successor = match stop_certificate.request_stop() {
                Ok(certificate) => certificate,
                Err(_) => panic!("the Stop marker must commit its fixed intent"),
            };
            drop(stop_successor);

            assert_eq!(
                continue_bytes,
                b"{\"record_kind\":\"TurnCompleted\",\"index\":7,\"outcome\":\"Continue\"}\n",
                "the Continue marker must select TurnCompleted(Continue)"
            );
            assert_eq!(
                stop_bytes, b"{\"record_kind\":\"StopRequested\",\"index\":7}\n",
                "the Stop marker must select StopRequested"
            );
        }

        /// Invariant: completion transitions preserve accepted index and logical
        /// time at zero, one, and the largest representable value.
        #[test]
        fn completion_transitions_preserve_index_and_time_boundaries() {
            for value in [0, 1, u64::MAX] {
                let continue_certificate: Certificate<_, Checkpointed<answer::Continue>> =
                    direct_checkpointed(Vec::new(), 128, value, value);
                let between_turns = match continue_certificate.complete_continue() {
                    Ok(certificate) => certificate,
                    Err(_) => panic!("a boundary-valued Continue completion must commit"),
                };
                assert_eq!(
                    between_turns.index.as_u64(),
                    value,
                    "a Continue completion must preserve its boundary-valued index"
                );
                assert_eq!(
                    between_turns.last_time.as_nanos(),
                    value,
                    "a Continue completion must preserve its boundary-valued logical time"
                );

                let stop_certificate: Certificate<_, Checkpointed<answer::Stop>> =
                    direct_checkpointed(Vec::new(), 128, value, value);
                let stop_pending = match stop_certificate.request_stop() {
                    Ok(certificate) => certificate,
                    Err(_) => panic!("a boundary-valued Stop request must commit"),
                };
                assert_eq!(
                    stop_pending.index.as_u64(),
                    value,
                    "a Stop request must preserve its boundary-valued index"
                );
                assert_eq!(
                    stop_pending.last_time.as_nanos(),
                    value,
                    "a Stop request must preserve its boundary-valued logical time"
                );
            }
        }

        /// Invariant: each completion record commits when its encoded JSON exactly
        /// fills the Journal's configured record capacity.
        #[test]
        fn completion_records_succeed_at_exact_record_capacity() {
            let mut continue_bytes = Vec::new();
            let continue_certificate: Certificate<_, Checkpointed<answer::Continue>> =
                direct_checkpointed(
                    &mut continue_bytes,
                    TURN_COMPLETED_CONTINUE_AT_ZERO.len() - 1,
                    0,
                    0,
                );
            let continue_successor = match continue_certificate.complete_continue() {
                Ok(certificate) => certificate,
                Err(_) => panic!("an exact-capacity Continue completion must commit"),
            };
            drop(continue_successor);
            assert_eq!(
                continue_bytes, TURN_COMPLETED_CONTINUE_AT_ZERO,
                "an exact-capacity Continue completion must write its complete JSON line"
            );

            let mut stop_bytes = Vec::new();
            let stop_certificate: Certificate<_, Checkpointed<answer::Stop>> =
                direct_checkpointed(&mut stop_bytes, STOP_REQUESTED_AT_ZERO.len() - 1, 0, 0);
            let stop_successor = match stop_certificate.request_stop() {
                Ok(certificate) => certificate,
                Err(_) => panic!("an exact-capacity Stop request must commit"),
            };
            drop(stop_successor);
            assert_eq!(
                stop_bytes, STOP_REQUESTED_AT_ZERO,
                "an exact-capacity Stop request must write its complete JSON line"
            );
        }

        /// Invariant: a completion record one byte beyond capacity fails before
        /// writing output and reports metadata for the attempted record.
        #[test]
        fn completion_records_one_byte_past_capacity_fail_without_output() {
            let mut continue_bytes = Vec::new();
            let continue_certificate: Certificate<_, Checkpointed<answer::Continue>> =
                direct_checkpointed(
                    &mut continue_bytes,
                    TURN_COMPLETED_CONTINUE_AT_ZERO.len() - 2,
                    0,
                    0,
                );
            let continue_fatal = match continue_certificate.complete_continue() {
                Err(fatal) => fatal,
                Ok(_) => panic!("an oversized Continue completion must fail"),
            };
            assert_eq!(
                continue_fatal.record_kind,
                RecordKind::TurnCompleted,
                "an oversized Continue completion must identify TurnCompleted"
            );
            assert_eq!(
                continue_fatal.outcome,
                Some(TurnOutcome::Continue),
                "an oversized Continue completion must retain its fixed outcome"
            );
            assert!(
                matches!(continue_fatal.error, JournalError::BoundExceeded),
                "an oversized Continue completion must report BoundExceeded"
            );
            assert!(
                continue_bytes.is_empty(),
                "an oversized Continue completion must write no partial bytes"
            );

            let mut stop_bytes = Vec::new();
            let stop_certificate: Certificate<_, Checkpointed<answer::Stop>> =
                direct_checkpointed(&mut stop_bytes, STOP_REQUESTED_AT_ZERO.len() - 2, 0, 0);
            let stop_fatal = match stop_certificate.request_stop() {
                Err(fatal) => fatal,
                Ok(_) => panic!("an oversized Stop request must fail"),
            };
            assert_eq!(
                stop_fatal.record_kind,
                RecordKind::StopRequested,
                "an oversized Stop request must identify StopRequested"
            );
            assert_eq!(
                stop_fatal.outcome, None,
                "an oversized Stop request must not carry a completion outcome"
            );
            assert!(
                matches!(stop_fatal.error, JournalError::BoundExceeded),
                "an oversized Stop request must report BoundExceeded"
            );
            assert!(
                stop_bytes.is_empty(),
                "an oversized Stop request must write no partial bytes"
            );
        }
    }

    mod stop_closing {
        use super::*;

        const TURN_COMPLETED_STOP_AT_ZERO: &[u8] =
            b"{\"record_kind\":\"TurnCompleted\",\"index\":0,\"outcome\":\"Stop\"}\n";

        fn stop_pending<W: io::Write>(
            writer: W,
            max_record_bytes: usize,
            index: u64,
            last_time: u64,
        ) -> Certificate<W, StopPending> {
            Certificate {
                journal: certificate_journal(writer, max_record_bytes),
                index: EventIndex::new(index),
                last_time: Timestamp::from_nanos(last_time),
                _phase: PhantomData,
            }
        }

        /// Invariant: a clean shutdown report is followed by exactly one Stop
        /// completion record before the certificate enters its closed phase.
        /// Design Doc: the Edges table, by name
        #[test]
        fn a_clean_report_commits_turn_completed_stop() {
            let calls = record_calls();
            let certificate = stop_pending(ScriptedWriter::new(Rc::clone(&calls), None), 128, 0, 9);
            let environment = ScriptedEnvironment::<u8>::new(Rc::clone(&calls), None);

            let closed = match certificate.close::<_, ()>(environment) {
                Ok(certificate) => certificate,
                Err(_) => panic!("a clean shutdown report must close the certificate"),
            };
            fn require_closed<W: io::Write>(_certificate: &Certificate<W, Closed>) {}
            require_closed(&closed);
            assert_eq!(
                closed.index.as_u64(),
                0,
                "closing must preserve the completed turn's event index"
            );
            assert_eq!(
                closed.last_time,
                Timestamp::from_nanos(9),
                "closing must preserve the completed turn's logical time"
            );
            drop(closed);

            assert_eq!(
                &*calls.borrow(),
                &[
                    ScriptedCall::Shutdown,
                    ScriptedCall::RecordCommitted(TURN_COMPLETED_STOP_AT_ZERO.to_vec()),
                ],
                "a clean close must consume the Environment before committing TurnCompleted(Stop)"
            );
        }

        /// Invariant: when an incomplete shutdown report also contains an error,
        /// the error is the fatal cause and the incomplete account is retained.
        /// Design Doc: the StopPending phase row, by name
        #[test]
        fn a_report_error_outranks_incomplete() {
            let calls = record_calls();
            let certificate = stop_pending(ScriptedWriter::new(Rc::clone(&calls), None), 128, 0, 0);
            let mut environment = ScriptedEnvironment::<u8>::new(Rc::clone(&calls), None);
            environment.shutdown_report = crate::environment::ShutdownReport {
                quiescence: Quiescence::Incomplete,
                error: Some("shutdown failure"),
            };

            let fatal = certificate.close::<_, ()>(environment);

            match fatal {
                Err((super::super::super::FatalCause::Environment(fatal), quiescence)) => {
                    assert_eq!(
                        fatal.operation,
                        super::super::super::EnvironmentOperation::Shutdown,
                        "a shutdown report error must identify the shutdown operation"
                    );
                    assert_eq!(
                        fatal.error, "shutdown failure",
                        "a shutdown fatal must preserve the report's error"
                    );
                    assert_eq!(
                        quiescence,
                        Quiescence::Incomplete,
                        "a shutdown fatal must retain the report's incomplete account"
                    );
                }
                Err(_) => panic!("a report error must remain an Environment shutdown fatal"),
                Ok(_) => panic!("a report error must prevent the closed phase"),
            }
            assert_eq!(
                &*calls.borrow(),
                &[ScriptedCall::Shutdown],
                "an erroneous shutdown report must prevent the completion record"
            );
        }

        /// Invariant: an incomplete shutdown account without an error is a Core
        /// shutdown-incomplete fatal carrying that same account.
        /// Design Doc: the StopPending phase row, by name
        #[test]
        fn incomplete_without_error_is_shutdown_incomplete() {
            let calls = record_calls();
            let certificate = stop_pending(ScriptedWriter::new(Rc::clone(&calls), None), 128, 0, 0);
            let mut environment = ScriptedEnvironment::<u8>::new(Rc::clone(&calls), None);
            environment.shutdown_report = crate::environment::ShutdownReport {
                quiescence: Quiescence::Incomplete,
                error: None,
            };

            let fatal = certificate.close::<_, ()>(environment);

            assert!(
                matches!(
                    fatal,
                    Err((
                        super::super::super::FatalCause::Core(
                            super::super::super::CoreError::ShutdownIncomplete
                        ),
                        Quiescence::Incomplete
                    ))
                ),
                "an incomplete error-free report must retain Incomplete on a ShutdownIncomplete fatal"
            );
            assert_eq!(
                &*calls.borrow(),
                &[ScriptedCall::Shutdown],
                "an incomplete shutdown report must prevent the completion record"
            );
        }

        /// Invariant: if the Stop completion record cannot commit after clean
        /// shutdown, the Journal fatal retains the report's quiesced account.
        /// Design Doc: RUN-FINALIZE
        #[test]
        fn commit_failure_after_a_clean_report_retains_quiesced() {
            let calls = record_calls();
            let certificate =
                stop_pending(ScriptedWriter::new(Rc::clone(&calls), Some(0)), 128, 0, 0);
            let environment = ScriptedEnvironment::<u8>::new(Rc::clone(&calls), None);

            let fatal = certificate.close::<_, ()>(environment);

            match fatal {
                Err((super::super::super::FatalCause::Journal(fatal), quiescence)) => {
                    assert_eq!(
                        fatal.record_kind,
                        RecordKind::TurnCompleted,
                        "a failed Stop completion must identify TurnCompleted"
                    );
                    assert_eq!(
                        fatal.outcome,
                        Some(TurnOutcome::Stop),
                        "a failed Stop completion must retain its fixed Stop outcome"
                    );
                    assert!(
                        matches!(
                            fatal.error,
                            JournalError::Sink {
                                operation: crate::journal::SinkOperation::Flush,
                                ..
                            }
                        ),
                        "the scripted Stop completion failure must remain a flush error"
                    );
                    assert_eq!(
                        quiescence,
                        Quiescence::Quiesced,
                        "a Stop completion failure must retain the clean report's quiescence"
                    );
                }
                Err(_) => panic!("a Stop completion commit failure must remain a Journal fatal"),
                Ok(_) => panic!("a failed Stop completion commit must return no closed phase"),
            }
            assert_eq!(
                &*calls.borrow(),
                &[ScriptedCall::Shutdown],
                "a failed Stop completion must occur after shutdown and commit no record"
            );
        }

        /// Invariant: an error-bearing report remains a shutdown fatal even when
        /// every unit of run-scoped activity is accounted complete.
        #[test]
        fn a_quiesced_report_error_is_shutdown_fatal() {
            let calls = record_calls();
            let certificate = stop_pending(Vec::new(), 128, 0, 0);
            let mut environment = ScriptedEnvironment::<u8>::new(Rc::clone(&calls), None);
            environment.shutdown_report = crate::environment::ShutdownReport {
                quiescence: Quiescence::Quiesced,
                error: Some("late shutdown failure"),
            };

            let fatal = certificate.close::<_, ()>(environment);

            match fatal {
                Err((super::super::super::FatalCause::Environment(fatal), quiescence)) => {
                    assert_eq!(
                        fatal.operation,
                        super::super::super::EnvironmentOperation::Shutdown,
                        "an error-bearing quiesced report must identify shutdown"
                    );
                    assert_eq!(
                        fatal.error, "late shutdown failure",
                        "an error-bearing quiesced report must preserve its error"
                    );
                    assert_eq!(
                        quiescence,
                        Quiescence::Quiesced,
                        "an error-bearing quiesced report must retain Quiesced"
                    );
                }
                Err(_) => panic!("a quiesced report error must remain an Environment fatal"),
                Ok(_) => panic!("a quiesced report error must prevent the closed phase"),
            }
            assert_eq!(
                &*calls.borrow(),
                &[ScriptedCall::Shutdown],
                "an error-bearing quiesced report must consume the Environment exactly once"
            );
        }

        /// Invariant: a Stop completion record commits when its JSON exactly fills
        /// the configured record capacity.
        #[test]
        fn completion_record_succeeds_at_exact_capacity() {
            let mut bytes = Vec::new();
            let certificate = stop_pending(&mut bytes, TURN_COMPLETED_STOP_AT_ZERO.len() - 1, 0, 0);
            let environment = ScriptedEnvironment::<u8>::new(record_calls(), None);

            let closed = match certificate.close::<_, ()>(environment) {
                Ok(certificate) => certificate,
                Err(_) => panic!("an exact-capacity Stop completion must commit"),
            };
            drop(closed);

            assert_eq!(
                bytes, TURN_COMPLETED_STOP_AT_ZERO,
                "an exact-capacity Stop completion must write its complete JSON line"
            );
        }

        /// Invariant: a Stop completion record one byte beyond capacity fails
        /// without output while preserving clean-shutdown quiescence.
        #[test]
        fn completion_record_one_byte_past_capacity_retains_quiesced() {
            let mut bytes = Vec::new();
            let certificate = stop_pending(&mut bytes, TURN_COMPLETED_STOP_AT_ZERO.len() - 2, 0, 0);
            let environment = ScriptedEnvironment::<u8>::new(record_calls(), None);

            let fatal = certificate.close::<_, ()>(environment);

            match fatal {
                Err((super::super::super::FatalCause::Journal(fatal), quiescence)) => {
                    assert_eq!(
                        fatal.outcome,
                        Some(TurnOutcome::Stop),
                        "an over-capacity Stop completion must retain its fixed outcome"
                    );
                    assert!(
                        matches!(fatal.error, JournalError::BoundExceeded),
                        "a Stop completion one byte beyond capacity must report BoundExceeded"
                    );
                    assert_eq!(
                        quiescence,
                        Quiescence::Quiesced,
                        "an over-capacity Stop completion must retain clean-shutdown quiescence"
                    );
                }
                Err(_) => panic!("an over-capacity Stop completion must remain a Journal fatal"),
                Ok(_) => panic!("an over-capacity Stop completion must return no closed phase"),
            }
            assert!(
                bytes.is_empty(),
                "an over-capacity Stop completion must write no partial output"
            );
        }

        /// Invariant: closing preserves event index in both the completion record
        /// and closed certificate, and preserves logical time at zero, one, and the
        /// largest representable value.
        #[test]
        fn closing_preserves_index_and_time_boundaries() {
            for value in [0, 1, u64::MAX] {
                let mut bytes = Vec::new();
                let certificate = stop_pending(&mut bytes, 128, value, value);
                let environment = ScriptedEnvironment::<u8>::new(record_calls(), None);

                let closed = match certificate.close::<_, ()>(environment) {
                    Ok(certificate) => certificate,
                    Err(_) => panic!("a boundary-valued Stop completion must close"),
                };

                assert_eq!(
                    closed.index.as_u64(),
                    value,
                    "closing must preserve a boundary-valued event index"
                );
                assert_eq!(
                    closed.last_time.as_nanos(),
                    value,
                    "closing must preserve a boundary-valued logical time"
                );
                drop(closed);
                assert_eq!(
                    bytes,
                    format!(
                        "{{\"record_kind\":\"TurnCompleted\",\"index\":{value},\"outcome\":\"Stop\"}}\n"
                    )
                    .into_bytes(),
                    "a Stop completion record must preserve its boundary-valued event index"
                );
            }
        }
    }

    mod event_acceptance {
        use super::*;
        use crate::{CoreError, FatalCause};

        const EVENT_ONE_AT_ONE: &[u8] =
            b"{\"record_kind\":\"EventAccepted\",\"index\":1,\"logical_time\":1,\"event\":1}\n";

        fn between_turns<W: io::Write>(
            writer: W,
            max_record_bytes: usize,
            index: u64,
            last_time: u64,
        ) -> Certificate<W, BetweenTurns> {
            Certificate {
                journal: certificate_journal(writer, max_record_bytes),
                index: EventIndex::new(index),
                last_time: Timestamp::from_nanos(last_time),
                _phase: PhantomData,
            }
        }

        struct OneEventEnvironment<Ev> {
            candidate: Option<(Ev, Timestamp)>,
            next_event_calls: usize,
        }

        impl<Ev> OneEventEnvironment<Ev> {
            fn new(event: Ev, timestamp: Timestamp) -> Self {
                Self {
                    candidate: Some((event, timestamp)),
                    next_event_calls: 0,
                }
            }
        }

        impl<Ev> Environment for OneEventEnvironment<Ev> {
            type Event = Ev;
            type Command = ();
            type Error = &'static str;

            fn start(&mut self) -> Result<Timestamp, Self::Error> {
                unreachable!("an event-acceptance fixture must not call start")
            }

            fn next_event(&mut self) -> Result<(Self::Event, Timestamp), Self::Error> {
                self.next_event_calls += 1;
                Ok(self
                    .candidate
                    .take()
                    .expect("an event-acceptance fixture must contain one candidate"))
            }

            fn dispatch(&mut self, _command: Self::Command) -> Result<(), Self::Error> {
                unreachable!("an event-acceptance fixture must not call dispatch")
            }

            fn take_error(&mut self) -> Option<Self::Error> {
                unreachable!("an event-acceptance fixture must not call take_error")
            }

            fn shutdown(self) -> crate::environment::ShutdownReport<Self::Error> {
                unreachable!("an event-acceptance fixture must not call shutdown")
            }
        }

        #[derive(Debug, PartialEq, Eq, Serialize)]
        struct NonCloneEvent {
            sequence: u8,
        }

        struct FailsToSerialize;

        impl Serialize for FailsToSerialize {
            fn serialize<S: Serializer>(&self, _serializer: S) -> Result<S::Ok, S::Error> {
                Err(serde::ser::Error::custom(
                    "scripted Event serialization failure",
                ))
            }
        }

        /// Invariant: once the largest event index has been accepted, another
        /// acceptance attempt fails before asking the Environment for a candidate.
        /// Design Doc: RUN-INDEX
        #[test]
        fn the_domain_check_precedes_next_event() {
            let calls = record_calls();
            let certificate = between_turns(
                ScriptedWriter::new(Rc::clone(&calls), None),
                512,
                u64::MAX - 1,
                1,
            );
            let mut environment = ScriptedEnvironment::<u8>::new(Rc::clone(&calls), None);

            let (certificate, event) = match certificate.accept_event::<_, ()>(&mut environment) {
                Ok(accepted) => accepted,
                Err(_) => panic!("the largest available event index must be accepted"),
            };
            assert_eq!(
                certificate.index.as_u64(),
                u64::MAX,
                "acceptance from one below the index maximum must reach the maximum"
            );
            assert_eq!(
                event, 1,
                "acceptance at the largest available index must return its candidate"
            );
            drop(certificate);
            environment.next_event = Some(Ok((2, Timestamp::from_nanos(2))));
            let exhausted_certificate = between_turns(
                ScriptedWriter::new(Rc::clone(&calls), None),
                512,
                u64::MAX,
                1,
            );
            let calls_before_exhaustion = calls.borrow().len();

            let fatal = exhausted_certificate.accept_event::<_, ()>(&mut environment);

            assert!(
                matches!(fatal, Err(FatalCause::Core(CoreError::IndexExhausted))),
                "acceptance past the event-index maximum must report index exhaustion"
            );
            assert_eq!(
                calls.borrow().len(),
                calls_before_exhaustion,
                "index exhaustion must occur before another Environment or Journal call"
            );
            assert!(
                environment.next_event.is_some(),
                "index exhaustion must leave the next candidate unconsumed"
            );
        }

        /// Invariant: a candidate stamped before the last accepted time is consumed
        /// but rejected without committing an acceptance record.
        /// Design Doc: the EventAccepted edge row, by name
        #[test]
        fn a_decreasing_stamp_is_time_regression_with_the_candidate_consumed() {
            let calls = record_calls();
            let certificate =
                between_turns(ScriptedWriter::new(Rc::clone(&calls), None), 512, 4, 10);
            let mut environment = ScriptedEnvironment::<u8>::new(Rc::clone(&calls), None);
            environment.next_event = Some(Ok((9, Timestamp::from_nanos(9))));

            let fatal = certificate.accept_event::<_, ()>(&mut environment);

            match fatal {
                Err(FatalCause::Core(CoreError::TimeRegression { previous, offered })) => {
                    assert_eq!(
                        previous,
                        Timestamp::from_nanos(10),
                        "time regression must preserve the last accepted timestamp"
                    );
                    assert_eq!(
                        offered,
                        Timestamp::from_nanos(9),
                        "time regression must preserve the rejected candidate timestamp"
                    );
                }
                Err(_) => panic!("a decreasing candidate stamp must remain a Core fatal"),
                Ok(_) => panic!("a decreasing candidate stamp must not be accepted"),
            }
            assert!(
                environment.next_event.is_none(),
                "a rejected regressing candidate must remain consumed"
            );
            assert_eq!(
                &*calls.borrow(),
                &[ScriptedCall::NextEvent],
                "time regression must occur after candidate consumption and before record commit"
            );
        }

        /// Invariant: a candidate carrying the same timestamp as the previous
        /// accepted turn advances to the next event index.
        /// Design Doc: ENV-TIME
        #[test]
        fn an_equal_stamp_is_accepted() {
            let calls = record_calls();
            let certificate =
                between_turns(ScriptedWriter::new(Rc::clone(&calls), None), 512, 0, 1);
            let mut environment = ScriptedEnvironment::<u8>::new(calls, None);

            let (certificate, event) = match certificate.accept_event::<_, ()>(&mut environment) {
                Ok(accepted) => accepted,
                Err(_) => panic!("an equal candidate timestamp must be accepted"),
            };

            assert_eq!(event, 1, "equal-time acceptance must return its candidate");
            assert_eq!(
                certificate.index.as_u64(),
                1,
                "equal-time acceptance from the start turn must advance to index one"
            );
            assert_eq!(
                certificate.last_time,
                Timestamp::from_nanos(1),
                "equal-time acceptance must preserve the accepted timestamp"
            );
        }

        /// Invariant: an accepted candidate's index and time become certificate
        /// state only when its acceptance record commits successfully.
        /// Design Doc: RUN-GRAMMAR
        #[test]
        fn acceptance_advances_index_and_time_only_on_commit() {
            let failed_calls = record_calls();
            let failed_certificate = between_turns(
                ScriptedWriter::new(Rc::clone(&failed_calls), Some(0)),
                512,
                7,
                10,
            );
            let mut failed_environment =
                ScriptedEnvironment::<u8>::new(Rc::clone(&failed_calls), None);
            failed_environment.next_event = Some(Ok((5, Timestamp::from_nanos(12))));

            let fatal = failed_certificate.accept_event::<_, ()>(&mut failed_environment);

            match fatal {
                Err(FatalCause::Journal(fatal)) => {
                    assert_eq!(
                        fatal.record_kind,
                        RecordKind::EventAccepted,
                        "a failed acceptance commit must identify EventAccepted"
                    );
                    assert_eq!(
                        fatal.outcome, None,
                        "an EventAccepted commit failure must carry no turn outcome"
                    );
                }
                Err(_) => panic!("an acceptance commit failure must remain a Journal fatal"),
                Ok(_) => panic!("a failed acceptance commit must return no successor certificate"),
            }
            assert_eq!(
                &*failed_calls.borrow(),
                &[ScriptedCall::NextEvent],
                "a failed acceptance commit must expose no advanced successor state"
            );

            let successful_calls = record_calls();
            let successful_certificate = between_turns(
                ScriptedWriter::new(Rc::clone(&successful_calls), None),
                512,
                7,
                10,
            );
            let mut successful_environment = ScriptedEnvironment::<u8>::new(successful_calls, None);
            successful_environment.next_event = Some(Ok((5, Timestamp::from_nanos(12))));
            let (successful_certificate, _) =
                match successful_certificate.accept_event::<_, ()>(&mut successful_environment) {
                    Ok(accepted) => accepted,
                    Err(_) => panic!("a successful acceptance commit must return its successor"),
                };
            assert_eq!(
                successful_certificate.index.as_u64(),
                8,
                "a committed acceptance must advance the certificate index"
            );
            assert_eq!(
                successful_certificate.last_time,
                Timestamp::from_nanos(12),
                "a committed acceptance must update the certificate timestamp"
            );
        }

        /// Invariant: an acceptance record serializes the newly derived index and
        /// candidate timestamp with the consumed Event in stable field order.
        /// Design Doc: RUN-RECORDS
        #[test]
        fn event_accepted_bytes_carry_the_new_index_and_time() {
            let calls = record_calls();
            let certificate =
                between_turns(ScriptedWriter::new(Rc::clone(&calls), None), 512, 6, 10);
            let mut environment = ScriptedEnvironment::<u8>::new(Rc::clone(&calls), None);
            environment.next_event = Some(Ok((42, Timestamp::from_nanos(12))));

            let (certificate, event) = match certificate.accept_event::<_, ()>(&mut environment) {
                Ok(accepted) => accepted,
                Err(_) => panic!("a valid candidate must commit its acceptance record"),
            };
            drop(certificate);

            assert_eq!(
                event, 42,
                "acceptance must return the Event written to its record"
            );
            assert_eq!(
                &*calls.borrow(),
                &[
                    ScriptedCall::NextEvent,
                    ScriptedCall::RecordCommitted(
                        b"{\"record_kind\":\"EventAccepted\",\"index\":7,\"logical_time\":12,\"event\":42}\n"
                            .to_vec(),
                    ),
                ],
                "EventAccepted bytes must carry the derived index, offered time, and candidate"
            );
        }

        /// Invariant: an EventAccepted record that exactly fills the configured
        /// record capacity commits completely and returns its candidate.
        #[test]
        fn event_record_succeeds_at_exact_record_capacity() {
            let mut bytes = Vec::new();
            let certificate = between_turns(&mut bytes, EVENT_ONE_AT_ONE.len() - 1, 0, 0);
            let mut environment = ScriptedEnvironment::<()>::new(record_calls(), None);

            let (certificate, event) = match certificate.accept_event::<_, ()>(&mut environment) {
                Ok(accepted) => accepted,
                Err(_) => panic!("an exact-capacity acceptance record must commit"),
            };
            drop(certificate);

            assert_eq!(
                event, 1,
                "an exact-capacity acceptance must return its consumed candidate"
            );
            assert_eq!(
                bytes, EVENT_ONE_AT_ONE,
                "an exact-capacity acceptance must write its complete JSON line"
            );
        }

        /// Invariant: an EventAccepted record one byte beyond capacity fails without
        /// output after consuming the candidate.
        #[test]
        fn event_record_one_byte_past_capacity_fails_after_consuming_candidate() {
            let mut bytes = Vec::new();
            let certificate = between_turns(&mut bytes, EVENT_ONE_AT_ONE.len() - 2, 0, 0);
            let mut environment = ScriptedEnvironment::<()>::new(record_calls(), None);

            let fatal = certificate.accept_event::<_, ()>(&mut environment);

            match fatal {
                Err(FatalCause::Journal(fatal)) => {
                    assert_eq!(
                        fatal.record_kind,
                        RecordKind::EventAccepted,
                        "an over-capacity acceptance must identify EventAccepted"
                    );
                    assert!(
                        matches!(fatal.error, JournalError::BoundExceeded),
                        "an acceptance one byte beyond capacity must report BoundExceeded"
                    );
                }
                Err(_) => panic!("an over-capacity acceptance must remain a Journal fatal"),
                Ok(_) => panic!("an over-capacity acceptance must return no successor"),
            }
            assert!(
                environment.next_event.is_none(),
                "an over-capacity acceptance must leave its candidate consumed"
            );
            assert!(
                bytes.is_empty(),
                "an over-capacity acceptance must write no partial output"
            );
        }

        /// Invariant: an Event that cannot serialize is consumed exactly once and
        /// produces a Journal fatal without writing output.
        #[test]
        fn event_serialization_failure_is_journal_fatal_after_consumption() {
            let mut bytes = Vec::new();
            let certificate = between_turns(&mut bytes, 512, 0, 0);
            let mut environment =
                OneEventEnvironment::new(FailsToSerialize, Timestamp::from_nanos(1));

            let fatal = certificate.accept_event::<_, ()>(&mut environment);

            match fatal {
                Err(FatalCause::Journal(fatal)) => {
                    assert_eq!(
                        fatal.record_kind,
                        RecordKind::EventAccepted,
                        "an Event serialization failure must identify EventAccepted"
                    );
                    match fatal.error {
                        JournalError::Encode(error) => assert!(
                            error
                                .to_string()
                                .contains("scripted Event serialization failure"),
                            "an Event serialization fatal must preserve the serializer error"
                        ),
                        _ => panic!("an Event serialization failure must remain an encode error"),
                    }
                }
                Err(_) => panic!("an Event serialization failure must remain a Journal fatal"),
                Ok(_) => panic!("an unserializable Event must return no successor"),
            }
            assert_eq!(
                environment.next_event_calls, 1,
                "an unserializable Event must be consumed exactly once"
            );
            assert!(
                environment.candidate.is_none(),
                "an unserializable Event must remain consumed after commit failure"
            );
            assert!(
                bytes.is_empty(),
                "an Event serialization failure must write no output"
            );
        }

        /// Invariant: accepting an Event returns the same owned value after its
        /// borrowed record has committed without requiring the Event to be cloneable.
        #[test]
        fn a_non_clone_event_is_returned_after_commit() {
            let mut bytes = Vec::new();
            let certificate = between_turns(&mut bytes, 512, 0, 0);
            let mut environment =
                OneEventEnvironment::new(NonCloneEvent { sequence: 3 }, Timestamp::from_nanos(1));

            let (certificate, event) = match certificate.accept_event::<_, ()>(&mut environment) {
                Ok(accepted) => accepted,
                Err(_) => panic!("a serializable non-Clone Event must be accepted"),
            };
            drop(certificate);

            assert_eq!(
                event,
                NonCloneEvent { sequence: 3 },
                "acceptance must return the exact non-Clone Event value"
            );
            assert_eq!(
                environment.next_event_calls, 1,
                "a non-Clone Event must be consumed exactly once"
            );
            assert_eq!(
                bytes,
                b"{\"record_kind\":\"EventAccepted\",\"index\":1,\"logical_time\":1,\"event\":{\"sequence\":3}}\n",
                "a non-Clone Event must be borrowed into the committed record before being returned"
            );
        }
    }

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
