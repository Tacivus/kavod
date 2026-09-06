#[allow(
    clippy::module_inception,
    reason = "the prescribed engine directory keeps declarations in engine.rs"
)]
mod engine;
mod record;

pub use engine::{
    BuildError, CoreError, Engine, EngineConfig, EngineExit, EnvironmentFatal,
    EnvironmentOperation, FatalCause,
};
pub use record::{JournalFatal, RecordKind, TurnOutcome};
