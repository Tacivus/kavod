pub mod duration;
pub mod live_clock;
pub mod sim_clock;
pub mod timestamp;

use thiserror::Error;

use crate::time::timestamp::Timestamp;

#[derive(Error, Debug, PartialEq, Eq)]
pub enum TimeError {
    #[error("time value overflowed")]
    Overflow,

    #[error("time value underflowed")]
    Underflow,

    #[error("timestamp is outside the representable range for Kavod")]
    OutOfRange,
}
