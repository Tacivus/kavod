use std::{any, fmt, marker::PhantomData};

use serde::Serialize;

use crate::{
    journal::{Journal, JournalError},
    time::{EventIndex, Timestamp},
};

use self::phase::{
    BetweenTurns, Closed, CommandsCommitted, CommandsPreped, Initial, StopPending, TurnOpen,
};

/// The record schema version stamped into every `RunStarted`
const SCHEMA_VERSION: u32 = 1;

/// The closed set of record names.
///
/// These get serialized as `record_kind` in the strings and get returned
/// in [`JournalFatal`] reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum RecordKind {
    RunStarted,
    EventAccepted,
    CommandsPrepared,
    CommandsDispatched,
    StopRequested,
    TurnCompleted,
}

/// A Journal failure, paired with the record it was evidencing.
#[derive(Debug)]
pub struct JournalFatal {
    pub record_kind: RecordKind,
    pub error: JournalError,
}

/// The record grammar's phases.
pub(super) mod phase {
    /// Before the run's first record.
    pub struct Initial;

    /// The turn's acceptance record is committed; its handler may run.
    pub struct TurnOpen;

    /// A nonempty batch's intent is recorded; dispatch may begin.
    pub struct CommandsPreped;

    /// The turn's commands have been committed.
    pub struct CommandsCommitted;

    /// A `Continue` turn is closed; the next Event may be acquired.
    pub struct BetweenTurns;

    /// Shutdown is requested and not yet witnessed complete.
    pub struct StopPending;

    /// The run's last record is committed.
    pub struct Closed;
}

/// The [`Engine::run()`] sole path to the Journal, in one record phase.
///
/// `S` is a marker from [`phase`]; the phase carries no data of its own, and
/// `PhantomData<fn() -> S>` keeps the Recorder's `Send`/`Sync` independent of
/// it. Deliberately not `Clone`, `Copy`, or `Default`: a transition consumes the
/// Recorder and returns it in the next phase only on success, so a Fatal path
/// drops it and takes the Journal with it.
#[must_use]
pub(super) struct Recorder<W: std::io::Write, S> {
    journal: Journal<W>,
    /// `EventIndex::START` until the first Event is accepted; the current
    /// accepted turn after.
    index: EventIndex,
    _phase: PhantomData<fn() -> S>,
}

impl<W: std::io::Write, S> Recorder<W, S> {
    /// The current accepted turn's index — the single authority for both the
    /// turn number and the accepted External Event count.
    #[must_use]
    pub(super) fn index(&self) -> EventIndex {
        self.index
    }

    /// Carries the Journal into the next phase of the current turn.
    #[must_use]
    fn into_phase<T>(self) -> Recorder<W, T> {
        let index = self.index;

        self.into_turn(index)
    }

    /// Carries the Journal into a newly accepted turn.
    ///
    /// Private to this module: everywhere else the only way to hold a Recorder
    /// in a given phase is to be handed one by [`Recorder::new`] or a transition.
    #[must_use]
    fn into_turn<T>(self, index: EventIndex) -> Recorder<W, T> {
        Recorder {
            journal: self.journal,
            index,
            _phase: PhantomData,
        }
    }

    /// Commits one record, pairing any failure with the kind it evidenced.
    fn commit<R: RecordPayload>(&mut self, record: &R) -> Result<(), JournalFatal> {
        self.journal.commit(record).map_err(|error| JournalFatal {
            record_kind: R::KIND,
            error,
        })
    }
}

impl<W: std::io::Write, S> fmt::Debug for Recorder<W, S> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let path = any::type_name::<S>();

        formatter
            .debug_struct("Recorder")
            .field("phase", &path.rsplit("::").next().unwrap_or(path))
            .field("index", &self.index)
            .finish_non_exhaustive()
    }
}

impl<W: std::io::Write> Recorder<W, Initial> {
    /// Wraps the engine constructed journal
    ///
    /// Consuming the run's one Journal is what makes a second grammar over it
    /// unconstructible.
    pub(super) fn new(journal: Journal<W>) -> Self {
        Self {
            journal,
            index: EventIndex::START,
            _phase: PhantomData,
        }
    }

    /// Transition to run started state
    pub(super) fn run_started(
        mut self,
        logical_time: Timestamp,
    ) -> Result<Recorder<W, TurnOpen>, JournalFatal> {
        assert_eq!(
            self.index,
            EventIndex::START,
            "kavod invariant violated: RECORD-GRAMMAR: a new Recorder must sit at the start turn"
        );

        self.commit(&RunStarted::new(logical_time))?;

        Ok(self.into_phase())
    }
}

impl<W: std::io::Write> Recorder<W, BetweenTurns> {
    /// Acquisition 4 and 5: the next checked `EventIndex`, then `EventAccepted`.
    ///
    /// The index is derived from the turn just completed, never supplied, so a
    /// repeated or out-of-order accepted index is unconstructible. Overflow is
    /// unreachable behind §8.4's turn bound and is an invariant panic
    /// (`BOUND-INDEX`).
    pub(super) fn accept_event<E: Serialize>(
        mut self,
        logical_time: Timestamp,
        event: &E,
    ) -> Result<Recorder<W, TurnOpen>, JournalFatal> {
        let index = self
            .index
            .checked_next()
            .expect("kavod invariant violated: BOUND-INDEX: event index overflow");

        self.commit(&EventAccepted::new(index, logical_time, event))?;

        Ok(self.into_turn(index))
    }
}

impl<W: std::io::Write> Recorder<W, TurnOpen> {
    /// Order 3's empty split: an empty batch commits neither `CommandsPrepared`
    /// nor `CommandsDispatched` (§8.2), so nothing here can fail.
    pub(super) fn no_commands(self) -> Recorder<W, CommandsCommitted> {
        self.into_phase()
    }

