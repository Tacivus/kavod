use super::record::{
    Certificate, Checkpointed, ClassifiedTurn, JournalFatal, TurnOpen, TurnOutcome,
};
use crate::application::{Application, Context, Outcome};
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

    #[allow(dead_code, reason = "called by Engine::run in a later build step")]
    fn turn(
        app: &A,
        state: &mut A::State,
        event: Option<&A::Event>,
        batch: &mut BoundedBuffer<A::Command>,
        certificate: Certificate<W, TurnOpen>,
    ) -> Result<ClassifiedTurn<W>, FatalCause<A::Error, E::Error>> {
        let index = certificate.index();
        let logical_time = certificate.logical_time();
        assert_eq!(
            index.as_u64() == 0,
            event.is_none(),
            "a start-turn certificate must have no Event, and a later-turn certificate must have one"
        );

        let (answer, overflowed) = {
            let mut context = Context::new(batch, index, logical_time);
            let answer = if index.as_u64() == 0 {
                app.on_start(state, &mut context)
            } else {
                app.on_event(
                    state,
                    event.expect("a later-turn certificate must have an accepted Event"),
                    &mut context,
                )
            };
            (answer, context.overflowed())
        };

        if overflowed {
            batch.clear();
            return Err(FatalCause::Core(CoreError::CommandBoundExceeded));
        }

        match answer {
            Outcome::Continue => Ok(certificate.classify(TurnOutcome::Continue)),
            Outcome::Stop => Ok(certificate.classify(TurnOutcome::Stop)),
            Outcome::Fatal(error) => {
                batch.clear();
                Err(FatalCause::Application(error))
            }
        }
    }

    #[allow(dead_code, reason = "called by Engine::run in a later build step")]
    fn finalize(
        state: A::State,
        cause: FatalCause<A::Error, E::Error>,
        retained_quiescence: Option<Quiescence>,
        environment: Option<E>,
    ) -> EngineExit<A::State, A::Error, E::Error> {
        let quiescence = match (retained_quiescence, environment) {
            (Some(quiescence), None) => quiescence,
            (None, Some(environment)) => environment.shutdown().quiescence,
            (None, None) => Quiescence::Quiesced,
            (Some(_), Some(_)) => unreachable!(
                "fatal finalization cannot retain quiescence while still owning the Environment"
            ),
        };

        EngineExit::Fatal {
            state,
            cause,
            quiescence,
        }
    }

    #[allow(
        clippy::type_complexity,
        reason = "the helper returns the typed checkpoint successor or the shared fatal cause"
    )]
    fn effects<M>(
        certificate: Certificate<W, TurnOpen<M>>,
        environment: &mut E,
        batch: &mut BoundedBuffer<A::Command>,
    ) -> Result<Certificate<W, Checkpointed<M>>, FatalCause<A::Error, E::Error>> {
        let certificate = if batch.is_empty() {
            certificate.no_commands(batch)
        } else {
            certificate.dispatch_batch(environment, batch)?
        };
        certificate.checkpoint(environment)
    }

    pub fn run(self) -> EngineExit<A::State, A::Error, E::Error> {
        let Self {
            app,
            mut env,
            journal,
            mut batch,
        } = self;
        let mut state = app.initial_state();
        let start_time = match env.start() {
            Ok(start_time) => start_time,
            Err(error) => {
                return Self::finalize(
                    state,
                    FatalCause::Environment(EnvironmentFatal {
                        error,
                        operation: EnvironmentOperation::Start,
                    }),
                    None,
                    None,
                );
            }
        };
        let certificate = Certificate::mint(journal, start_time);
        let mut certificate = match certificate.run_started() {
            Ok(certificate) => certificate,
            Err(fatal) => {
                return Self::finalize(state, FatalCause::Journal(fatal), None, Some(env));
            }
        };
        let mut pending_event = None;

        loop {
            let classified = match Self::turn(
                &app,
                &mut state,
                pending_event.as_ref(),
                &mut batch,
                certificate,
            ) {
                Ok(classified) => classified,
                Err(cause) => return Self::finalize(state, cause, None, Some(env)),
            };

            match classified {
                ClassifiedTurn::Continue(classified) => {
                    match Self::effects(classified, &mut env, &mut batch) {
                        Ok(checkpointed) => match checkpointed.complete_continue() {
                            Ok(between_turns) => match between_turns.accept_event(&mut env) {
                                Ok((next, event)) => {
                                    pending_event = Some(event);
                                    certificate = next;
                                }
                                Err(cause) => {
                                    return Self::finalize(state, cause, None, Some(env));
                                }
                            },
                            Err(fatal) => {
                                return Self::finalize(
                                    state,
                                    FatalCause::Journal(fatal),
                                    None,
                                    Some(env),
                                );
                            }
                        },
                        Err(cause) => return Self::finalize(state, cause, None, Some(env)),
                    }
                }
                ClassifiedTurn::Stop(classified) => {
                    match Self::effects(classified, &mut env, &mut batch) {
                        Ok(checkpointed) => match checkpointed.request_stop() {
                            Ok(stop_pending) => match stop_pending.close(env) {
                                Ok(_closed) => return EngineExit::Stopped { state },
                                Err((cause, quiescence)) => {
                                    return Self::finalize(state, cause, Some(quiescence), None);
                                }
                            },
                            Err(fatal) => {
                                return Self::finalize(
                                    state,
                                    FatalCause::Journal(fatal),
                                    None,
                                    Some(env),
                                );
                            }
                        },
                        Err(cause) => return Self::finalize(state, cause, None, Some(env)),
                    }
                }
            }
        }
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
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    #[derive(Debug, PartialEq, Eq)]
    enum HandlerCall {
        Start {
            index: u64,
            logical_time: u64,
        },
        Event {
            event: u8,
            index: u64,
            logical_time: u64,
        },
    }

    #[derive(Clone, Copy)]
    enum ScriptedAnswer {
        Continue,
        Stop,
        Fatal,
    }

    struct ScriptedError {
        label: &'static str,
        dropped: Rc<Cell<bool>>,
    }

    impl Drop for ScriptedError {
        fn drop(&mut self) {
            self.dropped.set(true);
        }
    }

    struct TurnApplication {
        calls: Rc<RefCell<Vec<HandlerCall>>>,
        answer: ScriptedAnswer,
        emissions: usize,
        fatal_dropped: Rc<Cell<bool>>,
    }

    impl TurnApplication {
        fn finish_handler(
            &self,
            state: &mut usize,
            context: &mut Context<'_, u8>,
        ) -> Outcome<ScriptedError> {
            *state += 1;
            for position in 0..self.emissions {
                context.emit(
                    u8::try_from(position)
                        .expect("a turn-helper test emission position must fit in u8"),
                );
            }

            match self.answer {
                ScriptedAnswer::Continue => Outcome::Continue,
                ScriptedAnswer::Stop => Outcome::Stop,
                ScriptedAnswer::Fatal => Outcome::Fatal(ScriptedError {
                    label: "scripted application fatal",
                    dropped: Rc::clone(&self.fatal_dropped),
                }),
            }
        }
    }

    impl Application for TurnApplication {
        type State = usize;
        type Event = u8;
        type Command = u8;
        type Error = ScriptedError;

        fn initial_state(&self) -> Self::State {
            0
        }

        fn on_start(
            &self,
            state: &mut Self::State,
            context: &mut Context<'_, Self::Command>,
        ) -> Outcome<Self::Error> {
            self.calls.borrow_mut().push(HandlerCall::Start {
                index: context.index().as_u64(),
                logical_time: context.logical_time().as_nanos(),
            });
            self.finish_handler(state, context)
        }

        fn on_event(
            &self,
            state: &mut Self::State,
            event: &Self::Event,
            context: &mut Context<'_, Self::Command>,
        ) -> Outcome<Self::Error> {
            self.calls.borrow_mut().push(HandlerCall::Event {
                event: *event,
                index: context.index().as_u64(),
                logical_time: context.logical_time().as_nanos(),
            });
            self.finish_handler(state, context)
        }
    }

    struct AcceptingEnvironment {
        next: Option<(u8, Timestamp)>,
    }

    impl Environment for AcceptingEnvironment {
        type Event = u8;
        type Command = u8;
        type Error = &'static str;

        fn start(&mut self) -> Result<Timestamp, Self::Error> {
            Ok(Timestamp::from_nanos(0))
        }

        fn next_event(&mut self) -> Result<(Self::Event, Timestamp), Self::Error> {
            Ok(self
                .next
                .take()
                .expect("a later-turn certificate fixture must contain one Event"))
        }

        fn dispatch(&mut self, _command: Self::Command) -> Result<(), Self::Error> {
            panic!("a certificate fixture with an empty batch must not dispatch")
        }

        fn take_error(&mut self) -> Option<Self::Error> {
            None
        }

        fn shutdown(self) -> ShutdownReport<Self::Error> {
            ShutdownReport {
                quiescence: Quiescence::Quiesced,
                error: None,
            }
        }
    }

    #[allow(
        clippy::type_complexity,
        reason = "the fixture returns the Application with both shared observation handles"
    )]
    fn turn_application(
        answer: ScriptedAnswer,
        emissions: usize,
    ) -> (
        TurnApplication,
        Rc<RefCell<Vec<HandlerCall>>>,
        Rc<Cell<bool>>,
    ) {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let fatal_dropped = Rc::new(Cell::new(false));
        (
            TurnApplication {
                calls: Rc::clone(&calls),
                answer,
                emissions,
                fatal_dropped: Rc::clone(&fatal_dropped),
            },
            calls,
            fatal_dropped,
        )
    }

    fn start_turn(logical_time: Timestamp) -> Certificate<Vec<u8>, TurnOpen> {
        let journal = Journal::new(
            Vec::new(),
            NonZeroUsize::new(256).expect("a turn-helper test record bound must be nonzero"),
        )
        .expect("a turn-helper test Journal must reserve its record buffer");
        let certificate = Certificate::mint(journal, logical_time);
        match certificate.run_started() {
            Ok(certificate) => certificate,
            Err(_) => panic!("a turn-helper fixture must commit RunStarted"),
        }
    }

    fn later_turn(event: u8, logical_time: Timestamp) -> (Certificate<Vec<u8>, TurnOpen>, u8) {
        let certificate = start_turn(Timestamp::from_nanos(0));
        let certificate = match certificate.classify(TurnOutcome::Continue) {
            ClassifiedTurn::Continue(certificate) => certificate,
            ClassifiedTurn::Stop(_) => {
                panic!("a later-turn fixture must classify its setup turn as Continue")
            }
        };
        let commands =
            BoundedBuffer::<u8>::new(1).expect("a later-turn fixture must reserve one command");
        let certificate = certificate.no_commands(&commands);
        let mut environment = AcceptingEnvironment {
            next: Some((event, logical_time)),
        };
        let certificate = match certificate.checkpoint::<_, ScriptedError>(&mut environment) {
            Ok(certificate) => certificate,
            Err(_) => panic!("a later-turn fixture checkpoint must be clear"),
        };
        let certificate = match certificate.complete_continue() {
            Ok(certificate) => certificate,
            Err(_) => panic!("a later-turn fixture must commit TurnCompleted"),
        };
        match certificate.accept_event::<_, ScriptedError>(&mut environment) {
            Ok(accepted) => accepted,
            Err(_) => panic!("a later-turn fixture must accept its scripted Event"),
        }
    }

    fn assert_continue(
        result: Result<
            ClassifiedTurn<Vec<u8>>,
            FatalCause<ScriptedError, <AcceptingEnvironment as Environment>::Error>,
        >,
    ) {
        match result {
            Ok(ClassifiedTurn::Continue(_)) => {}
            Ok(ClassifiedTurn::Stop(_)) => {
                panic!("a Continue handler answer must produce a Continue-classified turn")
            }
            Err(_) => panic!("a nonfatal Continue handler answer must not fail its turn"),
        }
    }

    fn assert_stop(
        result: Result<
            ClassifiedTurn<Vec<u8>>,
            FatalCause<ScriptedError, <AcceptingEnvironment as Environment>::Error>,
        >,
    ) {
        match result {
            Ok(ClassifiedTurn::Stop(_)) => {}
            Ok(ClassifiedTurn::Continue(_)) => {
                panic!("a Stop handler answer must produce a Stop-classified turn")
            }
            Err(_) => panic!("a nonfatal Stop handler answer must not fail its turn"),
        }
    }

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

    mod turn_handler_selection {
        use super::*;

        /// Invariant: the accepted start turn invokes only the start handler, once,
        /// with index zero and the certificate's frozen logical time.
        /// Design Doc: the Phases table, by name
        #[test]
        fn index_zero_calls_on_start_once() {
            let (app, calls, _) = turn_application(ScriptedAnswer::Continue, 0);
            let mut state = 0;
            let mut batch =
                BoundedBuffer::new(1).expect("a start-turn test must reserve one command");
            let certificate = start_turn(Timestamp::from_nanos(41));

            let result = Engine::<TurnApplication, AcceptingEnvironment, Vec<u8>>::turn(
                &app,
                &mut state,
                None,
                &mut batch,
                certificate,
            );

            assert_continue(result);
            assert_eq!(
                calls.borrow().as_slice(),
                &[HandlerCall::Start {
                    index: 0,
                    logical_time: 41,
                }],
                "a certificate at index zero must invoke only on_start, exactly once"
            );
            assert_eq!(
                state, 1,
                "one start-handler invocation must perform exactly one scripted state mutation"
            );
        }

        /// Invariant: an accepted turn after the start invokes only the event
        /// handler, once, with the accepted Event and certificate position.
        /// Design Doc: the Phases table, by name
        #[test]
        fn a_later_index_calls_on_event_once() {
            let (app, calls, _) = turn_application(ScriptedAnswer::Stop, 0);
            let mut state = 0;
            let mut batch =
                BoundedBuffer::new(1).expect("an event-turn test must reserve one command");
            let (certificate, event) = later_turn(23, Timestamp::from_nanos(u64::MAX));

            let result = Engine::<TurnApplication, AcceptingEnvironment, Vec<u8>>::turn(
                &app,
                &mut state,
                Some(&event),
                &mut batch,
                certificate,
            );

            assert_stop(result);
            assert_eq!(
                calls.borrow().as_slice(),
                &[HandlerCall::Event {
                    event: 23,
                    index: 1,
                    logical_time: u64::MAX,
                }],
                "a certificate after index zero must invoke only on_event with its accepted values"
            );
            assert_eq!(
                state, 1,
                "one event-handler invocation must perform exactly one scripted state mutation"
            );
        }
    }

    mod turn_overflow_precedence {
        use super::*;

        /// Invariant: exceeding the command bound discards every staged command and
        /// returns the Core overflow cause regardless of the handler's answer.
        /// Design Doc: APP-OVERFLOW, A4
        #[test]
        fn overflow_outranks_the_returned_outcome() {
            for answer in [
                ScriptedAnswer::Continue,
                ScriptedAnswer::Stop,
                ScriptedAnswer::Fatal,
            ] {
                let (app, calls, fatal_dropped) = turn_application(answer, 2);
                let mut state = 0;
                let mut batch =
                    BoundedBuffer::new(1).expect("an overflow test must reserve one command");
                let certificate = start_turn(Timestamp::from_nanos(0));

                let result = Engine::<TurnApplication, AcceptingEnvironment, Vec<u8>>::turn(
                    &app,
                    &mut state,
                    None,
                    &mut batch,
                    certificate,
                );

                assert!(
                    matches!(
                        result,
                        Err(FatalCause::Core(CoreError::CommandBoundExceeded))
                    ),
                    "command overflow must outrank Continue, Stop, and Application Fatal answers"
                );
                assert!(
                    batch.is_empty(),
                    "an overflowing turn must discard its entire staged command batch"
                );
                assert_eq!(
                    batch.capacity(),
                    1,
                    "discarding an overflowing batch must retain its fixed command capacity"
                );
                assert_eq!(
                    state, 1,
                    "command overflow must not roll back the handler's state mutation"
                );
                assert_eq!(
                    calls.borrow().as_slice(),
                    &[HandlerCall::Start {
                        index: 0,
                        logical_time: 0,
                    }],
                    "an overflowing turn must still have invoked exactly one handler"
                );
                assert_eq!(
                    fatal_dropped.get(),
                    matches!(answer, ScriptedAnswer::Fatal),
                    "an Application Fatal payload must be discarded when command overflow outranks it"
                );
            }
        }

        /// Invariant: command overflow after an event handler follows the same
        /// precedence and batch-discard rules as overflow during the start turn.
        #[test]
        fn later_index_overflow_outranks_a_fatal_outcome() {
            let (app, calls, fatal_dropped) = turn_application(ScriptedAnswer::Fatal, 2);
            let mut state = 0;
            let mut batch =
                BoundedBuffer::new(1).expect("an event-overflow test must reserve one command");
            let (certificate, event) = later_turn(29, Timestamp::from_nanos(53));

            let result = Engine::<TurnApplication, AcceptingEnvironment, Vec<u8>>::turn(
                &app,
                &mut state,
                Some(&event),
                &mut batch,
                certificate,
            );

            assert!(
                matches!(
                    result,
                    Err(FatalCause::Core(CoreError::CommandBoundExceeded))
                ),
                "event-handler command overflow must outrank its Application Fatal answer"
            );
            assert!(
                batch.is_empty(),
                "an overflowing event turn must discard its entire staged command batch"
            );
            assert_eq!(
                state, 1,
                "event-handler command overflow must not roll back its state mutation"
            );
            assert_eq!(
                calls.borrow().as_slice(),
                &[HandlerCall::Event {
                    event: 29,
                    index: 1,
                    logical_time: 53,
                }],
                "an overflowing later turn must invoke on_event exactly once"
            );
            assert!(
                fatal_dropped.get(),
                "event-handler overflow must discard the outranked Application Error payload"
            );
        }
    }

    mod turn_application_fatal {
        use super::*;

        /// Invariant: an application failure preserves the handler's state mutation
        /// and exact Error payload while discarding all staged commands.
        /// Design Doc: APP-STATE
        #[test]
        fn state_mutation_and_the_fatal_payload_both_stand() {
            let (app, calls, fatal_dropped) = turn_application(ScriptedAnswer::Fatal, 1);
            let mut state = 0;
            let mut batch =
                BoundedBuffer::new(2).expect("an application-fatal test must reserve commands");
            let certificate = start_turn(Timestamp::from_nanos(7));

            let result = Engine::<TurnApplication, AcceptingEnvironment, Vec<u8>>::turn(
                &app,
                &mut state,
                None,
                &mut batch,
                certificate,
            );
            let error = match result {
                Err(FatalCause::Application(error)) => error,
                Err(_) => panic!("a non-overflowing handler Fatal must be an Application cause"),
                Ok(_) => panic!("a handler Fatal must not return a classified turn"),
            };

            assert_eq!(
                state, 1,
                "an Application Fatal must preserve the handler's completed state mutation"
            );
            assert!(
                batch.is_empty(),
                "an Application Fatal must discard every command staged by that handler"
            );
            assert_eq!(
                error.label, "scripted application fatal",
                "an Application Fatal must carry the exact handler Error payload"
            );
            assert!(
                !fatal_dropped.get(),
                "the Application Error payload must remain owned by the Fatal cause"
            );
            assert_eq!(
                calls.borrow().as_slice(),
                &[HandlerCall::Start {
                    index: 0,
                    logical_time: 7,
                }],
                "an Application Fatal turn must invoke its selected handler exactly once"
            );
            drop(error);
            assert!(
                fatal_dropped.get(),
                "dropping the returned Fatal cause must drop its Application Error payload"
            );
        }

        /// Invariant: an event handler's fatal result preserves its state mutation
        /// and Error payload while discarding that event turn's staged commands.
        #[test]
        fn later_index_state_mutation_and_fatal_payload_both_stand() {
            let (app, calls, fatal_dropped) = turn_application(ScriptedAnswer::Fatal, 1);
            let mut state = 0;
            let mut batch =
                BoundedBuffer::new(2).expect("an event-handler fatal test must reserve commands");
            let (certificate, event) = later_turn(31, Timestamp::from_nanos(59));

            let result = Engine::<TurnApplication, AcceptingEnvironment, Vec<u8>>::turn(
                &app,
                &mut state,
                Some(&event),
                &mut batch,
                certificate,
            );
            let error = match result {
                Err(FatalCause::Application(error)) => error,
                Err(_) => {
                    panic!("a non-overflowing event-handler Fatal must be an Application cause")
                }
                Ok(_) => panic!("an event-handler Fatal must not return a classified turn"),
            };

            assert_eq!(
                state, 1,
                "an event-handler Fatal must preserve its completed state mutation"
            );
            assert!(
                batch.is_empty(),
                "an event-handler Fatal must discard every command it staged"
            );
            assert_eq!(
                error.label, "scripted application fatal",
                "an event-handler Fatal must carry its exact Error payload"
            );
            assert!(
                !fatal_dropped.get(),
                "the event handler's Error payload must remain owned by the Fatal cause"
            );
            assert_eq!(
                calls.borrow().as_slice(),
                &[HandlerCall::Event {
                    event: 31,
                    index: 1,
                    logical_time: 59,
                }],
                "a non-overflowing later Fatal turn must invoke on_event exactly once"
            );
            drop(error);
            assert!(
                fatal_dropped.get(),
                "dropping the event-handler Fatal cause must drop its Error payload"
            );
        }
    }

    mod turn_batch_reuse {
        use super::*;

        /// Invariant: beginning a turn removes stale commands from the reusable
        /// batch and retains only commands emitted by the current handler.
        #[test]
        fn fresh_turn_replaces_stale_batch_at_exact_capacity() {
            let (app, _, _) = turn_application(ScriptedAnswer::Continue, 1);
            let mut state = 0;
            let mut batch =
                BoundedBuffer::new(1).expect("a batch-reuse test must reserve one command");
            batch
                .try_push(99)
                .expect("the stale command must fit before the fresh turn begins");
            let certificate = start_turn(Timestamp::from_nanos(0));

            let result = Engine::<TurnApplication, AcceptingEnvironment, Vec<u8>>::turn(
                &app,
                &mut state,
                None,
                &mut batch,
                certificate,
            );

            assert_continue(result);
            assert_eq!(
                batch.as_slice(),
                &[0],
                "a fresh turn must replace stale commands with the current handler's exact batch"
            );
            assert_eq!(
                batch.capacity(),
                1,
                "reusing the command batch at exact capacity must retain its configured bound"
            );
        }
    }

    mod turn_event_invariant {
        use super::*;
        use std::panic::{AssertUnwindSafe, catch_unwind};

        /// Invariant: a start-turn certificate cannot be paired with an Event, and
        /// invalid input is rejected before either handler runs.
        #[test]
        fn start_turn_with_event_panics_before_handler() {
            let (app, calls, _) = turn_application(ScriptedAnswer::Continue, 0);
            let mut state = 0;
            let mut batch =
                BoundedBuffer::new(1).expect("an event-invariant test must reserve one command");
            let certificate = start_turn(Timestamp::from_nanos(0));
            let event = 1;

            let panic = catch_unwind(AssertUnwindSafe(|| {
                let _ = Engine::<TurnApplication, AcceptingEnvironment, Vec<u8>>::turn(
                    &app,
                    &mut state,
                    Some(&event),
                    &mut batch,
                    certificate,
                );
            }));

            assert!(
                panic.is_err(),
                "a start-turn certificate paired with an Event must panic"
            );
            assert!(
                calls.borrow().is_empty(),
                "a mismatched start-turn Event must be rejected before a handler runs"
            );
            assert_eq!(
                state, 0,
                "rejecting a mismatched start-turn Event must leave handler state untouched"
            );
        }

        /// Invariant: a certificate after the start must be paired with its accepted
        /// Event, and missing input is rejected before either handler runs.
        #[test]
        fn later_turn_without_event_panics_before_handler() {
            let (app, calls, _) = turn_application(ScriptedAnswer::Continue, 0);
            let mut state = 0;
            let mut batch =
                BoundedBuffer::new(1).expect("an event-invariant test must reserve one command");
            let (certificate, _) = later_turn(1, Timestamp::from_nanos(1));

            let panic = catch_unwind(AssertUnwindSafe(|| {
                let _ = Engine::<TurnApplication, AcceptingEnvironment, Vec<u8>>::turn(
                    &app,
                    &mut state,
                    None,
                    &mut batch,
                    certificate,
                );
            }));

            assert!(
                panic.is_err(),
                "a later-turn certificate without its accepted Event must panic"
            );
            assert!(
                calls.borrow().is_empty(),
                "a missing later-turn Event must be rejected before a handler runs"
            );
            assert_eq!(
                state, 0,
                "rejecting a missing later-turn Event must leave handler state untouched"
            );
        }
    }

    mod fatal_finalization {
        use super::*;
        use std::panic::{AssertUnwindSafe, catch_unwind};

        struct ShutdownError {
            label: &'static str,
            dropped: Rc<Cell<bool>>,
        }

        impl Drop for ShutdownError {
            fn drop(&mut self) {
                self.dropped.set(true);
            }
        }

        struct FinalizingEnvironment {
            shutdown_calls: Rc<Cell<usize>>,
            report: ShutdownReport<ShutdownError>,
        }

        impl Environment for FinalizingEnvironment {
            type Event = u8;
            type Command = u8;
            type Error = ShutdownError;

            fn start(&mut self) -> Result<Timestamp, Self::Error> {
                panic!("a finalization test must not start its Environment")
            }

            fn next_event(&mut self) -> Result<(Self::Event, Timestamp), Self::Error> {
                panic!("a finalization test must not request an Event")
            }

            fn dispatch(&mut self, _command: Self::Command) -> Result<(), Self::Error> {
                panic!("a finalization test must not dispatch a Command")
            }

            fn take_error(&mut self) -> Option<Self::Error> {
                panic!("a finalization test must not inspect the Error latch")
            }

            fn shutdown(self) -> ShutdownReport<Self::Error> {
                let Self {
                    shutdown_calls,
                    report,
                } = self;
                shutdown_calls.set(shutdown_calls.get() + 1);
                report
            }
        }

        fn environment(
            quiescence: Quiescence,
            error: Option<ShutdownError>,
            shutdown_calls: &Rc<Cell<usize>>,
        ) -> FinalizingEnvironment {
            FinalizingEnvironment {
                shutdown_calls: Rc::clone(shutdown_calls),
                report: ShutdownReport { quiescence, error },
            }
        }

        /// Invariant: fatal finalization shuts down a started, unconsumed
        /// Environment exactly once and returns the report's quiescence.
        /// Design Doc: RUN-FINALIZE
        #[test]
        fn a_started_environment_is_shutdown_exactly_once() {
            let shutdown_calls = Rc::new(Cell::new(0));
            let environment = environment(Quiescence::Incomplete, None, &shutdown_calls);

            let exit = Engine::<TurnApplication, FinalizingEnvironment, Vec<u8>>::finalize(
                17,
                FatalCause::Core(CoreError::IndexExhausted),
                None,
                Some(environment),
            );

            assert_eq!(
                shutdown_calls.get(),
                1,
                "fatal finalization must shut down an unconsumed Environment exactly once"
            );
            match exit {
                EngineExit::Fatal {
                    state,
                    cause: FatalCause::Core(CoreError::IndexExhausted),
                    quiescence,
                } => {
                    assert_eq!(
                        state, 17,
                        "fatal finalization must preserve the State it receives"
                    );
                    assert_eq!(
                        quiescence,
                        Quiescence::Incomplete,
                        "fatal finalization must use the shutdown report's quiescence"
                    );
                }
                _ => panic!("fatal finalization must preserve the fixed Core cause"),
            }
        }

        /// Invariant: an Error reported by finalizing shutdown is discarded and
        /// never replaces the failure that triggered finalization.
        /// Design Doc: A4, RUN-FINALIZE
        #[test]
        fn the_shutdown_error_never_replaces_the_fixed_cause() {
            let shutdown_calls = Rc::new(Cell::new(0));
            let shutdown_error_dropped = Rc::new(Cell::new(false));
            let cause_dropped = Rc::new(Cell::new(false));
            let environment = environment(
                Quiescence::Quiesced,
                Some(ShutdownError {
                    label: "later shutdown error",
                    dropped: Rc::clone(&shutdown_error_dropped),
                }),
                &shutdown_calls,
            );

            let exit = Engine::<TurnApplication, FinalizingEnvironment, Vec<u8>>::finalize(
                23,
                FatalCause::Application(ScriptedError {
                    label: "fixed application cause",
                    dropped: Rc::clone(&cause_dropped),
                }),
                None,
                Some(environment),
            );

            assert_eq!(
                shutdown_calls.get(),
                1,
                "discarding a shutdown Error must not repeat finalizing shutdown"
            );
            assert!(
                shutdown_error_dropped.get(),
                "the shutdown report's later Error must be discarded during fatal finalization"
            );
            let cause = match exit {
                EngineExit::Fatal {
                    state,
                    cause: FatalCause::Application(error),
                    quiescence,
                } => {
                    assert_eq!(
                        state, 23,
                        "discarding a shutdown Error must not change the returned State"
                    );
                    assert_eq!(
                        quiescence,
                        Quiescence::Quiesced,
                        "discarding a shutdown Error must retain the report's quiescence"
                    );
                    error
                }
                _ => panic!("a shutdown Error must not replace the fixed Application cause"),
            };
            assert_eq!(
                cause.label, "fixed application cause",
                "fatal finalization must return the exact first-observed Error payload"
            );
            assert!(
                !cause_dropped.get(),
                "the fixed Error payload must remain owned by the Fatal exit"
            );
            drop(cause);
            assert!(
                cause_dropped.get(),
                "dropping the Fatal cause must drop its preserved Error payload"
            );
        }

        /// Invariant: a failed Environment start is already quiesced, so fatal
        /// finalization returns without attempting shutdown.
        /// Design Doc: ENV-START, RUN-FINALIZE
        #[test]
        fn a_start_error_skips_shutdown_and_is_quiesced() {
            let start_error_dropped = Rc::new(Cell::new(false));

            let exit = Engine::<TurnApplication, FinalizingEnvironment, Vec<u8>>::finalize(
                29,
                FatalCause::Environment(EnvironmentFatal {
                    error: ShutdownError {
                        label: "start error",
                        dropped: Rc::clone(&start_error_dropped),
                    },
                    operation: EnvironmentOperation::Start,
                }),
                None,
                None,
            );

            match exit {
                EngineExit::Fatal {
                    state,
                    cause:
                        FatalCause::Environment(EnvironmentFatal {
                            error,
                            operation: EnvironmentOperation::Start,
                        }),
                    quiescence,
                } => {
                    assert_eq!(
                        state, 29,
                        "a start failure must preserve the State created before startup"
                    );
                    assert_eq!(
                        error.label, "start error",
                        "a start failure must remain the fixed Environment cause"
                    );
                    assert_eq!(
                        quiescence,
                        Quiescence::Quiesced,
                        "a failed Environment start must finalize as already quiesced"
                    );
                    assert!(
                        !start_error_dropped.get(),
                        "the start Error must remain owned by the Fatal exit"
                    );
                }
                _ => panic!("a start Error must finalize as an Environment Start cause"),
            }
        }

        /// Invariant: once shutdown has consumed the Environment, fatal
        /// finalization uses its retained quiescence without another shutdown.
        /// Design Doc: RUN-FINALIZE
        #[test]
        fn a_consumed_environment_uses_the_retained_quiescence() {
            let exit = Engine::<TurnApplication, FinalizingEnvironment, Vec<u8>>::finalize(
                31,
                FatalCause::Core(CoreError::ShutdownIncomplete),
                Some(Quiescence::Incomplete),
                None,
            );

            match exit {
                EngineExit::Fatal {
                    state,
                    cause: FatalCause::Core(CoreError::ShutdownIncomplete),
                    quiescence,
                } => {
                    assert_eq!(
                        state, 31,
                        "finalization after consuming the Environment must preserve State"
                    );
                    assert_eq!(
                        quiescence,
                        Quiescence::Incomplete,
                        "finalization must preserve the quiescence retained before the Environment was consumed"
                    );
                }
                _ => panic!("a consumed Environment must preserve its fixed Fatal cause"),
            }
        }

        /// Invariant: finalization returns retained quiesced status unchanged after
        /// shutdown has already consumed the Environment.
        #[test]
        fn a_consumed_environment_preserves_retained_quiesced() {
            let exit = Engine::<TurnApplication, FinalizingEnvironment, Vec<u8>>::finalize(
                37,
                FatalCause::Core(CoreError::IndexExhausted),
                Some(Quiescence::Quiesced),
                None,
            );

            match exit {
                EngineExit::Fatal {
                    state,
                    cause: FatalCause::Core(CoreError::IndexExhausted),
                    quiescence,
                } => {
                    assert_eq!(
                        state, 37,
                        "retained Quiesced finalization must preserve State"
                    );
                    assert_eq!(
                        quiescence,
                        Quiescence::Quiesced,
                        "retained Quiesced status must pass through finalization unchanged"
                    );
                }
                _ => panic!("retained Quiesced finalization must preserve its fixed Fatal cause"),
            }
        }

        /// Invariant: retained quiescence proves the Environment was consumed, so
        /// retaining both it and the Environment is rejected before shutdown.
        #[test]
        fn contradictory_retained_quiescence_and_environment_is_an_invariant_panic() {
            let shutdown_calls = Rc::new(Cell::new(0));
            let environment = environment(Quiescence::Quiesced, None, &shutdown_calls);

            let panic = catch_unwind(AssertUnwindSafe(|| {
                let _ = Engine::<TurnApplication, FinalizingEnvironment, Vec<u8>>::finalize(
                    0,
                    FatalCause::Core(CoreError::IndexExhausted),
                    Some(Quiescence::Quiesced),
                    Some(environment),
                );
            }));

            let payload = match panic {
                Err(payload) => payload,
                Ok(_) => panic!(
                    "fatal finalization must reject retained quiescence paired with an Environment"
                ),
            };
            let message = payload
                .downcast_ref::<&str>()
                .copied()
                .or_else(|| payload.downcast_ref::<String>().map(String::as_str));
            assert_eq!(
                message,
                Some(
                    "internal error: entered unreachable code: fatal finalization cannot retain quiescence while still owning the Environment"
                ),
                "the contradictory ownership panic must name the finalization invariant"
            );
            assert_eq!(
                shutdown_calls.get(),
                0,
                "a contradictory finalization state must panic before shutdown"
            );
        }
    }

    #[derive(Debug, PartialEq, Eq)]
    enum RunCall {
        InitialState,
        Start,
        OnStart {
            index: u64,
            logical_time: u64,
        },
        OnEvent {
            event: u8,
            index: u64,
            logical_time: u64,
        },
        NextEvent,
        Dispatch(u8),
        TakeError,
        Shutdown,
    }

    #[derive(Debug, PartialEq, Eq)]
    struct RunState {
        value: usize,
    }

    struct RunApplication {
        calls: Rc<RefCell<Vec<RunCall>>>,
        initial_value: usize,
    }

    impl Application for RunApplication {
        type State = RunState;
        type Event = u8;
        type Command = u8;
        type Error = &'static str;

        fn initial_state(&self) -> Self::State {
            self.calls.borrow_mut().push(RunCall::InitialState);
            RunState {
                value: self.initial_value,
            }
        }

        fn on_start(
            &self,
            state: &mut Self::State,
            context: &mut Context<'_, Self::Command>,
        ) -> Outcome<Self::Error> {
            self.calls.borrow_mut().push(RunCall::OnStart {
                index: context.index().as_u64(),
                logical_time: context.logical_time().as_nanos(),
            });
            state.value += 1;
            Outcome::Stop
        }

        fn on_event(
            &self,
            state: &mut Self::State,
            event: &Self::Event,
            context: &mut Context<'_, Self::Command>,
        ) -> Outcome<Self::Error> {
            self.calls.borrow_mut().push(RunCall::OnEvent {
                event: *event,
                index: context.index().as_u64(),
                logical_time: context.logical_time().as_nanos(),
            });
            state.value += 1;
            Outcome::Stop
        }
    }

    struct RunEnvironment {
        calls: Rc<RefCell<Vec<RunCall>>>,
        start_result: Option<Result<Timestamp, &'static str>>,
    }

    impl Environment for RunEnvironment {
        type Event = u8;
        type Command = u8;
        type Error = &'static str;

        fn start(&mut self) -> Result<Timestamp, Self::Error> {
            self.calls.borrow_mut().push(RunCall::Start);
            self.start_result
                .take()
                .expect("a run fixture must call Environment::start at most once")
        }

        fn next_event(&mut self) -> Result<(Self::Event, Timestamp), Self::Error> {
            self.calls.borrow_mut().push(RunCall::NextEvent);
            Ok((1, Timestamp::from_nanos(1)))
        }

        fn dispatch(&mut self, command: Self::Command) -> Result<(), Self::Error> {
            self.calls.borrow_mut().push(RunCall::Dispatch(command));
            Ok(())
        }

        fn take_error(&mut self) -> Option<Self::Error> {
            self.calls.borrow_mut().push(RunCall::TakeError);
            None
        }

        fn shutdown(self) -> ShutdownReport<Self::Error> {
            self.calls.borrow_mut().push(RunCall::Shutdown);
            ShutdownReport {
                quiescence: Quiescence::Quiesced,
                error: None,
            }
        }
    }

    fn run_fixture(
        start_result: Result<Timestamp, &'static str>,
        initial_value: usize,
        calls: Rc<RefCell<Vec<RunCall>>>,
        bytes: &mut Vec<u8>,
    ) -> EngineExit<RunState, &'static str, &'static str> {
        let app = RunApplication {
            calls: Rc::clone(&calls),
            initial_value,
        };
        let environment = RunEnvironment {
            calls,
            start_result: Some(start_result),
        };
        let config = EngineConfig {
            max_commands_per_turn: NonZeroUsize::new(1)
                .expect("a run fixture command bound must be nonzero"),
            max_record_bytes: NonZeroUsize::new(256)
                .expect("a run fixture record bound must be nonzero"),
        };
        let engine = match Engine::new(config, app, environment, bytes) {
            Ok(engine) => engine,
            Err(_) => panic!("a run fixture Engine must construct with small bounds"),
        };
        engine.run()
    }

    mod run_startup {
        use super::*;

        /// Invariant: initial State is created exactly once before the first
        /// Environment operation, even when that operation fails.
        /// Design Doc: the startup table, by name
        #[test]
        fn state_is_created_before_any_fallible_step() {
            let calls = Rc::new(RefCell::new(Vec::new()));
            let mut bytes = Vec::new();

            let _exit = run_fixture(Err("start failed"), 3, Rc::clone(&calls), &mut bytes);

            assert_eq!(
                calls.borrow().as_slice(),
                &[RunCall::InitialState, RunCall::Start],
                "initial State creation must precede the first fallible Environment operation"
            );
        }

        /// Invariant: a failed Environment start returns a fatal, quiesced run
        /// without invoking shutdown.
        /// Design Doc: ENV-START
        #[test]
        fn a_start_error_exits_fatal_quiesced_without_shutdown() {
            let calls = Rc::new(RefCell::new(Vec::new()));
            let mut bytes = Vec::new();

            let exit = run_fixture(Err("start failed"), 5, Rc::clone(&calls), &mut bytes);

            match exit {
                EngineExit::Fatal {
                    state,
                    cause:
                        FatalCause::Environment(EnvironmentFatal {
                            error,
                            operation: EnvironmentOperation::Start,
                        }),
                    quiescence,
                } => {
                    assert_eq!(
                        state,
                        RunState { value: 5 },
                        "a start failure must carry the State created before startup"
                    );
                    assert_eq!(
                        error, "start failed",
                        "a start failure must preserve the exact Environment Error"
                    );
                    assert_eq!(
                        quiescence,
                        Quiescence::Quiesced,
                        "a failed Environment start must exit already quiesced"
                    );
                }
                _ => panic!("a failed Environment start must be the fatal Start cause"),
            }
            assert_eq!(
                calls
                    .borrow()
                    .iter()
                    .filter(|call| matches!(call, RunCall::Shutdown))
                    .count(),
                0,
                "a failed Environment start must not be followed by shutdown"
            );
        }

        /// Invariant: when Environment startup fails, no handler runs and no
        /// Journal record is written before the fatal exit.
        #[test]
        fn a_start_error_invokes_no_handler_and_writes_no_record() {
            let calls = Rc::new(RefCell::new(Vec::new()));
            let mut bytes = Vec::new();

            let _exit = run_fixture(Err("start failed"), 7, Rc::clone(&calls), &mut bytes);

            assert!(
                !calls
                    .borrow()
                    .iter()
                    .any(|call| matches!(call, RunCall::OnStart { .. } | RunCall::OnEvent { .. })),
                "a failed Environment start must prevent every Application handler"
            );
            assert!(
                bytes.is_empty(),
                "a failed Environment start must leave the Journal sink untouched"
            );
        }

        /// Invariant: start times at both ends of the timestamp domain reach the
        /// first handler and first Journal record without alteration.
        #[test]
        fn boundary_start_times_reach_the_handler_and_journal_unchanged() {
            for nanos in [0, u64::MAX] {
                let calls = Rc::new(RefCell::new(Vec::new()));
                let mut bytes = Vec::new();

                let exit = run_fixture(
                    Ok(Timestamp::from_nanos(nanos)),
                    0,
                    Rc::clone(&calls),
                    &mut bytes,
                );

                assert!(
                    matches!(exit, EngineExit::Stopped { .. }),
                    "a boundary-valued start time must complete a clean Stop run"
                );
                assert!(
                    calls.borrow().contains(&RunCall::OnStart {
                        index: 0,
                        logical_time: nanos,
                    }),
                    "the start handler must observe the exact frozen boundary timestamp"
                );
                let first_record = format!(
                    "{{\"record_kind\":\"RunStarted\",\"index\":0,\"schema_version\":1,\"logical_time\":{nanos}}}\n"
                );
                assert!(
                    bytes.starts_with(first_record.as_bytes()),
                    "the first Journal record must contain the exact frozen boundary timestamp"
                );
            }
        }
    }

    mod run_stop_path {
        use super::*;

        /// Invariant: stopping during the start turn writes exactly RunStarted,
        /// StopRequested, and TurnCompleted Stop in that order.
        /// Design Doc: RUN-GRAMMAR, RUN-RECORDS
        #[test]
        fn stop_at_start_produces_the_three_record_journal() {
            let calls = Rc::new(RefCell::new(Vec::new()));
            let mut bytes = Vec::new();

            let exit = run_fixture(Ok(Timestamp::from_nanos(37)), 0, calls, &mut bytes);

            assert!(
                matches!(exit, EngineExit::Stopped { .. }),
                "a clean Stop answer during the start turn must return Stopped"
            );
            assert_eq!(
                bytes,
                br#"{"record_kind":"RunStarted","index":0,"schema_version":1,"logical_time":37}
{"record_kind":"StopRequested","index":0}
{"record_kind":"TurnCompleted","index":0,"outcome":"Stop"}
"#,
                "a Stop-at-start run must write exactly its three required records"
            );
        }

        /// Invariant: a stopped run returns the final State including mutations
        /// made by its start handler.
        /// Design Doc: EngineExit, by name
        #[test]
        fn stopped_carries_the_final_state() {
            let calls = Rc::new(RefCell::new(Vec::new()));
            let mut bytes = Vec::new();

            let exit = run_fixture(Ok(Timestamp::from_nanos(0)), 41, calls, &mut bytes);

            match exit {
                EngineExit::Stopped { state } => assert_eq!(
                    state,
                    RunState { value: 42 },
                    "Stopped must carry the State after the start handler's mutation"
                ),
                EngineExit::Fatal { .. } => {
                    panic!("a clean Stop-at-start run must not return Fatal")
                }
            }
        }

        /// Invariant: a Stop-at-start run invokes Environment operations serially
        /// as start, one checkpoint, and consuming shutdown.
        /// Design Doc: ENV-SERIAL
        #[test]
        fn the_call_sequence_matches_env_serial() {
            let calls = Rc::new(RefCell::new(Vec::new()));
            let mut bytes = Vec::new();

            let exit = run_fixture(
                Ok(Timestamp::from_nanos(11)),
                0,
                Rc::clone(&calls),
                &mut bytes,
            );

            assert!(
                matches!(exit, EngineExit::Stopped { .. }),
                "the serial call trace fixture must finish as Stopped"
            );
            assert_eq!(
                calls.borrow().as_slice(),
                &[
                    RunCall::InitialState,
                    RunCall::Start,
                    RunCall::OnStart {
                        index: 0,
                        logical_time: 11,
                    },
                    RunCall::TakeError,
                    RunCall::Shutdown,
                ],
                "a Stop-at-start run must call start first, checkpoint once, and shutdown last"
            );
        }

        /// Invariant: stopping during the start turn invokes the start handler once
        /// and never invokes the Event handler.
        #[test]
        fn stop_at_start_invokes_only_the_start_handler_once() {
            let calls = Rc::new(RefCell::new(Vec::new()));
            let mut bytes = Vec::new();

            let exit = run_fixture(
                Ok(Timestamp::from_nanos(13)),
                0,
                Rc::clone(&calls),
                &mut bytes,
            );

            assert!(
                matches!(exit, EngineExit::Stopped { .. }),
                "the handler-selection fixture must finish as Stopped"
            );
            let calls = calls.borrow();
            assert_eq!(
                calls
                    .iter()
                    .filter(|call| matches!(call, RunCall::OnStart { .. }))
                    .count(),
                1,
                "a Stop-at-start run must invoke on_start exactly once"
            );
            assert_eq!(
                calls
                    .iter()
                    .filter(|call| matches!(call, RunCall::OnEvent { .. }))
                    .count(),
                0,
                "a Stop-at-start run must never invoke on_event"
            );
        }
    }

    mod run_turn_loop {
        use super::*;
        use std::collections::VecDeque;

        #[derive(Clone, Copy)]
        enum LoopAnswer {
            Continue,
            Stop,
            Fatal,
        }

        struct TurnScript {
            mutation: u8,
            commands: Vec<u8>,
            answer: LoopAnswer,
        }

        struct LoopError {
            label: &'static str,
            drops: Rc<Cell<usize>>,
        }

        impl Drop for LoopError {
            fn drop(&mut self) {
                self.drops.set(self.drops.get() + 1);
            }
        }

        #[derive(Debug, PartialEq, Eq)]
        enum LoopCall {
            InitialState,
            Start,
            OnStart {
                index: u64,
                logical_time: u64,
            },
            OnEvent {
                event: u8,
                index: u64,
                logical_time: u64,
            },
            Dispatch(u8),
            TakeError,
            NextEvent,
            Shutdown,
        }

        struct LoopApplication {
            turns: RefCell<VecDeque<TurnScript>>,
            calls: Rc<RefCell<Vec<LoopCall>>>,
            error_drops: Rc<Cell<usize>>,
        }

        impl LoopApplication {
            fn handle(
                &self,
                state: &mut Vec<u8>,
                context: &mut Context<'_, u8>,
            ) -> Outcome<LoopError> {
                let turn =
                    self.turns.borrow_mut().pop_front().expect(
                        "the loop must not invoke more handlers than the test script provides",
                    );
                state.push(turn.mutation);
                for command in turn.commands {
                    context.emit(command);
                }
                match turn.answer {
                    LoopAnswer::Continue => Outcome::Continue,
                    LoopAnswer::Stop => Outcome::Stop,
                    LoopAnswer::Fatal => Outcome::Fatal(LoopError {
                        label: "scripted handler fatal",
                        drops: Rc::clone(&self.error_drops),
                    }),
                }
            }
        }

        impl Application for LoopApplication {
            type State = Vec<u8>;
            type Event = u8;
            type Command = u8;
            type Error = LoopError;

            fn initial_state(&self) -> Self::State {
                self.calls.borrow_mut().push(LoopCall::InitialState);
                Vec::new()
            }

            fn on_start(
                &self,
                state: &mut Self::State,
                context: &mut Context<'_, Self::Command>,
            ) -> Outcome<Self::Error> {
                self.calls.borrow_mut().push(LoopCall::OnStart {
                    index: context.index().as_u64(),
                    logical_time: context.logical_time().as_nanos(),
                });
                self.handle(state, context)
            }

            fn on_event(
                &self,
                state: &mut Self::State,
                event: &Self::Event,
                context: &mut Context<'_, Self::Command>,
            ) -> Outcome<Self::Error> {
                self.calls.borrow_mut().push(LoopCall::OnEvent {
                    event: *event,
                    index: context.index().as_u64(),
                    logical_time: context.logical_time().as_nanos(),
                });
                self.handle(state, context)
            }
        }

        struct LoopEnvironment {
            calls: Rc<RefCell<Vec<LoopCall>>>,
            events: VecDeque<(u8, Timestamp)>,
        }

        impl Environment for LoopEnvironment {
            type Event = u8;
            type Command = u8;
            type Error = &'static str;

            fn start(&mut self) -> Result<Timestamp, Self::Error> {
                self.calls.borrow_mut().push(LoopCall::Start);
                Ok(Timestamp::from_nanos(10))
            }

            fn next_event(&mut self) -> Result<(Self::Event, Timestamp), Self::Error> {
                self.calls.borrow_mut().push(LoopCall::NextEvent);
                Ok(self
                    .events
                    .pop_front()
                    .expect("the loop must not request more Events than the test script provides"))
            }

            fn dispatch(&mut self, command: Self::Command) -> Result<(), Self::Error> {
                self.calls.borrow_mut().push(LoopCall::Dispatch(command));
                Ok(())
            }

            fn take_error(&mut self) -> Option<Self::Error> {
                self.calls.borrow_mut().push(LoopCall::TakeError);
                None
            }

            fn shutdown(self) -> ShutdownReport<Self::Error> {
                self.calls.borrow_mut().push(LoopCall::Shutdown);
                ShutdownReport {
                    quiescence: Quiescence::Quiesced,
                    error: None,
                }
            }
        }

        fn turn(mutation: u8, commands: &[u8], answer: LoopAnswer) -> TurnScript {
            TurnScript {
                mutation,
                commands: commands.to_vec(),
                answer,
            }
        }

        #[allow(
            clippy::type_complexity,
            reason = "the fixture returns the run exit with both shared observation handles"
        )]
        fn run_loop(
            turns: Vec<TurnScript>,
            events: Vec<(u8, u64)>,
            max_commands_per_turn: usize,
            bytes: &mut Vec<u8>,
        ) -> (
            EngineExit<Vec<u8>, LoopError, &'static str>,
            Rc<RefCell<Vec<LoopCall>>>,
            Rc<Cell<usize>>,
        ) {
            let calls = Rc::new(RefCell::new(Vec::new()));
            let error_drops = Rc::new(Cell::new(0));
            let app = LoopApplication {
                turns: RefCell::new(turns.into()),
                calls: Rc::clone(&calls),
                error_drops: Rc::clone(&error_drops),
            };
            let environment = LoopEnvironment {
                calls: Rc::clone(&calls),
                events: events
                    .into_iter()
                    .map(|(event, nanos)| (event, Timestamp::from_nanos(nanos)))
                    .collect(),
            };
            let config = EngineConfig {
                max_commands_per_turn: NonZeroUsize::new(max_commands_per_turn)
                    .expect("a loop test command bound must be nonzero"),
                max_record_bytes: NonZeroUsize::new(256)
                    .expect("a loop test record bound must be nonzero"),
            };
            let engine = match Engine::new(config, app, environment, bytes) {
                Ok(engine) => engine,
                Err(_) => panic!("a loop test Engine must construct with small bounds"),
            };

            (engine.run(), calls, error_drops)
        }

        /// Invariant: each continued turn finishes its command handoff and
        /// checkpoint before the next Event is requested, and accepted Events reach
        /// handlers once in source order.
        /// Design Doc: A2
        #[test]
        fn continue_turns_accept_events_in_sequence() {
            let turns = vec![
                turn(1, &[10], LoopAnswer::Continue),
                turn(2, &[20], LoopAnswer::Continue),
                turn(3, &[30], LoopAnswer::Stop),
            ];
            let mut bytes = Vec::new();

            let (exit, calls, _) = run_loop(turns, vec![(7, 11), (8, 12)], 1, &mut bytes);

            match exit {
                EngineExit::Stopped { state } => assert_eq!(
                    state,
                    vec![1, 2, 3],
                    "a serial three-turn run must retain one mutation from each handler"
                ),
                EngineExit::Fatal { .. } => {
                    panic!("a serial Continue, Continue, Stop script must finish cleanly")
                }
            }
            assert_eq!(
                calls.borrow().as_slice(),
                &[
                    LoopCall::InitialState,
                    LoopCall::Start,
                    LoopCall::OnStart {
                        index: 0,
                        logical_time: 10,
                    },
                    LoopCall::Dispatch(10),
                    LoopCall::TakeError,
                    LoopCall::NextEvent,
                    LoopCall::OnEvent {
                        event: 7,
                        index: 1,
                        logical_time: 11,
                    },
                    LoopCall::Dispatch(20),
                    LoopCall::TakeError,
                    LoopCall::NextEvent,
                    LoopCall::OnEvent {
                        event: 8,
                        index: 2,
                        logical_time: 12,
                    },
                    LoopCall::Dispatch(30),
                    LoopCall::TakeError,
                    LoopCall::Shutdown,
                ],
                "each turn must complete before the next Event is requested, with Events handled in order"
            );
        }

        /// Invariant: exceeding the command bound fixes command overflow as the
        /// run's cause, discards its staged batch, and outranks every handler answer.
        /// Design Doc: the TurnOpen phase row, by name
        #[test]
        fn overflow_beats_the_returned_outcome_and_discards_the_batch() {
            for answer in [LoopAnswer::Continue, LoopAnswer::Stop, LoopAnswer::Fatal] {
                let mut bytes = Vec::new();
                let (exit, calls, error_drops) =
                    run_loop(vec![turn(1, &[10, 11], answer)], Vec::new(), 1, &mut bytes);

                match exit {
                    EngineExit::Fatal {
                        state,
                        cause: FatalCause::Core(CoreError::CommandBoundExceeded),
                        quiescence,
                    } => {
                        assert_eq!(
                            state,
                            vec![1],
                            "command overflow must retain the failing handler's State mutation"
                        );
                        assert_eq!(
                            quiescence,
                            Quiescence::Quiesced,
                            "command overflow must carry finalizing shutdown's quiescence"
                        );
                    }
                    _ => panic!("command overflow must outrank every returned Outcome"),
                }
                assert_eq!(
                    calls.borrow().as_slice(),
                    &[
                        LoopCall::InitialState,
                        LoopCall::Start,
                        LoopCall::OnStart {
                            index: 0,
                            logical_time: 10,
                        },
                        LoopCall::Shutdown,
                    ],
                    "an overflowing batch must be discarded before dispatch, checkpoint, or Event acquisition"
                );
                assert_eq!(
                    error_drops.get(),
                    if matches!(answer, LoopAnswer::Fatal) {
                        1
                    } else {
                        0
                    },
                    "an outranked Application Error must be discarded while nonfatal answers create no Error"
                );
            }
        }

        /// Invariant: a handler failure preserves its exact Error while discarding
        /// only that turn's staged commands and retaining effects from prior turns.
        /// Design Doc: A4
        #[test]
        fn a_handler_fatal_discards_the_batch_and_carries_the_error() {
            let turns = vec![
                turn(1, &[10], LoopAnswer::Continue),
                turn(2, &[20], LoopAnswer::Fatal),
            ];
            let mut bytes = Vec::new();

            let (exit, calls, error_drops) = run_loop(turns, vec![(7, 11)], 1, &mut bytes);
            let error = match exit {
                EngineExit::Fatal {
                    state,
                    cause: FatalCause::Application(error),
                    quiescence,
                } => {
                    assert_eq!(
                        state,
                        vec![1, 2],
                        "an event-handler failure must retain mutations from both completed handler calls"
                    );
                    assert_eq!(
                        quiescence,
                        Quiescence::Quiesced,
                        "a handler failure must carry finalizing shutdown's quiescence"
                    );
                    error
                }
                _ => panic!(
                    "a non-overflowing handler Fatal must remain the run's Application cause"
                ),
            };
            assert_eq!(
                error.label, "scripted handler fatal",
                "a handler Fatal must carry the exact Error payload returned by the handler"
            );
            assert_eq!(
                error_drops.get(),
                0,
                "the handler Error must remain owned by the Fatal exit"
            );
            assert_eq!(
                calls.borrow().as_slice(),
                &[
                    LoopCall::InitialState,
                    LoopCall::Start,
                    LoopCall::OnStart {
                        index: 0,
                        logical_time: 10,
                    },
                    LoopCall::Dispatch(10),
                    LoopCall::TakeError,
                    LoopCall::NextEvent,
                    LoopCall::OnEvent {
                        event: 7,
                        index: 1,
                        logical_time: 11,
                    },
                    LoopCall::Shutdown,
                ],
                "a fatal event turn must not dispatch its batch, checkpoint, or request another Event"
            );
            drop(error);
            assert_eq!(
                error_drops.get(),
                1,
                "dropping the Fatal cause must drop its preserved Application Error"
            );
        }

        /// Invariant: mutations made by a failing handler remain in the returned
        /// State for both application failure and command overflow, on start and
        /// Event turns alike.
        /// Design Doc: APP-STATE
        #[test]
        fn state_mutations_stand_on_every_fatal_exit() {
            enum ExpectedCause {
                Application,
                Overflow,
            }

            let scenarios = [
                (
                    vec![turn(1, &[], LoopAnswer::Fatal)],
                    Vec::new(),
                    ExpectedCause::Application,
                    vec![1],
                ),
                (
                    vec![turn(1, &[10, 11], LoopAnswer::Continue)],
                    Vec::new(),
                    ExpectedCause::Overflow,
                    vec![1],
                ),
                (
                    vec![
                        turn(1, &[], LoopAnswer::Continue),
                        turn(2, &[], LoopAnswer::Fatal),
                    ],
                    vec![(7, 11)],
                    ExpectedCause::Application,
                    vec![1, 2],
                ),
                (
                    vec![
                        turn(1, &[], LoopAnswer::Continue),
                        turn(2, &[20, 21], LoopAnswer::Stop),
                    ],
                    vec![(7, 11)],
                    ExpectedCause::Overflow,
                    vec![1, 2],
                ),
            ];

            for (turns, events, expected_cause, expected_state) in scenarios {
                let mut bytes = Vec::new();
                let (exit, _, _) = run_loop(turns, events, 1, &mut bytes);

                match exit {
                    EngineExit::Fatal { state, cause, .. } => {
                        assert_eq!(
                            state, expected_state,
                            "a handler-phase Fatal exit must retain every mutation through the failing handler"
                        );
                        assert!(
                            matches!(
                                (&expected_cause, cause),
                                (ExpectedCause::Application, FatalCause::Application(_))
                                    | (
                                        ExpectedCause::Overflow,
                                        FatalCause::Core(CoreError::CommandBoundExceeded)
                                    )
                            ),
                            "each State scenario must reach its scripted handler-phase Fatal cause"
                        );
                    }
                    EngineExit::Stopped { .. } => {
                        panic!("every State-retention scenario must end Fatal")
                    }
                }
            }
        }

        /// Invariant: when an accepted Event turn exceeds its command bound, the
        /// journal locates the turn but records none of its staged command intent.
        /// Design Doc: the intent-vacuum derivation, by name
        #[test]
        fn an_over_emitting_turn_leaves_no_command_record() {
            let turns = vec![
                turn(1, &[], LoopAnswer::Continue),
                turn(2, &[41, 42], LoopAnswer::Stop),
            ];
            let mut bytes = Vec::new();

            let (exit, calls, _) = run_loop(turns, vec![(9, 11)], 1, &mut bytes);

            assert!(
                matches!(
                    exit,
                    EngineExit::Fatal {
                        cause: FatalCause::Core(CoreError::CommandBoundExceeded),
                        ..
                    }
                ),
                "an over-emitting Event turn must exit with command-bound overflow"
            );
            let journal = std::str::from_utf8(&bytes)
                .expect("an intent-vacuum test Journal must contain UTF-8 JSON records");
            let records: Vec<_> = journal.lines().collect();
            assert_eq!(
                records.len(),
                3,
                "an overflowing first Event turn must stop after its EventAccepted record"
            );
            assert!(
                records[2].contains("\"record_kind\":\"EventAccepted\"")
                    && records[2].contains("\"index\":1"),
                "the EventAccepted record must identify the over-emitting turn"
            );
            assert!(
                !journal.contains("\"record_kind\":\"CommandsPrepared\"")
                    && !journal.contains("\"commands\""),
                "an overflowing turn must leave no command-intent record"
            );
            assert!(
                !calls
                    .borrow()
                    .iter()
                    .any(|call| matches!(call, LoopCall::Dispatch(_))),
                "commands staged by an overflowing turn must never be dispatched"
            );
        }

        /// Invariant: filling the command batch exactly on consecutive turns
        /// dispatches every command once in per-turn order without false overflow.
        #[test]
        fn exact_capacity_batches_dispatch_once_in_order_across_reused_turns() {
            let turns = vec![
                turn(1, &[10, 11], LoopAnswer::Continue),
                turn(2, &[20, 21], LoopAnswer::Stop),
            ];
            let mut bytes = Vec::new();

            let (exit, calls, _) = run_loop(turns, vec![(7, 11)], 2, &mut bytes);

            assert!(
                matches!(exit, EngineExit::Stopped { state } if state == vec![1, 2]),
                "exact-capacity batches on reused turns must complete without overflow"
            );
            let dispatched: Vec<_> = calls
                .borrow()
                .iter()
                .filter_map(|call| match call {
                    LoopCall::Dispatch(command) => Some(*command),
                    _ => None,
                })
                .collect();
            assert_eq!(
                dispatched,
                vec![10, 11, 20, 21],
                "each exact-capacity batch must dispatch once in turn and emission order"
            );
        }

        /// Invariant: a failure returned by the start handler ends the run before
        /// command effects, checkpointing, or Event acquisition and then shuts down
        /// exactly once.
        #[test]
        fn a_start_handler_fatal_performs_no_effect_phase_or_event_request() {
            let mut bytes = Vec::new();

            let (exit, calls, _) = run_loop(
                vec![turn(1, &[10], LoopAnswer::Fatal)],
                Vec::new(),
                1,
                &mut bytes,
            );

            assert!(
                matches!(
                    exit,
                    EngineExit::Fatal {
                        state,
                        cause: FatalCause::Application(_),
                        quiescence: Quiescence::Quiesced,
                    } if state == vec![1]
                ),
                "a start-handler Fatal must preserve State and remain the Application cause"
            );
            assert_eq!(
                calls.borrow().as_slice(),
                &[
                    LoopCall::InitialState,
                    LoopCall::Start,
                    LoopCall::OnStart {
                        index: 0,
                        logical_time: 10,
                    },
                    LoopCall::Shutdown,
                ],
                "a start-handler Fatal must skip effects and Event acquisition before one shutdown"
            );
        }

        /// Invariant: a Continue turn with no commands takes the empty effects path,
        /// accepts one Event, and a Stop answer prevents another Event request.
        #[test]
        fn an_empty_continue_turn_accepts_exactly_one_event_before_stop() {
            let turns = vec![
                turn(1, &[], LoopAnswer::Continue),
                turn(2, &[], LoopAnswer::Stop),
            ];
            let mut bytes = Vec::new();

            let (exit, calls, _) = run_loop(turns, vec![(7, 11)], 1, &mut bytes);

            assert!(
                matches!(exit, EngineExit::Stopped { state } if state == vec![1, 2]),
                "an empty Continue turn followed by Stop must finish with both State mutations"
            );
            assert_eq!(
                calls.borrow().as_slice(),
                &[
                    LoopCall::InitialState,
                    LoopCall::Start,
                    LoopCall::OnStart {
                        index: 0,
                        logical_time: 10,
                    },
                    LoopCall::TakeError,
                    LoopCall::NextEvent,
                    LoopCall::OnEvent {
                        event: 7,
                        index: 1,
                        logical_time: 11,
                    },
                    LoopCall::TakeError,
                    LoopCall::Shutdown,
                ],
                "an empty Continue turn must request one Event, and Stop must end the back edge"
            );
        }
    }
}
