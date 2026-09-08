use super::record::{
    Certificate, Checkpointed, ClassifiedTurn, CloseFatal, Initial, JournalFatal, StopPending,
    TurnOpen, TurnOutcome, answer,
};
use crate::application::{Application, Context, Outcome};
use crate::bounded_buffer::BoundedBuffer;
use crate::environment::{Environment, Quiescence};
use crate::journal::{Journal, JournalBuildError};
use crate::time::Timestamp;
use std::collections::TryReserveError;
use std::error::Error;
use std::fmt;
use std::io;
use std::num::NonZeroUsize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EngineConfig {
    pub max_commands_per_turn: NonZeroUsize,
    pub max_record_bytes: NonZeroUsize,
}

#[derive(Debug)]
pub struct Engine<A, E, W>
where
    A: Application,
    W: io::Write,
{
    app: A,
    env: E,
    journal: Journal<W>,
    batch: BoundedBuffer<A::Command>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildError {
    CommandBuffer(TryReserveError),
    Journal(JournalBuildError),
}

/// The source `RUN-FINALIZE` fixes a Fatal exit's quiescence from.
enum Finalization<E> {
    /// `start` returned `Err`: the Environment is already quiesced (`ENV-START`).
    StartFailed,
    /// `start` returned `Ok` and the Environment is unconsumed: `shutdown`
    /// supplies the quiescence and its Error is discarded (A4).
    Unconsumed(E),
    /// `StopPending` consumed the Environment: the quiescence its close retained.
    Retained(Quiescence),
}

impl<A, E, W> Engine<A, E, W>
where
    A: Application,
    E: Environment<Event = A::Event, Command = A::Command>,
    W: io::Write,
{
    /// # Errors
    ///
    /// `BuildError` names the reservation that failed; no run happened.
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
            "RUN-GRAMMAR: a start-turn certificate carries no Event and a later-turn certificate carries one"
        );

        let mut context = Context::new(batch, index, logical_time);
        let answer = match event {
            None => app.on_start(state, &mut context),
            Some(event) => app.on_event(state, event, &mut context),
        };
        if context.overflowed() {
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

    fn finalize(
        state: A::State,
        cause: FatalCause<A::Error, E::Error>,
        finalization: Finalization<E>,
    ) -> EngineExit<A::State, A::Error, E::Error> {
        let quiescence = match finalization {
            Finalization::StartFailed => Quiescence::Quiesced,
            Finalization::Unconsumed(environment) => environment.shutdown().quiescence,
            Finalization::Retained(quiescence) => quiescence,
        };

        EngineExit::Fatal {
            state,
            cause,
            quiescence,
        }
    }

    #[expect(
        clippy::type_complexity,
        reason = "the helper returns the typed checkpoint successor or the shared fatal cause"
    )]
    fn effects<M: answer::Answer>(
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

    fn drive(
        app: &A,
        state: &mut A::State,
        env: &mut E,
        batch: &mut BoundedBuffer<A::Command>,
        certificate: Certificate<W, Initial>,
    ) -> Result<Certificate<W, StopPending>, FatalCause<A::Error, E::Error>> {
        let mut certificate = certificate.run_started().map_err(FatalCause::Journal)?;
        let mut accepted_event = None;

        loop {
            match Self::turn(app, state, accepted_event.as_ref(), batch, certificate)? {
                ClassifiedTurn::Continue(classified) => {
                    let (next, event) = Self::effects(classified, env, batch)?
                        .complete_continue()
                        .map_err(FatalCause::Journal)?
                        .accept_event(env)?;
                    accepted_event = Some(event);
                    certificate = next;
                }
                ClassifiedTurn::Stop(classified) => {
                    return Self::effects(classified, env, batch)?
                        .request_stop()
                        .map_err(FatalCause::Journal);
                }
            }
        }
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
                    FatalCause::environment(error, EnvironmentOperation::Start),
                    Finalization::StartFailed,
                );
            }
        };
        let certificate = Certificate::mint(journal, start_time);
        let stop_pending = match Self::drive(&app, &mut state, &mut env, &mut batch, certificate) {
            Ok(stop_pending) => stop_pending,
            Err(cause) => return Self::finalize(state, cause, Finalization::Unconsumed(env)),
        };

        match stop_pending.close(env) {
            Ok(_closed) => EngineExit::Stopped { state },
            Err(CloseFatal { cause, quiescence }) => {
                Self::finalize(state, cause, Finalization::Retained(quiescence))
            }
        }
    }
}

#[derive(Debug)]
#[must_use = "a dropped exit loses the final State and the Fatal cause"]
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

#[derive(Debug)]
pub enum FatalCause<AE, EE> {
    Application(AE),
    Environment(EnvironmentFatal<EE>),
    Journal(JournalFatal),
    Core(CoreError),
}

impl<AE, EE> FatalCause<AE, EE> {
    /// The Environment cause observed at `operation`.
    pub(super) const fn environment(error: EE, operation: EnvironmentOperation) -> Self {
        Self::Environment(EnvironmentFatal { error, operation })
    }
}

impl<AE, EE> fmt::Display for FatalCause<AE, EE> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Application(_) => f.write_str("the Application handler answered Fatal"),
            Self::Environment(_) => f.write_str("an Environment operation observed an Error"),
            Self::Journal(_) => f.write_str("a Journal record did not commit"),
            Self::Core(_) => f.write_str("a Core condition ended the run"),
        }
    }
}

impl<AE, EE> Error for FatalCause<AE, EE>
where
    AE: Error + 'static,
    EE: Error + 'static,
{
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Application(error) => Some(error),
            Self::Environment(fatal) => Some(fatal),
            Self::Journal(fatal) => Some(fatal),
            Self::Core(error) => Some(error),
        }
    }
}

/// Names the operation where the Error was observed - not necessarily where it was
/// caused (`ENV-LATCH`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EnvironmentFatal<EE> {
    pub error: EE,
    pub operation: EnvironmentOperation,
}

impl<EE> fmt::Display for EnvironmentFatal<EE> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "an Error was observed at {}", self.operation)
    }
}

