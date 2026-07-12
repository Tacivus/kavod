use std::sync::Arc;

use thiserror::Error;

use crate::{
    message::{Message, SharedMessage},
    schedule::Scheduler,
    sequence::Sequencer,
    time::Timestamp,
};

/// Errors from actor production through [`ActorCtx`](crate::context::actor::ActorCtx).
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ActorOutputError {
    /// Callback sent a type not declared via `.produces::<M>()`.
    #[error("undeclared production: actor callback did not declare output of type `{type_name}`")]
    UndeclaredProduction { type_name: &'static str },

    /// `send_at` requested a timestamp before the message's dispatch time.
    #[error(
        "event scheduled in the past: requested {requested}, current dispatch time is {current}"
    )]
    PastEvent {
        requested: Timestamp,
        current: Timestamp,
    },

    /// The kernel sequence counter overflowed.
    #[error("sequence exhausted")]
    SequenceExhaustion,
}

/// One actor emission reported to the selected executor.
///
/// The actor never assigns kernel sequence. Immediate emissions carry no
/// final live dispatch timestamp — the kernel stamps receipt on ingress.
/// `At` carries only the user-requested time for later kernel validation.
pub(crate) enum ActorEmission {
    Immediate {
        payload: SharedMessage,
    },
    At {
        requested_time: Timestamp,
        payload: SharedMessage,
    },
}

/// Executor-owned sink for actor outputs.
///
/// Backtest and live executors implement this. Phase 17 tests use
/// [`RecordingActorSink`].
pub(crate) trait ActorOutputSink {
    fn emit(&mut self, emission: ActorEmission) -> Result<(), ActorOutputError>;
}

/// Backtest inline executor: schedule through the kernel heap + sequencer.
///
/// Immediate emissions are received at the current dispatch time (design §7.8 / §15.9).
/// No threads or channels. Does not dispatch recursively.
pub(crate) struct InlineActorSink<'a> {
    scheduler: &'a mut Scheduler,
    sequence: &'a mut Sequencer,
    dispatch_time: Timestamp,
}

impl<'a> InlineActorSink<'a> {
    pub(crate) fn new(
        scheduler: &'a mut Scheduler,
        sequence: &'a mut Sequencer,
        dispatch_time: Timestamp,
    ) -> Self {
        Self {
            scheduler,
            sequence,
            dispatch_time,
        }
    }
}

impl ActorOutputSink for InlineActorSink<'_> {
    fn emit(&mut self, emission: ActorEmission) -> Result<(), ActorOutputError> {
        match emission {
            ActorEmission::Immediate { payload } => {
                let seq = self
                    .sequence
                    .next()
                    .map_err(|_| ActorOutputError::SequenceExhaustion)?;
                self.scheduler.push_shared(self.dispatch_time, seq, payload);
            }
            ActorEmission::At {
                requested_time,
                payload,
            } => {
                // ActorCtx already rejects requested_time < input dispatch_time.
                if requested_time < self.dispatch_time {
                    return Err(ActorOutputError::PastEvent {
                        requested: requested_time,
                        current: self.dispatch_time,
                    });
                }
                let seq = self
                    .sequence
                    .next()
                    .map_err(|_| ActorOutputError::SequenceExhaustion)?;
                self.scheduler.push_shared(requested_time, seq, payload);
            }
        }
        Ok(())
    }
}

/// Test / diagnostic sink that records emissions in order.
#[cfg(test)]
pub(crate) struct RecordingActorSink {
    pub emissions: Vec<ActorEmission>,
}

#[cfg(test)]
impl RecordingActorSink {
    pub(crate) fn new() -> Self {
        Self {
            emissions: Vec::new(),
        }
    }
}

#[cfg(test)]
impl ActorOutputSink for RecordingActorSink {
    fn emit(&mut self, emission: ActorEmission) -> Result<(), ActorOutputError> {
        self.emissions.push(emission);
        Ok(())
    }
}

