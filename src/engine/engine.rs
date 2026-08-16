use std::{collections::TryReserveError, num::NonZeroUsize};

use serde::Serialize;

use crate::{
    application::{Application, Context, Outcome},
    bounded_buffer::BoundedBuffer,
    environment::{Environment, Quiescence},
    journal::{Journal, JournalBuildError, JournalError},
    time::{EventIndex, Timestamp},
};

pub(crate) const SCHEMA_VERSION: u32 = 1;

pub struct Engine<A, E, W>
where
    A: Application,
    E: Environment,
    W: std::io::Write,
{
    journal: Journal<W>,
    app: A,
    env: E,
    cmd_buf: BoundedBuffer<A::Command>,
    max_turns: NonZeroUsize,
}

impl<A, E, W> Engine<A, E, W>
where
    A: Application,
    E: Environment<Event = A::Event, Command = A::Command>,
    W: std::io::Write,
{
    pub fn new(config: EngineConfig, app: A, env: E, writer: W) -> Result<Self, BuildError> {
        let cmd_buf = BoundedBuffer::new(config.max_commands_per_turn.get())
            .map_err(BuildError::CommandBuffer)?;
        let journal = Journal::new(writer, config.max_record_bytes).map_err(BuildError::Journal)?;

        Ok(Self {
            journal,
            app,
            env,
            cmd_buf,
            max_turns: config.max_turns,
        })
    }

    pub fn run(self) -> EngineExit<A::State, A::Fatal, E::Error> {
        // Makes the borrow checker happy to split things off this way
        let Self {
            mut journal,
            app,
            mut env,
            mut cmd_buf,
            max_turns,
        } = self;

        // Startup state
        let mut state = app.initial_state();
        let start_time = match env.start() {
            Ok(ts) => ts,
            Err(error) => {
                return EngineExit::Fatal {
                    state,
                    cause: FatalCause::Environment(EnvironmentFatal {
                        error: error,
                        operation: EnvironmentOperation::Start,
                    }),
                    quiescence: Quiescence::Quiesced,
                };
            }
        };

        // Startup 1: before any fallible step, so every exit path carries state.
        let mut state = app.initial_state();

        // From here on: started ∧ unconsumed ⇔ `Some` (FAIL-FINALIZE).
        let mut env = Some(env);
        let mut last_record_kind: Option<RecordKind> = None;

        // Startup 3
        let record: Record<'_, A::Event, A::Command> = Record::RunStarted {
            schema_version: SCHEMA_VERSION,
            logical_time: start_time,
        };
        if let Err(fatal) = Self::commit_record(
            &mut journal,
            &mut last_record_kind,
            &record,
            RecordKind::RunStarted,
        ) {
            return Self::fatal_exit(state, FatalCause::Journal(fatal), env);
        }

        // Startup 4: the start turn, at index 0, through the shared protocol.
        let mut index = EventIndex::START;
        let mut ctx = Context::new(index, start_time, &mut cmd_buf);
        let outcome = app.on_start(&mut state, &mut ctx);
        let overflowed = ctx.is_overflowed();
        match Self::process_turn(
            index,
            outcome,
            overflowed,
            &mut cmd_buf,
            &mut journal,
            &mut last_record_kind,
            &mut env,
        ) {
            TurnFlow::Continue => {}
            TurnFlow::Stop => return EngineExit::Stopped { state },
            TurnFlow::Fatal(cause) => return Self::fatal_exit(state, cause, env),
        }

        // `max_turns` counts accepted External Events, excluding the start turn.
        let mut accepted: usize = 0;
        let mut last_time = start_time;

        loop {
            // Acquisition 1: checked before `next_event` is called.
            if accepted >= self.max_turns.get() {
                return Self::fatal_exit(
                    state,
                    FatalCause::Core(CoreFatal::TurnBoundExceeded),
                    env,
                );
            }

            // Acquisition 2
            let (event, time) = match env
                .as_mut()
                .expect("kavod invariant violated: ENV-CALLS: environment consumed before event acquisition")
                .next_event()
            {
                Ok(candidate) => candidate,
                Err(error) => {
                    return Self::fatal_exit(
                        state,
                        FatalCause::Environment(EnvironmentFatal {
                            error,
                            operation: EnvironmentOperation::NextEvent,
                        }),
                        env,
                    );
                }
            };

            // Acquisition 3: against the last ACCEPTED time; equal is valid (ENV-TIME).
            // The candidate stays consumed either way — no retry, no handler.
            if time < last_time {
                return Self::fatal_exit(
                    state,
                    FatalCause::Core(CoreFatal::TimeRegression {
                        previous: last_time,
                        offered: time,
                    }),
                    env,
                );
            }

            // Acquisition 4: index == accepted < max_turns <= usize::MAX <= u64::MAX,
            // so overflow is unreachable behind acquisition step 1 (BOUND-INDEX).
            index = index
                .checked_next()
                .expect("kavod invariant violated: BOUND-INDEX: event index overflow");

            // Acquisition 5: on Err the candidate stays consumed but never becomes current.
            let record: Record<'_, A::Event, A::Command> = Record::EventAccepted {
                index,
                logical_time: time,
                event: &event,
            };
            if let Err(fatal) = Self::commit_record(
                &mut journal,
                &mut last_record_kind,
                &record,
                RecordKind::EventAccepted,
            ) {
                return Self::fatal_exit(state, FatalCause::Journal(fatal), env);
            }
            assert!(
                accepted < max_turns.get(),
                "kavod invariant violated: BOUND-LOOPS: accepting a turn past max_turns"
            );
            accepted = accepted
                .checked_add(1)
                .expect("kavod invariant violated: BOUND-LOOPS: accepted-turn counter overflow");
            last_time = time;

            // Acquisition 6: the same turn protocol as the start turn (A2).
            let mut ctx = Context::new(index, time, &mut cmd_buf);
            let outcome = app.on_event(&mut state, &event, &mut ctx);
            let overflowed = ctx.is_overflowed();
            match Self::process_turn(
                index,
                outcome,
                overflowed,
                &mut cmd_buf,
                &mut journal,
                &mut last_record_kind,
                &mut env,
            ) {
                TurnFlow::Continue => {}
                TurnFlow::Stop => return EngineExit::Stopped { state },
                TurnFlow::Fatal(cause) => return Self::fatal_exit(state, cause, env),
            }
        }
    }

    /// §8.4's turn-result protocol, orders 1–9b; `on_start` and `on_event`
    /// both land here (A2).
    fn process_turn(
        index: EventIndex,
        outcome: Outcome<A::Fatal>,
        overflowed: bool,
        cmd_buf: &mut BoundedBuffer<A::Command>,
        journal: &mut Journal<W>,
        last_kind: &mut Option<RecordKind>,
        env: &mut Option<E>,
    ) -> TurnFlow<A::Fatal, E::Error> {
        // Order 1: overflow beats every Outcome, including Fatal, whose payload
        // is discarded with the batch (APP-OVERFLOW).
        if overflowed {
            cmd_buf.clear();
            assert!(
                cmd_buf.is_empty(),
                "kavod invariant violated: APP-OVERFLOW: batch not discarded"
            );
            return TurnFlow::Fatal(FatalCause::Core(CoreFatal::CommandBoundExceeded));
        }

        // Order 2
        let outcome = match outcome {
            Outcome::Fatal(reason) => {
                cmd_buf.clear();
                assert!(
                    cmd_buf.is_empty(),
                    "kavod invariant violated: A3: batch not discarded on Application Fatal"
                );
                return TurnFlow::Fatal(FatalCause::Application(reason));
            }
            other => other,
        };

        // Captured before `drain` empties the buffer.
        let staged = !cmd_buf.is_empty();
        if staged {
            // Order 3: A5 — the complete Command intent commits before the
            // first handoff. The `as_slice` borrow ends at this call, before
            // `drain`, so serialize-before-handoff is compiler-enforced.
            let record: Record<'_, A::Event, A::Command> = Record::CommandsPrepared {
                index,
                commands: cmd_buf.as_slice(),
            };
            if let Err(fatal) =
                Self::commit_record(journal, last_kind, &record, RecordKind::CommandsPrepared)
            {
                cmd_buf.clear();
                return TurnFlow::Fatal(FatalCause::Journal(fatal));
            }

            // Order 4: an Err at `position` returns while the `Drain` is live,
            // dropping the undispatched suffix exactly once (A3); the prefix
            // `[0, position)` stands.
            for (position, command) in cmd_buf.drain().enumerate() {
                let dispatched = env
                    .as_mut()
                    .expect(
                        "kavod invariant violated: ENV-CALLS: environment consumed before dispatch",
                    )
                    .dispatch(command);
                if let Err(error) = dispatched {
                    return TurnFlow::Fatal(FatalCause::Environment(EnvironmentFatal {
                        error,
                        operation: EnvironmentOperation::Dispatch { position },
                    }));
                }
            }
            assert!(
                cmd_buf.is_empty(),
                "kavod invariant violated: PORT-HANDOFF: batch not empty after full dispatch"
            );

            // Order 5: a failure here leaves every handoff real.
            let record: Record<'_, A::Event, A::Command> = Record::CommandsDispatched { index };
            if let Err(fatal) =
                Self::commit_record(journal, last_kind, &record, RecordKind::CommandsDispatched)
            {
                return TurnFlow::Fatal(FatalCause::Journal(fatal));
            }
        }

        match outcome {
            // Order 6a: only this commit's success permits the next acquisition.
            Outcome::Continue => {
                let record: Record<'_, A::Event, A::Command> = Record::TurnCompleted {
                    index,
                    outcome: TurnOutcome::Continue,
                };
                match Self::commit_record(journal, last_kind, &record, RecordKind::TurnCompleted) {
                    Ok(()) => TurnFlow::Continue,
                    Err(fatal) => TurnFlow::Fatal(FatalCause::Journal(fatal)),
                }
            }
            Outcome::Stop => {
                // Order 6b: a failure here means graceful is never attempted.
                let record: Record<'_, A::Event, A::Command> = Record::StopRequested { index };
                if let Err(fatal) =
                    Self::commit_record(journal, last_kind, &record, RecordKind::StopRequested)
                {
                    return TurnFlow::Fatal(FatalCause::Journal(fatal));
                }

                // Order 7b: `take` consumes the Environment — a second
                // shutdown call is unrepresentable from here on.
                let graceful = env
                    .take()
                    .expect("kavod invariant violated: ENV-CALLS: environment consumed before graceful shutdown")
                    .shutdown(ShutdownMode::Graceful);
                if let Err(error) = graceful {
                    return TurnFlow::Fatal(FatalCause::Environment(EnvironmentFatal {
                        error,
                        operation: EnvironmentOperation::ShutdownGraceful,
                    }));
                }

                // Order 8b: `env` is None, so a failure here structurally
                // skips Abort in finalization.
                let record: Record<'_, A::Event, A::Command> = Record::TurnCompleted {
                    index,
                    outcome: TurnOutcome::Stop,
                };
                if let Err(fatal) =
                    Self::commit_record(journal, last_kind, &record, RecordKind::TurnCompleted)
                {
                    return TurnFlow::Fatal(FatalCause::Journal(fatal));
                }

                // Order 9b
                TurnFlow::Stop
            }
            Outcome::Fatal(_) => {
                unreachable!("kavod invariant violated: APP-OUTCOME: Fatal outcome past order 2")
            }
        }
    }

    /// Shuts down the environment and retuns `EngineExit::Fatal`  
    fn fatal_exit(
        state: A::State,
        cause: FatalCause<A::Fatal, E::Error>,
        env: E,
    ) -> EngineExit<A::State, A::Fatal, E::Error> {
        let quiescence = env.shutdown();

        EngineExit::Fatal {
            state,
            cause,
            quiescence,
        }
    }

    /// Commits one record, pairing any failure with the `RecordKind` it was
    /// evidencing, and asserts the §8.2 record grammar at every commit site:
    /// `RunStarted` first and only first, `CommandsDispatched` only after
    /// `CommandsPrepared`, `TurnCompleted(Stop)` only after `StopRequested`.
    fn commit_record<R: Serialize>(
        journal: &mut Journal<W>,
        last_kind: &mut Option<RecordKind>,
        record: &R,
        record_kind: RecordKind,
    ) -> Result<(), JournalFatal> {
        let grammatical = match record_kind {
            RecordKind::RunStarted => last_kind.is_none(),
            RecordKind::EventAccepted => matches!(
                last_kind,
                Some(RecordKind::RunStarted | RecordKind::TurnCompleted)
            ),
            RecordKind::CommandsPrepared => matches!(
                last_kind,
                Some(RecordKind::RunStarted | RecordKind::EventAccepted)
            ),
            RecordKind::CommandsDispatched => {
                matches!(last_kind, Some(RecordKind::CommandsPrepared))
            }
            RecordKind::StopRequested => matches!(
                last_kind,
                Some(
                    RecordKind::RunStarted
                        | RecordKind::EventAccepted
                        | RecordKind::CommandsDispatched
                )
            ),
            RecordKind::TurnCompleted => matches!(
                last_kind,
                Some(
                    RecordKind::RunStarted
                        | RecordKind::EventAccepted
                        | RecordKind::CommandsDispatched
                        | RecordKind::StopRequested
                )
            ),
        };
        assert!(
            grammatical,
            "kavod invariant violated: record grammar: {record_kind:?} after {last_kind:?}"
        );

        match journal.commit(record) {
            Ok(()) => {
                *last_kind = Some(record_kind);
                Ok(())
            }
            Err(error) => Err(JournalFatal { record_kind, error }),
        }
    }
}

