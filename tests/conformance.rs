#[allow(dead_code, unused_imports)]
mod support;

#[cfg(test)]
mod tests {
    use super::support::{
        AppCall, EnvCall, GoldenLines, RecordingApp, ScriptedAnswer, ScriptedEnv, ScriptedSink,
        ScriptedTurn, SinkStep,
    };
    use kavod::{
        CoreError, Engine, EngineConfig, EngineExit, EnvironmentOperation, FatalCause,
        JournalError, Quiescence, RecordKind, ShutdownReport, SinkOperation, Timestamp,
        TurnOutcome,
    };
    use serde_json::Value;
    use std::io;
    use std::num::NonZeroUsize;

    type TestExit = EngineExit<Vec<u8>, &'static str, &'static str>;

    const RUN_STARTED: &[u8] =
        b"{\"record_kind\":\"RunStarted\",\"index\":0,\"schema_version\":1,\"logical_time\":100}\n";
    const COMMANDS_PREPARED: &[u8] =
        b"{\"record_kind\":\"CommandsPrepared\",\"index\":0,\"commands\":[10,11]}\n";

    #[derive(Clone, Copy, Debug)]
    enum ScriptedTrace {
        EmptyStop,
        OneCommandStop,
        CapacityEventStop,
        ApplicationFailure,
        StartFailure,
        FirstDispatchFailure,
        LaterDispatchFailure,
        CheckpointFailure,
        NextEventFailure,
        ShutdownFailure,
        JournalFailure,
        JournalSinkWriteFailure,
        JournalSinkFlushFailure,
        TimeRegression,
        CommandOverflow,
        IncompleteShutdown,
    }

    impl ScriptedTrace {
        const ALL: [Self; 16] = [
            Self::EmptyStop,
            Self::OneCommandStop,
            Self::CapacityEventStop,
            Self::ApplicationFailure,
            Self::StartFailure,
            Self::FirstDispatchFailure,
            Self::LaterDispatchFailure,
            Self::CheckpointFailure,
            Self::NextEventFailure,
            Self::ShutdownFailure,
            Self::JournalFailure,
            Self::JournalSinkWriteFailure,
            Self::JournalSinkFlushFailure,
            Self::TimeRegression,
            Self::CommandOverflow,
            Self::IncompleteShutdown,
        ];

        const FAILURES: [Self; 13] = [
            Self::ApplicationFailure,
            Self::StartFailure,
            Self::FirstDispatchFailure,
            Self::LaterDispatchFailure,
            Self::CheckpointFailure,
            Self::NextEventFailure,
            Self::ShutdownFailure,
            Self::JournalFailure,
            Self::JournalSinkWriteFailure,
            Self::JournalSinkFlushFailure,
            Self::TimeRegression,
            Self::CommandOverflow,
            Self::IncompleteShutdown,
        ];

