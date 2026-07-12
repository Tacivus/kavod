mod live;
mod sim;
pub use live::LiveClock;
pub use sim::SimClock;

use crate::time::Timestamp;

/// Clock is the main clock abstraction used throughout the engine in live
/// and bakctesting mode. Depending on the context, this will be swapped out
/// for an artificial clock or the real system clock.
///
/// The provided `LiveClock` and `SimClock` implement this.
pub trait Clock: Send {
    fn now(&self) -> Timestamp;

    fn set(&mut self, ts: Timestamp);
}
