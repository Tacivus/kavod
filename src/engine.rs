use std::{fmt::Error, sync::Mutex};

use crate::{
    application::{AppEvent, Application, EventEnvelope},
    audit::{AuditLog, AuditWriter},
    environment::Environment,
    time::Timestamp,
};

pub enum EngineEvent {
    Ready,
}

pub enum CoreFatal {}

pub enum FatalCause<A, E, L> {
    Application(A),
    Environment(E),
    Audit(L),
    Core(CoreFatal),
}

pub enum EngineExit<S, A, E, L> {
    Stopped {
        state: S,
    },
    Fatal {
        state: S,
        cause: FatalCause<A, E, L>,
    },
}

pub struct Engine<A, E, W>
where
    A: Application,
    E: Environment,
    W: AuditWriter,
{
    app: A,
    env: E,
    audit_log: AuditLog<W>,
    fatal: Option<FatalCause<A::Fatal, E::Fatal, W::Fatal>>,
}

impl<A, E, W> Engine<A, E, W>
where
    A: Application,
    E: Environment,
    W: AuditWriter,
{
    pub fn run(mut self) -> EngineExit<A::State, A::Fatal, E::Fatal, W::Fatal> {
        todo!()
    }
}