        fn script(self) -> TraceScript {
            let clean_shutdown = || ShutdownReport {
                quiescence: Quiescence::Quiesced,
                error: None,
            };
            let mut script = TraceScript {
                turns: Vec::new(),
                start: Ok(Timestamp::from_nanos(100)),
                next_events: Vec::new(),
                dispatches: Vec::new(),
                checkpoints: Vec::new(),
                shutdown: clean_shutdown(),
                max_commands_per_turn: 2,
                max_record_bytes: 256,
                sink_steps: None,
            };

            match self {
                Self::EmptyStop => {
                    script
                        .turns
                        .push(ScriptedTurn::new(1, vec![], ScriptedAnswer::Stop));
                    script.checkpoints.push(None);
                }
                Self::OneCommandStop => {
                    script
                        .turns
                        .push(ScriptedTurn::new(1, vec![10], ScriptedAnswer::Stop));
                    script.dispatches.push(Ok(()));
                    script.checkpoints.push(None);
                }
                Self::CapacityEventStop => {
                    script.turns.extend([
                        ScriptedTurn::new(1, vec![10, 11], ScriptedAnswer::Continue),
                        ScriptedTurn::new(2, vec![], ScriptedAnswer::Stop),
                    ]);
                    script.next_events.push(Ok((7, Timestamp::from_nanos(105))));
                    script.dispatches.extend([Ok(()), Ok(())]);
                    script.checkpoints.extend([None, None]);
                }
                Self::ApplicationFailure => {
                    script.turns.push(ScriptedTurn::new(
                        1,
                        vec![10],
                        ScriptedAnswer::Fatal("application failure"),
                    ));
                }
                Self::StartFailure => script.start = Err("start failure"),
                Self::FirstDispatchFailure => {
                    script
                        .turns
                        .push(ScriptedTurn::new(1, vec![10, 11], ScriptedAnswer::Continue));
                    script.dispatches.push(Err("first dispatch failure"));
                }
                Self::LaterDispatchFailure => {
                    script
                        .turns
                        .push(ScriptedTurn::new(1, vec![10, 11], ScriptedAnswer::Continue));
                    script
                        .dispatches
                        .extend([Ok(()), Err("later dispatch failure")]);
                }
                Self::CheckpointFailure => {
                    script
                        .turns
                        .push(ScriptedTurn::new(1, vec![], ScriptedAnswer::Continue));
                    script.checkpoints.push(Some("checkpoint failure"));
                }
                Self::NextEventFailure => {
                    script
                        .turns
                        .push(ScriptedTurn::new(1, vec![], ScriptedAnswer::Continue));
                    script.next_events.push(Err("next event failure"));
                    script.checkpoints.push(None);
                }
                Self::ShutdownFailure => {
                    script
                        .turns
                        .push(ScriptedTurn::new(1, vec![], ScriptedAnswer::Stop));
                    script.checkpoints.push(None);
                    script.shutdown = ShutdownReport {
                        quiescence: Quiescence::Incomplete,
                        error: Some("shutdown failure"),
                    };
                }
                Self::JournalFailure => script.max_record_bytes = 1,
                Self::JournalSinkWriteFailure => {
                    script
                        .turns
                        .push(ScriptedTurn::new(1, vec![10, 11], ScriptedAnswer::Continue));
                    script.sink_steps = Some(vec![
                        SinkStep::Write(Ok(RUN_STARTED.len())),
                        SinkStep::Flush(Ok(())),
                        SinkStep::Write(Err(io::Error::new(
                            io::ErrorKind::BrokenPipe,
                            "scripted conformance write failure",
                        ))),
                    ]);
                }
                Self::JournalSinkFlushFailure => {
                    script
                        .turns
                        .push(ScriptedTurn::new(1, vec![10, 11], ScriptedAnswer::Continue));
                    script.sink_steps = Some(vec![
                        SinkStep::Write(Ok(RUN_STARTED.len())),
                        SinkStep::Flush(Ok(())),
                        SinkStep::Write(Ok(COMMANDS_PREPARED.len())),
                        SinkStep::Flush(Err(io::Error::new(
                            io::ErrorKind::BrokenPipe,
                            "scripted conformance flush failure",
                        ))),
                    ]);
                }
                Self::TimeRegression => {
                    script
                        .turns
                        .push(ScriptedTurn::new(1, vec![], ScriptedAnswer::Continue));
                    script.next_events.push(Ok((7, Timestamp::from_nanos(99))));
                    script.checkpoints.push(None);
                }
                Self::CommandOverflow => {
                    script.turns.push(ScriptedTurn::new(
                        1,
                        vec![10, 11, 12],
                        ScriptedAnswer::Continue,
                    ));
                }
                Self::IncompleteShutdown => {
                    script
                        .turns
                        .push(ScriptedTurn::new(1, vec![], ScriptedAnswer::Stop));
                    script.checkpoints.push(None);
                    script.shutdown = ShutdownReport {
                        quiescence: Quiescence::Incomplete,
                        error: None,
                    };
                }
            }

            script
        }
    }

    struct TraceScript {
        turns: Vec<ScriptedTurn<u8, &'static str>>,
        start: Result<Timestamp, &'static str>,
        next_events: Vec<Result<(u8, Timestamp), &'static str>>,
        dispatches: Vec<Result<(), &'static str>>,
        checkpoints: Vec<Option<&'static str>>,
        shutdown: ShutdownReport<&'static str>,
        max_commands_per_turn: usize,
        max_record_bytes: usize,
        sink_steps: Option<Vec<SinkStep>>,
    }

