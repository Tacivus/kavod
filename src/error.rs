use thiserror::Error;

use crate::time::timestamp::Timestamp;

/// Errors returned during engine construction via [`EngineBuilder::build`].
///
/// [`EngineBuilder::build`]: crate::builder::EngineBuilder::build
#[derive(Debug, Error)]
pub enum BuildError {
    /// A duplicate state value was seeded in the global cache.
    #[error("duplicate seeded state for type `{type_name}`")]
    DuplicateSeededState {
        /// The name of the state type whose key was duplicated.
        type_name: &'static str,
    },

    /// A declared production has no registered consumer.
    #[error("no consumer registered for produced message type `{message_type}`")]
    MissingConsumer {
        /// The name of the message type that has no consumer.
        message_type: &'static str,
    },

    /// A registration identity (e.g. actor name) was used more than once.
    #[error("duplicate registration identity: `{name}`")]
    DuplicateRegistrationIdentity {
        /// The duplicated identity name.
        name: &'static str,
    },

    /// The requested engine mode is not yet implemented.
    #[error("mode `{mode}` is not yet supported")]
    UnsupportedMode {
        /// The name of the unsupported mode.
        mode: &'static str,
    },
}

/// Errors returned during engine execution via [`Engine::run`] or
/// [`Engine::push_event`].
///
/// [`Engine::run`]: crate::engine::Engine::run
/// [`Engine::push_event`]: crate::engine::Engine::push_event
#[derive(Debug, Error)]
pub enum EngineError {
    /// An incoming message has no registered consumer.
    #[error("unconsumed ingress: no consumer registered for message type `{message_type}`")]
    UnconsumedIngress {
        /// The name of the message type that has no consumer.
        message_type: &'static str,
    },

    /// A message was scheduled at a timestamp before the engine's
    /// current logical dispatch time.
    #[error(
        "event scheduled in the past: requested {requested}, current dispatch time is {current}"
    )]
    PastEvent {
        /// The requested dispatch time.
        requested: Timestamp,
        /// The engine's current logical dispatch time.
        current: Timestamp,
    },

    /// The same-instant event limit was exceeded, halting a potential
    /// unbounded cascade.
    #[error("same-instant event limit of {max_events} exceeded")]
    SameInstantLimitExceeded {
        /// The configured maximum events per instant.
        max_events: usize,
    },

    /// The kernel sequence counter overflowed.
    #[error("sequence exhausted")]
    SequenceExhaustion,
}

impl From<crate::sequence::SequenceError> for EngineError {
    fn from(_: crate::sequence::SequenceError) -> Self {
        EngineError::SequenceExhaustion
    }
}