    /// Order 3: `CommandsPrepared`, before the first handoff of a nonempty
    /// batch (A5).
    pub(super) fn prepare_commands<C: Serialize>(
        mut self,
        commands: &[C],
    ) -> Result<Recorder<W, CommandsPreped>, JournalFatal> {
        assert!(
            !commands.is_empty(),
            "kavod invariant violated: RECORD-GRAMMAR: an empty batch commits no CommandsPrepared"
        );

        self.commit(&CommandsPrepared::new(self.index, commands))?;

        Ok(self.into_phase())
    }
}

impl<W: std::io::Write> Recorder<W, CommandsPreped> {
    /// Order 5: `CommandsDispatched`, after the last handoff (§8.4).
    pub(super) fn commands_dispatched(
        mut self,
    ) -> Result<Recorder<W, CommandsCommitted>, JournalFatal> {
        self.commit(&CommandsDispatched::new(self.index))?;

        Ok(self.into_phase())
    }
}

impl<W: std::io::Write> Recorder<W, CommandsCommitted> {
    /// Order 7a: `TurnCompleted(Continue)`. Only its success permits the next
    /// Event acquisition (§8.4).
    pub(super) fn complete_continue(mut self) -> Result<Recorder<W, BetweenTurns>, JournalFatal> {
        self.commit(&TurnCompleted::new(self.index, TurnOutcome::Continue))?;

        Ok(self.into_phase())
    }

    /// Order 7b: `StopRequested`, after a `Stop` outcome and before shutdown
    /// (§8.4).
    pub(super) fn request_stop(mut self) -> Result<Recorder<W, StopPending>, JournalFatal> {
        self.commit(&StopRequested::new(self.index))?;

        Ok(self.into_phase())
    }
}

impl<W: std::io::Write> Recorder<W, StopPending> {
    /// Order 10b: `TurnCompleted(Stop)`, reachable only once `shutdown` has
    /// returned `Quiesced` (§8.4).
    pub(super) fn complete_stop(mut self) -> Result<Recorder<W, Closed>, JournalFatal> {
        self.commit(&TurnCompleted::new(self.index, TurnOutcome::Stop))?;

        Ok(self.into_phase())
    }
}

/// One record's payload.
///
/// `KIND` is the single source for both the serialized `record_kind` member and
/// the kind a [`JournalFatal`] reports, so the two cannot diverge.
trait RecordPayload: Serialize {
    const KIND: RecordKind;
}

#[derive(Debug, Serialize)]
struct RunStarted {
    record_kind: RecordKind,
    schema_version: u32,
    logical_time: Timestamp,
}

impl RunStarted {
    fn new(logical_time: Timestamp) -> Self {
        Self {
            record_kind: Self::KIND,
            schema_version: SCHEMA_VERSION,
            logical_time,
        }
    }
}

impl RecordPayload for RunStarted {
    const KIND: RecordKind = RecordKind::RunStarted;
}

#[derive(Debug, Serialize)]
struct EventAccepted<'a, E> {
    record_kind: RecordKind,
    index: EventIndex,
    logical_time: Timestamp,
    event: &'a E,
}

impl<'a, E: Serialize> EventAccepted<'a, E> {
    fn new(index: EventIndex, logical_time: Timestamp, event: &'a E) -> Self {
        Self {
            record_kind: Self::KIND,
            index,
            logical_time,
            event,
        }
    }
}

impl<E: Serialize> RecordPayload for EventAccepted<'_, E> {
    const KIND: RecordKind = RecordKind::EventAccepted;
}

#[derive(Debug, Serialize)]
struct CommandsPrepared<'a, C> {
    record_kind: RecordKind,
    index: EventIndex,
    commands: &'a [C],
}

impl<'a, C: Serialize> CommandsPrepared<'a, C> {
    fn new(index: EventIndex, commands: &'a [C]) -> Self {
        Self {
            record_kind: Self::KIND,
            index,
            commands,
        }
    }
}

impl<C: Serialize> RecordPayload for CommandsPrepared<'_, C> {
    const KIND: RecordKind = RecordKind::CommandsPrepared;
}

#[derive(Debug, Serialize)]
struct CommandsDispatched {
    record_kind: RecordKind,
    index: EventIndex,
}

impl CommandsDispatched {
    fn new(index: EventIndex) -> Self {
        Self {
            record_kind: Self::KIND,
            index,
        }
    }
}

impl RecordPayload for CommandsDispatched {
    const KIND: RecordKind = RecordKind::CommandsDispatched;
}

#[derive(Debug, Serialize)]
struct StopRequested {
    record_kind: RecordKind,
    index: EventIndex,
}

impl StopRequested {
    fn new(index: EventIndex) -> Self {
        Self {
            record_kind: Self::KIND,
            index,
        }
    }
}

impl RecordPayload for StopRequested {
    const KIND: RecordKind = RecordKind::StopRequested;
}

#[derive(Debug, Serialize)]
struct TurnCompleted {
    record_kind: RecordKind,
    index: EventIndex,
    outcome: TurnOutcome,
}

impl TurnCompleted {
    fn new(index: EventIndex, outcome: TurnOutcome) -> Self {
        Self {
            record_kind: Self::KIND,
            index,
            outcome,
        }
    }
}

impl RecordPayload for TurnCompleted {
    const KIND: RecordKind = RecordKind::TurnCompleted;
}

/// A completed turn's outcome, chosen by the transition and never by its caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
enum TurnOutcome {
    Continue,
    Stop,
}
