mod support;

#[cfg(test)]
mod tests {
    use super::support::*;
    use kavod::{
        Engine, EngineConfig, EngineExit, Environment, FatalCause, Quiescence, ShutdownReport,
        Timestamp,
    };
    use std::io::{self, Write};
    use std::num::NonZeroUsize;
    use std::panic::{AssertUnwindSafe, catch_unwind};

    fn clean_shutdown() -> ShutdownReport<&'static str> {
        ShutdownReport {
            quiescence: Quiescence::Quiesced,
            error: None,
        }
    }

    fn config(max_commands_per_turn: usize) -> EngineConfig {
        EngineConfig {
            max_commands_per_turn: NonZeroUsize::new(max_commands_per_turn)
                .expect("a harness command bound must be nonzero"),
            max_record_bytes: NonZeroUsize::new(256)
                .expect("a harness record bound must be nonzero"),
        }
    }

    fn assert_panics(action: impl FnOnce()) {
        assert!(
            catch_unwind(AssertUnwindSafe(action)).is_err(),
            "an operation outside the scripted Environment graph must panic"
        );
    }

    mod scripted_environment_trace {
        use super::*;

        /// Invariant: every Environment operation records its returned payload or
        /// Error presence at the exact position where the call occurred.
        /// Design Doc: the Trace definition, by name
        #[test]
        fn records_each_operation_and_result_in_order() {
            let shutdown = ShutdownReport {
                quiescence: Quiescence::Incomplete,
                error: Some("shutdown error"),
            };
            let (mut environment, trace) = ScriptedEnv::<u8, u8, &'static str>::new(
                Ok(Timestamp::from_nanos(10)),
                [Ok((7, Timestamp::from_nanos(11)))],
                [Ok(()), Ok(())],
                [None, None],
                shutdown,
            );

            assert_eq!(
                environment.start(),
                Ok(Timestamp::from_nanos(10)),
                "startup must return its scripted timestamp"
            );
            environment
                .dispatch(1)
                .expect("the first scripted handoff must succeed");
            environment
                .dispatch(2)
                .expect("the second scripted handoff must succeed");
            assert_eq!(
                environment.take_error(),
                None,
                "the first checkpoint must return its scripted empty result"
            );
            assert_eq!(
                environment.next_event(),
                Ok((7, Timestamp::from_nanos(11))),
                "event selection must return its scripted payload and timestamp"
            );
            assert_eq!(
                environment.take_error(),
                None,
                "the second checkpoint must return its scripted empty result"
            );
            let report = environment.shutdown();

            assert_eq!(
                report.quiescence,
                Quiescence::Incomplete,
                "shutdown must return its scripted quiescence"
            );
            assert_eq!(
                report.error,
                Some("shutdown error"),
                "shutdown must return its scripted Error"
            );
            assert_eq!(
                trace.borrow().calls,
                [
                    EnvCall::Start(Ok(Timestamp::from_nanos(10))),
                    EnvCall::Dispatch {
                        command: 1,
                        result: Ok(()),
                    },
                    EnvCall::Dispatch {
                        command: 2,
                        result: Ok(()),
                    },
                    EnvCall::TakeError {
                        returned_error: false,
                    },
                    EnvCall::NextEvent(Ok((7, Timestamp::from_nanos(11)))),
                    EnvCall::TakeError {
                        returned_error: false,
                    },
                    EnvCall::Shutdown {
                        quiescence: TraceQuiescence::Incomplete,
                        returned_error: true,
                    },
                ],
                "the Environment trace must retain every operation and result in call order"
            );
            assert_eq!(
                trace.borrow().handoffs,
                [1, 2],
                "each successful dispatch must record exactly one handoff"
            );
            assert_eq!(
                trace.borrow().shutdown_count,
                1,
                "consuming shutdown must be recorded exactly once"
            );
        }

        /// Invariant: a Command whose dispatch returns an Error is recorded as an
        /// attempted call but never as a completed handoff.
        /// Design Doc: the dispatch commitment row, by name
        #[test]
        fn a_failed_dispatch_records_no_handoff() {
            let (mut environment, trace) = ScriptedEnv::<u8, u8, &'static str>::new(
                Ok(Timestamp::from_nanos(0)),
                [],
                [Ok(()), Err("dispatch failed")],
                [],
                clean_shutdown(),
            );
            environment
                .start()
                .expect("the dispatch fixture must start successfully");
            environment
                .dispatch(3)
                .expect("the first dispatch must establish one successful handoff");

            assert_eq!(
                environment.dispatch(5),
                Err("dispatch failed"),
                "the second dispatch must return its scripted Error"
            );
            let _report = environment.shutdown();

            assert_eq!(
                trace.borrow().handoffs,
                [3],
                "a failed dispatch must leave the prior handoffs unchanged"
            );
            assert_eq!(
                trace.borrow().calls[2],
                EnvCall::Dispatch {
                    command: 5,
                    result: Err(()),
                },
                "the failed dispatch call must preserve its position without becoming a handoff"
            );
        }

        /// Invariant: operation Errors are represented in the trace without
        /// retaining or comparing the component-specific Error values.
        #[test]
        fn records_every_error_result_by_presence() {
            let (mut start_failure, start_trace) = ScriptedEnv::<u8, u8, &'static str>::new(
                Err("start failed"),
                [],
                [],
                [],
                clean_shutdown(),
            );
            assert_eq!(
                start_failure.start(),
                Err("start failed"),
                "startup must return its scripted Error"
            );
            assert_eq!(
                start_trace.borrow().calls,
                [EnvCall::Start(Err(()))],
                "a startup Error must retain its operation position and erase its value"
            );

            let (mut event_failure, event_trace) = ScriptedEnv::<u8, u8, &'static str>::new(
                Ok(Timestamp::from_nanos(0)),
                [Err("event failed")],
                [],
                [None],
                clean_shutdown(),
            );
            event_failure
                .start()
                .expect("the event failure fixture must start");
            assert_eq!(
                event_failure.take_error(),
                None,
                "the event failure fixture must first pass its checkpoint"
            );
            assert_eq!(
                event_failure.next_event(),
                Err("event failed"),
                "event selection must return its scripted Error"
            );
            let _report = event_failure.shutdown();
            assert_eq!(
                event_trace.borrow().calls[2],
                EnvCall::NextEvent(Err(())),
                "a next-event Error must retain its operation position and erase its value"
            );

            let (mut checkpoint_failure, checkpoint_trace) =
                ScriptedEnv::<u8, u8, &'static str>::new(
                    Ok(Timestamp::from_nanos(0)),
                    [],
                    [],
                    [Some("pending")],
                    clean_shutdown(),
                );
            checkpoint_failure
                .start()
                .expect("the checkpoint failure fixture must start");
            assert_eq!(
                checkpoint_failure.take_error(),
                Some("pending"),
                "the checkpoint must return its scripted pending Error"
            );
            let _report = checkpoint_failure.shutdown();
            assert_eq!(
                checkpoint_trace.borrow().calls[1],
                EnvCall::TakeError {
                    returned_error: true,
                },
                "a pending Error must be recorded at its checkpoint"
            );
        }
    }

    mod scripted_environment_graph {
        use super::*;

        /// Invariant: shutdown is rejected both before startup and after startup
        /// fails, because neither position owns an activated Environment to close.
        #[test]
        fn shutdown_rejects_before_start_and_after_failed_start() {
            let (before_start, _) = ScriptedEnv::<u8, u8, &'static str>::new(
                Ok(Timestamp::from_nanos(0)),
                [],
                [],
                [],
                clean_shutdown(),
            );
            assert_panics(|| {
                let _report = before_start.shutdown();
            });

            let (mut start_failed, _) = ScriptedEnv::<u8, u8, &'static str>::new(
                Err("start failed"),
                [],
                [],
                [],
                clean_shutdown(),
            );
            assert_eq!(
                start_failed.start(),
                Err("start failed"),
                "the failed-start fixture must enter its terminal startup phase"
            );
            assert_panics(|| {
                let _report = start_failed.shutdown();
            });
        }

        /// Invariant: startup, turn work, checkpointing, event selection, and
        /// shutdown reject calls made outside their legal serial positions.
        #[test]
        fn rejects_operations_outside_the_environment_graph() {
            let (mut before_start, _) = ScriptedEnv::<u8, u8, &'static str>::new(
                Ok(Timestamp::from_nanos(0)),
                [],
                [Ok(())],
                [],
                clean_shutdown(),
            );
            assert_panics(|| {
                let _ = before_start.dispatch(1);
            });

            let (mut repeated_start, _) = ScriptedEnv::<u8, u8, &'static str>::new(
                Ok(Timestamp::from_nanos(0)),
                [],
                [],
                [],
                clean_shutdown(),
            );
            repeated_start
                .start()
                .expect("the first startup call must succeed");
            assert_panics(|| {
                let _ = repeated_start.start();
            });

            let (mut before_checkpoint, _) = ScriptedEnv::<u8, u8, &'static str>::new(
                Ok(Timestamp::from_nanos(0)),
                [Ok((1, Timestamp::from_nanos(1)))],
                [],
                [],
                clean_shutdown(),
            );
            before_checkpoint
                .start()
                .expect("the event-order fixture must start");
            assert_panics(|| {
                let _ = before_checkpoint.next_event();
            });

            let (mut after_checkpoint, _) = ScriptedEnv::<u8, u8, &'static str>::new(
                Ok(Timestamp::from_nanos(0)),
                [],
                [Ok(())],
                [None],
                clean_shutdown(),
            );
            after_checkpoint
                .start()
                .expect("the checkpoint-order fixture must start");
            assert_eq!(
                after_checkpoint.take_error(),
                None,
                "the checkpoint-order fixture must reach its checkpointed phase"
            );
            assert_panics(|| {
                let _ = after_checkpoint.dispatch(1);
            });

            let (mut after_failure, trace) = ScriptedEnv::<u8, u8, &'static str>::new(
                Ok(Timestamp::from_nanos(0)),
                [],
                [Err("dispatch failed")],
                [None],
                clean_shutdown(),
            );
            after_failure
                .start()
                .expect("the post-failure fixture must start");
            assert_eq!(
                after_failure.dispatch(1),
                Err("dispatch failed"),
                "the post-failure fixture must enter its terminal phase"
            );
            assert_panics(|| {
                let _ = after_failure.take_error();
            });
            let _report = after_failure.shutdown();
            assert_eq!(
                trace.borrow().shutdown_count,
                1,
                "shutdown must remain legal after a post-start operation failure"
            );
        }

        /// Invariant: each event selection removes exactly one result from its
        /// script and a later unscripted call cannot reuse it.
        #[test]
        fn each_event_result_is_consumed_exactly_once() {
            let (mut environment, _) = ScriptedEnv::<u8, u8, &'static str>::new(
                Ok(Timestamp::from_nanos(0)),
                [Ok((1, Timestamp::from_nanos(1)))],
                [],
                [None, None],
                clean_shutdown(),
            );
            environment
                .start()
                .expect("the one-event fixture must start");
            assert_eq!(
                environment.take_error(),
                None,
                "the first turn must pass its checkpoint"
            );
            environment
                .next_event()
                .expect("the sole scripted Event must be returned once");
            assert_eq!(
                environment.take_error(),
                None,
                "the event turn must pass its checkpoint"
            );
            assert_panics(|| {
                let _ = environment.next_event();
            });
        }
    }

    mod scripted_sink_trace {
        use super::*;

        /// Invariant: when a sink reports a successful partial write, only that
        /// exact input prefix is retained as accepted bytes.
        /// Design Doc: TRUST-SINK
        #[test]
        fn stores_exactly_the_reported_prefix() {
            let (mut sink, trace) =
                ScriptedSink::new([SinkStep::Write(Ok(2)), SinkStep::Write(Ok(1))]);

            assert_eq!(
                sink.write(b"abcd")
                    .expect("the first partial write must succeed"),
                2,
                "the first write must report its scripted two-byte prefix"
            );
            assert_eq!(
                sink.write(b"cd")
                    .expect("the second partial write must succeed"),
                1,
                "the second write must report its scripted one-byte prefix"
            );

            assert_eq!(
                trace.borrow().accepted_bytes(),
                b"abc",
                "accepted bytes must concatenate exactly the two reported prefixes"
            );
            assert_eq!(
                trace.borrow().calls,
                [
                    SinkCall::Write {
                        bytes: b"abcd".to_vec(),
                        result: Ok(2),
                    },
                    SinkCall::Write {
                        bytes: b"cd".to_vec(),
                        result: Ok(1),
                    },
                ],
                "the sink trace must preserve each offered buffer and returned count"
            );
        }

        /// Invariant: zero progress, a returned Error, and an impossible
        /// over-report never cause the scripted sink to invent accepted bytes.
        #[test]
        fn invalid_or_failed_write_results_capture_no_bytes() {
            let cases = [
                SinkStep::Write(Ok(0)),
                SinkStep::Write(Err(io::Error::other("write failed"))),
                SinkStep::Write(Ok(4)),
            ];

            for step in cases {
                let (mut sink, trace) = ScriptedSink::new([step]);
                let _result = sink.write(b"abc");
                assert!(
                    trace.borrow().accepted_bytes().is_empty(),
                    "a non-accepting write result must leave captured bytes empty"
                );
            }
        }

        /// Invariant: reported counts at one byte and at the full input length
        /// retain exactly those boundary-sized prefixes.
        #[test]
        fn one_byte_and_full_writes_capture_their_exact_boundaries() {
            let (mut sink, trace) =
                ScriptedSink::new([SinkStep::Write(Ok(1)), SinkStep::Write(Ok(3))]);

            assert_eq!(
                sink.write(b"abc")
                    .expect("the one-byte boundary write must succeed"),
                1,
                "the one-byte boundary write must report one accepted byte"
            );
            assert_eq!(
                sink.write(b"def")
                    .expect("the full boundary write must succeed"),
                3,
                "the full boundary write must report every offered byte"
            );

            assert_eq!(
                trace.borrow().accepted_bytes(),
                b"adef",
                "boundary writes must retain one byte followed by the complete input"
            );
        }

        /// Invariant: successful flushes advance the committed boundary, while a
        /// failed flush leaves subsequently accepted bytes in the uncertain suffix.
        #[test]
        fn flush_results_preserve_committed_and_uncertain_regions() {
            let (mut sink, trace) = ScriptedSink::new([
                SinkStep::Write(Ok(2)),
                SinkStep::Flush(Ok(())),
                SinkStep::Write(Ok(1)),
                SinkStep::Flush(Err(io::Error::other("flush failed"))),
            ]);

            assert_eq!(
                sink.write(b"ab").expect("the committed write must succeed"),
                2,
                "the committed write must report both offered bytes"
            );
            sink.flush().expect("the first flush must commit its bytes");
            assert_eq!(
                sink.write(b"c").expect("the uncertain write must succeed"),
                1,
                "the uncertain write must report its one offered byte"
            );
            assert_eq!(
                sink.flush().map_err(|error| error.kind()),
                Err(io::ErrorKind::Other),
                "the second flush must return its scripted Error"
            );

            assert_eq!(
                trace.borrow().committed_bytes(),
                b"ab",
                "a failed later flush must retain the prior committed prefix"
            );
            assert_eq!(
                trace.borrow().uncertain_suffix(),
                b"c",
                "bytes accepted after the last successful flush must remain uncertain"
            );
            assert_eq!(
                trace.borrow().calls[3],
                SinkCall::Flush { result: Err(()) },
                "the failed flush must retain its call position and Error presence"
            );
        }
    }

    mod scripted_sink_script {
        use super::*;

        /// Invariant: a write cannot consume a result scripted for a flush call.
        #[test]
        #[should_panic(expected = "a sink write call must consume a write result")]
        fn a_write_rejects_a_flush_result() {
            let (mut sink, _) = ScriptedSink::new([SinkStep::Flush(Ok(()))]);
            let _result = sink.write(b"value");
        }

        /// Invariant: a flush cannot consume a result scripted for a write call.
        #[test]
        #[should_panic(expected = "a sink flush call must consume a flush result")]
        fn a_flush_rejects_a_write_result() {
            let (mut sink, _) = ScriptedSink::new([SinkStep::Write(Ok(1))]);
            let _result = sink.flush();
        }

        /// Invariant: a sink result can satisfy only one call and cannot be reused
        /// after it has been consumed.
        #[test]
        #[should_panic(expected = "each sink call must consume exactly one scripted result")]
        fn a_scripted_result_is_consumed_exactly_once() {
            let (mut sink, _) = ScriptedSink::new([SinkStep::Write(Ok(1))]);
            assert_eq!(
                sink.write(b"a")
                    .expect("the sole scripted write must succeed once"),
                1,
                "the sole scripted write must report its one accepted byte"
            );
            let _result = sink.write(b"b");
        }
    }

    mod recording_application_behavior {
        use super::*;

        /// Invariant: scripted handlers record their context, mutate State, and
        /// emit Commands in the order supplied by their respective turns.
        #[test]
        fn records_handlers_mutates_state_and_emits_in_order() {
            let turns = [
                ScriptedTurn::new(1, vec![10], ScriptedAnswer::Continue),
                ScriptedTurn::new(2, vec![20, 21], ScriptedAnswer::Stop),
            ];
            let (app, app_trace) = RecordingApp::<u8, u8, &'static str>::new(vec![0], turns);
            let (environment, env_trace) = ScriptedEnv::new(
                Ok(Timestamp::from_nanos(100)),
                [Ok((7, Timestamp::from_nanos(105)))],
                [Ok(()), Ok(()), Ok(())],
                [None, None],
                clean_shutdown(),
            );
            let mut bytes = Vec::new();
            let engine = Engine::new(config(2), app, environment, &mut bytes)
                .unwrap_or_else(|_| panic!("the recording Application fixture must construct"));

            let state = match engine.run() {
                EngineExit::Stopped { state } => state,
                EngineExit::Fatal { .. } => {
                    panic!("the Continue-then-Stop Application script must stop cleanly")
                }
            };

            assert_eq!(
                state,
                [0, 1, 2],
                "each handler must append its scripted mutation to State"
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
                "handler calls must preserve their turn order and exact Context values"
            );
            assert_eq!(
                env_trace.borrow().handoffs,
                [10, 20, 21],
                "Commands emitted by both handlers must be handed off in script order"
            );
        }

        /// Invariant: a scripted Fatal answer returns its Error after preserving
        /// the handler's State mutation and without handing off its staged Commands.
        #[test]
        fn fatal_answer_preserves_mutation_without_command_handoffs() {
            let turns = [ScriptedTurn::new(
                9,
                vec![30],
                ScriptedAnswer::Fatal("application failed"),
            )];
            let (app, _) = RecordingApp::<u8, u8, &'static str>::new(vec![1], turns);
            let (environment, env_trace) =
                ScriptedEnv::new(Ok(Timestamp::from_nanos(10)), [], [], [], clean_shutdown());
            let mut bytes = Vec::new();
            let engine = Engine::new(config(1), app, environment, &mut bytes)
                .unwrap_or_else(|_| panic!("the fatal Application fixture must construct"));

            match engine.run() {
                EngineExit::Fatal {
                    state,
                    cause: FatalCause::Application(error),
                    quiescence,
                } => {
                    assert_eq!(
                        state,
                        [1, 9],
                        "the Fatal handler's mutation must remain in returned State"
                    );
                    assert_eq!(
                        error, "application failed",
                        "the Engine must return the scripted Application Error"
                    );
                    assert_eq!(
                        quiescence,
                        Quiescence::Quiesced,
                        "fatal finalization must retain the scripted clean shutdown account"
                    );
                }
                _ => panic!("a scripted Fatal answer must produce an Application fatal exit"),
            }
            assert!(
                env_trace.borrow().handoffs.is_empty(),
                "Commands staged by a Fatal handler must never become handoffs"
            );
        }

        /// Invariant: one scripted Application turn can satisfy only one handler
        /// invocation and cannot be replayed by a later turn.
        #[test]
        #[should_panic(
            expected = "each Application handler call must consume exactly one scripted turn"
        )]
        fn a_scripted_turn_is_consumed_exactly_once() {
            let turns = [ScriptedTurn::new(
                1,
                Vec::<u8>::new(),
                ScriptedAnswer::<&'static str>::Continue,
            )];
            let (app, _) = RecordingApp::<u8, u8, &'static str>::new(Vec::new(), turns);
            let (environment, _) = ScriptedEnv::new(
                Ok(Timestamp::from_nanos(0)),
                [Ok((1, Timestamp::from_nanos(1)))],
                [],
                [None],
                clean_shutdown(),
            );
            let mut bytes = Vec::new();
            let engine = Engine::new(config(1), app, environment, &mut bytes)
                .unwrap_or_else(|_| panic!("the exhausted Application fixture must construct"));
            let _exit = engine.run();
        }
    }

    mod golden_line_helpers {
        use super::*;

        /// Invariant: empty, one-line, and multi-line Journal byte sequences split
        /// into exact newline-inclusive slices without changing any byte.
        #[test]
        fn preserves_zero_one_and_multiple_complete_lines() {
            assert!(
                GoldenLines::split(b"").is_empty(),
                "empty Journal bytes must contain no lines"
            );
            assert_eq!(
                GoldenLines::split(b"first\n"),
                [b"first\n".as_slice()],
                "one complete line must remain byte-exact"
            );
            assert_eq!(
                GoldenLines::split(b"first\nsecond\n"),
                [b"first\n".as_slice(), b"second\n".as_slice()],
                "multiple complete lines must retain order and newline bytes"
            );
        }

        /// Invariant: a Journal byte sequence ending inside a record is rejected
        /// instead of being treated as a complete golden line.
        #[test]
        #[should_panic(
            expected = "golden Journal bytes must be empty or end at a newline boundary"
        )]
        fn rejects_an_unterminated_suffix() {
            let _lines = GoldenLines::split(b"incomplete");
        }
    }
}