/// Shared helpers used by [`ActorCtx`](crate::context::actor::ActorCtx).
pub(crate) fn wrap_message<M: Message>(msg: M) -> SharedMessage {
    Arc::new(msg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sequence::SeqNo;
    use std::any::Any;

    // ========================================================================
    // Test message types
    // ========================================================================

    #[derive(Debug, Clone, PartialEq)]
    struct TestMsg(u64);

    impl Message for TestMsg {}

    // ========================================================================
    // Recording sink
    // ========================================================================

    /// Invariant: recording sink preserves emission order
    #[test]
    fn test_recording_sink_preserves_order() {
        let mut sink = RecordingActorSink::new();
        sink.emit(ActorEmission::Immediate {
            payload: wrap_message(TestMsg(1)),
        })
        .unwrap();
        sink.emit(ActorEmission::At {
            requested_time: Timestamp::new(200),
            payload: wrap_message(TestMsg(2)),
        })
        .unwrap();

        assert_eq!(sink.emissions.len(), 2);
        match &sink.emissions[0] {
            ActorEmission::Immediate { payload } => {
                let p: &dyn Any = &**payload;
                assert_eq!(p.downcast_ref::<TestMsg>(), Some(&TestMsg(1)));
            }
            ActorEmission::At { .. } => panic!("expected Immediate"),
        }
        match &sink.emissions[1] {
            ActorEmission::At {
                requested_time,
                payload,
            } => {
                assert_eq!(*requested_time, Timestamp::new(200));
                let p: &dyn Any = &**payload;
                assert_eq!(p.downcast_ref::<TestMsg>(), Some(&TestMsg(2)));
            }
            ActorEmission::Immediate { .. } => panic!("expected At"),
        }
    }

    /// Invariant: wrap_message produces Arc shared payload
    #[test]
    fn test_wrap_message_is_shared_arc() {
        let shared = wrap_message(TestMsg(42));
        let clone = Arc::clone(&shared);
        assert!(Arc::ptr_eq(&shared, &clone));
        let p: &dyn Any = &*shared;
        assert_eq!(p.downcast_ref::<TestMsg>(), Some(&TestMsg(42)));
    }

    // ========================================================================
    // Inline sink
    // ========================================================================

    /// Invariant: InlineActorSink schedules Immediate at dispatch_time with kernel sequence
    #[test]
    fn test_inline_sink_immediate_at_dispatch_time() {
        let mut sched = Scheduler::new();
        let mut seq = Sequencer::initial();
        let ts = Timestamp::new(100);
        let mut sink = InlineActorSink::new(&mut sched, &mut seq, ts);
        sink.emit(ActorEmission::Immediate {
            payload: wrap_message(TestMsg(1)),
        })
        .unwrap();
        let item = sched.pop().unwrap();
        assert_eq!(item.dispatch_time(), ts);
        assert_eq!(item.sequence_num(), SeqNo::from_raw(1));
    }

    /// Invariant: InlineActorSink schedules At at requested future time
    #[test]
    fn test_inline_sink_at_requested_time() {
        let mut sched = Scheduler::new();
        let mut seq = Sequencer::initial();
        let mut sink = InlineActorSink::new(&mut sched, &mut seq, Timestamp::new(100));
        sink.emit(ActorEmission::At {
            requested_time: Timestamp::new(250),
            payload: wrap_message(TestMsg(2)),
        })
        .unwrap();
        let item = sched.pop().unwrap();
        assert_eq!(item.dispatch_time(), Timestamp::new(250));
    }

    /// Invariant: InlineActorSink rejects At before dispatch_time
    #[test]
    fn test_inline_sink_rejects_past_at() {
        let mut sched = Scheduler::new();
        let mut seq = Sequencer::initial();
        let mut sink = InlineActorSink::new(&mut sched, &mut seq, Timestamp::new(100));
        let err = sink
            .emit(ActorEmission::At {
                requested_time: Timestamp::new(50),
                payload: wrap_message(TestMsg(2)),
            })
            .unwrap_err();
        assert!(matches!(err, ActorOutputError::PastEvent { .. }));
        assert_eq!(sched.len(), 0);
    }

    // ========================================================================
    // Error formatting
    // ========================================================================

    /// Invariant: UndeclaredProduction Display includes type name
    #[test]
    fn test_undeclared_production_error_is_readable() {
        let err = ActorOutputError::UndeclaredProduction {
            type_name: "some::Fill",
        };
        let msg = err.to_string();
        assert!(
            msg.contains("some::Fill"),
            "error message should contain the type name, got: {msg}"
        );
    }

    /// Invariant: PastEvent Display includes both timestamps
    #[test]
    fn test_past_event_error_is_readable() {
        let err = ActorOutputError::PastEvent {
            requested: Timestamp::new(50),
            current: Timestamp::new(100),
        };
        let msg = err.to_string();
        assert!(
            msg.contains("50ns"),
            "error message should contain the requested timestamp, got: {msg}"
        );
        assert!(
            msg.contains("100ns"),
            "error message should contain the current timestamp, got: {msg}"
        );
    }

    /// Invariant: SequenceExhaustion Display is readable
    #[test]
    fn test_sequence_exhaustion_error_is_readable() {
        let err = ActorOutputError::SequenceExhaustion;
        assert!(err.to_string().contains("sequence"));
    }
}
