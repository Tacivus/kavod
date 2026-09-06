#[allow(dead_code, unused_imports)]
mod support;

#[cfg(test)]
mod tests {
    use super::support::{
        AppCall, EnvCall, GoldenLines, RecordingApp, ScriptedAnswer, ScriptedEnv, ScriptedTurn,
        TraceQuiescence,
    };
    use kavod::{Engine, EngineConfig, EngineExit, Quiescence, ShutdownReport, Timestamp};
    use std::num::NonZeroUsize;

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

            let (app, app_trace) = RecordingApp::<u8, u8, &'static str>::new(
                vec![0],
                [ScriptedTurn::new(1, Vec::<u8>::new(), ScriptedAnswer::Stop)],
            );
            let (environment, env_trace) = ScriptedEnv::<u8, u8, &'static str>::new(
                Ok(Timestamp::from_nanos(100)),
                [],
                [],
                [None],
                clean_shutdown(),
            );
            let mut bytes = Vec::new();
            let engine = Engine::new(config(), app, environment, &mut bytes).unwrap_or_else(|_| {
                panic!("stop-run construction invariant: the Engine must construct")
            });

            let state = match engine.run() {
                EngineExit::Stopped { state } => state,
                EngineExit::Fatal { .. } => {
                    panic!("stop-run exit invariant: a clean Stop answer must return Stopped")
                }
            };

            assert_eq!(
                state,
                [0, 1],
                "stop-run state invariant: the sole start-turn mutation must be retained"
            );
            assert_eq!(
                app_trace.borrow().calls,
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
                env_trace.borrow().calls,
                [
                    EnvCall::Start(Ok(Timestamp::from_nanos(100))),
                    EnvCall::TakeError {
                        returned_error: false,
                    },
                    EnvCall::Shutdown {
                        quiescence: TraceQuiescence::Quiesced,
                        returned_error: false,
                    },
                ],
                "stop-run environment invariant: the complete Environment script must be consumed"
            );
            assert!(
                env_trace.borrow().handoffs.is_empty(),
                "stop-run handoff invariant: a commandless turn must hand off no Commands"
            );
            assert_eq!(
                env_trace.borrow().shutdown_count,
                1,
                "stop-run shutdown invariant: the Environment must be shut down exactly once"
            );
            assert_eq!(
                GoldenLines::split(&bytes).len(),
                3,
                "stop-run record invariant: the Journal must contain three complete lines"
            );
            assert_eq!(
                bytes.as_slice(),
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

            let (app, app_trace) = RecordingApp::<u8, u8, &'static str>::new(
                vec![0],
                [ScriptedTurn::new(1, vec![10, 11], ScriptedAnswer::Stop)],
            );
            let (environment, env_trace) = ScriptedEnv::<u8, u8, &'static str>::new(
                Ok(Timestamp::from_nanos(100)),
                [],
                [Ok(()), Ok(())],
                [None],
                clean_shutdown(),
            );
            let mut bytes = Vec::new();
            let engine = Engine::new(config(), app, environment, &mut bytes).unwrap_or_else(|_| {
                panic!("command-run construction invariant: the Engine must construct")
            });

            let state = match engine.run() {
                EngineExit::Stopped { state } => state,
                EngineExit::Fatal { .. } => panic!(
                    "command-run exit invariant: an exact-capacity command turn must return Stopped"
                ),
            };

            assert_eq!(
                state,
                [0, 1],
                "command-run state invariant: the sole start-turn mutation must be retained"
            );
            assert_eq!(
                app_trace.borrow().calls,
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
                env_trace.borrow().calls,
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
                        quiescence: TraceQuiescence::Quiesced,
                        returned_error: false,
                    },
                ],
                "command-run environment invariant: the complete Environment script must be consumed"
            );
            assert_eq!(
                env_trace.borrow().handoffs,
                [10, 11],
                "command-run handoff invariant: every Command must be handed off once in order"
            );
            assert_eq!(
                env_trace.borrow().shutdown_count,
                1,
                "command-run shutdown invariant: the Environment must be shut down exactly once"
            );
            assert_eq!(
                GoldenLines::split(&bytes).len(),
                5,
                "command-run record invariant: the Journal must contain five complete lines"
            );
            assert_eq!(
                bytes.as_slice(),
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

            let (app, app_trace) = RecordingApp::<u8, u8, &'static str>::new(
                vec![0],
                [
                    ScriptedTurn::new(1, Vec::<u8>::new(), ScriptedAnswer::Continue),
                    ScriptedTurn::new(2, Vec::<u8>::new(), ScriptedAnswer::Stop),
                ],
            );
            let (environment, env_trace) = ScriptedEnv::<u8, u8, &'static str>::new(
                Ok(Timestamp::from_nanos(100)),
                [Ok((7, Timestamp::from_nanos(105)))],
                [],
                [None, None],
                clean_shutdown(),
            );
            let mut bytes = Vec::new();
            let engine = Engine::new(config(), app, environment, &mut bytes).unwrap_or_else(|_| {
                panic!("event-run construction invariant: the Engine must construct")
            });

            let state = match engine.run() {
                EngineExit::Stopped { state } => state,
                EngineExit::Fatal { .. } => {
                    panic!("event-run exit invariant: a clean event-turn Stop must return Stopped")
                }
            };

            assert_eq!(
                state,
                [0, 1, 2],
                "event-run state invariant: both turn mutations must be retained"
            );
            assert_eq!(
                app_trace.borrow().calls,
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
                env_trace.borrow().calls,
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
                        quiescence: TraceQuiescence::Quiesced,
                        returned_error: false,
                    },
                ],
                "event-run environment invariant: the complete Environment script must be consumed"
            );
            assert!(
                env_trace.borrow().handoffs.is_empty(),
                "event-run handoff invariant: commandless turns must hand off no Commands"
            );
            assert_eq!(
                env_trace.borrow().shutdown_count,
                1,
                "event-run shutdown invariant: the Environment must be shut down exactly once"
            );
            assert_eq!(
                GoldenLines::split(&bytes).len(),
                5,
                "event-run record invariant: the Journal must contain five complete lines"
            );
            assert_eq!(
                bytes.as_slice(),
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

            let (app, app_trace) = RecordingApp::<u8, u8, &'static str>::new(
                vec![0],
                [
                    ScriptedTurn::new(1, Vec::<u8>::new(), ScriptedAnswer::Continue),
                    ScriptedTurn::new(2, Vec::<u8>::new(), ScriptedAnswer::Continue),
                    ScriptedTurn::new(3, Vec::<u8>::new(), ScriptedAnswer::Stop),
                ],
            );
            let (environment, env_trace) = ScriptedEnv::<u8, u8, &'static str>::new(
                Ok(Timestamp::from_nanos(0)),
                [
                    Ok((7, Timestamp::from_nanos(0))),
                    Ok((8, Timestamp::from_nanos(1))),
                ],
                [],
                [None, None, None],
                clean_shutdown(),
            );
            let mut bytes = Vec::new();
            let engine = Engine::new(config(), app, environment, &mut bytes).unwrap_or_else(|_| {
                panic!("repeated-event construction invariant: the Engine must construct")
            });

            let state = match engine.run() {
                EngineExit::Stopped { state } => state,
                EngineExit::Fatal { .. } => panic!(
                    "repeated-event exit invariant: two clean Events ending in Stop must return Stopped"
                ),
            };

            assert_eq!(
                state,
                [0, 1, 2, 3],
                "repeated-event state invariant: all three turn mutations must be retained"
            );
            assert_eq!(
                app_trace.borrow().calls,
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
                env_trace.borrow().calls,
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
                        quiescence: TraceQuiescence::Quiesced,
                        returned_error: false,
                    },
                ],
                "repeated-event environment invariant: both Events and all checkpoints must be consumed in order"
            );
            assert!(
                env_trace.borrow().handoffs.is_empty(),
                "repeated-event handoff invariant: commandless turns must hand off no Commands"
            );
            assert_eq!(
                env_trace.borrow().shutdown_count,
                1,
                "repeated-event shutdown invariant: the Environment must be shut down exactly once"
            );
            assert_eq!(
                GoldenLines::split(&bytes).len(),
                7,
                "repeated-event record invariant: the Journal must contain seven complete lines"
            );
            assert_eq!(
                bytes.as_slice(),
                EXPECTED,
                "repeated-event byte invariant: every repeated Event record must match the golden sequence"
            );
        }
    }
}
