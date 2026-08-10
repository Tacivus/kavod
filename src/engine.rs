use std::{collections::TryReserveError, num::NonZeroUsize};

use crate::{
    application::Application,
    bounded_buffer::BoundedBuffer,
    environment::Environment,
    journal::{Journal, JournalBuildError, JournalError},
    time::Timestamp,
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
        let journal = Journal::new(writer, config.max_record_bytes).map_err(BuildError::Journal)?;
        let cmd_buf = BoundedBuffer::new(config.max_commands_per_turn.get())
            .map_err(BuildError::CommandBuffer)?;

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