impl<EE: Error + 'static> Error for EnvironmentFatal<EE> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.error)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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

impl fmt::Display for EnvironmentOperation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Start => f.write_str("start"),
            Self::NextEvent => f.write_str("next_event"),
            Self::Dispatch { position } => write!(f, "dispatch position {position}"),
            Self::Checkpoint => f.write_str("the checkpoint"),
            Self::Shutdown => f.write_str("shutdown"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CoreError {
    TimeRegression {
        previous: Timestamp,
        offered: Timestamp,
    },
    IndexExhausted,
    CommandBoundExceeded,
    ShutdownIncomplete,
}

impl fmt::Display for CoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TimeRegression { previous, offered } => write!(
                f,
                "the candidate's time {} precedes the last accepted time {}",
                offered.as_nanos(),
                previous.as_nanos()
            ),
            Self::IndexExhausted => f.write_str("the index domain is exhausted"),
            Self::CommandBoundExceeded => {
                f.write_str("the turn emitted more Commands than max_commands_per_turn")
            }
            Self::ShutdownIncomplete => f.write_str("shutdown reported Incomplete with no Error"),
        }
    }
}

impl Error for CoreError {}

impl fmt::Display for BuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CommandBuffer(_) => f.write_str("the Command batch could not be reserved"),
            Self::Journal(_) => f.write_str("the Journal could not be built"),
        }
    }
}

