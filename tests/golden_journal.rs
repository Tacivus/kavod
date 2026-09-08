#[expect(dead_code, unused_imports)]
mod support;

#[cfg(test)]
mod tests {
    use super::support::{
        AppCall, EnvCall, GoldenLines, RecordingApp, ScriptedEnv, ScriptedTurn,
        expect_environment_fatal, expect_journal_fatal, stopped,
    };
    use kavod::{
        Engine, EngineConfig, EngineExit, EnvironmentOperation, JournalError, Outcome, Quiescence,
        RecordKind, ShutdownReport, Timestamp, TurnOutcome,
    };
    use serde::Serialize;
    use serde_json::value::RawValue;
    use std::num::NonZeroUsize;

    type TestExit = EngineExit<Vec<u8>, &'static str, &'static str>;

    /// Everything one golden run leaves behind.
    struct Golden<E, C> {
        exit: TestExit,
        bytes: Vec<u8>,
        app_calls: Vec<AppCall<E>>,
        env_calls: Vec<EnvCall<E, C>>,
        handoffs: Vec<C>,
        shutdown_count: usize,
    }

    fn config() -> EngineConfig {
        EngineConfig {
            max_commands_per_turn: NonZeroUsize::new(2)
                .expect("golden fixture invariant: command capacity must be nonzero"),
            max_record_bytes: NonZeroUsize::new(256)
                .expect("golden fixture invariant: record capacity must be nonzero"),
        }
    }

