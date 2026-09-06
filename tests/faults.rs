#[allow(dead_code, unused_imports)]
mod support;

#[cfg(test)]
mod tests {
    use super::support::{
        AppCall, EnvCall, RecordingApp, ScriptedAnswer, ScriptedEnv, ScriptedSink, ScriptedTurn,
        SinkStep,
    };
    use kavod::{
        Engine, EngineConfig, EngineExit, EnvironmentFatal, EnvironmentOperation, FatalCause,
        JournalError, JournalFatal, Quiescence, RecordKind, ShutdownReport, SinkOperation,
        Timestamp, TurnOutcome,
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
}