    impl TraceScript {
        fn run<W: io::Write>(self, writer: W) -> CoreObservation {
            let config = EngineConfig {
                max_commands_per_turn: NonZeroUsize::new(self.max_commands_per_turn)
                    .expect("conformance fixture invariant: command capacity must be nonzero"),
                max_record_bytes: NonZeroUsize::new(self.max_record_bytes)
                    .expect("conformance fixture invariant: record capacity must be nonzero"),
            };
            let (app, app_trace) = RecordingApp::new(vec![0], self.turns);
            let (environment, environment_trace) = ScriptedEnv::new(
                self.start,
                self.next_events,
                self.dispatches,
                self.checkpoints,
                self.shutdown,
            );
            let engine = Engine::new(config, app, environment, writer).unwrap_or_else(|_| {
                panic!("conformance fixture invariant: the Engine must construct")
            });
            let (state_transitions, exit) = summarize_exit(engine.run());
            let app_calls = app_trace.borrow().calls.clone();
            let environment_trace = environment_trace.borrow();

            CoreObservation {
                app_calls,
                state_transitions,
                environment_calls: environment_trace.calls.clone(),
                handoffs: environment_trace.handoffs.clone(),
                shutdown_count: environment_trace.shutdown_count,
                exit,
            }
        }
    }

    struct CoreObservation {
        app_calls: Vec<AppCall<u8>>,
        state_transitions: Vec<u8>,
        environment_calls: Vec<EnvCall<u8, u8>>,
        handoffs: Vec<u8>,
        shutdown_count: usize,
        exit: ExitShape,
    }

    #[derive(Debug, PartialEq, Eq)]
    struct RunObservation {
        app_calls: Vec<AppCall<u8>>,
        state_transitions: Vec<u8>,
        command_intent: Vec<Vec<u8>>,
        environment_calls: Vec<EnvCall<u8, u8>>,
        handoffs: Vec<u8>,
        shutdown_count: usize,
        journal_bytes: Vec<u8>,
        exit: ExitShape,
    }

    impl RunObservation {
        fn from_core(core: CoreObservation, journal_bytes: Vec<u8>) -> Self {
            Self {
                app_calls: core.app_calls,
                state_transitions: core.state_transitions,
                command_intent: committed_command_intent(&journal_bytes),
                environment_calls: core.environment_calls,
                handoffs: core.handoffs,
                shutdown_count: core.shutdown_count,
                journal_bytes,
                exit: core.exit,
            }
        }
    }

    #[derive(Debug, PartialEq, Eq)]
    enum ExitShape {
        Stopped,
        Fatal {
            cause: CauseShape,
            quiescence: Quiescence,
        },
    }

    #[derive(Debug, PartialEq, Eq)]
    enum CauseShape {
        Application(&'static str),
        Environment {
            error: &'static str,
            operation: EnvironmentOperation,
        },
        Journal {
            record_kind: RecordKind,
            outcome: Option<TurnOutcome>,
            error: JournalErrorShape,
        },
        Core(CoreError),
    }

    #[derive(Debug, PartialEq, Eq)]
    enum JournalErrorShape {
        Encode(String),
        NotAnObject,
        BoundExceeded,
        Sink {
            operation: SinkOperation,
            kind: io::ErrorKind,
            raw_os_error: Option<i32>,
            message: String,
        },
    }

    fn summarize_exit(exit: TestExit) -> (Vec<u8>, ExitShape) {
        match exit {
            EngineExit::Stopped { state } => (state, ExitShape::Stopped),
            EngineExit::Fatal {
                state,
                cause,
                quiescence,
            } => {
                let cause = match cause {
                    FatalCause::Application(error) => CauseShape::Application(error),
                    FatalCause::Environment(fatal) => CauseShape::Environment {
                        error: fatal.error,
                        operation: fatal.operation,
                    },
                    FatalCause::Journal(fatal) => {
                        let error = match fatal.error {
                            JournalError::Encode(error) => {
                                JournalErrorShape::Encode(error.to_string())
                            }
                            JournalError::NotAnObject => JournalErrorShape::NotAnObject,
                            JournalError::BoundExceeded => JournalErrorShape::BoundExceeded,
                            JournalError::Sink { operation, error } => JournalErrorShape::Sink {
                                operation,
                                kind: error.kind(),
                                raw_os_error: error.raw_os_error(),
                                message: error.to_string(),
                            },
                        };
                        CauseShape::Journal {
                            record_kind: fatal.record_kind,
                            outcome: fatal.outcome,
                            error,
                        }
                    }
                    FatalCause::Core(error) => CauseShape::Core(error),
                };
                (state, ExitShape::Fatal { cause, quiescence })
            }
        }
    }

