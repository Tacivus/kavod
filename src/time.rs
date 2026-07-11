pub mod duration;
pub mod timestamp;

use thiserror::Error;

#[derive(Error, Debug, PartialEq, Eq)]
pub enum TimeError {
    #[error("time value overflowed")]
    Overflow,

    #[error("time value underflowed")]
    Underflow,

    #[error("timestamp is outside the representable range for Kavod")]
    OutOfRange,
}
