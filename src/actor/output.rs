use std::sync::Arc;

use thiserror::Error;

use crate::{
    message::{Message, SharedMessage},
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
}

/// One actor emission reported to the selected executor.
///
/// The actor never assigns kernel sequence. Immediate emissions carry no
/// final live dispatch timestamp — the kernel stamps receipt on ingress.
/// `At` carries only the user-requested time for later kernel validation.
pub(crate) enum ActorEmission {
    Immediate {
        #[allow(dead_code)] // read by executor ingress (Phase 18+)
        payload: SharedMessage,
    },
    At {
        #[allow(dead_code)] // read by executor ingress (Phase 18+)
        requested_time: Timestamp,
        #[allow(dead_code)] // read by executor ingress (Phase 18+)
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
}
