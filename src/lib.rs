#![forbid(unsafe_code)]

mod bounded_buffer;
mod journal;
mod time;
pub use journal::{Journal, JournalBuildError, JournalError, SinkOperation};
pub use time::{EventIndex, Timestamp};
