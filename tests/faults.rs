#[allow(dead_code, unused_imports)]
mod support;

#[cfg(test)]
mod tests {
    use super::support::{
        AppCall, EnvCall, RecordingApp, ScriptedAnswer, ScriptedEnv, ScriptedSink, ScriptedTurn,
        SinkStep, TraceQuiescence,
    };
    use kavod::{
        CoreError, Engine, EngineConfig, EngineExit, EnvironmentFatal, EnvironmentOperation,
        FatalCause, JournalError, JournalFatal, Quiescence, RecordKind, ShutdownReport,
        SinkOperation, Timestamp, TurnOutcome,
    };
    use std::io;
    use std::num::NonZeroUsize;

    type TestExit = EngineExit<Vec<u8>, &'static str, &'static str>;

    const COMMANDS: [u8; 2] = [10, 11];
    const RECORDS: [&[u8]; 7] = [
        b"{\"record_kind\":\"RunStarted\",\"index\":0,\"schema_version\":1,\"logical_time\":100}\n",
        b"{\"record_kind\":\"CommandsPrepared\",\"index\":0,\"commands\":[10,11]}\n",
        b"{\"record_kind\":\"CommandsDispatched\",\"index\":0}\n",
        b"{\"record_kind\":\"TurnCompleted\",\"index\":0,\"outcome\":\"Continue\"}\n",
        b"{\"record_kind\":\"EventAccepted\",\"index\":1,\"logical_time\":105,\"event\":7}\n",
        b"{\"record_kind\":\"StopRequested\",\"index\":1}\n",
        b"{\"record_kind\":\"TurnCompleted\",\"index\":1,\"outcome\":\"Stop\"}\n",
    ];

    struct FaultCase {
        name: &'static str,
        failed_record: usize,
        record_kind: RecordKind,
        outcome: Option<TurnOutcome>,
        state: &'static [u8],
        handoff_count: usize,
    }

    fn fault_cases() -> [FaultCase; 6] {
        [
            FaultCase {
                name: "RunStarted",
                failed_record: 0,
                record_kind: RecordKind::RunStarted,
                outcome: None,
                state: &[0],
                handoff_count: 0,
            },
            FaultCase {
                name: "CommandsPrepared",
                failed_record: 1,
                record_kind: RecordKind::CommandsPrepared,
                outcome: None,
                state: &[0, 1],
                handoff_count: 0,
            },
            FaultCase {
                name: "CommandsDispatched",
                failed_record: 2,
                record_kind: RecordKind::CommandsDispatched,
                outcome: None,
                state: &[0, 1],
                handoff_count: 2,
            },
            FaultCase {
                name: "TurnCompleted",
                failed_record: 3,
                record_kind: RecordKind::TurnCompleted,
                outcome: Some(TurnOutcome::Continue),
                state: &[0, 1],
                handoff_count: 2,
            },
            FaultCase {
                name: "EventAccepted",
                failed_record: 4,
                record_kind: RecordKind::EventAccepted,
                outcome: None,
                state: &[0, 1],
                handoff_count: 2,
            },
            FaultCase {
                name: "StopRequested",
                failed_record: 5,
                record_kind: RecordKind::StopRequested,
                outcome: None,
                state: &[0, 1, 2],
                handoff_count: 2,
            },
        ]
    }

    #[derive(Clone, Copy)]
    enum SinkFailure {
        Write,
        Flush,
    }

    #[derive(Clone, Copy)]
    enum EnvironmentFaultPoint {
        Dispatch { position: usize },
        Checkpoint,
        NextEvent,
    }

    impl EnvironmentFaultPoint {
        fn name(self) -> &'static str {
            match self {
                Self::Dispatch { position: 0 } => "first dispatch",
                Self::Dispatch { position: 1 } => "second dispatch",
                Self::Dispatch { .. } => "dispatch",
                Self::Checkpoint => "checkpoint",
                Self::NextEvent => "next event",
            }
        }