    fn committed_command_intent(bytes: &[u8]) -> Vec<Vec<u8>> {
        let mut intent = Vec::new();
        for line in GoldenLines::split(bytes) {
            let record: Value = serde_json::from_slice(line)
                .expect("command-intent invariant: every committed Journal line must be JSON");
            if record.get("record_kind").and_then(Value::as_str) != Some("CommandsPrepared") {
                continue;
            }
            let commands = record
                .get("commands")
                .and_then(Value::as_array)
                .expect("command-intent invariant: CommandsPrepared must contain an array");
            intent.push(
                commands
                    .iter()
                    .map(|command| {
                        let value = command.as_u64().expect(
                            "command-intent invariant: each fixture Command must be an integer",
                        );
                        u8::try_from(value)
                            .expect("command-intent invariant: each fixture Command must fit in u8")
                    })
                    .collect(),
            );
        }
        intent
    }

    fn run(trace: ScriptedTrace) -> RunObservation {
        let mut script = trace.script();
        match script.sink_steps.take() {
            Some(steps) => {
                let (sink, sink_trace) = ScriptedSink::new(steps);
                let core = script.run(sink);
                let journal_bytes = sink_trace.borrow().committed_bytes().to_vec();
                RunObservation::from_core(core, journal_bytes)
            }
            None => {
                let mut journal_bytes = Vec::new();
                let core = script.run(&mut journal_bytes);
                RunObservation::from_core(core, journal_bytes)
            }
        }
    }

    fn run_twice(trace: ScriptedTrace) -> (RunObservation, RunObservation) {
        (run(trace), run(trace))
    }

    mod conformance_within_type {
        use super::*;

        /// Invariant: repeating any scripted run with fresh collaborators commits
        /// exactly the same Journal byte sequence through its final durable record.
        /// Design Doc: DET-RUN
        #[test]
        fn the_same_trace_reproduces_identical_journal_bytes() {
            for trace in ScriptedTrace::ALL {
                let (first, second) = run_twice(trace);
                assert_eq!(
                    first.journal_bytes, second.journal_bytes,
                    "Journal determinism invariant: {trace:?} must reproduce identical committed bytes"
                );
            }
        }

        /// Invariant: repeating any scripted run reproduces the same exit variant,
        /// failure shape, Core-owned payloads, corresponding Errors, and quiescence.
        /// Design Doc: DET-RUN
        #[test]
        fn the_same_trace_reproduces_det_run_equal_exits() {
            for trace in ScriptedTrace::ALL {
                let (first, second) = run_twice(trace);
                assert_eq!(
                    first.exit, second.exit,
                    "exit determinism invariant: {trace:?} must reproduce an equal exit"
                );
            }
        }

        /// Invariant: every scripted Environment call follows the legal lifecycle
        /// order, and only successful dispatches become completed Command handoffs.
        /// Design Doc: VERIFY-CONFORMANCE
        #[test]
        fn every_environment_call_is_graph_conformant() {
            for trace in ScriptedTrace::ALL {
                let observation = run(trace);
                let completed_handoffs: Vec<u8> = observation
                    .environment_calls
                    .iter()
                    .filter_map(|call| match call {
                        EnvCall::Dispatch {
                            command,
                            result: Ok(()),
                        } => Some(*command),
                        _ => None,
                    })
                    .collect();

                assert_eq!(
                    completed_handoffs, observation.handoffs,
                    "Environment graph invariant: {trace:?} must record exactly its successful dispatches as handoffs"
                );
                match observation.environment_calls.first() {
                    Some(EnvCall::Start(Err(()))) => {
                        assert_eq!(
                            observation.environment_calls.len(),
                            1,
                            "Environment graph invariant: failed startup must be the only call"
                        );
                        assert_eq!(
                            observation.shutdown_count, 0,
                            "Environment graph invariant: failed startup must not be followed by shutdown"
                        );
                    }
                    Some(EnvCall::Start(Ok(_))) => {
                        assert_eq!(
                            observation.shutdown_count, 1,
                            "Environment graph invariant: every started trace must shut down exactly once"
                        );
                        assert!(
                            matches!(
                                observation.environment_calls.last(),
                                Some(EnvCall::Shutdown { .. })
                            ),
                            "Environment graph invariant: shutdown must be the final call"
                        );
                    }
                    _ => panic!("Environment graph invariant: start must be the first call"),
                }
            }
        }

