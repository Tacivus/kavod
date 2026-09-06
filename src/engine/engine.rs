use super::record::JournalFatal;
use crate::environment::Quiescence;
use crate::journal::JournalBuildError;
use crate::time::Timestamp;
use std::collections::TryReserveError;
use std::num::NonZeroUsize;

pub struct EngineConfig {
    pub max_commands_per_turn: NonZeroUsize,
    pub max_record_bytes: NonZeroUsize,
}

pub enum BuildError {
    CommandBuffer(TryReserveError),
    Journal(JournalBuildError),
}

pub enum EngineExit<S, AE, EE> {
    Stopped {
        state: S,
    },
    Fatal {
        state: S,
        cause: FatalCause<AE, EE>,
        quiescence: Quiescence,
    },
}

pub enum FatalCause<AE, EE> {
    Application(AE),
    Environment(EnvironmentFatal<EE>),
    Journal(JournalFatal),
    Core(CoreError),
}

/// Names the operation where the Error was observed - not necessarily where it was
/// caused (`ENV-LATCH`).
pub struct EnvironmentFatal<EE> {
    pub error: EE,
    pub operation: EnvironmentOperation,
}

#[derive(Debug, PartialEq, Eq)]
pub enum EnvironmentOperation {
    Start,
    /// Where observed - possibly an unrelated already-latched Error, per
    /// ENV-LATCH.
    NextEvent,
    /// Where in the dispatch loop the Error was observed - possibly an
    /// unrelated already-latched Error, per ENV-LATCH.
    Dispatch {
        position: usize,
    },
    /// The per-turn latch snapshot (RUN-CHECKPOINT) returned a pending Error.
    Checkpoint,
    /// The Stop-path shutdown report carried the latch's final pending Error.
    Shutdown,
}

#[derive(Debug, PartialEq, Eq)]
pub enum CoreError {
    TimeRegression {
        previous: Timestamp,
        offered: Timestamp,
    },
    IndexExhausted,
    CommandBoundExceeded,
    ShutdownIncomplete,
}