        fn error(self) -> &'static str {
            match self {
                Self::Dispatch { position: 0 } => "first dispatch failed",
                Self::Dispatch { position: 1 } => "second dispatch failed",
                Self::Dispatch { .. } => "dispatch failed",
                Self::Checkpoint => "checkpoint failed",
                Self::NextEvent => "next event failed",
            }
        }

        fn operation(self) -> EnvironmentOperation {
            match self {
                Self::Dispatch { position } => EnvironmentOperation::Dispatch { position },
                Self::Checkpoint => EnvironmentOperation::Checkpoint,
                Self::NextEvent => EnvironmentOperation::NextEvent,
            }
        }

        fn committed_record_count(self) -> usize {
            match self {
                Self::Dispatch { .. } => 2,
                Self::Checkpoint => 3,
                Self::NextEvent => 4,
            }
        }

        fn handoff_count(self) -> usize {
            match self {
                Self::Dispatch { position } => position,
                Self::Checkpoint | Self::NextEvent => COMMANDS.len(),
            }
        }
    }

    fn environment_fault_points() -> [EnvironmentFaultPoint; 4] {
        [
            EnvironmentFaultPoint::Dispatch { position: 0 },
            EnvironmentFaultPoint::Dispatch { position: 1 },
            EnvironmentFaultPoint::Checkpoint,
            EnvironmentFaultPoint::NextEvent,
        ]
    }

    impl SinkFailure {
        fn operation(self) -> SinkOperation {
            match self {
                Self::Write => SinkOperation::Write,
                Self::Flush => SinkOperation::Flush,
            }
        }
    }

    struct RunObservation {
        exit: TestExit,
        committed_bytes: Vec<u8>,
        uncertain_suffix: Vec<u8>,
        sink_call_count: usize,
        app_calls: Vec<AppCall<u8>>,
        env_calls: Vec<EnvCall<u8, u8>>,
        handoffs: Vec<u8>,
        shutdown_count: usize,
    }

    fn config() -> EngineConfig {
        EngineConfig {
            max_commands_per_turn: NonZeroUsize::new(COMMANDS.len())
                .expect("fault fixture invariant: command capacity must be nonzero"),
            max_record_bytes: NonZeroUsize::new(256)
                .expect("fault fixture invariant: record capacity must be nonzero"),
        }
    }

    fn clean_shutdown() -> ShutdownReport<&'static str> {
        ShutdownReport {
            quiescence: Quiescence::Quiesced,
            error: None,
        }
    }

    fn sink_error(message: &'static str) -> io::Error {
        io::Error::new(io::ErrorKind::BrokenPipe, message)
    }

    fn sink_steps(failed_record: usize, failure: SinkFailure) -> Vec<SinkStep> {
        assert!(
            failed_record < RECORDS.len(),
            "fault fixture invariant: the failed record position must exist"
        );
        let mut steps = Vec::new();
        for (position, record) in RECORDS.iter().enumerate() {
            if position == failed_record {
                match failure {
                    SinkFailure::Write => steps.push(SinkStep::Write(Err(sink_error(
                        "scripted record write failure",
                    )))),
                    SinkFailure::Flush => {
                        steps.push(SinkStep::Write(Ok(record.len())));
                        steps.push(SinkStep::Flush(Err(sink_error(
                            "scripted record flush failure",
                        ))));
                    }
                }
                break;
            }
            steps.push(SinkStep::Write(Ok(record.len())));
            steps.push(SinkStep::Flush(Ok(())));
        }
        steps
    }

    fn successful_sink_steps(record_count: usize) -> Vec<SinkStep> {
        assert!(
            record_count <= RECORDS.len(),
            "fault fixture invariant: every successful record must exist"
        );
        let mut steps = Vec::new();
        for record in &RECORDS[..record_count] {
            steps.push(SinkStep::Write(Ok(record.len())));
            steps.push(SinkStep::Flush(Ok(())));
        }
        steps
    }

    fn run_with_sink_steps(steps: impl IntoIterator<Item = SinkStep>) -> RunObservation {
        let (app, app_trace) = RecordingApp::<u8, u8, &'static str>::new(
            vec![0],
            [
                ScriptedTurn::new(1, COMMANDS.to_vec(), ScriptedAnswer::Continue),
                ScriptedTurn::new(2, Vec::new(), ScriptedAnswer::Stop),
            ],
        );
        let (environment, env_trace) = ScriptedEnv::<u8, u8, &'static str>::new(
            Ok(Timestamp::from_nanos(100)),
            [Ok((7, Timestamp::from_nanos(105)))],
            [Ok(()), Ok(())],
            [None, None],
            clean_shutdown(),
        );
        let (sink, sink_trace) = ScriptedSink::new(steps);
        let engine = Engine::new(config(), app, environment, sink)
            .unwrap_or_else(|_| panic!("fault fixture invariant: the Engine must construct"));
        let exit = engine.run();
        let sink_trace = sink_trace.borrow();
        let env_trace = env_trace.borrow();

        RunObservation {
            exit,
            committed_bytes: sink_trace.committed_bytes().to_vec(),
            uncertain_suffix: sink_trace.uncertain_suffix().to_vec(),
            sink_call_count: sink_trace.calls.len(),
            app_calls: app_trace.borrow().calls.clone(),
            env_calls: env_trace.calls.clone(),
            handoffs: env_trace.handoffs.clone(),
            shutdown_count: env_trace.shutdown_count,
        }
    }

    fn run_journal_fault(failed_record: usize, failure: SinkFailure) -> RunObservation {
        run_with_sink_steps(sink_steps(failed_record, failure))
    }

    fn run_startup_fault() -> RunObservation {
        let (app, app_trace) = RecordingApp::<u8, u8, &'static str>::new(vec![0], []);
        let (environment, env_trace) = ScriptedEnv::<u8, u8, &'static str>::new(
            Err("start failed"),
            [],
            [],
            [],
            clean_shutdown(),
        );
        let (sink, sink_trace) = ScriptedSink::new([]);
        let engine = Engine::new(config(), app, environment, sink).unwrap_or_else(|_| {
            panic!("startup fault fixture invariant: the Engine must construct")
        });
        let exit = engine.run();
        let sink_trace = sink_trace.borrow();
        let env_trace = env_trace.borrow();

        RunObservation {
            exit,
            committed_bytes: sink_trace.committed_bytes().to_vec(),
            uncertain_suffix: sink_trace.uncertain_suffix().to_vec(),
            sink_call_count: sink_trace.calls.len(),
            app_calls: app_trace.borrow().calls.clone(),
            env_calls: env_trace.calls.clone(),
            handoffs: env_trace.handoffs.clone(),
            shutdown_count: env_trace.shutdown_count,
        }
    }

    fn run_scripted_scenario(
        turns: Vec<ScriptedTurn<u8, &'static str>>,
        next_events: Vec<Result<(u8, Timestamp), &'static str>>,
        dispatches: Vec<Result<(), &'static str>>,
        checkpoints: Vec<Option<&'static str>>,
        shutdown: ShutdownReport<&'static str>,
        record_count: usize,
    ) -> RunObservation {
        let (app, app_trace) = RecordingApp::<u8, u8, &'static str>::new(vec![0], turns);
        let (environment, env_trace) = ScriptedEnv::<u8, u8, &'static str>::new(
            Ok(Timestamp::from_nanos(100)),
            next_events,
            dispatches,
            checkpoints,
            shutdown,
        );
        let (sink, sink_trace) = ScriptedSink::new(successful_sink_steps(record_count));
        let engine = Engine::new(config(), app, environment, sink).unwrap_or_else(|_| {
            panic!("scripted fault fixture invariant: the Engine must construct")
        });
        let exit = engine.run();
        let sink_trace = sink_trace.borrow();
        let env_trace = env_trace.borrow();

        RunObservation {
            exit,
            committed_bytes: sink_trace.committed_bytes().to_vec(),
            uncertain_suffix: sink_trace.uncertain_suffix().to_vec(),
            sink_call_count: sink_trace.calls.len(),
            app_calls: app_trace.borrow().calls.clone(),
            env_calls: env_trace.calls.clone(),
            handoffs: env_trace.handoffs.clone(),
            shutdown_count: env_trace.shutdown_count,
        }
    }

    fn run_environment_fault(
        point: EnvironmentFaultPoint,
        shutdown: ShutdownReport<&'static str>,
    ) -> RunObservation {
        type EventResult = Result<(u8, Timestamp), &'static str>;
        type DispatchResult = Result<(), &'static str>;

        let (next_events, dispatches, checkpoints): (
            Vec<EventResult>,
            Vec<DispatchResult>,
            Vec<Option<&'static str>>,
        ) = match point {
            EnvironmentFaultPoint::Dispatch { position } => {
                assert!(
                    position < COMMANDS.len(),
                    "fault fixture invariant: the failed dispatch position must exist"
                );
                let dispatches = (0..=position)
                    .map(|current| {
                        if current == position {
                            Err(point.error())
                        } else {
                            Ok(())
                        }
                    })
                    .collect();
                (Vec::new(), dispatches, Vec::new())
            }
            EnvironmentFaultPoint::Checkpoint => {
                (Vec::new(), vec![Ok(()), Ok(())], vec![Some(point.error())])
            }
            EnvironmentFaultPoint::NextEvent => {
                (vec![Err(point.error())], vec![Ok(()), Ok(())], vec![None])
            }
        };

        run_scripted_scenario(
            vec![ScriptedTurn::new(
                1,
                COMMANDS.to_vec(),
                ScriptedAnswer::Continue,
            )],
            next_events,
            dispatches,
            checkpoints,
            shutdown,
            point.committed_record_count(),
        )
    }

    fn run_shutdown_fault(shutdown: ShutdownReport<&'static str>) -> RunObservation {
        run_scripted_scenario(
            vec![
                ScriptedTurn::new(1, COMMANDS.to_vec(), ScriptedAnswer::Continue),
                ScriptedTurn::new(2, Vec::new(), ScriptedAnswer::Stop),
            ],
            vec![Ok((7, Timestamp::from_nanos(105)))],
            vec![Ok(()), Ok(())],
            vec![None, None],
            shutdown,
            6,
        )
    }

    fn run_time_regression() -> RunObservation {
        run_scripted_scenario(
            vec![ScriptedTurn::new(
                1,
                COMMANDS.to_vec(),
                ScriptedAnswer::Continue,
            )],
            vec![Ok((7, Timestamp::from_nanos(99)))],
            vec![Ok(()), Ok(())],
            vec![None],
            clean_shutdown(),
            4,
        )
    }

    fn run_handler_fault(
        on_event: bool,
        failing_turn: ScriptedTurn<u8, &'static str>,
    ) -> RunObservation {
        if on_event {
            run_scripted_scenario(
                vec![
                    ScriptedTurn::new(1, COMMANDS.to_vec(), ScriptedAnswer::Continue),
                    failing_turn,
                ],
                vec![Ok((7, Timestamp::from_nanos(105)))],
                vec![Ok(()), Ok(())],
                vec![None],
                clean_shutdown(),
                5,
            )
        } else {
            run_scripted_scenario(
                vec![failing_turn],
                Vec::new(),
                Vec::new(),
                Vec::new(),
                clean_shutdown(),
                1,
            )
        }
    }

    fn run_application_fatal(on_event: bool) -> RunObservation {
        run_handler_fault(
            on_event,
            ScriptedTurn::new(
                if on_event { 2 } else { 1 },
                vec![20],
                ScriptedAnswer::Fatal("application failed"),
            ),
        )
    }

    fn run_command_overflow(on_event: bool) -> RunObservation {
        run_handler_fault(
            on_event,
            ScriptedTurn::new(
                if on_event { 2 } else { 1 },
                vec![20, 21, 22],
                ScriptedAnswer::Continue,
            ),
        )
    }

    fn expect_journal_fatal(exit: TestExit) -> (Vec<u8>, JournalFatal, Quiescence) {
        match exit {
            EngineExit::Fatal {
                state,
                cause: FatalCause::Journal(fatal),
                quiescence,
            } => (state, fatal, quiescence),
            _ => {
                panic!("journal fault invariant: a sink failure must produce a Journal fatal exit")
            }
        }
    }

    fn expect_environment_fatal(
        exit: TestExit,
    ) -> (Vec<u8>, EnvironmentFatal<&'static str>, Quiescence) {
        match exit {
            EngineExit::Fatal {
                state,
                cause: FatalCause::Environment(fatal),
                quiescence,
            } => (state, fatal, quiescence),
            _ => panic!(
                "environment fault invariant: an Environment failure must produce an Environment fatal exit"
            ),
        }
    }

    fn expect_core_fatal(exit: TestExit) -> (Vec<u8>, CoreError, Quiescence) {
        match exit {
            EngineExit::Fatal {
                state,
                cause: FatalCause::Core(error),
                quiescence,
            } => (state, error, quiescence),
            _ => panic!("core fault invariant: a Core failure must produce a Core fatal exit"),
        }
    }

    fn expect_application_fatal(exit: TestExit) -> (Vec<u8>, &'static str, Quiescence) {
        match exit {
            EngineExit::Fatal {
                state,
                cause: FatalCause::Application(error),
                quiescence,
            } => (state, error, quiescence),
            _ => panic!(
                "application fault invariant: an Application failure must produce an Application fatal exit"
            ),
        }
    }

    fn assert_sink_failure(error: JournalError, expected_operation: SinkOperation) {
        match error {
            JournalError::Sink { operation, error } => {
                assert_eq!(
                    operation, expected_operation,
                    "journal fault invariant: the fatal error must identify the failed sink operation"
                );
                assert_eq!(
                    error.kind(),
                    io::ErrorKind::BrokenPipe,
                    "journal fault invariant: the sink's Error kind must survive in the fatal exit"
                );
            }
            _ => {
                panic!("journal fault invariant: a scripted sink failure must remain a Sink error")
            }
        }
    }

    mod journal_fault_matrix {
        use super::*;

        /// Invariant: a sink failure at any record identifies that exact record,
        /// preserves the completed state and durable prefix, and performs no extra
        /// command handoffs.
        /// Design Doc: VERIFY-FAULTS
        #[test]
        fn each_record_kind_maps_to_its_journal_fatal() {
            for case in fault_cases() {
                let observation = run_journal_fault(case.failed_record, SinkFailure::Write);
                let (state, fatal, quiescence) = expect_journal_fatal(observation.exit);

                assert_eq!(
                    fatal.record_kind, case.record_kind,
                    "journal fault invariant: the {} failure must retain its record kind",
                    case.name
                );
                assert_eq!(
                    fatal.outcome, case.outcome,
                    "journal fault invariant: the {} failure must retain only its applicable outcome",
                    case.name
                );
                assert_sink_failure(fatal.error, SinkFailure::Write.operation());
                assert_eq!(
                    state.as_slice(),
                    case.state,
                    "journal fault invariant: the {} failure must preserve every completed state mutation",
                    case.name
                );
                assert_eq!(
                    quiescence,
                    Quiescence::Quiesced,
                    "journal fault invariant: the {} failure must finalize with the clean shutdown account",
                    case.name
                );
                assert_eq!(
                    observation.committed_bytes,
                    RECORDS[..case.failed_record].concat(),
                    "journal fault invariant: the {} failure must preserve exactly the prior committed records",
                    case.name
                );
                assert!(
                    observation.uncertain_suffix.is_empty(),
                    "journal fault invariant: a direct {} write failure must accept none of the failed record",
                    case.name
                );
                assert_eq!(
                    observation.handoffs.as_slice(),
                    &COMMANDS[..case.handoff_count],
                    "journal fault invariant: the {} failure must retain exactly the completed handoffs",
                    case.name
                );
                assert_eq!(
                    observation.shutdown_count, 1,
                    "journal fault invariant: the {} failure must finalize shutdown exactly once",
                    case.name
                );
            }
        }

        /// Invariant: failed turn-completion records carry their attempted answer,
        /// while failures of every other record carry no answer.
        /// Design Doc: JournalFatal
        #[test]
        fn only_turn_completed_carries_an_outcome() {
            for case in fault_cases() {
                let observation = run_journal_fault(case.failed_record, SinkFailure::Write);
                let (_, fatal, _) = expect_journal_fatal(observation.exit);

                assert_eq!(
                    fatal.record_kind, case.record_kind,
                    "journal outcome invariant: each fixture must fail at its selected record"
                );
                assert_eq!(
                    fatal.outcome.is_some(),
                    fatal.record_kind == RecordKind::TurnCompleted,
                    "journal outcome invariant: only TurnCompleted may retain an attempted outcome"
                );
                assert_eq!(
                    fatal.outcome, case.outcome,
                    "journal outcome invariant: each failed record must retain its exact outcome metadata"
                );
            }

            let observation = run_journal_fault(6, SinkFailure::Write);
            let (_, fatal, _) = expect_journal_fatal(observation.exit);
            assert_eq!(
                fatal.record_kind,
                RecordKind::TurnCompleted,
                "journal outcome invariant: the final Stop fixture must fail at TurnCompleted"
            );
            assert_eq!(
                fatal.outcome,
                Some(TurnOutcome::Stop),
                "journal outcome invariant: a failed Stop completion must retain the Stop outcome"
            );
        }

        /// Invariant: when the final stop-completion commit fails after clean
        /// shutdown, the fatal exit retains the shutdown's completed account.
        /// Design Doc: RUN-FINALIZE
        #[test]
        fn a_stop_commit_failure_retains_quiesced() {
            for failure in [SinkFailure::Write, SinkFailure::Flush] {
                let observation = run_journal_fault(6, failure);
                let (state, fatal, quiescence) = expect_journal_fatal(observation.exit);

                assert_eq!(
                    state,
                    [0, 1, 2],
                    "stop commit invariant: both completed handler mutations must remain in State"
                );
                assert_eq!(
                    fatal.record_kind,
                    RecordKind::TurnCompleted,
                    "stop commit invariant: the final commit failure must identify TurnCompleted"
                );
                assert_eq!(
                    fatal.outcome,
                    Some(TurnOutcome::Stop),
                    "stop commit invariant: the final commit failure must retain the Stop outcome"
                );
                assert_sink_failure(fatal.error, failure.operation());
                assert_eq!(
                    quiescence,
                    Quiescence::Quiesced,
                    "stop commit invariant: the consumed Environment's clean quiescence must be retained"
                );
                assert_eq!(
                    observation.shutdown_count, 1,
                    "stop commit invariant: finalization must not repeat the already completed shutdown"
                );
                assert_eq!(
                    observation.committed_bytes,
                    RECORDS[..6].concat(),
                    "stop commit invariant: StopRequested must remain the last committed record"
                );
                let expected_uncertain: &[u8] = match failure {
                    SinkFailure::Write => b"",
                    SinkFailure::Flush => RECORDS[6],
                };
                assert_eq!(
                    observation.uncertain_suffix, expected_uncertain,
                    "stop commit invariant: only a failed flush may leave the final record uncertain"
                );
                assert_eq!(
                    observation.handoffs, COMMANDS,
                    "stop commit invariant: all commands handed off before shutdown must remain counted"
                );
            }
        }

        /// Invariant: a failed flush leaves the entire failed record outside the
        /// committed boundary for every record kind.
        #[test]
        fn flush_failures_leave_the_failed_record_uncommitted() {
            for case in fault_cases() {
                let observation = run_journal_fault(case.failed_record, SinkFailure::Flush);
                let (state, fatal, quiescence) = expect_journal_fatal(observation.exit);

                assert_eq!(
                    fatal.record_kind, case.record_kind,
                    "flush fault invariant: the {} flush failure must retain its record kind",
                    case.name
                );
                assert_eq!(
                    fatal.outcome, case.outcome,
                    "flush fault invariant: the {} flush failure must retain its outcome metadata",
                    case.name
                );
                assert_sink_failure(fatal.error, SinkFailure::Flush.operation());
                assert_eq!(
                    state.as_slice(),
                    case.state,
                    "flush fault invariant: the {} failure must preserve completed State",
                    case.name
                );
                assert_eq!(
                    quiescence,
                    Quiescence::Quiesced,
                    "flush fault invariant: the {} failure must finalize cleanly",
                    case.name
                );
                assert_eq!(
                    observation.committed_bytes,
                    RECORDS[..case.failed_record].concat(),
                    "flush fault invariant: the {} failure must not advance the committed boundary",
                    case.name
                );
                assert_eq!(
                    observation.uncertain_suffix, RECORDS[case.failed_record],
                    "flush fault invariant: the {} failed record must remain wholly uncertain",
                    case.name
                );
                assert_eq!(
                    observation.handoffs.as_slice(),
                    &COMMANDS[..case.handoff_count],
                    "flush fault invariant: the {} failure must retain exactly the prior handoffs",
                    case.name
                );
                assert_eq!(
                    observation.shutdown_count, 1,
                    "flush fault invariant: the {} failure must perform shutdown exactly once",
                    case.name
                );
            }
        }

        /// Invariant: if a record write makes one byte of progress and then fails,
        /// the prior records stay committed while only that accepted byte is
        /// uncertain and no command is handed off.
        #[test]
        fn a_partial_write_failure_preserves_the_prior_commit_boundary() {
            let observation = run_with_sink_steps([
                SinkStep::Write(Ok(RECORDS[0].len())),
                SinkStep::Flush(Ok(())),
                SinkStep::Write(Ok(1)),
                SinkStep::Write(Err(sink_error("scripted partial write failure"))),
            ]);
            let (state, fatal, quiescence) = expect_journal_fatal(observation.exit);

            assert_eq!(
                state,
                [0, 1],
                "partial write invariant: the start handler's completed mutation must remain in State"
            );
            assert_eq!(
                fatal.record_kind,
                RecordKind::CommandsPrepared,
                "partial write invariant: the failed command-intent record must be identified"
            );
            assert_eq!(
                fatal.outcome, None,
                "partial write invariant: CommandsPrepared must not carry an outcome"
            );
            assert_sink_failure(fatal.error, SinkOperation::Write);
            assert_eq!(
                quiescence,
                Quiescence::Quiesced,
                "partial write invariant: finalization must retain clean quiescence"
            );
            assert_eq!(
                observation.committed_bytes, RECORDS[0],
                "partial write invariant: RunStarted must remain the complete committed prefix"
            );
            assert_eq!(
                observation.uncertain_suffix, b"{",
                "partial write invariant: only the accepted first byte may be uncertain"
            );
            assert!(
                observation.handoffs.is_empty(),
                "partial write invariant: a partially written intent must precede every handoff"
            );
            assert_eq!(
                observation.shutdown_count, 1,
                "partial write invariant: the started Environment must be shut down exactly once"
            );
        }
    }

    mod startup_faults {
        use super::*;

        /// Invariant: when Environment startup fails, the Engine returns the start
        /// failure as already quiesced without calling shutdown.
        /// Design Doc: VERIFY-FAULTS
        #[test]
        fn a_start_error_performs_no_shutdown() {
            let observation = run_startup_fault();

            match observation.exit {
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
                        [0],
                        "startup fault invariant: the initial State must be returned unchanged"
                    );
                    assert_eq!(
                        error, "start failed",
                        "startup fault invariant: the exact start Error must remain the fatal cause"
                    );
                    assert_eq!(
                        quiescence,
                        Quiescence::Quiesced,
                        "startup fault invariant: a failed start must be treated as already quiesced"
                    );
                }
                _ => panic!(
                    "startup fault invariant: a start Error must produce an Environment Start fatal exit"
                ),
            }
            assert_eq!(
                observation.shutdown_count, 0,
                "startup fault invariant: a failed start must perform no shutdown call"
            );
            assert_eq!(
                observation.env_calls,
                [EnvCall::Start(Err(()))],
                "startup fault invariant: start must be the sole Environment operation"
            );
        }

        /// Invariant: a startup failure occurs before any handler, journal sink
        /// operation, or command handoff can happen.
        #[test]
        fn a_start_error_invokes_no_handler_or_sink() {
            let observation = run_startup_fault();

            assert_eq!(
                observation.app_calls,
                [AppCall::InitialState],
                "startup isolation invariant: only initial State construction may precede start"
            );
            assert_eq!(
                observation.sink_call_count, 0,
                "startup isolation invariant: a failed start must not call the Journal sink"
            );
            assert!(
                observation.committed_bytes.is_empty(),
                "startup isolation invariant: a failed start must commit no Journal bytes"
            );
            assert!(
                observation.uncertain_suffix.is_empty(),
                "startup isolation invariant: a failed start must leave no uncertain sink bytes"
            );
            assert!(
                observation.handoffs.is_empty(),
                "startup isolation invariant: a failed start must hand off no commands"
            );
            assert_eq!(
                observation.shutdown_count, 0,
                "startup isolation invariant: no later lifecycle operation may follow failed startup"
            );
        }
    }

    mod environment_fault_matrix {
        use super::*;

        /// Invariant: every Environment failure identifies the operation that
        /// observed it, preserves that Error, and returns shutdown's final activity
        /// account.
        /// Design Doc: VERIFY-FAULTS
        #[test]
        fn each_operation_error_maps_to_its_cause_and_quiescence() {
            for point in environment_fault_points() {
                let observation = run_environment_fault(point, clean_shutdown());
                let (state, fatal, quiescence) = expect_environment_fatal(observation.exit);

                assert_eq!(
                    fatal.error,
                    point.error(),
                    "environment fault invariant: the {} Error must remain in the exit",
                    point.name()
                );
                assert_eq!(
                    fatal.operation,
                    point.operation(),
                    "environment fault invariant: the {} failure must identify its observation point",
                    point.name()
                );
                assert_eq!(
                    state,
                    [0, 1],
                    "environment fault invariant: the completed start-handler mutation must survive"
                );
                assert_eq!(
                    quiescence,
                    Quiescence::Quiesced,
                    "environment fault invariant: clean finalization must report Quiesced"
                );
                assert_eq!(
                    observation.shutdown_count, 1,
                    "environment fault invariant: each post-start failure must shut down exactly once"
                );
            }

            let observation = run_shutdown_fault(ShutdownReport {
                quiescence: Quiescence::Quiesced,
                error: Some("shutdown failed"),
            });
            let (state, fatal, quiescence) = expect_environment_fatal(observation.exit);
            assert_eq!(
                fatal.error, "shutdown failed",
                "shutdown fault invariant: the report's Error must remain in the exit"
            );
            assert_eq!(
                fatal.operation,
                EnvironmentOperation::Shutdown,
                "shutdown fault invariant: a report Error must identify Shutdown"
            );
            assert_eq!(
                state,
                [0, 1, 2],
                "shutdown fault invariant: both completed handler mutations must survive"
            );
            assert_eq!(
                quiescence,
                Quiescence::Quiesced,
                "shutdown fault invariant: the report's Quiesced account must be retained"
            );

            let observation = run_shutdown_fault(ShutdownReport {
                quiescence: Quiescence::Incomplete,
                error: None,
            });
            let (state, error, quiescence) = expect_core_fatal(observation.exit);
            assert!(
                matches!(error, CoreError::ShutdownIncomplete),
                "shutdown fault invariant: an error-free incomplete report must be ShutdownIncomplete"
            );
            assert_eq!(
                state,
                [0, 1, 2],
                "incomplete shutdown invariant: both completed handler mutations must survive"
            );
            assert_eq!(
                quiescence,
                Quiescence::Incomplete,
                "incomplete shutdown invariant: the report's Incomplete account must be retained"
            );
        }

        /// Invariant: once an Environment operation fails, an Error reported by
        /// cleanup cannot replace it, although cleanup's activity account remains
        /// authoritative.
        /// Design Doc: A4, RUN-FINALIZE
        #[test]
        fn the_operation_error_outranks_the_report_error() {
            for point in environment_fault_points() {
                let observation = run_environment_fault(
                    point,
                    ShutdownReport {
                        quiescence: Quiescence::Incomplete,
                        error: Some("later shutdown error"),
                    },
                );
                let (state, fatal, quiescence) = expect_environment_fatal(observation.exit);

                assert_eq!(
                    fatal.error,
                    point.error(),
                    "failure precedence invariant: the {} Error must outrank cleanup's Error",
                    point.name()
                );
                assert_eq!(
                    fatal.operation,
                    point.operation(),
                    "failure precedence invariant: cleanup must not replace the original operation"
                );
                assert_eq!(
                    state,
                    [0, 1],
                    "failure precedence invariant: cleanup must not alter returned State"
                );
                assert_eq!(
                    quiescence,
                    Quiescence::Incomplete,
                    "failure precedence invariant: the later report's quiescence must be retained"
                );
                assert_eq!(
                    observation.env_calls.last(),
                    Some(&EnvCall::Shutdown {
                        quiescence: TraceQuiescence::Incomplete,
                        returned_error: true,
                    }),
                    "failure precedence invariant: finalization must consume the secondary report"
                );
                assert_eq!(
                    observation.shutdown_count, 1,
                    "failure precedence invariant: cleanup must run exactly once"
                );
            }
        }

        /// Invariant: an Event stamped before the last accepted logical time is
        /// rejected as a Core time regression before it reaches the Application or
        /// Journal.
        /// Design Doc: VERIFY-FAULTS
        #[test]
        fn a_decreasing_stamp_is_time_regression() {
            let observation = run_time_regression();
            let (state, error, quiescence) = expect_core_fatal(observation.exit);

            match error {
                CoreError::TimeRegression { previous, offered } => {
                    assert_eq!(
                        previous,
                        Timestamp::from_nanos(100),
                        "time regression invariant: the last accepted time must be retained"
                    );
                    assert_eq!(
                        offered,
                        Timestamp::from_nanos(99),
                        "time regression invariant: the rejected candidate time must be retained"
                    );
                }
                _ => panic!(
                    "time regression invariant: a decreasing stamp must produce TimeRegression"
                ),
            }
            assert_eq!(
                state,
                [0, 1],
                "time regression invariant: only the completed start turn may mutate State"
            );
            assert_eq!(
                quiescence,
                Quiescence::Quiesced,
                "time regression invariant: clean finalization must report Quiesced"
            );
            assert_eq!(
                observation.app_calls,
                [
                    AppCall::InitialState,
                    AppCall::OnStart {
                        index: 0,
                        logical_time: 100,
                    },
                ],
                "time regression invariant: the rejected Event must not invoke on_event"
            );
            assert_eq!(
                observation.committed_bytes,
                RECORDS[..4].concat(),
                "time regression invariant: EventAccepted must not be committed"
            );
            assert_eq!(
                observation.shutdown_count, 1,
                "time regression invariant: the started Environment must shut down exactly once"
            );
        }

        /// Invariant: a failed dispatch transfers only commands whose earlier
        /// dispatch calls succeeded, and no checkpoint follows the failed call.
        #[test]
        fn a_failed_dispatch_retains_only_the_successful_handoff_prefix() {
            for point in [
                EnvironmentFaultPoint::Dispatch { position: 0 },
                EnvironmentFaultPoint::Dispatch { position: 1 },
            ] {
                let observation = run_environment_fault(point, clean_shutdown());
                let expected_calls = match point {
                    EnvironmentFaultPoint::Dispatch { position: 0 } => vec![
                        EnvCall::Start(Ok(Timestamp::from_nanos(100))),
                        EnvCall::Dispatch {
                            command: 10,
                            result: Err(()),
                        },
                        EnvCall::Shutdown {
                            quiescence: TraceQuiescence::Quiesced,
                            returned_error: false,
                        },
                    ],
                    EnvironmentFaultPoint::Dispatch { position: 1 } => vec![
                        EnvCall::Start(Ok(Timestamp::from_nanos(100))),
                        EnvCall::Dispatch {
                            command: 10,
                            result: Ok(()),
                        },
                        EnvCall::Dispatch {
                            command: 11,
                            result: Err(()),
                        },
                        EnvCall::Shutdown {
                            quiescence: TraceQuiescence::Quiesced,
                            returned_error: false,
                        },
                    ],
                    _ => panic!(
                        "dispatch prefix invariant: this hardening matrix contains only dispatch failures"
                    ),
                };

                assert_eq!(
                    observation.handoffs.as_slice(),
                    &COMMANDS[..point.handoff_count()],
                    "dispatch prefix invariant: only commands before the failed position may transfer"
                );
                assert_eq!(
                    observation.env_calls, expected_calls,
                    "dispatch prefix invariant: shutdown must immediately follow the failed dispatch"
                );
            }
        }

        /// Invariant: each Environment-side fatal condition leaves exactly the
        /// records committed before its failure boundary and no uncertain bytes.
        #[test]
        fn environment_failures_commit_no_records_after_their_boundary() {
            for point in environment_fault_points() {
                let observation = run_environment_fault(point, clean_shutdown());
                let record_count = point.committed_record_count();

                assert_eq!(
                    observation.committed_bytes,
                    RECORDS[..record_count].concat(),
                    "environment record boundary invariant: the {} failure must preserve only its prior records",
                    point.name()
                );
                assert!(
                    observation.uncertain_suffix.is_empty(),
                    "environment record boundary invariant: operation failures must add no uncertain bytes"
                );
                assert_eq!(
                    observation.sink_call_count,
                    record_count * 2,
                    "environment record boundary invariant: no sink call may follow the fixed failure"
                );
            }

            for report in [
                ShutdownReport {
                    quiescence: Quiescence::Quiesced,
                    error: Some("shutdown failed"),
                },
                ShutdownReport {
                    quiescence: Quiescence::Incomplete,
                    error: None,
                },
            ] {
                let observation = run_shutdown_fault(report);
                assert_eq!(
                    observation.committed_bytes,
                    RECORDS[..6].concat(),
                    "shutdown record boundary invariant: StopRequested must be the final committed record"
                );
                assert!(
                    observation.uncertain_suffix.is_empty(),
                    "shutdown record boundary invariant: a bad report must attempt no completion record"
                );
                assert_eq!(
                    observation.sink_call_count, 12,
                    "shutdown record boundary invariant: no sink call may follow the bad report"
                );
            }
        }
    }

    mod application_fault_matrix {
        use super::*;

        /// Invariant: emitting one command beyond the configured per-turn capacity
        /// returns a command-bound Core failure without handing off the staged batch.
        /// Design Doc: VERIFY-FAULTS
        #[test]
        fn an_over_emitting_application_is_command_bound_exceeded() {
            let observation = run_command_overflow(false);
            let (state, error, quiescence) = expect_core_fatal(observation.exit);

            assert!(
                matches!(error, CoreError::CommandBoundExceeded),
                "application overflow invariant: one command past capacity must be CommandBoundExceeded"
            );
            assert_eq!(
                state,
                [0, 1],
                "application overflow invariant: the overflowing handler's State mutation must survive"
            );
            assert_eq!(
                quiescence,
                Quiescence::Quiesced,
                "application overflow invariant: clean finalization must report Quiesced"
            );
            assert!(
                observation.handoffs.is_empty(),
                "application overflow invariant: no staged command may be handed off"
            );
            assert_eq!(
                observation.committed_bytes, RECORDS[0],
                "application overflow invariant: no command-intent record may be committed"
            );
            assert_eq!(
                observation.env_calls,
                [
                    EnvCall::Start(Ok(Timestamp::from_nanos(100))),
                    EnvCall::Shutdown {
                        quiescence: TraceQuiescence::Quiesced,
                        returned_error: false,
                    },
                ],
                "application overflow invariant: overflow must precede dispatch and checkpoint"
            );
        }

        /// Invariant: every failure observed after a handler returns preserves all
        /// State mutations through that handler, including the mutation from a
        /// failing handler itself.
        /// Design Doc: VERIFY-CONTEXT, APP-STATE
        #[test]
        fn state_mutations_survive_each_post_handler_fatal_exit() {
            let mut scenarios: Vec<(&str, RunObservation, &[u8])> = vec![
                (
                    "start-handler Application failure",
                    run_application_fatal(false),
                    &[0, 1],
                ),
                (
                    "event-handler Application failure",
                    run_application_fatal(true),
                    &[0, 1, 2],
                ),
                (
                    "start-handler overflow",
                    run_command_overflow(false),
                    &[0, 1],
                ),
                (
                    "event-handler overflow",
                    run_command_overflow(true),
                    &[0, 1, 2],
                ),
                ("time regression", run_time_regression(), &[0, 1]),
                (
                    "shutdown Error",
                    run_shutdown_fault(ShutdownReport {
                        quiescence: Quiescence::Quiesced,
                        error: Some("shutdown failed"),
                    }),
                    &[0, 1, 2],
                ),
                (
                    "incomplete shutdown",
                    run_shutdown_fault(ShutdownReport {
                        quiescence: Quiescence::Incomplete,
                        error: None,
                    }),
                    &[0, 1, 2],
                ),
            ];
            for point in environment_fault_points() {
                scenarios.push((
                    point.name(),
                    run_environment_fault(point, clean_shutdown()),
                    &[0, 1],
                ));
            }

            for (name, observation, expected_state) in scenarios {
                match observation.exit {
                    EngineExit::Fatal { state, .. } => assert_eq!(
                        state.as_slice(),
                        expected_state,
                        "State retention invariant: the {name} exit must preserve every completed handler mutation"
                    ),
                    EngineExit::Stopped { .. } => panic!(
                        "State retention invariant: every post-handler failure scenario must exit Fatal"
                    ),
                }
            }
        }

        /// Invariant: an Application failure preserves its exact Error while
        /// discarding every command staged by the failing start or Event handler.
        #[test]
        fn an_application_fatal_preserves_its_error_and_discards_its_batch() {
            for (on_event, expected_state, record_count, handoff_count) in [
                (false, &[0, 1][..], 1, 0),
                (true, &[0, 1, 2][..], 5, COMMANDS.len()),
            ] {
                let observation = run_application_fatal(on_event);
                let (state, error, quiescence) = expect_application_fatal(observation.exit);

                assert_eq!(
                    error, "application failed",
                    "Application failure invariant: the exact handler Error must remain in the exit"
                );
                assert_eq!(
                    state.as_slice(),
                    expected_state,
                    "Application failure invariant: State must include the failing handler's mutation"
                );
                assert_eq!(
                    quiescence,
                    Quiescence::Quiesced,
                    "Application failure invariant: clean finalization must report Quiesced"
                );
                assert_eq!(
                    observation.handoffs.as_slice(),
                    &COMMANDS[..handoff_count],
                    "Application failure invariant: the failing handler's staged commands must not transfer"
                );
                assert_eq!(
                    observation.committed_bytes,
                    RECORDS[..record_count].concat(),
                    "Application failure invariant: the failing handler's command intent must not be recorded"
                );
                assert_eq!(
                    observation.shutdown_count, 1,
                    "Application failure invariant: finalization must shut down exactly once"
                );
            }
        }

        /// Invariant: overflow in an Event handler preserves earlier turn effects
        /// but dispatches and records none of the overflowing turn's commands.
        #[test]
        fn event_turn_overflow_preserves_prior_effects_and_dispatches_nothing_new() {
            let observation = run_command_overflow(true);
            let (state, error, quiescence) = expect_core_fatal(observation.exit);

            assert!(
                matches!(error, CoreError::CommandBoundExceeded),
                "Event overflow invariant: the event handler must fail with CommandBoundExceeded"
            );
            assert_eq!(
                state,
                [0, 1, 2],
                "Event overflow invariant: both handler mutations must survive"
            );
            assert_eq!(
                quiescence,
                Quiescence::Quiesced,
                "Event overflow invariant: clean finalization must report Quiesced"
            );
            assert_eq!(
                observation.handoffs, COMMANDS,
                "Event overflow invariant: only the prior turn's commands may be handed off"
            );
            assert_eq!(
                observation.committed_bytes,
                RECORDS[..5].concat(),
                "Event overflow invariant: EventAccepted must be the final committed record"
            );
            assert_eq!(
                observation.app_calls,
                [
                    AppCall::InitialState,
                    AppCall::OnStart {
                        index: 0,
                        logical_time: 100,
                    },
                    AppCall::OnEvent {
                        event: 7,
                        index: 1,
                        logical_time: 105,
                    },
                ],
                "Event overflow invariant: the accepted Event must reach exactly one failing handler"
            );
            assert_eq!(
                observation.env_calls,
                [
                    EnvCall::Start(Ok(Timestamp::from_nanos(100))),
                    EnvCall::Dispatch {
                        command: 10,
                        result: Ok(()),
                    },
                    EnvCall::Dispatch {
                        command: 11,
                        result: Ok(()),
                    },
                    EnvCall::TakeError {
                        returned_error: false,
                    },
                    EnvCall::NextEvent(Ok((7, Timestamp::from_nanos(105)))),
                    EnvCall::Shutdown {
                        quiescence: TraceQuiescence::Quiesced,
                        returned_error: false,
                    },
                ],
                "Event overflow invariant: no dispatch or checkpoint may follow the overflowing handler"
            );
        }
    }
}