impl Error for BuildError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::CommandBuffer(error) => Some(error),
            Self::Journal(error) => Some(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Context, Outcome, ShutdownReport};
    use std::cell::{Cell, RefCell};
    use std::collections::VecDeque;
    use std::rc::Rc;

    type TestEngine = Engine<ScriptedApplication, ScriptedEnvironment, Vec<u8>>;
    type TestExit = EngineExit<Vec<u8>, ScriptedError, ScriptedError>;
    type TestCause = FatalCause<ScriptedError, ScriptedError>;
    type TurnResult = Result<ClassifiedTurn<Vec<u8>>, TestCause>;
    type Calls = Rc<RefCell<Vec<Call>>>;
    type Drops = Rc<Cell<usize>>;

    const FATAL: &str = "scripted handler fatal";

    /// Every Application and Environment call the fixtures observe, in the order
    /// the Engine made them.
    #[derive(Debug, PartialEq, Eq)]
    enum Call {
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

    /// Every Error the fixtures hand the Engine; `drops` counts how many of one
    /// fixture's Errors have been released.
    #[derive(Debug)]
    struct ScriptedError {
        label: &'static str,
        drops: Drops,
    }

    impl Drop for ScriptedError {
        fn drop(&mut self) {
            self.drops.set(self.drops.get() + 1);
        }
    }

    struct Turn {
        mutation: u8,
        commands: Vec<u8>,
        answer: Outcome<&'static str>,
    }

    fn turn(mutation: u8, commands: &[u8], answer: Outcome<&'static str>) -> Turn {
        Turn {
            mutation,
            commands: commands.to_vec(),
            answer,
        }
    }

    struct ScriptedApplication {
        turns: RefCell<VecDeque<Turn>>,
        calls: Calls,
        drops: Drops,
    }

    impl ScriptedApplication {
        fn handle(
            &self,
            state: &mut Vec<u8>,
            context: &mut Context<'_, u8>,
        ) -> Outcome<ScriptedError> {
            let turn = self
                .turns
                .borrow_mut()
                .pop_front()
                .expect("the loop must not invoke more handlers than the test script provides");
            state.push(turn.mutation);
            for command in turn.commands {
                context.emit(command);
            }
            match turn.answer {
                Outcome::Continue => Outcome::Continue,
                Outcome::Stop => Outcome::Stop,
                Outcome::Fatal(label) => Outcome::Fatal(ScriptedError {
                    label,
                    drops: Rc::clone(&self.drops),
                }),
            }
        }
    }

    impl Application for ScriptedApplication {
        type State = Vec<u8>;
        type Event = u8;
        type Command = u8;
        type Error = ScriptedError;

        fn initial_state(&self) -> Self::State {
            self.calls.borrow_mut().push(Call::InitialState);
            Vec::new()
        }

        fn on_start(
            &self,
            state: &mut Self::State,
            context: &mut Context<'_, Self::Command>,
        ) -> Outcome<Self::Error> {
            self.calls.borrow_mut().push(Call::OnStart {
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
            self.calls.borrow_mut().push(Call::OnEvent {
                event: *event,
                index: context.index().as_u64(),
                logical_time: context.logical_time().as_nanos(),
            });
            self.handle(state, context)
        }
    }

    struct ScriptedEnvironment {
        calls: Calls,
        start: Option<Result<Timestamp, ScriptedError>>,
        events: VecDeque<(u8, Timestamp)>,
        report: ShutdownReport<ScriptedError>,
    }

    impl Environment for ScriptedEnvironment {
        type Event = u8;
        type Command = u8;
        type Error = ScriptedError;

        fn start(&mut self) -> Result<Timestamp, Self::Error> {
            self.calls.borrow_mut().push(Call::Start);
            self.start
                .take()
                .expect("a scripted Environment must start at most once")
        }

        fn next_event(&mut self) -> Result<(Self::Event, Timestamp), Self::Error> {
            self.calls.borrow_mut().push(Call::NextEvent);
            Ok(self
                .events
                .pop_front()
                .expect("the loop must not request more Events than the test script provides"))
        }

        fn dispatch(&mut self, command: Self::Command) -> Result<(), Self::Error> {
            self.calls.borrow_mut().push(Call::Dispatch(command));
            Ok(())
        }

        fn take_error(&mut self) -> Option<Self::Error> {
            self.calls.borrow_mut().push(Call::TakeError);
            None
        }

        fn shutdown(self) -> ShutdownReport<Self::Error> {
            let Self { calls, report, .. } = self;
            calls.borrow_mut().push(Call::Shutdown);
            report
        }
    }

    struct Scripted {
        app: ScriptedApplication,
        environment: ScriptedEnvironment,
        calls: Calls,
        drops: Drops,
    }

    fn scripted(
        turns: Vec<Turn>,
        start: Result<Timestamp, &'static str>,
        events: &[(u8, u64)],
        report: ShutdownReport<&'static str>,
    ) -> Scripted {
        let calls: Calls = Rc::new(RefCell::new(Vec::new()));
        let drops: Drops = Rc::new(Cell::new(0));
        let error = |label| ScriptedError {
            label,
            drops: Rc::clone(&drops),
        };
        Scripted {
            app: ScriptedApplication {
                turns: RefCell::new(turns.into()),
                calls: Rc::clone(&calls),
                drops: Rc::clone(&drops),
            },
            environment: ScriptedEnvironment {
                calls: Rc::clone(&calls),
                start: Some(start.map_err(error)),
                events: events
                    .iter()
                    .map(|&(event, nanos)| (event, Timestamp::from_nanos(nanos)))
                    .collect(),
                report: ShutdownReport {
                    quiescence: report.quiescence,
                    error: report.error.map(error),
                },
            },
            calls,
            drops,
        }
    }

    fn clean_report() -> ShutdownReport<&'static str> {
        ShutdownReport {
            quiescence: Quiescence::Quiesced,
            error: None,
        }
    }

    fn one_turn(commands: &[u8], answer: Outcome<&'static str>) -> Scripted {
        scripted(
            vec![turn(1, commands, answer)],
            Ok(Timestamp::from_nanos(0)),
            &[],
            clean_report(),
        )
    }

    fn config(max_commands_per_turn: usize, max_record_bytes: usize) -> EngineConfig {
        EngineConfig {
            max_commands_per_turn: NonZeroUsize::new(max_commands_per_turn)
                .expect("an Engine test command bound must be nonzero"),
            max_record_bytes: NonZeroUsize::new(max_record_bytes)
                .expect("an Engine test record bound must be nonzero"),
        }
    }

    fn reserved(capacity: usize) -> BoundedBuffer<u8> {
        BoundedBuffer::new(capacity).expect("an Engine test batch must reserve its capacity")
    }

    fn start_turn(logical_time: Timestamp) -> Certificate<Vec<u8>, TurnOpen> {
        let journal = Journal::new(
            Vec::new(),
            NonZeroUsize::new(256).expect("a turn-helper test record bound must be nonzero"),
        )
        .expect("a turn-helper test Journal must reserve its record buffer");
        Certificate::mint(journal, logical_time)
            .run_started()
            .expect("a turn-helper fixture must commit RunStarted")
    }

    fn later_turn(event: u8, logical_time: Timestamp) -> (Certificate<Vec<u8>, TurnOpen>, u8) {
        let ClassifiedTurn::Continue(certificate) =
            start_turn(Timestamp::from_nanos(0)).classify(TurnOutcome::Continue)
        else {
            panic!("a later-turn fixture must classify its setup turn as Continue")
        };
        let Scripted {
            mut environment, ..
        } = scripted(
            Vec::new(),
            Ok(Timestamp::from_nanos(0)),
            &[(event, logical_time.as_nanos())],
            clean_report(),
        );
        certificate
            .no_commands(&reserved(1))
            .checkpoint::<_, ScriptedError>(&mut environment)
            .expect("a later-turn fixture checkpoint must be clear")
            .complete_continue()
            .expect("a later-turn fixture must commit TurnCompleted")
            .accept_event::<_, ScriptedError>(&mut environment)
            .expect("a later-turn fixture must accept its scripted Event")
    }

    fn assert_continue(result: &TurnResult) {
        assert!(
            matches!(result, Ok(ClassifiedTurn::Continue(_))),
            "a Continue handler answer must produce a Continue-classified turn"
        );
    }

    fn assert_stop(result: &TurnResult) {
        assert!(
            matches!(result, Ok(ClassifiedTurn::Stop(_))),
            "a Stop handler answer must produce a Stop-classified turn"
        );
    }

    fn run(
        turns: Vec<Turn>,
        start: Result<Timestamp, &'static str>,
        events: &[(u8, u64)],
        max_commands_per_turn: usize,
        bytes: &mut Vec<u8>,
    ) -> (TestExit, Calls, Drops) {
        let Scripted {
            app,
            environment,
            calls,
            drops,
        } = scripted(turns, start, events, clean_report());
        let engine = Engine::new(config(max_commands_per_turn, 256), app, environment, bytes)
            .expect("a run fixture Engine must construct with small bounds");
        (engine.run(), calls, drops)
    }

    mod engine_construction {
        use super::*;

        fn inputs() -> (ScriptedApplication, ScriptedEnvironment, Calls) {
            let Scripted {
                app,
                environment,
                calls,
                ..
            } = scripted(
                Vec::new(),
                Ok(Timestamp::from_nanos(0)),
                &[],
                clean_report(),
            );
            (app, environment, calls)
        }

        /// Invariant: failure to reserve the complete command batch is reported as
        /// a command-buffer construction error.
        /// Design Doc: the construction table, by name
        #[test]
        fn batch_reservation_failure_is_command_buffer() {
            let (app, env, _) = inputs();
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
            let (app, env, _) = inputs();
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
            let (app, env, calls) = inputs();

            let engine = Engine::new(config(1, 1), app, env, Vec::new())
                .expect("minimum nonzero bounds must construct an Engine");

            assert!(
                calls.borrow().is_empty(),
                "Engine construction must not invoke an Application or Environment method"
            );
            drop(engine);
            assert!(
                calls.borrow().is_empty(),
                "dropping an unstarted Engine must not invoke an Application or Environment method"
            );
        }

        /// Invariant: the minimum nonzero bounds construct an engine with an empty
        /// fully reserved command batch and a fresh journal.
        #[test]
        fn one_slot_bounds_construct_an_empty_unpoisoned_engine() {
            let (app, env, _) = inputs();

            let engine = Engine::new(config(1, 1), app, env, Vec::new())
                .expect("minimum nonzero bounds must construct an Engine");

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
            let (app, env, _) = inputs();
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
            for (max_commands_per_turn, max_record_bytes) in [(usize::MAX, 1), (1, usize::MAX)] {
                let (app, env, calls) = inputs();

                assert!(
                    Engine::new(
                        config(max_commands_per_turn, max_record_bytes),
                        app,
                        env,
                        Vec::new()
                    )
                    .is_err(),
                    "the scripted impossible bound must fail Engine construction"
                );
                assert!(
                    calls.borrow().is_empty(),
                    "failed Engine construction must not invoke an Application or Environment method"
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
            let Scripted { app, calls, .. } = one_turn(&[], Outcome::Continue);
            let mut state = Vec::new();
            let mut batch = reserved(1);
            let certificate = start_turn(Timestamp::from_nanos(41));

            let result = TestEngine::turn(&app, &mut state, None, &mut batch, certificate);

            assert_continue(&result);
            assert_eq!(
                calls.borrow().as_slice(),
                &[Call::OnStart {
                    index: 0,
                    logical_time: 41,
                }],
                "a certificate at index zero must invoke only on_start, exactly once"
            );
            assert_eq!(
                state,
                [1],
                "one start-handler invocation must perform exactly one scripted state mutation"
            );
        }

        /// Invariant: an accepted turn after the start invokes only the event
        /// handler, once, with the accepted Event and certificate position.
        /// Design Doc: the Phases table, by name
        #[test]
        fn a_later_index_calls_on_event_once() {
            let Scripted { app, calls, .. } = one_turn(&[], Outcome::Stop);
            let mut state = Vec::new();
            let mut batch = reserved(1);
            let (certificate, event) = later_turn(23, Timestamp::from_nanos(u64::MAX));

            let result = TestEngine::turn(&app, &mut state, Some(&event), &mut batch, certificate);

            assert_stop(&result);
            assert_eq!(
                calls.borrow().as_slice(),
                &[Call::OnEvent {
                    event: 23,
                    index: 1,
                    logical_time: u64::MAX,
                }],
                "a certificate after index zero must invoke only on_event with its accepted values"
            );
            assert_eq!(
                state,
                [1],
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
            for answer in [Outcome::Continue, Outcome::Stop, Outcome::Fatal(FATAL)] {
                let Scripted {
                    app, calls, drops, ..
                } = one_turn(&[0, 1], answer);
                let mut state = Vec::new();
                let mut batch = reserved(1);
                let certificate = start_turn(Timestamp::from_nanos(0));

                let result = TestEngine::turn(&app, &mut state, None, &mut batch, certificate);

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
                    state,
                    [1],
                    "command overflow must not roll back the handler's state mutation"
                );
                assert_eq!(
                    calls.borrow().as_slice(),
                    &[Call::OnStart {
                        index: 0,
                        logical_time: 0,
                    }],
                    "an overflowing turn must still have invoked exactly one handler"
                );
                assert_eq!(
                    drops.get(),
                    usize::from(matches!(answer, Outcome::Fatal(_))),
                    "an Application Fatal payload must be discarded when command overflow outranks it"
                );
            }
        }

        /// Invariant: command overflow after an event handler follows the same
        /// precedence and batch-discard rules as overflow during the start turn.
        #[test]
        fn later_index_overflow_outranks_a_fatal_outcome() {
            let Scripted {
                app, calls, drops, ..
            } = one_turn(&[0, 1], Outcome::Fatal(FATAL));
            let mut state = Vec::new();
            let mut batch = reserved(1);
            let (certificate, event) = later_turn(29, Timestamp::from_nanos(53));

            let result = TestEngine::turn(&app, &mut state, Some(&event), &mut batch, certificate);

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
                state,
                [1],
                "event-handler command overflow must not roll back its state mutation"
            );
            assert_eq!(
                calls.borrow().as_slice(),
                &[Call::OnEvent {
                    event: 29,
                    index: 1,
                    logical_time: 53,
                }],
                "an overflowing later turn must invoke on_event exactly once"
            );
            assert_eq!(
                drops.get(),
                1,
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
            let Scripted {
                app, calls, drops, ..
            } = one_turn(&[0], Outcome::Fatal(FATAL));
            let mut state = Vec::new();
            let mut batch = reserved(2);
            let certificate = start_turn(Timestamp::from_nanos(7));

            let result = TestEngine::turn(&app, &mut state, None, &mut batch, certificate);
            let Err(FatalCause::Application(error)) = result else {
                panic!("a non-overflowing handler Fatal must be an Application cause")
            };

            assert_eq!(
                state,
                [1],
                "an Application Fatal must preserve the handler's completed state mutation"
            );
            assert!(
                batch.is_empty(),
                "an Application Fatal must discard every command staged by that handler"
            );
            assert_eq!(
                error.label, FATAL,
                "an Application Fatal must carry the exact handler Error payload"
            );
            assert_eq!(
                drops.get(),
                0,
                "the Application Error payload must remain owned by the Fatal cause"
            );
            assert_eq!(
                calls.borrow().as_slice(),
                &[Call::OnStart {
                    index: 0,
                    logical_time: 7,
                }],
                "an Application Fatal turn must invoke its selected handler exactly once"
            );
            drop(error);
            assert_eq!(
                drops.get(),
                1,
                "dropping the returned Fatal cause must drop its Application Error payload"
            );
        }

        /// Invariant: an event handler's fatal result preserves its state mutation
        /// and Error payload while discarding that event turn's staged commands.
        #[test]
        fn later_index_state_mutation_and_fatal_payload_both_stand() {
            let Scripted {
                app, calls, drops, ..
            } = one_turn(&[0], Outcome::Fatal(FATAL));
            let mut state = Vec::new();
            let mut batch = reserved(2);
            let (certificate, event) = later_turn(31, Timestamp::from_nanos(59));

            let result = TestEngine::turn(&app, &mut state, Some(&event), &mut batch, certificate);
            let Err(FatalCause::Application(error)) = result else {
                panic!("a non-overflowing event-handler Fatal must be an Application cause")
            };

            assert_eq!(
                state,
                [1],
                "an event-handler Fatal must preserve its completed state mutation"
            );
            assert!(
                batch.is_empty(),
                "an event-handler Fatal must discard every command it staged"
            );
            assert_eq!(
                error.label, FATAL,
                "an event-handler Fatal must carry its exact Error payload"
            );
            assert_eq!(
                drops.get(),
                0,
                "the event handler's Error payload must remain owned by the Fatal cause"
            );
            assert_eq!(
                calls.borrow().as_slice(),
                &[Call::OnEvent {
                    event: 31,
                    index: 1,
                    logical_time: 59,
                }],
                "a non-overflowing later Fatal turn must invoke on_event exactly once"
            );
            drop(error);
            assert_eq!(
                drops.get(),
                1,
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
            let Scripted { app, .. } = one_turn(&[0], Outcome::Continue);
            let mut state = Vec::new();
            let mut batch = reserved(1);
            batch
                .try_push(99)
                .expect("the stale command must fit before the fresh turn begins");
            let certificate = start_turn(Timestamp::from_nanos(0));

            let result = TestEngine::turn(&app, &mut state, None, &mut batch, certificate);

            assert_continue(&result);
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
            let Scripted { app, calls, .. } = one_turn(&[], Outcome::Continue);
            let mut state = Vec::new();
            let mut batch = reserved(1);
            let certificate = start_turn(Timestamp::from_nanos(0));
            let event = 1;

            let panic = catch_unwind(AssertUnwindSafe(|| {
                let _ = TestEngine::turn(&app, &mut state, Some(&event), &mut batch, certificate);
            }));

            assert!(
                panic.is_err(),
                "a start-turn certificate paired with an Event must panic"
            );
            assert!(
                calls.borrow().is_empty(),
                "a mismatched start-turn Event must be rejected before a handler runs"
            );
            assert!(
                state.is_empty(),
                "rejecting a mismatched start-turn Event must leave handler state untouched"
            );
        }

        /// Invariant: a certificate after the start must be paired with its accepted
        /// Event, and missing input is rejected before either handler runs.
        #[test]
        fn later_turn_without_event_panics_before_handler() {
            let Scripted { app, calls, .. } = one_turn(&[], Outcome::Continue);
            let mut state = Vec::new();
            let mut batch = reserved(1);
            let (certificate, _) = later_turn(1, Timestamp::from_nanos(1));

            let panic = catch_unwind(AssertUnwindSafe(|| {
                let _ = TestEngine::turn(&app, &mut state, None, &mut batch, certificate);
            }));

            assert!(
                panic.is_err(),
                "a later-turn certificate without its accepted Event must panic"
            );
            assert!(
                calls.borrow().is_empty(),
                "a missing later-turn Event must be rejected before a handler runs"
            );
            assert!(
                state.is_empty(),
                "rejecting a missing later-turn Event must leave handler state untouched"
            );
        }
    }

    mod fatal_finalization {
        use super::*;

        fn environment(
            quiescence: Quiescence,
            error: Option<&'static str>,
        ) -> (ScriptedEnvironment, Calls, Drops) {
            let Scripted {
                environment,
                calls,
                drops,
                ..
            } = scripted(
                Vec::new(),
                Ok(Timestamp::from_nanos(0)),
                &[],
                ShutdownReport { quiescence, error },
            );
            (environment, calls, drops)
        }

        /// Invariant: fatal finalization shuts down a started, unconsumed
        /// Environment exactly once and returns the report's quiescence.
        /// Design Doc: RUN-FINALIZE
        #[test]
        fn a_started_environment_is_shutdown_exactly_once() {
            let (environment, calls, _) = environment(Quiescence::Incomplete, None);

            let exit = TestEngine::finalize(
                vec![17],
                FatalCause::Core(CoreError::IndexExhausted),
                Finalization::Unconsumed(environment),
            );

            assert_eq!(
                calls.borrow().as_slice(),
                &[Call::Shutdown],
                "fatal finalization must shut down an unconsumed Environment exactly once"
            );
            let EngineExit::Fatal {
                state,
                cause: FatalCause::Core(CoreError::IndexExhausted),
                quiescence,
            } = exit
            else {
                panic!("fatal finalization must preserve the fixed Core cause")
            };
            assert_eq!(
                state,
                [17],
                "fatal finalization must preserve the State it receives"
            );
            assert_eq!(
                quiescence,
                Quiescence::Incomplete,
                "fatal finalization must use the shutdown report's quiescence"
            );
        }

        /// Invariant: an Error reported by finalizing shutdown is discarded and
        /// never replaces the failure that triggered finalization.
        /// Design Doc: A4, RUN-FINALIZE
        #[test]
        fn the_shutdown_error_never_replaces_the_fixed_cause() {
            let (environment, calls, drops) =
                environment(Quiescence::Quiesced, Some("later shutdown error"));

            let exit = TestEngine::finalize(
                vec![23],
                FatalCause::Application(ScriptedError {
                    label: "fixed application cause",
                    drops: Rc::clone(&drops),
                }),
                Finalization::Unconsumed(environment),
            );

            assert_eq!(
                calls.borrow().as_slice(),
                &[Call::Shutdown],
                "discarding a shutdown Error must not repeat finalizing shutdown"
            );
            let EngineExit::Fatal {
                state,
                cause: FatalCause::Application(cause),
                quiescence,
            } = exit
            else {
                panic!("a shutdown Error must not replace the fixed Application cause")
            };
            assert_eq!(
                state,
                [23],
                "discarding a shutdown Error must not change the returned State"
            );
            assert_eq!(
                quiescence,
                Quiescence::Quiesced,
                "discarding a shutdown Error must retain the report's quiescence"
            );
            assert_eq!(
                cause.label, "fixed application cause",
                "fatal finalization must return the exact first-observed Error payload"
            );
            assert_eq!(
                drops.get(),
                1,
                "the shutdown report's later Error must be discarded during fatal finalization while the fixed payload remains owned by the Fatal exit"
            );
            drop(cause);
            assert_eq!(
                drops.get(),
                2,
                "dropping the Fatal cause must drop its preserved Error payload"
            );
        }

        /// Invariant: a failed Environment start is already quiesced, so fatal
        /// finalization returns without attempting shutdown.
        /// Design Doc: ENV-START, RUN-FINALIZE
        #[test]
        fn a_start_error_skips_shutdown_and_is_quiesced() {
            let drops: Drops = Rc::new(Cell::new(0));

            let exit = TestEngine::finalize(
                vec![29],
                FatalCause::Environment(EnvironmentFatal {
                    error: ScriptedError {
                        label: "start error",
                        drops: Rc::clone(&drops),
                    },
                    operation: EnvironmentOperation::Start,
                }),
                Finalization::StartFailed,
            );

            let EngineExit::Fatal {
                state,
                cause:
                    FatalCause::Environment(EnvironmentFatal {
                        error,
                        operation: EnvironmentOperation::Start,
                    }),
                quiescence,
            } = exit
            else {
                panic!("a start Error must finalize as an Environment Start cause")
            };
            assert_eq!(
                state,
                [29],
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
            assert_eq!(
                drops.get(),
                0,
                "the start Error must remain owned by the Fatal exit"
            );
        }

        /// Invariant: once shutdown has consumed the Environment, fatal
        /// finalization uses its retained quiescence without another shutdown.
        /// Design Doc: RUN-FINALIZE
        #[test]
        fn a_consumed_environment_uses_the_retained_quiescence() {
            let exit = TestEngine::finalize(
                vec![31],
                FatalCause::Core(CoreError::ShutdownIncomplete),
                Finalization::Retained(Quiescence::Incomplete),
            );

            let EngineExit::Fatal {
                state,
                cause: FatalCause::Core(CoreError::ShutdownIncomplete),
                quiescence,
            } = exit
            else {
                panic!("a consumed Environment must preserve its fixed Fatal cause")
            };
            assert_eq!(
                state,
                [31],
                "finalization after consuming the Environment must preserve State"
            );
            assert_eq!(
                quiescence,
                Quiescence::Incomplete,
                "finalization must preserve the quiescence retained before the Environment was consumed"
            );
        }

        /// Invariant: finalization returns retained quiesced status unchanged after
        /// shutdown has already consumed the Environment.
        #[test]
        fn a_consumed_environment_preserves_retained_quiesced() {
            let exit = TestEngine::finalize(
                vec![37],
                FatalCause::Core(CoreError::IndexExhausted),
                Finalization::Retained(Quiescence::Quiesced),
            );

            let EngineExit::Fatal {
                state,
                cause: FatalCause::Core(CoreError::IndexExhausted),
                quiescence,
            } = exit
            else {
                panic!("retained Quiesced finalization must preserve its fixed Fatal cause")
            };
            assert_eq!(
                state,
                [37],
                "retained Quiesced finalization must preserve State"
            );
            assert_eq!(
                quiescence,
                Quiescence::Quiesced,
                "retained Quiesced status must pass through finalization unchanged"
            );
        }
    }

    mod run_startup {
        use super::*;

        /// Invariant: initial State is created exactly once before the first
        /// Environment operation, even when that operation fails.
        /// Design Doc: the startup table, by name
        #[test]
        fn state_is_created_before_any_fallible_step() {
            let mut bytes = Vec::new();

            let (_exit, calls, _) = run(Vec::new(), Err("start failed"), &[], 1, &mut bytes);

            assert_eq!(
                calls.borrow().as_slice(),
                &[Call::InitialState, Call::Start],
                "initial State creation must precede the first fallible Environment operation"
            );
        }

        /// Invariant: a failed Environment start returns a fatal, quiesced run
        /// without invoking shutdown.
        /// Design Doc: ENV-START
        #[test]
        fn a_start_error_exits_fatal_quiesced_without_shutdown() {
            let mut bytes = Vec::new();

            let (exit, calls, _) = run(Vec::new(), Err("start failed"), &[], 1, &mut bytes);

            let EngineExit::Fatal {
                state,
                cause:
                    FatalCause::Environment(EnvironmentFatal {
                        error,
                        operation: EnvironmentOperation::Start,
                    }),
                quiescence,
            } = exit
            else {
                panic!("a failed Environment start must be the fatal Start cause")
            };
            assert!(
                state.is_empty(),
                "a start failure must carry the State created before startup"
            );
            assert_eq!(
                error.label, "start failed",
                "a start failure must preserve the exact Environment Error"
            );
            assert_eq!(
                quiescence,
                Quiescence::Quiesced,
                "a failed Environment start must exit already quiesced"
            );
            assert!(
                !calls.borrow().contains(&Call::Shutdown),
                "a failed Environment start must not be followed by shutdown"
            );
        }

        /// Invariant: when Environment startup fails, no handler runs and no
        /// Journal record is written before the fatal exit.
        #[test]
        fn a_start_error_invokes_no_handler_and_writes_no_record() {
            let mut bytes = Vec::new();

            let (_exit, calls, _) = run(Vec::new(), Err("start failed"), &[], 1, &mut bytes);

            assert!(
                !calls
                    .borrow()
                    .iter()
                    .any(|call| matches!(call, Call::OnStart { .. } | Call::OnEvent { .. })),
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
                let mut bytes = Vec::new();

                let (exit, calls, _) = run(
                    vec![turn(1, &[], Outcome::Stop)],
                    Ok(Timestamp::from_nanos(nanos)),
                    &[],
                    1,
                    &mut bytes,
                );

                assert!(
                    matches!(exit, EngineExit::Stopped { .. }),
                    "a boundary-valued start time must complete a clean Stop run"
                );
                assert!(
                    calls.borrow().contains(&Call::OnStart {
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

        fn stop_at_start(nanos: u64, bytes: &mut Vec<u8>) -> (TestExit, Calls) {
            let (exit, calls, _) = run(
                vec![turn(1, &[], Outcome::Stop)],
                Ok(Timestamp::from_nanos(nanos)),
                &[],
                1,
                bytes,
            );
            (exit, calls)
        }

        /// Invariant: stopping during the start turn writes exactly `RunStarted`,
        /// `StopRequested`, and `TurnCompleted` Stop in that order.
        /// Design Doc: RUN-GRAMMAR, RUN-RECORDS
        #[test]
        fn stop_at_start_produces_the_three_record_journal() {
            let mut bytes = Vec::new();

            let (exit, _) = stop_at_start(37, &mut bytes);

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
        /// Design Doc: `EngineExit`, by name
        #[test]
        fn stopped_carries_the_final_state() {
            let mut bytes = Vec::new();

            let (exit, _) = stop_at_start(0, &mut bytes);

            let EngineExit::Stopped { state } = exit else {
                panic!("a clean Stop-at-start run must not return Fatal")
            };
            assert_eq!(
                state,
                [1],
                "Stopped must carry the State after the start handler's mutation"
            );
        }

        /// Invariant: a Stop-at-start run invokes Environment operations serially
        /// as start, one checkpoint, and consuming shutdown.
        /// Design Doc: ENV-SERIAL
        #[test]
        fn the_call_sequence_matches_env_serial() {
            let mut bytes = Vec::new();

            let (exit, calls) = stop_at_start(11, &mut bytes);

            assert!(
                matches!(exit, EngineExit::Stopped { .. }),
                "the serial call trace fixture must finish as Stopped"
            );
            assert_eq!(
                calls.borrow().as_slice(),
                &[
                    Call::InitialState,
                    Call::Start,
                    Call::OnStart {
                        index: 0,
                        logical_time: 11,
                    },
                    Call::TakeError,
                    Call::Shutdown,
                ],
                "a Stop-at-start run must call start first, checkpoint once, and shutdown last"
            );
        }

        /// Invariant: stopping during the start turn invokes the start handler once
        /// and never invokes the Event handler.
        #[test]
        fn stop_at_start_invokes_only_the_start_handler_once() {
            let mut bytes = Vec::new();

            let (exit, calls) = stop_at_start(13, &mut bytes);

            assert!(
                matches!(exit, EngineExit::Stopped { .. }),
                "the handler-selection fixture must finish as Stopped"
            );
            let calls = calls.borrow();
            assert_eq!(
                calls
                    .iter()
                    .filter(|call| matches!(call, Call::OnStart { .. }))
                    .count(),
                1,
                "a Stop-at-start run must invoke on_start exactly once"
            );
            assert_eq!(
                calls
                    .iter()
                    .filter(|call| matches!(call, Call::OnEvent { .. }))
                    .count(),
                0,
                "a Stop-at-start run must never invoke on_event"
            );
        }
    }

    mod run_turn_loop {
        use super::*;

        fn run_loop(
            turns: Vec<Turn>,
            events: &[(u8, u64)],
            max_commands_per_turn: usize,
            bytes: &mut Vec<u8>,
        ) -> (TestExit, Calls, Drops) {
            run(
                turns,
                Ok(Timestamp::from_nanos(10)),
                events,
                max_commands_per_turn,
                bytes,
            )
        }

        /// Invariant: each continued turn finishes its command handoff and
        /// checkpoint before the next Event is requested, and accepted Events reach
        /// handlers once in source order.
        /// Design Doc: A2
        #[test]
        fn continue_turns_accept_events_in_sequence() {
            let turns = vec![
                turn(1, &[10], Outcome::Continue),
                turn(2, &[20], Outcome::Continue),
                turn(3, &[30], Outcome::Stop),
            ];
            let mut bytes = Vec::new();

            let (exit, calls, _) = run_loop(turns, &[(7, 11), (8, 12)], 1, &mut bytes);

            let EngineExit::Stopped { state } = exit else {
                panic!("a serial Continue, Continue, Stop script must finish cleanly")
            };
            assert_eq!(
                state,
                [1, 2, 3],
                "a serial three-turn run must retain one mutation from each handler"
            );
            assert_eq!(
                calls.borrow().as_slice(),
                &[
                    Call::InitialState,
                    Call::Start,
                    Call::OnStart {
                        index: 0,
                        logical_time: 10,
                    },
                    Call::Dispatch(10),
                    Call::TakeError,
                    Call::NextEvent,
                    Call::OnEvent {
                        event: 7,
                        index: 1,
                        logical_time: 11,
                    },
                    Call::Dispatch(20),
                    Call::TakeError,
                    Call::NextEvent,
                    Call::OnEvent {
                        event: 8,
                        index: 2,
                        logical_time: 12,
                    },
                    Call::Dispatch(30),
                    Call::TakeError,
                    Call::Shutdown,
                ],
                "each turn must complete before the next Event is requested, with Events handled in order"
            );
        }

        /// Invariant: exceeding the command bound fixes command overflow as the
        /// run's cause, discards its staged batch, and outranks every handler answer.
        /// Design Doc: the `TurnOpen` phase row, by name
        #[test]
        fn overflow_beats_the_returned_outcome_and_discards_the_batch() {
            for answer in [Outcome::Continue, Outcome::Stop, Outcome::Fatal(FATAL)] {
                let mut bytes = Vec::new();
                let (exit, calls, drops) =
                    run_loop(vec![turn(1, &[10, 11], answer)], &[], 1, &mut bytes);

                let EngineExit::Fatal {
                    state,
                    cause: FatalCause::Core(CoreError::CommandBoundExceeded),
                    quiescence,
                } = exit
                else {
                    panic!("command overflow must outrank every returned Outcome")
                };
                assert_eq!(
                    state,
                    [1],
                    "command overflow must retain the failing handler's State mutation"
                );
                assert_eq!(
                    quiescence,
                    Quiescence::Quiesced,
                    "command overflow must carry finalizing shutdown's quiescence"
                );
                assert_eq!(
                    calls.borrow().as_slice(),
                    &[
                        Call::InitialState,
                        Call::Start,
                        Call::OnStart {
                            index: 0,
                            logical_time: 10,
                        },
                        Call::Shutdown,
                    ],
                    "an overflowing batch must be discarded before dispatch, checkpoint, or Event acquisition"
                );
                assert_eq!(
                    drops.get(),
                    usize::from(matches!(answer, Outcome::Fatal(_))),
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
                turn(1, &[10], Outcome::Continue),
                turn(2, &[20], Outcome::Fatal(FATAL)),
            ];
            let mut bytes = Vec::new();

            let (exit, calls, drops) = run_loop(turns, &[(7, 11)], 1, &mut bytes);
            let EngineExit::Fatal {
                state,
                cause: FatalCause::Application(error),
                quiescence,
            } = exit
            else {
                panic!("a non-overflowing handler Fatal must remain the run's Application cause")
            };

            assert_eq!(
                state,
                [1, 2],
                "an event-handler failure must retain mutations from both completed handler calls"
            );
            assert_eq!(
                quiescence,
                Quiescence::Quiesced,
                "a handler failure must carry finalizing shutdown's quiescence"
            );
            assert_eq!(
                error.label, FATAL,
                "a handler Fatal must carry the exact Error payload returned by the handler"
            );
            assert_eq!(
                drops.get(),
                0,
                "the handler Error must remain owned by the Fatal exit"
            );
            assert_eq!(
                calls.borrow().as_slice(),
                &[
                    Call::InitialState,
                    Call::Start,
                    Call::OnStart {
                        index: 0,
                        logical_time: 10,
                    },
                    Call::Dispatch(10),
                    Call::TakeError,
                    Call::NextEvent,
                    Call::OnEvent {
                        event: 7,
                        index: 1,
                        logical_time: 11,
                    },
                    Call::Shutdown,
                ],
                "a fatal event turn must not dispatch its batch, checkpoint, or request another Event"
            );
            drop(error);
            assert_eq!(
                drops.get(),
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
                    vec![turn(1, &[], Outcome::Fatal(FATAL))],
                    Vec::new(),
                    ExpectedCause::Application,
                    vec![1],
                ),
                (
                    vec![turn(1, &[10, 11], Outcome::Continue)],
                    Vec::new(),
                    ExpectedCause::Overflow,
                    vec![1],
                ),
                (
                    vec![
                        turn(1, &[], Outcome::Continue),
                        turn(2, &[], Outcome::Fatal(FATAL)),
                    ],
                    vec![(7, 11)],
                    ExpectedCause::Application,
                    vec![1, 2],
                ),
                (
                    vec![
                        turn(1, &[], Outcome::Continue),
                        turn(2, &[20, 21], Outcome::Stop),
                    ],
                    vec![(7, 11)],
                    ExpectedCause::Overflow,
                    vec![1, 2],
                ),
            ];

            for (turns, events, expected_cause, expected_state) in scenarios {
                let mut bytes = Vec::new();
                let (exit, _, _) = run_loop(turns, &events, 1, &mut bytes);

                let EngineExit::Fatal { state, cause, .. } = exit else {
                    panic!("every State-retention scenario must end Fatal")
                };
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
        }

        /// Invariant: when an accepted Event turn exceeds its command bound, the
        /// journal locates the turn but records none of its staged command intent.
        /// Design Doc: the intent-vacuum derivation, by name
        #[test]
        fn an_over_emitting_turn_leaves_no_command_record() {
            let turns = vec![
                turn(1, &[], Outcome::Continue),
                turn(2, &[41, 42], Outcome::Stop),
            ];
            let mut bytes = Vec::new();

            let (exit, calls, _) = run_loop(turns, &[(9, 11)], 1, &mut bytes);

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
                    .any(|call| matches!(call, Call::Dispatch(_))),
                "commands staged by an overflowing turn must never be dispatched"
            );
        }

        /// Invariant: filling the command batch exactly on consecutive turns
        /// dispatches every command once in per-turn order without false overflow.
        #[test]
        fn exact_capacity_batches_dispatch_once_in_order_across_reused_turns() {
            let turns = vec![
                turn(1, &[10, 11], Outcome::Continue),
                turn(2, &[20, 21], Outcome::Stop),
            ];
            let mut bytes = Vec::new();

            let (exit, calls, _) = run_loop(turns, &[(7, 11)], 2, &mut bytes);

            assert!(
                matches!(exit, EngineExit::Stopped { state } if state == vec![1, 2]),
                "exact-capacity batches on reused turns must complete without overflow"
            );
            let dispatched: Vec<_> = calls
                .borrow()
                .iter()
                .filter_map(|call| match call {
                    Call::Dispatch(command) => Some(*command),
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
                vec![turn(1, &[10], Outcome::Fatal(FATAL))],
                &[],
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
                    Call::InitialState,
                    Call::Start,
                    Call::OnStart {
                        index: 0,
                        logical_time: 10,
                    },
                    Call::Shutdown,
                ],
                "a start-handler Fatal must skip effects and Event acquisition before one shutdown"
            );
        }

        /// Invariant: a Continue turn with no commands takes the empty effects path,
        /// accepts one Event, and a Stop answer prevents another Event request.
        #[test]
        fn an_empty_continue_turn_accepts_exactly_one_event_before_stop() {
            let turns = vec![turn(1, &[], Outcome::Continue), turn(2, &[], Outcome::Stop)];
            let mut bytes = Vec::new();

            let (exit, calls, _) = run_loop(turns, &[(7, 11)], 1, &mut bytes);

            assert!(
                matches!(exit, EngineExit::Stopped { state } if state == vec![1, 2]),
                "an empty Continue turn followed by Stop must finish with both State mutations"
            );
            assert_eq!(
                calls.borrow().as_slice(),
                &[
                    Call::InitialState,
                    Call::Start,
                    Call::OnStart {
                        index: 0,
                        logical_time: 10,
                    },
                    Call::TakeError,
                    Call::NextEvent,
                    Call::OnEvent {
                        event: 7,
                        index: 1,
                        logical_time: 11,
                    },
                    Call::TakeError,
                    Call::Shutdown,
                ],
                "an empty Continue turn must request one Event, and Stop must end the back edge"
            );
        }
    }
}