    fn clean_shutdown() -> ShutdownReport<&'static str> {
        ShutdownReport {
            quiescence: Quiescence::Quiesced,
            error: None,
        }
    }

    /// Runs one Application script against a clean-shutdown Environment script
    /// started at `start`, journaling into a `Vec`.
    fn run_golden<E: Clone + Serialize, C: Clone + Serialize>(
        start: Timestamp,
        turns: Vec<ScriptedTurn<C, &'static str>>,
        next_events: Vec<Result<(E, Timestamp), &'static str>>,
        dispatches: Vec<Result<(), &'static str>>,
        checkpoints: Vec<Option<&'static str>>,
    ) -> Golden<E, C> {
        let (app, app_trace) = RecordingApp::new(vec![0], turns);
        let (environment, env_trace) = ScriptedEnv::new(
            Ok(start),
            next_events,
            dispatches,
            checkpoints,
            clean_shutdown(),
        );
        let mut bytes = Vec::new();
        let engine = Engine::new(config(), app, environment, &mut bytes)
            .expect("golden fixture invariant: the Engine must construct");
        let exit = engine.run();
        let env_trace = env_trace.borrow();

        Golden {
            exit,
            bytes,
            app_calls: app_trace.borrow().calls.clone(),
            env_calls: env_trace.calls.clone(),
            handoffs: env_trace.handoffs.clone(),
            shutdown_count: env_trace.shutdown_count,
        }
    }

    fn run_start_turn(commands: Vec<u8>, answer: TurnOutcome) -> (TestExit, Vec<u8>) {
        let command_count = commands.len();
        let next_events = match answer {
            TurnOutcome::Continue => vec![Err("end after tested Continue turn")],
            TurnOutcome::Stop => Vec::new(),
        };
        let scripted_answer = match answer {
            TurnOutcome::Continue => Outcome::Continue,
            TurnOutcome::Stop => Outcome::Stop,
        };
        let golden = run_golden::<u8, u8>(
            Timestamp::from_nanos(100),
            vec![ScriptedTurn::new(1, commands, scripted_answer)],
            next_events,
            (0..command_count).map(|_| Ok(())).collect(),
            vec![None],
        );
        (golden.exit, golden.bytes)
    }

    mod golden_sequences {
        use super::*;

        /// Invariant: a clean stop during the commandless start turn writes only the
        /// start, stop request, and stop completion records.
        /// Design Doc: VERIFY-JOURNAL
        #[test]
        fn a_stop_run_writes_exactly_its_records() {
            const EXPECTED: &[u8] =
                br#"{"record_kind":"RunStarted","index":0,"schema_version":1,"logical_time":100}
{"record_kind":"StopRequested","index":0}
{"record_kind":"TurnCompleted","index":0,"outcome":"Stop"}
"#;

            let golden = run_golden::<u8, u8>(
                Timestamp::from_nanos(100),
                vec![ScriptedTurn::new(1, Vec::new(), Outcome::Stop)],
                Vec::new(),
                Vec::new(),
                vec![None],
            );

            let state = stopped(golden.exit);
            assert_eq!(
                state,
                [0, 1],
                "stop-run state invariant: the sole start-turn mutation must be retained"
            );
            assert_eq!(
                golden.app_calls,
                [
                    AppCall::InitialState,
                    AppCall::OnStart {
                        index: 0,
                        logical_time: 100,
                    },
                ],
                "stop-run application invariant: the complete Application script must be consumed"
            );
            assert_eq!(
                golden.env_calls,
                [
                    EnvCall::Start(Ok(Timestamp::from_nanos(100))),
                    EnvCall::TakeError {
                        returned_error: false,
                    },
                    EnvCall::Shutdown {
                        quiescence: Quiescence::Quiesced,
                        returned_error: false,
                    },
                ],
                "stop-run environment invariant: the complete Environment script must be consumed"
            );
            assert!(
                golden.handoffs.is_empty(),
                "stop-run handoff invariant: a commandless turn must hand off no Commands"
            );
            assert_eq!(
                golden.shutdown_count, 1,
                "stop-run shutdown invariant: the Environment must be shut down exactly once"
            );
            assert_eq!(
                GoldenLines::split(&golden.bytes).len(),
                3,
                "stop-run record invariant: the Journal must contain three complete lines"
            );
            assert_eq!(
                golden.bytes.as_slice(),
                EXPECTED,
                "stop-run byte invariant: the complete Journal must match the golden sequence"
            );
        }

        /// Invariant: a clean start turn that fills the command capacity records and
        /// hands off the entire batch before recording its stop.
        /// Design Doc: VERIFY-JOURNAL
        #[test]
        fn a_command_run_writes_exactly_its_records() {
            const EXPECTED: &[u8] =
                br#"{"record_kind":"RunStarted","index":0,"schema_version":1,"logical_time":100}
{"record_kind":"CommandsPrepared","index":0,"commands":[10,11]}
{"record_kind":"CommandsDispatched","index":0}
{"record_kind":"StopRequested","index":0}
{"record_kind":"TurnCompleted","index":0,"outcome":"Stop"}
"#;

            let golden = run_golden::<u8, u8>(
                Timestamp::from_nanos(100),
                vec![ScriptedTurn::new(1, vec![10, 11], Outcome::Stop)],
                Vec::new(),
                vec![Ok(()), Ok(())],
                vec![None],
            );

            let state = stopped(golden.exit);
            assert_eq!(
                state,
                [0, 1],
                "command-run state invariant: the sole start-turn mutation must be retained"
            );
            assert_eq!(
                golden.app_calls,
                [
                    AppCall::InitialState,
                    AppCall::OnStart {
                        index: 0,
                        logical_time: 100,
                    },
                ],
                "command-run application invariant: the complete Application script must be consumed"
            );
            assert_eq!(
                golden.env_calls,
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
                    EnvCall::Shutdown {
                        quiescence: Quiescence::Quiesced,
                        returned_error: false,
                    },
                ],
                "command-run environment invariant: the complete Environment script must be consumed"
            );
            assert_eq!(
                golden.handoffs,
                [10, 11],
                "command-run handoff invariant: every Command must be handed off once in order"
            );
            assert_eq!(
                golden.shutdown_count, 1,
                "command-run shutdown invariant: the Environment must be shut down exactly once"
            );
            assert_eq!(
                GoldenLines::split(&golden.bytes).len(),
                5,
                "command-run record invariant: the Journal must contain five complete lines"
            );
            assert_eq!(
                golden.bytes.as_slice(),
                EXPECTED,
                "command-run byte invariant: the complete Journal must match the golden sequence"
            );
        }

        /// Invariant: a commandless continued start accepts one event and records the
        /// event turn's clean stop at the accepted index and time.
        /// Design Doc: VERIFY-JOURNAL
        #[test]
        fn an_event_run_writes_exactly_its_records() {
            const EXPECTED: &[u8] =
                br#"{"record_kind":"RunStarted","index":0,"schema_version":1,"logical_time":100}
{"record_kind":"TurnCompleted","index":0,"outcome":"Continue"}
{"record_kind":"EventAccepted","index":1,"logical_time":105,"event":7}
{"record_kind":"StopRequested","index":1}
{"record_kind":"TurnCompleted","index":1,"outcome":"Stop"}
"#;

            let golden = run_golden::<u8, u8>(
                Timestamp::from_nanos(100),
                vec![
                    ScriptedTurn::new(1, Vec::new(), Outcome::Continue),
                    ScriptedTurn::new(2, Vec::new(), Outcome::Stop),
                ],
                vec![Ok((7, Timestamp::from_nanos(105)))],
                Vec::new(),
                vec![None, None],
            );

            let state = stopped(golden.exit);
            assert_eq!(
                state,
                [0, 1, 2],
                "event-run state invariant: both turn mutations must be retained"
            );
            assert_eq!(
                golden.app_calls,
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
                "event-run application invariant: the complete Application script must be consumed"
            );
            assert_eq!(
                golden.env_calls,
                [
                    EnvCall::Start(Ok(Timestamp::from_nanos(100))),
                    EnvCall::TakeError {
                        returned_error: false,
                    },
                    EnvCall::NextEvent(Ok((7, Timestamp::from_nanos(105)))),
                    EnvCall::TakeError {
                        returned_error: false,
                    },
                    EnvCall::Shutdown {
                        quiescence: Quiescence::Quiesced,
                        returned_error: false,
                    },
                ],
                "event-run environment invariant: the complete Environment script must be consumed"
            );
            assert!(
                golden.handoffs.is_empty(),
                "event-run handoff invariant: commandless turns must hand off no Commands"
            );
            assert_eq!(
                golden.shutdown_count, 1,
                "event-run shutdown invariant: the Environment must be shut down exactly once"
            );
            assert_eq!(
                GoldenLines::split(&golden.bytes).len(),
                5,
                "event-run record invariant: the Journal must contain five complete lines"
            );
            assert_eq!(
                golden.bytes.as_slice(),
                EXPECTED,
                "event-run byte invariant: the complete Journal must match the golden sequence"
            );
        }

        /// Invariant: repeated events advance each record's index while zero and
        /// equal logical times are preserved exactly.
        #[test]
        fn repeated_events_advance_indices_and_preserve_time_boundaries() {
            const EXPECTED: &[u8] =
                br#"{"record_kind":"RunStarted","index":0,"schema_version":1,"logical_time":0}
{"record_kind":"TurnCompleted","index":0,"outcome":"Continue"}
{"record_kind":"EventAccepted","index":1,"logical_time":0,"event":7}
{"record_kind":"TurnCompleted","index":1,"outcome":"Continue"}
{"record_kind":"EventAccepted","index":2,"logical_time":1,"event":8}
{"record_kind":"StopRequested","index":2}
{"record_kind":"TurnCompleted","index":2,"outcome":"Stop"}
"#;

            let golden = run_golden::<u8, u8>(
                Timestamp::from_nanos(0),
                vec![
                    ScriptedTurn::new(1, Vec::new(), Outcome::Continue),
                    ScriptedTurn::new(2, Vec::new(), Outcome::Continue),
                    ScriptedTurn::new(3, Vec::new(), Outcome::Stop),
                ],
                vec![
                    Ok((7, Timestamp::from_nanos(0))),
                    Ok((8, Timestamp::from_nanos(1))),
                ],
                Vec::new(),
                vec![None, None, None],
            );

            let state = stopped(golden.exit);
            assert_eq!(
                state,
                [0, 1, 2, 3],
                "repeated-event state invariant: all three turn mutations must be retained"
            );
            assert_eq!(
                golden.app_calls,
                [
                    AppCall::InitialState,
                    AppCall::OnStart {
                        index: 0,
                        logical_time: 0,
                    },
                    AppCall::OnEvent {
                        event: 7,
                        index: 1,
                        logical_time: 0,
                    },
                    AppCall::OnEvent {
                        event: 8,
                        index: 2,
                        logical_time: 1,
                    },
                ],
                "repeated-event application invariant: each indexed turn must consume one scripted handler"
            );
            assert_eq!(
                golden.env_calls,
                [
                    EnvCall::Start(Ok(Timestamp::from_nanos(0))),
                    EnvCall::TakeError {
                        returned_error: false,
                    },
                    EnvCall::NextEvent(Ok((7, Timestamp::from_nanos(0)))),
                    EnvCall::TakeError {
                        returned_error: false,
                    },
                    EnvCall::NextEvent(Ok((8, Timestamp::from_nanos(1)))),
                    EnvCall::TakeError {
                        returned_error: false,
                    },
                    EnvCall::Shutdown {
                        quiescence: Quiescence::Quiesced,
                        returned_error: false,
                    },
                ],
                "repeated-event environment invariant: both Events and all checkpoints must be consumed in order"
            );
            assert!(
                golden.handoffs.is_empty(),
                "repeated-event handoff invariant: commandless turns must hand off no Commands"
            );
            assert_eq!(
                golden.shutdown_count, 1,
                "repeated-event shutdown invariant: the Environment must be shut down exactly once"
            );
            assert_eq!(
                GoldenLines::split(&golden.bytes).len(),
                7,
                "repeated-event record invariant: the Journal must contain seven complete lines"
            );
            assert_eq!(
                golden.bytes.as_slice(),
                EXPECTED,
                "repeated-event byte invariant: every repeated Event record must match the golden sequence"
            );
        }
    }

    mod classify_call_site {
        use super::*;

        /// Invariant: each non-fatal handler answer produces the matching completion
        /// outcome, and only a Stop answer produces a stop-request record.
        /// Design Doc: RUN-ENFORCEMENT
        #[test]
        fn each_non_fatal_answer_yields_its_required_outcome_records() {
            let (continue_exit, continue_bytes) = run_start_turn(Vec::new(), TurnOutcome::Continue);
            let (state, fatal, quiescence) = expect_environment_fatal(continue_exit);
            assert_eq!(
                state,
                [0, 1],
                "Continue classification state invariant: the tested turn mutation must survive termination"
            );
            assert_eq!(
                fatal.operation,
                EnvironmentOperation::NextEvent,
                "Continue classification termination invariant: the fixture must end only after recording the outcome"
            );
            assert_eq!(
                quiescence,
                Quiescence::Quiesced,
                "Continue classification shutdown invariant: fixture termination must quiesce"
            );
            let continue_lines = GoldenLines::split(&continue_bytes);
            assert_eq!(
                continue_lines.last().copied(),
                Some(
                    b"{\"record_kind\":\"TurnCompleted\",\"index\":0,\"outcome\":\"Continue\"}\n"
                        .as_slice()
                ),
                "Continue classification record invariant: Continue must be the committed turn outcome"
            );

            let (stop_exit, stop_bytes) = run_start_turn(Vec::new(), TurnOutcome::Stop);
            let stop_state = stopped(stop_exit);
            assert_eq!(
                stop_state,
                [0, 1],
                "Stop classification state invariant: the tested turn mutation must survive completion"
            );
            let stop_lines = GoldenLines::split(&stop_bytes);
            assert!(
                stop_lines.len() >= 2,
                "Stop classification record invariant: Stop must have both request and completion records"
            );
            let expected_stop_records: &[&[u8]] = &[
                b"{\"record_kind\":\"StopRequested\",\"index\":0}\n",
                b"{\"record_kind\":\"TurnCompleted\",\"index\":0,\"outcome\":\"Stop\"}\n",
            ];
            assert_eq!(
                &stop_lines[stop_lines.len() - 2..],
                expected_stop_records,
                "Stop classification record invariant: Stop must commit its request before its matching outcome"
            );
        }
    }

    mod encoding_rejection {
        use super::*;

        /// Invariant: a command payload whose encoded JSON contains a literal newline
        /// fails before any byte of that command record reaches the Journal sink.
        /// Design Doc: VERIFY-JOURNAL
        #[test]
        fn an_interior_newline_payload_is_rejected_with_nothing_written() {
            const EXPECTED_PREFIX: &[u8] =
                br#"{"record_kind":"RunStarted","index":0,"schema_version":1,"logical_time":100}
"#;

            let raw = RawValue::from_string(String::from("{\"a\":\n1}"))
                .expect("interior-newline fixture invariant: the raw JSON must be valid");
            let golden = run_golden::<u8, Box<RawValue>>(
                Timestamp::from_nanos(100),
                vec![ScriptedTurn::new(1, vec![raw], Outcome::Continue)],
                Vec::new(),
                Vec::new(),
                Vec::new(),
            );

            let (state, fatal, quiescence) = expect_journal_fatal(golden.exit);
            assert_eq!(
                state,
                [0, 1],
                "newline-command state invariant: the handler mutation must survive encoding rejection"
            );
            assert_eq!(
                fatal.record_kind,
                RecordKind::CommandsPrepared,
                "newline-command record invariant: rejection must identify CommandsPrepared"
            );
            assert_eq!(
                fatal.outcome, None,
                "newline-command outcome invariant: CommandsPrepared must carry no outcome"
            );
            assert!(
                matches!(fatal.error, JournalError::NotAnObject),
                "newline-command error invariant: a literal newline must be NotAnObject"
            );
            assert_eq!(
                quiescence,
                Quiescence::Quiesced,
                "newline-command shutdown invariant: finalization must retain clean quiescence"
            );
            assert_eq!(
                golden.app_calls,
                [
                    AppCall::InitialState,
                    AppCall::OnStart {
                        index: 0,
                        logical_time: 100,
                    },
                ],
                "newline-command application invariant: only the start handler must run"
            );
            assert!(
                golden.handoffs.is_empty(),
                "newline-command handoff invariant: an unrecordable batch must hand off no Commands"
            );
            assert_eq!(
                golden.shutdown_count, 1,
                "newline-command shutdown invariant: the Environment must be shut down exactly once"
            );
            assert_eq!(
                golden.bytes.as_slice(),
                EXPECTED_PREFIX,
                "newline-command byte invariant: the rejected record must add nothing to the committed prefix"
            );
        }

        /// Invariant: rejecting an event record with a literal newline preserves all
        /// earlier records and prevents the rejected Event from reaching its handler.
        #[test]
        fn an_interior_newline_event_preserves_the_committed_prefix() {
            const EXPECTED_PREFIX: &[u8] =
                br#"{"record_kind":"RunStarted","index":0,"schema_version":1,"logical_time":100}
{"record_kind":"TurnCompleted","index":0,"outcome":"Continue"}
"#;

            let raw = RawValue::from_string(String::from("{\"a\":\n1}"))
                .expect("newline-event fixture invariant: the raw JSON must be valid");
            let golden = run_golden::<Box<RawValue>, u8>(
                Timestamp::from_nanos(100),
                vec![ScriptedTurn::new(1, Vec::new(), Outcome::Continue)],
                vec![Ok((raw, Timestamp::from_nanos(105)))],
                Vec::new(),
                vec![None],
            );

            let (state, fatal, quiescence) = expect_journal_fatal(golden.exit);
            assert_eq!(
                state,
                [0, 1],
                "newline-event state invariant: an unaccepted Event must not run another handler"
            );
            assert_eq!(
                fatal.record_kind,
                RecordKind::EventAccepted,
                "newline-event record invariant: rejection must identify EventAccepted"
            );
            assert_eq!(
                fatal.outcome, None,
                "newline-event outcome invariant: EventAccepted must carry no outcome"
            );
            assert!(
                matches!(fatal.error, JournalError::NotAnObject),
                "newline-event error invariant: a literal newline must be NotAnObject"
            );
            assert_eq!(
                quiescence,
                Quiescence::Quiesced,
                "newline-event shutdown invariant: finalization must retain clean quiescence"
            );
            assert_eq!(
                golden.app_calls.len(),
                2,
                "newline-event handler invariant: the rejected Event must not reach on_event"
            );
            assert!(
                golden.handoffs.is_empty(),
                "newline-event handoff invariant: commandless turns must hand off nothing"
            );
            assert_eq!(
                golden.shutdown_count, 1,
                "newline-event shutdown invariant: the Environment must be shut down exactly once"
            );
            assert_eq!(
                golden.bytes.as_slice(),
                EXPECTED_PREFIX,
                "newline-event byte invariant: rejection must preserve the complete committed prefix"
            );
        }
    }

    mod fatal_tails {
        use super::*;

        /// Invariant: when the post-dispatch checkpoint reports an Error, every
        /// command remains handed off and `CommandsDispatched` is the final record.
        /// Design Doc: RUN-CHECKPOINT
        #[test]
        fn commands_dispatched_can_be_the_final_record() {
            const EXPECTED: &[u8] =
                br#"{"record_kind":"RunStarted","index":0,"schema_version":1,"logical_time":100}
{"record_kind":"CommandsPrepared","index":0,"commands":[10]}
{"record_kind":"CommandsDispatched","index":0}
"#;

            let golden = run_golden::<u8, u8>(
                Timestamp::from_nanos(100),
                vec![ScriptedTurn::new(1, vec![10], Outcome::Stop)],
                Vec::new(),
                vec![Ok(())],
                vec![Some("checkpoint failed")],
            );

            let (state, fatal, quiescence) = expect_environment_fatal(golden.exit);
            assert_eq!(
                state,
                [0, 1],
                "fatal-tail state invariant: the completed handler mutation must survive the checkpoint Error"
            );
            assert_eq!(
                fatal.error, "checkpoint failed",
                "fatal-tail error invariant: the checkpoint Error must remain the cause"
            );
            assert_eq!(
                fatal.operation,
                EnvironmentOperation::Checkpoint,
                "fatal-tail operation invariant: the Error must be localized to the checkpoint"
            );
            assert_eq!(
                quiescence,
                Quiescence::Quiesced,
                "fatal-tail shutdown invariant: finalization must retain clean quiescence"
            );
            assert_eq!(
                golden.handoffs,
                [10],
                "fatal-tail handoff invariant: the complete batch must be handed off before checkpoint"
            );
            assert_eq!(
                golden.shutdown_count, 1,
                "fatal-tail shutdown invariant: the Environment must be shut down exactly once"
            );
            assert_eq!(
                golden.bytes.as_slice(),
                EXPECTED,
                "fatal-tail byte invariant: CommandsDispatched must be the exact final committed record"
            );
            assert_eq!(
                GoldenLines::split(&golden.bytes).last().copied(),
                Some(b"{\"record_kind\":\"CommandsDispatched\",\"index\":0}\n".as_slice()),
                "fatal-tail record invariant: the Journal must end at CommandsDispatched"
            );
        }
    }

    mod graph_sequences {
        use super::*;

        /// Invariant: empty and nonempty command batches each follow their required
        /// record path for both Continue and Stop answers.
        /// Design Doc: VERIFY-JOURNAL
        #[test]
        fn every_empty_and_command_turn_shape_has_its_required_sequence() {
            struct Case {
                name: &'static str,
                commands: &'static [u8],
                answer: TurnOutcome,
                expected: &'static [u8],
            }

            let cases = [
                Case {
                    name: "empty Continue",
                    commands: &[],
                    answer: TurnOutcome::Continue,
                    expected: br#"{"record_kind":"RunStarted","index":0,"schema_version":1,"logical_time":100}
{"record_kind":"TurnCompleted","index":0,"outcome":"Continue"}
"#,
                },
                Case {
                    name: "nonempty Continue",
                    commands: &[10],
                    answer: TurnOutcome::Continue,
                    expected: br#"{"record_kind":"RunStarted","index":0,"schema_version":1,"logical_time":100}
{"record_kind":"CommandsPrepared","index":0,"commands":[10]}
{"record_kind":"CommandsDispatched","index":0}
{"record_kind":"TurnCompleted","index":0,"outcome":"Continue"}
"#,
                },
                Case {
                    name: "empty Stop",
                    commands: &[],
                    answer: TurnOutcome::Stop,
                    expected: br#"{"record_kind":"RunStarted","index":0,"schema_version":1,"logical_time":100}
{"record_kind":"StopRequested","index":0}
{"record_kind":"TurnCompleted","index":0,"outcome":"Stop"}
"#,
                },
                Case {
                    name: "nonempty Stop",
                    commands: &[10],
                    answer: TurnOutcome::Stop,
                    expected: br#"{"record_kind":"RunStarted","index":0,"schema_version":1,"logical_time":100}
{"record_kind":"CommandsPrepared","index":0,"commands":[10]}
{"record_kind":"CommandsDispatched","index":0}
{"record_kind":"StopRequested","index":0}
{"record_kind":"TurnCompleted","index":0,"outcome":"Stop"}
"#,
                },
            ];

            for case in cases {
                let (exit, bytes) = run_start_turn(case.commands.to_vec(), case.answer);
                match case.answer {
                    TurnOutcome::Continue => {
                        let (state, fatal, quiescence) = expect_environment_fatal(exit);
                        assert_eq!(
                            state,
                            [0, 1],
                            "graph-sequence state invariant: a Continue turn mutation must survive fixture termination"
                        );
                        assert_eq!(
                            fatal.operation,
                            EnvironmentOperation::NextEvent,
                            "graph-sequence termination invariant: Continue fixtures must end after completing the tested turn"
                        );
                        assert_eq!(
                            quiescence,
                            Quiescence::Quiesced,
                            "graph-sequence shutdown invariant: Continue fixture termination must quiesce"
                        );
                    }
                    TurnOutcome::Stop => assert_eq!(
                        stopped(exit),
                        [0, 1],
                        "graph-sequence state invariant: a Stop turn mutation must survive completion"
                    ),
                }
                assert_eq!(
                    bytes.as_slice(),
                    case.expected,
                    "graph-sequence byte invariant: the {} path must match its exact required records",
                    case.name
                );
            }
        }

        /// Invariant: a command-emitting event turn records the accepted Event, both
        /// command edges, and its completion at the Event's accepted index.
        #[test]
        fn an_event_turn_with_commands_uses_the_accepted_index() {
            const EXPECTED: &[u8] =
                br#"{"record_kind":"RunStarted","index":0,"schema_version":1,"logical_time":100}
{"record_kind":"TurnCompleted","index":0,"outcome":"Continue"}
{"record_kind":"EventAccepted","index":1,"logical_time":105,"event":7}
{"record_kind":"CommandsPrepared","index":1,"commands":[10]}
{"record_kind":"CommandsDispatched","index":1}
{"record_kind":"StopRequested","index":1}
{"record_kind":"TurnCompleted","index":1,"outcome":"Stop"}
"#;

            let golden = run_golden::<u8, u8>(
                Timestamp::from_nanos(100),
                vec![
                    ScriptedTurn::new(1, Vec::new(), Outcome::Continue),
                    ScriptedTurn::new(2, vec![10], Outcome::Stop),
                ],
                vec![Ok((7, Timestamp::from_nanos(105)))],
                vec![Ok(())],
                vec![None, None],
            );

            let state = stopped(golden.exit);
            assert_eq!(
                state,
                [0, 1, 2],
                "event-command state invariant: both turn mutations must survive completion"
            );
            assert_eq!(
                golden.app_calls,
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
                "event-command application invariant: the Event handler must observe the accepted index and time"
            );
            assert_eq!(
                golden.env_calls,
                [
                    EnvCall::Start(Ok(Timestamp::from_nanos(100))),
                    EnvCall::TakeError {
                        returned_error: false,
                    },
                    EnvCall::NextEvent(Ok((7, Timestamp::from_nanos(105)))),
                    EnvCall::Dispatch {
                        command: 10,
                        result: Ok(()),
                    },
                    EnvCall::TakeError {
                        returned_error: false,
                    },
                    EnvCall::Shutdown {
                        quiescence: Quiescence::Quiesced,
                        returned_error: false,
                    },
                ],
                "event-command environment invariant: acceptance, handoff, checkpoint, and shutdown must remain ordered"
            );
            assert_eq!(
                golden.handoffs,
                [10],
                "event-command handoff invariant: the Event turn's Command must be handed off exactly once"
            );
            assert_eq!(
                golden.bytes.as_slice(),
                EXPECTED,
                "event-command byte invariant: every Event-turn record must use the accepted index"
            );
        }
    }
}