enum TurnFlow<AF, EE> {
    Continue,
    Stop,
    Fatal(FatalCause<AF, EE>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EngineConfig {
    pub max_turns: NonZeroUsize,
    pub max_commands_per_turn: NonZeroUsize,
    pub max_record_bytes: NonZeroUsize,
}

#[derive(Debug)]
pub enum EngineExit<S, AF, EE> {
    Stopped {
        state: S,
    },
    Fatal {
        state: S,
        cause: FatalCause<AF, EE>,
        quiescence: Quiescence,
    },
}

#[derive(Debug)]
pub enum FatalCause<AF, EE> {
    Application(AF),
    Environment(EnvironmentFatal<EE>),
    Journal(JournalFatal),
    Core(CoreFatal),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreFatal {
    TimeRegression {
        previous: Timestamp,
        offered: Timestamp,
    },
    TurnBoundExceeded,
    CommandBoundExceeded,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildError {
    CommandBuffer(TryReserveError),
    Journal(JournalBuildError),
}

#[derive(Debug)]
pub struct JournalFatal {
    pub record_kind: RecordKind,
    pub error: JournalError,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordKind {
    RunStarted,
    EventAccepted,
    CommandsPrepared,
    CommandsDispatched,
    StopRequested,
    TurnCompleted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvironmentFatal<EE> {
    pub error: EE,
    pub operation: EnvironmentOperation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvironmentOperation {
    Start,
    NextEvent,
    /// Where in the dispatch loop the Error was observed — not necessarily
    /// this Command's own routing/admission; an unrelated already-latched
    /// failure can surface here too, per `ENV-LATCH`.
    Dispatch {
        position: usize,
    },
    ShutdownGraceful,
}

#[derive(Serialize)]
enum Record<'a, E, C> {
    RunStarted {
        schema_version: u32,
        logical_time: Timestamp,
    },
    EventAccepted {
        index: EventIndex,
        logical_time: Timestamp,
        event: &'a E,
    },
    CommandsPrepared {
        index: EventIndex,
        commands: &'a [C],
    },
    CommandsDispatched {
        index: EventIndex,
    },
    StopRequested {
        index: EventIndex,
    },
    TurnCompleted {
        index: EventIndex,
        outcome: TurnOutcome,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
enum TurnOutcome {
    Continue,
    Stop,
}
