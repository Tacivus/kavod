use crate::bounded_buffer::BoundedBuffer;
use serde::Serialize;
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

    #[allow(
        dead_code,
        reason = "used by Journal line encoding in the next build step"
    )]
    fn encode_raw<R: Serialize>(&mut self, record: &R) -> Result<(), JournalError> {
        self.region.clear();
        serde_json::to_writer(&mut self.region, record).map_err(|error| {
            if error.io_error_kind() == Some(io::ErrorKind::WriteZero) {
                JournalError::BoundExceeded
            } else {
                JournalError::Encode(error)
            }
        })
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

    mod journal_encoding {
        use super::*;

        struct AlwaysFails;

        impl Serialize for AlwaysFails {
            fn serialize<S: serde::Serializer>(&self, _serializer: S) -> Result<S::Ok, S::Error> {
                Err(serde::ser::Error::custom("intentional serializer failure"))
            }
        }

        #[derive(Default)]
        struct CountingWriter {
            write_calls: usize,
            flush_calls: usize,
        }

        impl io::Write for CountingWriter {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                self.write_calls += 1;
                Ok(bytes.len())
            }

            fn flush(&mut self) -> io::Result<()> {
                self.flush_calls += 1;
                Ok(())
            }
        }

        /// Invariant: a record larger than the bounded encode region is rejected
        /// before either writing to or flushing the sink.
        /// Design Doc: JRN-ENCODE
        #[test]
        fn oversized_record_is_bound_exceeded_without_sink_calls() {
            let max_record_bytes =
                NonZeroUsize::new(1).expect("one must be a valid nonzero record bound");
            let mut journal = Journal::new(CountingWriter::default(), max_record_bytes)
                .expect("the minimum encode region must be reservable");

            let error = journal
                .encode_raw(&serde_json::json!({"oversized": true}))
                .expect_err("a record larger than the encode region must fail");

            assert!(
                matches!(error, JournalError::BoundExceeded),
                "an encode-region overflow must return BoundExceeded"
            );
            assert_eq!(
                journal.writer.write_calls, 0,
                "an oversized record must not call the sink's write operation"
            );
            assert_eq!(
                journal.writer.flush_calls, 0,
                "an oversized record must not call the sink's flush operation"
            );
        }

        /// Invariant: a serializer-originated failure is reported as an encode
        /// error before either writing to or flushing the sink.
        /// Design Doc: JRN-ENCODE
        #[test]
        fn serializer_failure_is_encode_without_sink_calls() {
            let max_record_bytes =
                NonZeroUsize::new(16).expect("sixteen must be a valid nonzero record bound");
            let mut journal = Journal::new(CountingWriter::default(), max_record_bytes)
                .expect("a small encode region must be reservable");

            let error = journal
                .encode_raw(&AlwaysFails)
                .expect_err("the deliberately failing serializer must fail");

            assert!(
                matches!(error, JournalError::Encode(_)),
                "a non-I/O serializer failure must return Encode"
            );
            assert_eq!(
                journal.writer.write_calls, 0,
                "a serializer failure must not call the sink's write operation"
            );
            assert_eq!(
                journal.writer.flush_calls, 0,
                "a serializer failure must not call the sink's flush operation"
            );
        }

        /// Invariant: rejecting records during encoding leaves the Journal ready
        /// to process another record rather than permanently disabling it.
        /// Design Doc: JRN-ENCODE
        #[test]
        fn encode_failures_do_not_poison() {
            let max_record_bytes =
                NonZeroUsize::new(1).expect("one must be a valid nonzero record bound");
            let mut journal = Journal::new(CountingWriter::default(), max_record_bytes)
                .expect("the minimum encode region must be reservable");

            let oversized_error = journal
                .encode_raw(&serde_json::json!({"oversized": true}))
                .expect_err("an oversized record must fail");
            assert!(
                matches!(oversized_error, JournalError::BoundExceeded),
                "the oversized-record setup must reach BoundExceeded"
            );
            assert!(
                !journal.is_poisoned(),
                "a bounded encode failure must not poison the Journal"
            );

            let serializer_error = journal
                .encode_raw(&AlwaysFails)
                .expect_err("the deliberately failing serializer must fail");
            assert!(
                matches!(serializer_error, JournalError::Encode(_)),
                "the serializer-failure setup must reach Encode"
            );
            assert!(
                !journal.is_poisoned(),
                "a serializer failure must not poison the Journal"
            );
        }

        /// Invariant: successful encoding stores exactly the raw JSON bytes while
        /// leaving the sink untouched and the Journal usable.
        #[test]
        fn successful_record_is_buffered_without_sink_calls() {
            let expected = br#"{"ready":true}"#;
            let max_record_bytes = NonZeroUsize::new(expected.len())
                .expect("a nonempty JSON object must define a nonzero record bound");
            let mut journal = Journal::new(CountingWriter::default(), max_record_bytes)
                .expect("the exact encode region must be reservable");

            journal
                .encode_raw(&serde_json::json!({"ready": true}))
                .expect("a fitting record must encode successfully");

            assert_eq!(
                journal.region.as_slice(),
                expected,
                "successful raw encoding must retain exactly the serialized JSON bytes"
            );
            assert_eq!(
                journal.writer.write_calls, 0,
                "raw encoding must not call the sink's write operation"
            );
            assert_eq!(
                journal.writer.flush_calls, 0,
                "raw encoding must not call the sink's flush operation"
            );
            assert!(
                !journal.is_poisoned(),
                "successful raw encoding must not poison the Journal"
            );
        }

        /// Invariant: encoding a second record replaces the first record's bytes
        /// instead of appending to them or changing the bounded capacity.
        #[test]
        fn successive_encodes_replace_previous_bytes() {
            let max_record_bytes =
                NonZeroUsize::new(32).expect("thirty-two must be a valid nonzero record bound");
            let mut journal = Journal::new(CountingWriter::default(), max_record_bytes)
                .expect("a small encode region must be reservable");
            let region_capacity = journal.region.capacity();

            journal
                .encode_raw(&serde_json::json!({"first": 1}))
                .expect("the first fitting record must encode");
            journal
                .encode_raw(&serde_json::json!({"second": 2}))
                .expect("the second fitting record must encode");

            assert_eq!(
                journal.region.as_slice(),
                br#"{"second":2}"#,
                "a later encode must replace all bytes from the previous record"
            );
            assert_eq!(
                journal.region.capacity(),
                region_capacity,
                "reusing the encode region must preserve its bounded capacity"
            );
        }

        /// Invariant: after a record exhausts the encode region, the same region
        /// can be cleared and reused for a later fitting record.
        #[test]
        fn successful_encode_after_bound_exceeded_reuses_region() {
            let max_record_bytes =
                NonZeroUsize::new(16).expect("sixteen must be a valid nonzero record bound");
            let mut journal = Journal::new(CountingWriter::default(), max_record_bytes)
                .expect("a small encode region must be reservable");
            let region_capacity = journal.region.capacity();

            let error = journal
                .encode_raw(&serde_json::json!({"value": "far too large for the region"}))
                .expect_err("the oversized record must exhaust the encode region");
            assert!(
                matches!(error, JournalError::BoundExceeded),
                "the recovery setup must reach BoundExceeded"
            );
            journal
                .encode_raw(&serde_json::json!({"ok": 1}))
                .expect("a fitting record must encode after a bound failure");

            assert_eq!(
                journal.region.as_slice(),
                br#"{"ok":1}"#,
                "recovery must replace the oversized record's retained prefix"
            );
            assert_eq!(
                journal.region.capacity(),
                region_capacity,
                "recovery after a bound failure must preserve bounded capacity"
            );
            assert!(
                !journal.is_poisoned(),
                "recovery after a bound failure must leave the Journal unpoisoned"
            );
        }

        /// Invariant: a serializer failure clears a prior record's bytes and leaves
        /// the reusable region able to encode a subsequent record.
        #[test]
        fn serializer_failure_clears_previous_bytes_and_region_remains_reusable() {
            let max_record_bytes =
                NonZeroUsize::new(32).expect("thirty-two must be a valid nonzero record bound");
            let mut journal = Journal::new(CountingWriter::default(), max_record_bytes)
                .expect("a small encode region must be reservable");

            journal
                .encode_raw(&serde_json::json!({"stale": true}))
                .expect("the initial fitting record must encode");
            let error = journal
                .encode_raw(&AlwaysFails)
                .expect_err("the deliberately failing serializer must fail");
            assert!(
                matches!(error, JournalError::Encode(_)),
                "the recovery setup must reach Encode"
            );
            assert!(
                journal.region.is_empty(),
                "a serializer failure must not leave bytes from the prior record"
            );

            journal
                .encode_raw(&serde_json::json!({"fresh": true}))
                .expect("a fitting record must encode after a serializer failure");
            assert_eq!(
                journal.region.as_slice(),
                br#"{"fresh":true}"#,
                "the reusable region must hold only the record encoded after failure"
            );
            assert!(
                !journal.is_poisoned(),
                "recovery after a serializer failure must leave the Journal unpoisoned"
            );
        }
    }
}