        /// Invariant: repeating a trace invokes the same handlers with the same
        /// Events, indices, and logical times in the same order.
        #[test]
        fn the_same_trace_reproduces_identical_handler_calls() {
            for trace in ScriptedTrace::ALL {
                let (first, second) = run_twice(trace);
                assert_eq!(
                    first.app_calls, second.app_calls,
                    "handler determinism invariant: {trace:?} must reproduce identical calls"
                );
            }
        }

        /// Invariant: repeating a trace reproduces every completed State mutation,
        /// represented by the append-only State returned from the run.
        #[test]
        fn the_same_trace_reproduces_identical_state_transitions() {
            for trace in ScriptedTrace::ALL {
                let (first, second) = run_twice(trace);
                assert_eq!(
                    first.state_transitions, second.state_transitions,
                    "State determinism invariant: {trace:?} must reproduce identical transitions"
                );
            }
        }

        /// Invariant: repeating a trace reproduces each committed Command batch and
        /// each successfully completed handoff in exact order.
        #[test]
        fn the_same_trace_reproduces_identical_command_intent() {
            for trace in ScriptedTrace::ALL {
                let (first, second) = run_twice(trace);
                assert_eq!(
                    first.command_intent, second.command_intent,
                    "Command determinism invariant: {trace:?} must reproduce identical intent"
                );
                assert_eq!(
                    first.handoffs, second.handoffs,
                    "Command determinism invariant: {trace:?} must reproduce identical handoffs"
                );
            }
        }

        /// Invariant: runs with zero, one, and exactly the configured number of
        /// Commands remain reproducible across every captured observation.
        #[test]
        fn zero_one_and_capacity_command_traces_are_fully_reproducible() {
            for trace in [
                ScriptedTrace::EmptyStop,
                ScriptedTrace::OneCommandStop,
                ScriptedTrace::CapacityEventStop,
            ] {
                let (first, second) = run_twice(trace);
                assert_eq!(
                    first, second,
                    "command-boundary determinism invariant: {trace:?} must reproduce its complete observation"
                );
            }
        }

        /// Invariant: every catalogued failure reproduces all observations retained
        /// before and during that failure, including State, handoffs, and bytes.
        #[test]
        fn every_failure_trace_reproduces_its_complete_observation() {
            for trace in ScriptedTrace::FAILURES {
                let (first, second) = run_twice(trace);
                assert!(
                    matches!(first.exit, ExitShape::Fatal { .. }),
                    "failure-catalog invariant: {trace:?} must produce a Fatal exit"
                );
                assert_eq!(
                    first, second,
                    "failure determinism invariant: {trace:?} must reproduce its complete observation"
                );
            }
        }

        /// Invariant: sink write and flush failures repeat with the same committed
        /// prefix, failed operation, record kind, Error shape, and quiescence.
        #[test]
        fn sink_write_and_flush_failures_reproduce_their_operations() {
            for (trace, expected_operation) in [
                (ScriptedTrace::JournalSinkWriteFailure, SinkOperation::Write),
                (ScriptedTrace::JournalSinkFlushFailure, SinkOperation::Flush),
            ] {
                let (first, second) = run_twice(trace);
                assert_eq!(
                    first, second,
                    "sink-failure determinism invariant: {trace:?} must reproduce its complete observation"
                );
                match first.exit {
                    ExitShape::Fatal {
                        cause:
                            CauseShape::Journal {
                                record_kind,
                                outcome,
                                error:
                                    JournalErrorShape::Sink {
                                        operation, kind, ..
                                    },
                            },
                        quiescence,
                    } => {
                        assert_eq!(
                            record_kind,
                            RecordKind::CommandsPrepared,
                            "sink-failure invariant: the failure must identify CommandsPrepared"
                        );
                        assert_eq!(
                            outcome, None,
                            "sink-failure invariant: CommandsPrepared must carry no outcome"
                        );
                        assert_eq!(
                            operation, expected_operation,
                            "sink-failure invariant: the exit must retain the failed sink operation"
                        );
                        assert_eq!(
                            kind,
                            io::ErrorKind::BrokenPipe,
                            "sink-failure invariant: the exit must retain the sink Error kind"
                        );
                        assert_eq!(
                            quiescence,
                            Quiescence::Quiesced,
                            "sink-failure invariant: fatal finalization must retain clean quiescence"
                        );
                    }
                    _ => panic!(
                        "sink-failure invariant: the trace must produce a Journal Sink fatal exit"
                    ),
                }
            }
        }
    }
}
