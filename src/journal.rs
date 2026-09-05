use crate::bounded_buffer::BoundedBuffer;
use std::collections::TryReserveError;
use std::io;
use std::num::NonZeroUsize;

/// An error that prevents a Journal from reserving its bounded encode region.
#[derive(Debug)]
pub enum JournalBuildError {
    /// `max_record_bytes` leaves no room for the reserved newline byte.
    MaxBytesTooLarge,
    /// The reusable record buffer could not reserve its storage.
    AllocationFailed(TryReserveError),
}

/// An error encountered while encoding or persisting a Journal record.
#[derive(Debug)]
pub enum JournalError {
    Encode(serde_json::Error),
    /// The payload serialized to something other than one single-line JSON object.
    NotAnObject,
    BoundExceeded,
    Sink {
        operation: SinkOperation,
        error: io::Error,
    },
}

/// The sink operation that failed while persisting a Journal record.
#[derive(Debug, PartialEq, Eq)]
pub enum SinkOperation {
    Write,
    Flush,
}

/// A bounded JSON Lines writer.
pub struct Journal<W: io::Write> {
    #[allow(
        dead_code,
        reason = "used by Journal sink operations in later build steps"
    )]
    writer: W,
    #[allow(dead_code, reason = "used by Journal encoding in later build steps")]
    region: BoundedBuffer<u8>,
    poisoned: bool,
}

impl<W: io::Write> Journal<W> {
    /// Reserves the bounded encode region up front.
    pub fn new(writer: W, max_record_bytes: NonZeroUsize) -> Result<Self, JournalBuildError> {
        let region_size = max_record_bytes
            .get()
            .checked_add(1)
            .ok_or(JournalBuildError::MaxBytesTooLarge)?;
        let region =
            BoundedBuffer::new(region_size).map_err(JournalBuildError::AllocationFailed)?;

        Ok(Self {
            writer,
            region,
            poisoned: false,
        })
    }

    /// Reports whether a sink failure has permanently poisoned the Journal.
    pub fn is_poisoned(&self) -> bool {
        self.poisoned
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod journal_construction {
        use super::*;

        /// Invariant: a record bound at the numeric maximum is rejected before an
        /// impossible encode-region size can be allocated.
        /// Design Doc: JRN-ENCODE
        #[test]
        fn region_size_overflow_is_max_bytes_too_large() {
            let result = Journal::new(Vec::<u8>::new(), NonZeroUsize::MAX);

            assert!(
                matches!(result, Err(JournalBuildError::MaxBytesTooLarge)),
                "an overflowing encode-region size must return MaxBytesTooLarge"
            );
        }

        /// Invariant: failure to reserve the complete encode region returns the
        /// allocator's reservation error instead of a partially constructed Journal.
        /// Design Doc: JournalBuildError
        #[test]
        fn failed_reservation_is_allocation_failed() {
            let largest_nonoverflowing = NonZeroUsize::new(usize::MAX - 1)
                .expect("one below usize::MAX must remain nonzero");
            let result = Journal::new(Vec::<u8>::new(), largest_nonoverflowing);

            let _reservation_error = match result {
                Err(JournalBuildError::AllocationFailed(error)) => error,
                Err(JournalBuildError::MaxBytesTooLarge) => {
                    panic!("a nonoverflowing region size must reach reservation")
                }
                Ok(_) => panic!("an impossible reservation must not construct a Journal"),
            };
        }

        /// Invariant: a newly constructed Journal begins ready to accept records.
        /// Design Doc: JRN-POISON
        #[test]
        fn fresh_journal_is_not_poisoned() {
            let max_record_bytes =
                NonZeroUsize::new(1).expect("one must be a valid nonzero record bound");
            let journal = Journal::new(Vec::<u8>::new(), max_record_bytes)
                .expect("the minimum encode region must be reservable");

            assert!(
                !journal.is_poisoned(),
                "a newly constructed Journal must not be poisoned"
            );
        }

        /// Invariant: the smallest valid record bound reserves one byte for the
        /// record and one additional byte for its newline without pre-filling either.
        #[test]
        fn minimum_record_bound_reserves_object_plus_newline_region() {
            let max_record_bytes =
                NonZeroUsize::new(1).expect("one must be a valid nonzero record bound");
            let journal = Journal::new(Vec::<u8>::new(), max_record_bytes)
                .expect("the minimum encode region must be reservable");

            assert_eq!(
                journal.region.capacity(),
                2,
                "a one-byte record bound must reserve a two-byte encode region"
            );
            assert!(
                journal.region.is_empty(),
                "a newly reserved encode region must start empty"
            );
        }
    }
}
