use std::{collections::TryReserveError, num::NonZeroUsize};

use serde::Serialize;

use crate::{
    application::{Application, Outcome},
    bounded_buffer::BoundedBuffer,
    environment::Environment,
    journal::{Journal, JournalBuildError, JournalError},
    time::{EventIndex, Timestamp},
};

pub struct Engine<A, E, W>
where
    A: Application,
    E: Environment,
    W: std::io::Write,
{
    journal: Journal<W>,
    app: A,
    env: E,
    cmd_buf: BoundedBuffer<A::Command>,
    max_turns: NonZeroUsize,
}

impl<A, E, W> Engine<A, E, W>
where
    A: Application,
    E: Environment<Event = A::Event, Command = A::Command>,
    W: std::io::Write,
{
    pub fn new(config: EngineConfig, app: A, env: E, writer: W) -> Result<Self, BuildError> {
        let cmd_buf = BoundedBuffer::new(config.max_commands_per_turn.get())
            .map_err(BuildError::CommandBuffer)?;
        let journal = Journal::new(writer, config.max_record_bytes).map_err(BuildError::Journal)?;

        Ok(Self {
            journal: journal,
            app,
            env,
            cmd_buf,
            max_turns: config.max_turns,
        })
    }

    pub fn run(self) -> EngineExit<A::State, A::Fatal, E::Error> {
        todo!()
    }

    fn process_turn(
        index: EventIndex,
        outcome: Outcome<A::Fatal>,
        overflowed: bool,
        cmd_buf: &mut BoundedBuffer<A::Command>,
        journal: &mut Journal<W>,
        env: &mut Option<E>,
    ) -> TurnFlow<A::Fatal, E::Error> {
        todo!()
    }

    fn fatal_exit() -> EngineExit<A::State, A::Fatal, E::Error> {
        todo!()
    }
}

enum TurnFlow<AF, EE> {
    Continue,
    Stop,
    Fatal(FatalCause<AF, EE>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EngineConfig {
    pub max_turns: NonZeroUsize,
    pub max_commands_per_turn: NonZeroUsize,
    pub max_record_bytes: NonZeroUsize,
}

#[derive(Debug)]
pub enum EngineExit<S, AF, EE> {
    Stopped { state: S },
    Fatal { state: S, cause: FatalCause<AF, EE> },
}

#[derive(Debug)]
pub enum FatalCause<AF, EE> {
    Application(AF),
    Environment(EnvironmentFatal<EE>),
    Journal(JournalFatal),
    Core(CoreFatal),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreFatal {
    TimeRegression {
        previous: Timestamp,
        offered: Timestamp,
    },
    TurnBoundExceeded,
    CommandBoundExceeded,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildError {
    CommandBuffer(TryReserveError),
    Journal(JournalBuildError),
}

#[derive(Debug)]
pub struct JournalFatal {
    pub record_kind: RecordKind,
    pub error: JournalError,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordKind {
    RunStarted,
    EventAccepted,
    CommandsPrepared,
    CommandsDispatched,
    StopRequested,
    TurnCompleted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvironmentFatal<EE> {
    pub error: EE,
    pub operation: EnvironmentOperation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvironmentOperation {
    Start,
    NextEvent,
    Dispatch { position: usize },
    ShutdownGraceful,
}

#[derive(Serialize)]
enum Record<'a, E, C> {
    RunStarted {
        schema_version: u32,
        logical_time: Timestamp,
    },
    EventAccepted {
        index: EventIndex,
        logical_time: Timestamp,
        event: &'a E,
    },
    CommandsPrepared {
        index: EventIndex,
        commands: &'a [C],
    },
    CommandsDispatched {
        index: EventIndex,
    },
    StopRequested {
        index: EventIndex,
    },
    TurnCompleted {
        index: EventIndex,
        outcome: TurnOutcome,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
enum TurnOutcome {
    Continue,
    Stop,
}
