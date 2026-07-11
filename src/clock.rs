use crate::time::timestamp::Timestamp;

pub mod live;
pub mod sim;

/// Clock is the main clock abstraction used throughout the engine in live
/// and bakctesting mode. Depending on the context, this will be swapped out
/// for an artificial clock or the real system clock.
pub trait Clock: Send {
    fn now(&self) -> Timestamp;
}
