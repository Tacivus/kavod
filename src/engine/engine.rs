use super::record::JournalFatal;
use crate::application::Application;
use crate::bounded_buffer::BoundedBuffer;
use crate::environment::{Environment, Quiescence};
use crate::journal::{Journal, JournalBuildError};
use crate::time::Timestamp;
use std::collections::TryReserveError;
use std::io;
use std::num::NonZeroUsize;

pub struct EngineConfig {
    pub max_commands_per_turn: NonZeroUsize,
    pub max_record_bytes: NonZeroUsize,
}

#[allow(
    dead_code,
    reason = "the Engine fields are consumed by run in later build steps"
)]
pub struct Engine<A, E, W>
where
    A: Application,
    E: Environment<Event = A::Event, Command = A::Command>,
    W: io::Write,
{
    app: A,
    env: E,
    journal: Journal<W>,
    batch: BoundedBuffer<A::Command>,
}

pub enum BuildError {
    CommandBuffer(TryReserveError),
    Journal(JournalBuildError),
}

impl<A, E, W> Engine<A, E, W>
where
    A: Application,
    E: Environment<Event = A::Event, Command = A::Command>,
    W: io::Write,
{
    pub fn new(config: EngineConfig, app: A, env: E, writer: W) -> Result<Self, BuildError> {
        let batch = BoundedBuffer::new(config.max_commands_per_turn.get())
            .map_err(BuildError::CommandBuffer)?;
        let journal = Journal::new(writer, config.max_record_bytes).map_err(BuildError::Journal)?;

        Ok(Self {
            app,
            env,
            journal,
            batch,
        })
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Context, Outcome, ShutdownReport};
    use std::cell::Cell;
    use std::rc::Rc;

    mod engine_construction {
        use super::*;

        struct TrackingApplication {
            calls: Rc<Cell<usize>>,
        }

        impl Application for TrackingApplication {
            type State = ();
            type Event = u8;
            type Command = u8;
            type Error = ();

            fn initial_state(&self) -> Self::State {
                self.calls.set(self.calls.get() + 1);
            }

            fn on_start(
                &self,
                _state: &mut Self::State,
                _ctx: &mut Context<'_, Self::Command>,
            ) -> Outcome<Self::Error> {
                self.calls.set(self.calls.get() + 1);
                Outcome::Continue
            }

            fn on_event(
                &self,
                _state: &mut Self::State,
                _event: &Self::Event,
                _ctx: &mut Context<'_, Self::Command>,
            ) -> Outcome<Self::Error> {
                self.calls.set(self.calls.get() + 1);
                Outcome::Continue
            }
        }

        struct TrackingEnvironment {
            calls: Rc<Cell<usize>>,
        }

        impl Environment for TrackingEnvironment {
            type Event = u8;
            type Command = u8;
            type Error = ();

            fn start(&mut self) -> Result<Timestamp, Self::Error> {
                self.calls.set(self.calls.get() + 1);
                Ok(Timestamp::from_nanos(0))
            }

            fn next_event(&mut self) -> Result<(Self::Event, Timestamp), Self::Error> {
                self.calls.set(self.calls.get() + 1);
                Ok((0, Timestamp::from_nanos(0)))
            }

            fn dispatch(&mut self, _command: Self::Command) -> Result<(), Self::Error> {
                self.calls.set(self.calls.get() + 1);
                Ok(())
            }

            fn take_error(&mut self) -> Option<Self::Error> {
                self.calls.set(self.calls.get() + 1);
                None
            }

            fn shutdown(self) -> ShutdownReport<Self::Error> {
                self.calls.set(self.calls.get() + 1);
                ShutdownReport {
                    quiescence: Quiescence::Quiesced,
                    error: None,
                }
            }
        }

        fn config(max_commands_per_turn: usize, max_record_bytes: usize) -> EngineConfig {
            EngineConfig {
                max_commands_per_turn: NonZeroUsize::new(max_commands_per_turn)
                    .expect("an Engine test command bound must be nonzero"),
                max_record_bytes: NonZeroUsize::new(max_record_bytes)
                    .expect("an Engine test record bound must be nonzero"),
            }
        }

        fn tracked_inputs() -> (
            TrackingApplication,
            TrackingEnvironment,
            Rc<Cell<usize>>,
            Rc<Cell<usize>>,
        ) {
            let application_calls = Rc::new(Cell::new(0));
            let environment_calls = Rc::new(Cell::new(0));
            (
                TrackingApplication {
                    calls: Rc::clone(&application_calls),
                },
                TrackingEnvironment {
                    calls: Rc::clone(&environment_calls),
                },
                application_calls,
                environment_calls,
            )
        }

        /// Invariant: failure to reserve the complete command batch is reported as
        /// a command-buffer construction error.
        /// Design Doc: the construction table, by name
        #[test]
        fn batch_reservation_failure_is_command_buffer() {
            let (app, env, _, _) = tracked_inputs();
            let result = Engine::new(config(usize::MAX, 1), app, env, Vec::new());

            assert!(
                matches!(result, Err(BuildError::CommandBuffer(_))),
                "an impossible command-batch reservation must be a CommandBuffer error"
            );
        }

        /// Invariant: failure to build the journal after reserving the command
        /// batch is reported as a journal construction error.
        /// Design Doc: the construction table, by name
        #[test]
        fn journal_build_failure_is_journal() {
            let (app, env, _, _) = tracked_inputs();
            let result = Engine::new(config(1, usize::MAX), app, env, Vec::new());

            assert!(
                matches!(
                    result,
                    Err(BuildError::Journal(JournalBuildError::MaxBytesTooLarge))
                ),
                "an overflowing journal region must be a Journal MaxBytesTooLarge error"
            );
        }

        /// Invariant: constructing an engine stores its collaborators without
        /// invoking any application or environment operation.
        /// Design Doc: the construction table, by name
        #[test]
        fn construction_invokes_no_application_or_environment_method() {
            let (app, env, application_calls, environment_calls) = tracked_inputs();

            let engine = match Engine::new(config(1, 1), app, env, Vec::new()) {
                Ok(engine) => engine,
                Err(_) => panic!("minimum nonzero bounds must construct an Engine"),
            };

            assert_eq!(
                application_calls.get(),
                0,
                "Engine construction must not invoke an Application method"
            );
            assert_eq!(
                environment_calls.get(),
                0,
                "Engine construction must not invoke an Environment method"
            );
            drop(engine);
            assert_eq!(
                application_calls.get(),
                0,
                "dropping an unstarted Engine must not invoke an Application method"
            );
            assert_eq!(
                environment_calls.get(),
                0,
                "dropping an unstarted Engine must not invoke an Environment method"
            );
        }

        /// Invariant: the minimum nonzero bounds construct an engine with an empty
        /// fully reserved command batch and a fresh journal.
        #[test]
        fn one_slot_bounds_construct_an_empty_unpoisoned_engine() {
            let (app, env, _, _) = tracked_inputs();

            let engine = match Engine::new(config(1, 1), app, env, Vec::new()) {
                Ok(engine) => engine,
                Err(_) => panic!("minimum nonzero bounds must construct an Engine"),
            };

            assert_eq!(
                engine.batch.capacity(),
                1,
                "the Engine command batch must retain its configured one-slot bound"
            );
            assert!(
                engine.batch.is_empty(),
                "a newly constructed Engine command batch must be empty"
            );
            assert!(
                !engine.journal.is_poisoned(),
                "a newly constructed Engine journal must not be poisoned"
            );
        }

        /// Invariant: when neither allocation can be constructed, command-batch
        /// reservation fails before journal construction is attempted.
        #[test]
        fn command_buffer_failure_precedes_journal_failure() {
            let (app, env, _, _) = tracked_inputs();
            let result = Engine::new(config(usize::MAX, usize::MAX), app, env, Vec::new());

            assert!(
                matches!(result, Err(BuildError::CommandBuffer(_))),
                "command-batch failure must outrank a later journal build failure"
            );
        }

        /// Invariant: construction failures return without invoking any application
        /// or environment operation, regardless of which allocation fails.
        #[test]
        fn failures_invoke_no_application_or_environment_method() {
            for bounds in [(usize::MAX, 1), (1, usize::MAX)] {
                let (app, env, application_calls, environment_calls) = tracked_inputs();

                assert!(
                    Engine::new(config(bounds.0, bounds.1), app, env, Vec::new()).is_err(),
                    "the scripted impossible bound must fail Engine construction"
                );
                assert_eq!(
                    application_calls.get(),
                    0,
                    "failed Engine construction must not invoke an Application method"
                );
                assert_eq!(
                    environment_calls.get(),
                    0,
                    "failed Engine construction must not invoke an Environment method"
                );
            }
        }
    }
}
