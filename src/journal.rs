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
    writer: W,
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

    /// Encodes, writes, and flushes one JSON Lines record.
    ///
    /// # Panics
    ///
    /// Panics if a prior sink failure poisoned the Journal.
    pub fn commit<R: Serialize>(&mut self, record: &R) -> Result<(), JournalError> {
        assert!(
            !self.poisoned,
            "JRN-POISON: a poisoned Journal cannot commit another record"
        );

        self.encode_line(record)?;
        self.write_line()?;
        self.writer.flush().map_err(|error| {
            self.poisoned = true;
            JournalError::Sink {
                operation: SinkOperation::Flush,
                error,
            }
        })
    }

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

    fn encode_line<R: Serialize>(&mut self, record: &R) -> Result<(), JournalError> {
        self.encode_raw(record)?;

        let encoded = self.region.as_slice();
        if encoded.first() != Some(&b'{')
            || encoded.last() != Some(&b'}')
            || encoded.contains(&b'\n')
        {
            return Err(JournalError::NotAnObject);
        }

        self.region
            .try_push(b'\n')
            .map_err(|_| JournalError::BoundExceeded)
    }

    fn write_line(&mut self) -> Result<(), JournalError> {
        let mut offset = 0;

        while offset < self.region.len() {
            let remaining = &self.region.as_slice()[offset..];
            let remaining_len = remaining.len();

            match self.writer.write(remaining) {
                Ok(0) => {
                    self.poisoned = true;
                    return Err(JournalError::Sink {
                        operation: SinkOperation::Write,
                        error: io::ErrorKind::WriteZero.into(),
                    });
                }
                Ok(count) if count > remaining_len => {
                    self.poisoned = true;
                    return Err(JournalError::Sink {
                        operation: SinkOperation::Write,
                        error: io::Error::new(
                            io::ErrorKind::InvalidData,
                            "sink reported writing more bytes than provided",
                        ),
                    });
                }
                Ok(count) => offset += count,
                Err(error) => {
                    self.poisoned = true;
                    return Err(JournalError::Sink {
                        operation: SinkOperation::Write,
                        error,
                    });
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    #[derive(Default)]
    struct LineCountingWriter {
        write_calls: usize,
        flush_calls: usize,
    }

    impl io::Write for LineCountingWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.write_calls += 1;
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            self.flush_calls += 1;
            Ok(())
        }
    }

    fn line_journal(max_record_bytes: usize) -> Journal<LineCountingWriter> {
        let max_record_bytes = NonZeroUsize::new(max_record_bytes)
            .expect("a line-encoding test bound must be nonzero");
        Journal::new(LineCountingWriter::default(), max_record_bytes)
            .expect("a small line-encoding region must be reservable")
    }

    enum ScriptedResult {
        Write(io::Result<usize>),
        Flush(io::Result<()>),
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    enum SinkCall {
        Write(Vec<u8>),
        Flush,
    }

    struct ScriptedSink {
        results: VecDeque<ScriptedResult>,
        calls: Vec<SinkCall>,
        accepted_bytes: Vec<u8>,
        committed_len: usize,
    }

    impl ScriptedSink {
        fn new(results: impl IntoIterator<Item = ScriptedResult>) -> Self {
            Self {
                results: results.into_iter().collect(),
                calls: Vec::new(),
                accepted_bytes: Vec::new(),
                committed_len: 0,
            }
        }

        fn committed_bytes(&self) -> &[u8] {
            &self.accepted_bytes[..self.committed_len]
        }

        fn uncertain_suffix(&self) -> &[u8] {
            &self.accepted_bytes[self.committed_len..]
        }
    }

    impl io::Write for ScriptedSink {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.calls.push(SinkCall::Write(bytes.to_vec()));
            let result = match self
                .results
                .pop_front()
                .expect("every scripted sink call must have a result")
            {
                ScriptedResult::Write(result) => result,
                ScriptedResult::Flush(_) => {
                    panic!("a scripted write call must consume a write result")
                }
            };

            match result {
                Ok(count) => {
                    if count <= bytes.len() {
                        self.accepted_bytes.extend_from_slice(&bytes[..count]);
                    }
                    Ok(count)
                }
                Err(error) => Err(error),
            }
        }

        fn flush(&mut self) -> io::Result<()> {
            self.calls.push(SinkCall::Flush);
            let result = match self
                .results
                .pop_front()
                .expect("every scripted sink call must have a result")
            {
                ScriptedResult::Flush(result) => result,
                ScriptedResult::Write(_) => {
                    panic!("a scripted flush call must consume a flush result")
                }
            };

            match result {
                Ok(()) => {
                    self.committed_len = self.accepted_bytes.len();
                    Ok(())
                }
                Err(error) => Err(error),
            }
        }
    }

    fn scripted_journal(
        max_record_bytes: usize,
        results: impl IntoIterator<Item = ScriptedResult>,
    ) -> Journal<ScriptedSink> {
        let max_record_bytes =
            NonZeroUsize::new(max_record_bytes).expect("a scripted Journal bound must be nonzero");
        Journal::new(ScriptedSink::new(results), max_record_bytes)
            .expect("a small scripted Journal region must be reservable")
    }

    fn scripted_line_journal(
        results: impl IntoIterator<Item = ScriptedResult>,
    ) -> Journal<ScriptedSink> {
        let mut journal = scripted_journal(2, results);
        journal
            .encode_line(&serde_json::json!({}))
            .expect("the minimum JSON object must fit its exact record bound");
        journal
    }

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

    mod journal_object_validation {
        use super::*;
        use serde_json::value::RawValue;

        /// Invariant: a valid JSON object is buffered as exactly one object followed
        /// by exactly one newline, without touching the sink.
        /// Design Doc: JRN-FORMAT
        #[test]
        fn object_plus_newline_is_the_encoded_line() {
            let encoded_object = br#"{"ready":true}"#;
            let mut journal = line_journal(encoded_object.len());

            journal
                .encode_line(&serde_json::json!({"ready": true}))
                .expect("a fitting single-line object must encode");

            assert_eq!(
                journal.region.as_slice(),
                br#"{"ready":true}
"#,
                "the encoded line must contain exactly the object and its newline"
            );
            assert_eq!(
                journal.writer.write_calls, 0,
                "line encoding must not call the sink's write operation"
            );
            assert_eq!(
                journal.writer.flush_calls, 0,
                "line encoding must not call the sink's flush operation"
            );
            assert!(
                !journal.is_poisoned(),
                "successful line encoding must not poison the Journal"
            );
        }

        /// Invariant: an otherwise valid JSON object containing a literal newline
        /// byte is rejected before the sink is touched.
        /// Design Doc: JRN-ENCODE
        #[test]
        fn interior_newline_is_not_an_object() {
            let raw = RawValue::from_string(String::from("{\"a\":\n1}"))
                .expect("JSON permits a literal newline between tokens");
            let mut journal = line_journal(raw.get().len());

            let error = journal
                .encode_line(&raw)
                .expect_err("an object with a literal newline must be rejected");

            assert!(
                matches!(error, JournalError::NotAnObject),
                "a literal newline in encoded bytes must return NotAnObject"
            );
            assert_eq!(
                journal.region.as_slice(),
                raw.get().as_bytes(),
                "newline rejection must leave the classified bytes without appending a newline"
            );
            assert_eq!(
                journal.writer.write_calls, 0,
                "newline rejection must not call the sink's write operation"
            );
            assert_eq!(
                journal.writer.flush_calls, 0,
                "newline rejection must not call the sink's flush operation"
            );
            assert!(
                !journal.is_poisoned(),
                "newline rejection must not poison the Journal"
            );
        }

        /// Invariant: a completed top-level JSON value that is not an object is
        /// rejected even when it fills all available line-encoding space.
        /// Design Doc: JRN-ENCODE
        #[test]
        fn non_object_top_level_is_rejected() {
            let mut journal = line_journal(1);

            let error = journal
                .encode_line(&42_u8)
                .expect_err("a top-level number must be rejected");

            assert!(
                matches!(error, JournalError::NotAnObject),
                "a full non-object must return NotAnObject before newline capacity is checked"
            );
            assert_eq!(
                journal.region.as_slice(),
                b"42",
                "non-object rejection must not append a newline"
            );
            assert_eq!(
                journal.writer.write_calls, 0,
                "non-object rejection must not call the sink's write operation"
            );
            assert_eq!(
                journal.writer.flush_calls, 0,
                "non-object rejection must not call the sink's flush operation"
            );
            assert!(
                !journal.is_poisoned(),
                "non-object rejection must not poison the Journal"
            );
        }

        /// Invariant: nulls, booleans, numbers, strings, and arrays are all rejected
        /// because none is a top-level JSON object.
        #[test]
        fn every_non_object_json_kind_is_rejected() {
            let mut journal = line_journal(32);
            let non_objects = [
                serde_json::Value::Null,
                serde_json::json!(true),
                serde_json::json!(7),
                serde_json::json!("text"),
                serde_json::json!([1, 2]),
            ];

            for value in non_objects {
                let error = journal
                    .encode_line(&value)
                    .expect_err("every non-object JSON kind must be rejected");
                assert!(
                    matches!(error, JournalError::NotAnObject),
                    "every non-object JSON kind must return NotAnObject"
                );
            }

            assert_eq!(
                journal.writer.write_calls, 0,
                "repeated non-object rejection must not call the sink's write operation"
            );
            assert_eq!(
                journal.writer.flush_calls, 0,
                "repeated non-object rejection must not call the sink's flush operation"
            );
            assert!(
                !journal.is_poisoned(),
                "repeated non-object rejection must not poison the Journal"
            );
        }

        /// Invariant: a newline in an ordinary string is escaped in the JSON bytes
        /// and therefore remains a valid single-line object.
        #[test]
        fn ordinary_string_newline_is_escaped_and_allowed() {
            let expected_object = br#"{"message":"first\nsecond"}"#;
            let mut journal = line_journal(expected_object.len());

            journal
                .encode_line(&serde_json::json!({"message": "first\nsecond"}))
                .expect("an escaped string newline must remain a valid object");

            let mut expected_line = expected_object.to_vec();
            expected_line.push(b'\n');
            assert_eq!(
                journal.region.as_slice(),
                expected_line,
                "an ordinary string newline must be represented by escaped bytes"
            );
            assert!(
                !journal.region.as_slice()[..expected_object.len()].contains(&b'\n'),
                "the encoded object must contain no literal newline byte"
            );
        }

        /// Invariant: rejecting an object with a literal newline leaves the Journal
        /// able to encode a later valid object in the same bounded region.
        #[test]
        fn valid_object_encodes_after_rejected_non_object() {
            let raw = RawValue::from_string(String::from("{\"a\":\n1}"))
                .expect("JSON permits a literal newline between tokens");
            let mut journal = line_journal(raw.get().len());

            let error = journal
                .encode_line(&raw)
                .expect_err("the literal-newline object must be rejected");
            assert!(
                matches!(error, JournalError::NotAnObject),
                "the recovery setup must reach NotAnObject"
            );
            journal
                .encode_line(&serde_json::json!({}))
                .expect("a valid object must encode after classification failure");

            assert_eq!(
                journal.region.as_slice(),
                b"{}\n",
                "recovery must replace the rejected bytes with the later valid line"
            );
            assert_eq!(
                journal.writer.write_calls, 0,
                "classification failure and recovery must leave the sink untouched"
            );
            assert!(
                !journal.is_poisoned(),
                "classification failure and recovery must leave the Journal unpoisoned"
            );
        }

        /// Invariant: a non-object value too long for the encode region is rejected
        /// as a bound failure, not as a non-object, because the bound is checked
        /// before the object shape.
        /// Design Doc: JRN-ENCODE
        #[test]
        fn an_overrunning_non_object_is_bound_exceeded_before_classification() {
            let mut journal = line_journal(1);

            let error = journal
                .encode_line(&123_u8)
                .expect_err("a three-byte number cannot fit a two-byte region");

            assert!(
                matches!(error, JournalError::BoundExceeded),
                "an overrunning non-object must report the bound before its shape"
            );
            assert_eq!(
                journal.writer.write_calls, 0,
                "an overrunning non-object must not call the sink's write operation"
            );
            assert_eq!(
                journal.writer.flush_calls, 0,
                "an overrunning non-object must not call the sink's flush operation"
            );
            assert!(
                !journal.is_poisoned(),
                "an overrunning non-object must not poison the Journal"
            );
        }
    }

    mod journal_newline_reservation {
        use super::*;
        use serde_json::value::RawValue;

        /// Invariant: a valid object that fills the complete encode region is
        /// rejected because no reserved byte remains for its newline.
        /// Design Doc: JRN-ENCODE
        #[test]
        fn object_of_region_size_has_no_newline_room() {
            let mut journal = line_journal(1);

            let error = journal
                .encode_line(&serde_json::json!({}))
                .expect_err("an object filling the region must leave no newline room");

            assert!(
                matches!(error, JournalError::BoundExceeded),
                "an object with no newline room must return BoundExceeded"
            );
            assert_eq!(
                journal.region.as_slice(),
                b"{}",
                "a failed newline append must preserve the complete object bytes"
            );
            assert_eq!(
                journal.writer.write_calls, 0,
                "a missing newline slot must not call the sink's write operation"
            );
            assert_eq!(
                journal.writer.flush_calls, 0,
                "a missing newline slot must not call the sink's flush operation"
            );
            assert!(
                !journal.is_poisoned(),
                "a missing newline slot must not poison the Journal"
            );
        }

        /// Invariant: an object exactly at the configured record-byte maximum fits
        /// together with its separately reserved newline byte.
        /// Design Doc: JRN-ENCODE
        #[test]
        fn encode_at_exactly_max_bytes_completes() {
            let mut journal = line_journal(2);

            journal
                .encode_line(&serde_json::json!({}))
                .expect("an object exactly at the configured maximum must fit");

            assert_eq!(
                journal.region.as_slice(),
                b"{}\n",
                "an exact-maximum object must retain the separately reserved newline"
            );
            assert_eq!(
                journal.region.len(),
                journal.region.capacity(),
                "an exact-maximum object plus newline must fill the encode region"
            );
            assert_eq!(
                journal.writer.write_calls, 0,
                "exact-boundary line encoding must leave the sink untouched"
            );
            assert!(
                !journal.is_poisoned(),
                "exact-boundary line encoding must leave the Journal unpoisoned"
            );
        }

        /// Invariant: encoding a line after a prior line filled the bounded region
        /// replaces the prior bytes and succeeds without growing the region.
        #[test]
        fn consecutive_exact_maximum_lines_reuse_full_region() {
            let mut journal = line_journal(7);
            let region_capacity = journal.region.capacity();

            journal
                .encode_line(&serde_json::json!({"a": 0}))
                .expect("the first exact-maximum object must encode");
            assert_eq!(
                journal.region.as_slice(),
                b"{\"a\":0}\n",
                "the first exact-maximum line must fill the region"
            );
            assert_eq!(
                journal.region.len(),
                region_capacity,
                "the first exact-maximum line must leave the region full"
            );

            journal
                .encode_line(&serde_json::json!({"b": 1}))
                .expect("the second exact-maximum object must encode after a full line");

            assert_eq!(
                journal.region.as_slice(),
                b"{\"b\":1}\n",
                "the second line must replace every byte of the first line"
            );
            assert_eq!(
                journal.region.capacity(),
                region_capacity,
                "consecutive full lines must not grow the bounded region"
            );
            assert_eq!(
                journal.writer.write_calls, 0,
                "consecutive line encoding must leave the sink untouched"
            );
            assert!(
                !journal.is_poisoned(),
                "consecutive successful line encodes must leave the Journal unpoisoned"
            );
        }

        /// Invariant: failing to append a newline leaves the bounded region reusable
        /// for a later object whose object bytes fit within the configured maximum.
        #[test]
        fn valid_object_encodes_after_missing_newline_room() {
            let region_sized_object = RawValue::from_string(String::from("{ }"))
                .expect("an object may contain insignificant interior whitespace");
            let mut journal = line_journal(2);

            let error = journal
                .encode_line(&region_sized_object)
                .expect_err("a three-byte object must fill the three-byte region");
            assert!(
                matches!(error, JournalError::BoundExceeded),
                "the recovery setup must fail while appending the newline"
            );
            journal
                .encode_line(&serde_json::json!({}))
                .expect("a maximum-sized object must encode after newline failure");

            assert_eq!(
                journal.region.as_slice(),
                b"{}\n",
                "recovery must replace the prior object with the later valid line"
            );
            assert_eq!(
                journal.writer.write_calls, 0,
                "newline failure and recovery must leave the sink untouched"
            );
            assert!(
                !journal.is_poisoned(),
                "newline failure and recovery must leave the Journal unpoisoned"
            );
        }
    }

    mod journal_line_errors {
        use super::*;

        struct LineAlwaysFails;

        impl Serialize for LineAlwaysFails {
            fn serialize<S: serde::Serializer>(&self, _serializer: S) -> Result<S::Ok, S::Error> {
                Err(serde::ser::Error::custom(
                    "intentional line serializer failure",
                ))
            }
        }

        /// Invariant: a record that exceeds the raw encode region reports the bound
        /// failure without touching the sink and does not prevent a later valid line.
        #[test]
        fn raw_bound_failure_leaves_journal_reusable() {
            let mut journal = line_journal(2);

            let error = journal
                .encode_line(&serde_json::json!({"oversized": true}))
                .expect_err("the oversized object must exceed the raw encode region");
            assert!(
                matches!(error, JournalError::BoundExceeded),
                "a raw encode overflow must propagate BoundExceeded"
            );
            journal
                .encode_line(&serde_json::json!({}))
                .expect("a valid object must encode after a raw bound failure");

            assert_eq!(
                journal.region.as_slice(),
                b"{}\n",
                "recovery must clear the oversized prefix before encoding the valid line"
            );
            assert_eq!(
                journal.writer.write_calls, 0,
                "raw bound failure and recovery must leave the sink untouched"
            );
            assert_eq!(
                journal.writer.flush_calls, 0,
                "raw bound failure and recovery must not flush the sink"
            );
            assert!(
                !journal.is_poisoned(),
                "raw bound failure and recovery must leave the Journal unpoisoned"
            );
        }

        /// Invariant: a serializer error is returned without touching the sink and
        /// leaves the bounded region able to encode a later valid line.
        #[test]
        fn serializer_failure_leaves_journal_reusable() {
            let mut journal = line_journal(2);

            let error = journal
                .encode_line(&LineAlwaysFails)
                .expect_err("the deliberately failing serializer must fail");
            assert!(
                matches!(error, JournalError::Encode(_)),
                "a serializer-originated line failure must propagate Encode"
            );
            journal
                .encode_line(&serde_json::json!({}))
                .expect("a valid object must encode after a serializer failure");

            assert_eq!(
                journal.region.as_slice(),
                b"{}\n",
                "recovery must clear serializer-failure state before encoding the valid line"
            );
            assert_eq!(
                journal.writer.write_calls, 0,
                "serializer failure and recovery must leave the sink untouched"
            );
            assert_eq!(
                journal.writer.flush_calls, 0,
                "serializer failure and recovery must not flush the sink"
            );
            assert!(
                !journal.is_poisoned(),
                "serializer failure and recovery must leave the Journal unpoisoned"
            );
        }
    }

    mod journal_sink_writes {
        use super::*;

        /// Invariant: each positive partial write advances through the unwritten
        /// suffix until the complete encoded line reaches the sink in order.
        /// Design Doc: JRN-POISON
        #[test]
        fn short_successful_writes_are_retried_to_completion() {
            let mut journal = scripted_line_journal([
                ScriptedResult::Write(Ok(1)),
                ScriptedResult::Write(Ok(1)),
                ScriptedResult::Write(Ok(1)),
            ]);

            journal
                .write_line()
                .expect("positive partial writes must complete the encoded line");

            assert_eq!(
                journal.writer.calls,
                [
                    SinkCall::Write(b"{}\n".to_vec()),
                    SinkCall::Write(b"}\n".to_vec()),
                    SinkCall::Write(b"\n".to_vec()),
                ],
                "each short write must be followed by exactly the remaining suffix"
            );
            assert!(
                journal.writer.results.is_empty(),
                "successful completion must consume each required write result"
            );
            assert!(
                !journal.is_poisoned(),
                "successful short writes must not poison the Journal"
            );
        }

        /// Invariant: an interrupted sink write is returned immediately and
        /// permanently disables the Journal without attempting another write.
        /// Design Doc: JRN-POISON
        #[test]
        fn interrupted_write_poisons_without_retry() {
            let mut journal = scripted_line_journal([
                ScriptedResult::Write(Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "scripted interruption",
                ))),
                ScriptedResult::Write(Ok(3)),
            ]);

            let error = journal
                .write_line()
                .expect_err("an interrupted write must fail immediately");

            match error {
                JournalError::Sink { operation, error } => {
                    assert_eq!(
                        operation,
                        SinkOperation::Write,
                        "an interrupted write must identify the failed operation as Write"
                    );
                    assert_eq!(
                        error.kind(),
                        io::ErrorKind::Interrupted,
                        "an interrupted write must preserve the sink's error kind"
                    );
                    assert_eq!(
                        error.to_string(),
                        "scripted interruption",
                        "an interrupted write must preserve the sink's returned error"
                    );
                }
                _ => panic!("an interrupted write must return a typed sink error"),
            }
            assert_eq!(
                journal.writer.calls,
                [SinkCall::Write(b"{}\n".to_vec())],
                "an interrupted write must make exactly one sink call"
            );
            assert_eq!(
                journal.writer.results.len(),
                1,
                "an interrupted write must not consume a retry result"
            );
            assert!(
                journal.is_poisoned(),
                "an interrupted write must poison the Journal before returning"
            );
        }

        /// Invariant: a sink write that accepts no bytes is reported as a write-zero
        /// failure and permanently disables the Journal without retrying.
        /// Design Doc: JRN-POISON
        #[test]
        fn zero_progress_maps_to_write_zero() {
            let mut journal =
                scripted_line_journal([ScriptedResult::Write(Ok(0)), ScriptedResult::Write(Ok(3))]);

            let error = journal
                .write_line()
                .expect_err("a zero-progress write must fail immediately");

            match error {
                JournalError::Sink { operation, error } => {
                    assert_eq!(
                        operation,
                        SinkOperation::Write,
                        "a zero-progress write must identify the failed operation as Write"
                    );
                    assert_eq!(
                        error.kind(),
                        io::ErrorKind::WriteZero,
                        "a zero-progress write must map to WriteZero"
                    );
                }
                _ => panic!("a zero-progress write must return a typed sink error"),
            }
            assert_eq!(
                journal.writer.calls,
                [SinkCall::Write(b"{}\n".to_vec())],
                "a zero-progress write must make exactly one sink call"
            );
            assert_eq!(
                journal.writer.results.len(),
                1,
                "a zero-progress write must not consume a retry result"
            );
            assert!(
                journal.is_poisoned(),
                "a zero-progress write must poison the Journal before returning"
            );
        }

        /// Invariant: a sink claiming to accept more than the offered suffix is
        /// reported as invalid data and permanently disables the Journal.
        /// Design Doc: JRN-POISON
        #[test]
        fn over_reported_count_maps_to_invalid_data() {
            let mut journal =
                scripted_line_journal([ScriptedResult::Write(Ok(4)), ScriptedResult::Write(Ok(3))]);

            let error = journal
                .write_line()
                .expect_err("an over-reported write count must fail immediately");

            match error {
                JournalError::Sink { operation, error } => {
                    assert_eq!(
                        operation,
                        SinkOperation::Write,
                        "an over-reported count must identify the failed operation as Write"
                    );
                    assert_eq!(
                        error.kind(),
                        io::ErrorKind::InvalidData,
                        "an over-reported count must map to InvalidData"
                    );
                }
                _ => panic!("an over-reported count must return a typed sink error"),
            }
            assert_eq!(
                journal.writer.calls,
                [SinkCall::Write(b"{}\n".to_vec())],
                "an over-reported count must make exactly one sink call"
            );
            assert_eq!(
                journal.writer.results.len(),
                1,
                "an over-reported count must not consume a retry result"
            );
            assert!(
                journal.is_poisoned(),
                "an over-reported count must poison the Journal before returning"
            );
        }

        /// Invariant: after a partial write, a count larger than the unwritten
        /// suffix is rejected even when it is no larger than the original line.
        #[test]
        fn over_reported_count_is_measured_against_remaining_suffix() {
            let mut journal =
                scripted_line_journal([ScriptedResult::Write(Ok(1)), ScriptedResult::Write(Ok(3))]);

            let error = journal
                .write_line()
                .expect_err("a count larger than the remaining suffix must fail");

            match error {
                JournalError::Sink { operation, error } => {
                    assert_eq!(
                        operation,
                        SinkOperation::Write,
                        "a suffix over-report must identify the failed operation as Write"
                    );
                    assert_eq!(
                        error.kind(),
                        io::ErrorKind::InvalidData,
                        "a count larger than the remaining suffix must map to InvalidData"
                    );
                }
                _ => panic!("a count larger than the remaining suffix must return a sink error"),
            }
            assert_eq!(
                journal.writer.calls,
                [
                    SinkCall::Write(b"{}\n".to_vec()),
                    SinkCall::Write(b"}\n".to_vec()),
                ],
                "a suffix over-report must occur after advancing by the accepted prefix"
            );
            assert!(
                journal.writer.results.is_empty(),
                "a suffix over-report must consume exactly the two attempted writes"
            );
            assert!(
                journal.is_poisoned(),
                "a count larger than the remaining suffix must poison the Journal"
            );
        }

        /// Invariant: an empty encoded region completes without invoking or
        /// poisoning the sink because there are no bytes to persist.
        #[test]
        fn empty_line_completes_without_sink_calls() {
            let max_record_bytes =
                NonZeroUsize::new(1).expect("one must be a valid nonzero record bound");
            let mut journal = Journal::new(ScriptedSink::new([]), max_record_bytes)
                .expect("the minimum encode region must be reservable");

            journal
                .write_line()
                .expect("an empty encoded region must require no sink operation");

            assert!(
                journal.writer.calls.is_empty(),
                "an empty encoded region must not call the sink"
            );
            assert!(
                !journal.is_poisoned(),
                "an empty encoded region must not poison the Journal"
            );
        }

        /// Invariant: a sink accepting the complete encoded line finishes after one
        /// write and leaves the Journal ready for further work.
        #[test]
        fn complete_write_finishes_without_retry() {
            let mut journal = scripted_line_journal([ScriptedResult::Write(Ok(b"{}\n".len()))]);

            journal
                .write_line()
                .expect("a complete sink write must finish successfully");

            assert_eq!(
                journal.writer.calls,
                [SinkCall::Write(b"{}\n".to_vec())],
                "a complete write must call the sink exactly once with the full line"
            );
            assert!(
                journal.writer.results.is_empty(),
                "a complete write must consume exactly its one scripted result"
            );
            assert!(
                !journal.is_poisoned(),
                "a complete write must not poison the Journal"
            );
        }
    }

    mod journal_commit {
        use super::*;

        struct CommitAlwaysFails;

        impl Serialize for CommitAlwaysFails {
            fn serialize<S: serde::Serializer>(&self, _serializer: S) -> Result<S::Ok, S::Error> {
                Err(serde::ser::Error::custom(
                    "intentional commit serializer failure",
                ))
            }
        }

        /// Invariant: a record becomes committed only after the sink receives the
        /// exact encoded line and successfully flushes it.
        /// Design Doc: JRN-COMMIT
        #[test]
        fn successful_flush_commits_exactly_the_line() {
            let mut journal = scripted_line_journal([
                ScriptedResult::Write(Ok(b"{}\n".len())),
                ScriptedResult::Flush(Ok(())),
            ]);

            journal
                .commit(&serde_json::json!({}))
                .expect("a complete write followed by a successful flush must commit");

            assert_eq!(
                journal.writer.calls,
                [SinkCall::Write(b"{}\n".to_vec()), SinkCall::Flush],
                "a successful commit must write exactly one encoded line before flushing"
            );
            assert!(
                journal.writer.results.is_empty(),
                "a successful commit must consume exactly its write and flush results"
            );
            assert!(
                !journal.is_poisoned(),
                "a successful commit must leave the Journal ready for another record"
            );
        }

        /// Invariant: if flushing fails, the written line remains an uncertain
        /// suffix and the commit reports the flush error instead of succeeding.
        /// Design Doc: JRN-COMMIT
        #[test]
        fn flush_failure_is_sink_flush_and_uncommitted() {
            let mut journal = scripted_line_journal([
                ScriptedResult::Write(Ok(b"{}\n".len())),
                ScriptedResult::Flush(Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "scripted flush failure",
                ))),
            ]);

            let error = journal
                .commit(&serde_json::json!({}))
                .expect_err("a failed flush must leave the record uncommitted");

            match error {
                JournalError::Sink { operation, error } => {
                    assert_eq!(
                        operation,
                        SinkOperation::Flush,
                        "a flush failure must identify the failed operation as Flush"
                    );
                    assert_eq!(
                        error.kind(),
                        io::ErrorKind::BrokenPipe,
                        "a flush failure must preserve the sink's error kind"
                    );
                    assert_eq!(
                        error.to_string(),
                        "scripted flush failure",
                        "a flush failure must preserve the sink's returned error"
                    );
                }
                _ => panic!("a flush failure must return a typed sink error"),
            }
            assert_eq!(
                journal.writer.calls,
                [SinkCall::Write(b"{}\n".to_vec()), SinkCall::Flush],
                "a failed flush must leave the preceding written line as an uncertain suffix"
            );
            assert!(
                journal.writer.results.is_empty(),
                "a failed flush must consume exactly its write and flush results"
            );
        }

        /// Invariant: every write or flush error observed while committing a record
        /// permanently marks the Journal as unusable.
        /// Design Doc: JRN-POISON
        #[test]
        fn sink_error_poisons_permanently() {
            let mut write_failure = scripted_line_journal([
                ScriptedResult::Write(Err(io::Error::other("scripted write failure"))),
                ScriptedResult::Flush(Ok(())),
            ]);

            let write_error = write_failure
                .commit(&serde_json::json!({}))
                .expect_err("a write error must fail the commit");
            assert!(
                matches!(
                    write_error,
                    JournalError::Sink {
                        operation: SinkOperation::Write,
                        ..
                    }
                ),
                "a write error must remain classified as a failed Write operation"
            );
            assert!(
                write_failure.is_poisoned(),
                "a write error must poison the Journal before returning"
            );
            assert_eq!(
                write_failure.writer.calls,
                [SinkCall::Write(b"{}\n".to_vec())],
                "a write error must prevent the commit from attempting a flush"
            );
            assert_eq!(
                write_failure.writer.results.len(),
                1,
                "a write error must leave the unused flush result untouched"
            );

            let mut flush_failure = scripted_line_journal([
                ScriptedResult::Write(Ok(b"{}\n".len())),
                ScriptedResult::Flush(Err(io::Error::other("scripted flush failure"))),
            ]);

            let flush_error = flush_failure
                .commit(&serde_json::json!({}))
                .expect_err("a flush error must fail the commit");
            assert!(
                matches!(
                    flush_error,
                    JournalError::Sink {
                        operation: SinkOperation::Flush,
                        ..
                    }
                ),
                "a flush error must remain classified as a failed Flush operation"
            );
            assert!(
                flush_failure.is_poisoned(),
                "a flush error must poison the Journal before returning"
            );
            assert_eq!(
                flush_failure.writer.calls,
                [SinkCall::Write(b"{}\n".to_vec()), SinkCall::Flush],
                "a flush error must occur only after the complete line is written"
            );
        }

        /// Invariant: encoding failures never call the sink or poison the Journal,
        /// and the same Journal can subsequently commit a valid record.
        #[test]
        fn every_encode_error_skips_sink_and_allows_later_commit() {
            let mut journal = scripted_line_journal([
                ScriptedResult::Write(Ok(b"{}\n".len())),
                ScriptedResult::Flush(Ok(())),
            ]);

            let serializer_error = journal
                .commit(&CommitAlwaysFails)
                .expect_err("the deliberately failing serializer must fail the commit");
            assert!(
                matches!(serializer_error, JournalError::Encode(_)),
                "a serializer failure during commit must return Encode"
            );
            assert!(
                journal.writer.calls.is_empty(),
                "a serializer failure during commit must not call the sink"
            );
            assert!(
                !journal.is_poisoned(),
                "a serializer failure during commit must not poison the Journal"
            );

            let object_error = journal
                .commit(&0_u8)
                .expect_err("a top-level number must fail the commit");
            assert!(
                matches!(object_error, JournalError::NotAnObject),
                "a non-object commit must return NotAnObject"
            );
            assert!(
                journal.writer.calls.is_empty(),
                "a non-object commit must not call the sink"
            );
            assert!(
                !journal.is_poisoned(),
                "a non-object commit must not poison the Journal"
            );

            let bound_error = journal
                .commit(&serde_json::json!({"oversized": true}))
                .expect_err("an oversized object must fail the commit");
            assert!(
                matches!(bound_error, JournalError::BoundExceeded),
                "an oversized commit must return BoundExceeded"
            );
            assert!(
                journal.writer.calls.is_empty(),
                "an oversized commit must not call the sink"
            );
            assert!(
                !journal.is_poisoned(),
                "an oversized commit must not poison the Journal"
            );

            journal
                .commit(&serde_json::json!({}))
                .expect("a valid record must commit after every recoverable encode error");
            assert_eq!(
                journal.writer.calls,
                [SinkCall::Write(b"{}\n".to_vec()), SinkCall::Flush],
                "recovery must write and flush only the later valid record"
            );
            assert!(
                !journal.is_poisoned(),
                "a successful recovery commit must leave the Journal unpoisoned"
            );
        }
    }

    mod journal_poisoning {
        use super::*;

        /// Invariant: once a Journal has been poisoned, committing to it panics
        /// before performing another sink operation.
        /// Design Doc: JRN-POISON, A8
        #[test]
        #[should_panic(expected = "JRN-POISON")]
        fn commit_on_poisoned_journal_panics() {
            let mut journal = scripted_line_journal([ScriptedResult::Write(Err(
                io::Error::other("scripted write failure"),
            ))]);

            let error = journal
                .commit(&serde_json::json!({}))
                .expect_err("a real sink failure must poison the Journal");
            assert!(
                matches!(
                    error,
                    JournalError::Sink {
                        operation: SinkOperation::Write,
                        ..
                    }
                ),
                "the poisoning setup must fail during the sink write"
            );
            assert!(
                journal.is_poisoned(),
                "a real sink failure must poison the Journal before the retry"
            );
            assert_eq!(
                journal.writer.calls,
                [SinkCall::Write(b"{}\n".to_vec())],
                "the poisoning setup must perform exactly one sink operation"
            );
            assert!(
                journal.writer.results.is_empty(),
                "the sink must have no scripted result available to a poisoned retry"
            );

            let _ = journal.commit(&serde_json::json!({}));
        }
    }

    mod journal_commit_boundaries {
        use super::*;

        /// Invariant: bytes from a record remain outside the committed prefix until
        /// its flush succeeds, even when the complete line reached the sink.
        /// Design Doc: JRN-COMMIT
        #[test]
        fn only_successful_flush_advances_the_committed_boundary() {
            let mut journal = scripted_journal(
                7,
                [
                    ScriptedResult::Write(Ok(b"{\"a\":1}\n".len())),
                    ScriptedResult::Flush(Ok(())),
                    ScriptedResult::Write(Ok(b"{\"b\":2}\n".len())),
                    ScriptedResult::Flush(Err(io::Error::new(
                        io::ErrorKind::BrokenPipe,
                        "scripted second flush failure",
                    ))),
                ],
            );

            assert!(
                journal.writer.committed_bytes().is_empty(),
                "a fresh sink must have an empty committed prefix"
            );
            journal
                .commit(&serde_json::json!({"a": 1}))
                .expect("the first successful flush must commit its record");
            assert_eq!(
                journal.writer.committed_bytes(),
                b"{\"a\":1}\n",
                "a successful first flush must advance the boundary through its line"
            );
            assert!(
                journal.writer.uncertain_suffix().is_empty(),
                "a successful first flush must leave no uncertain suffix"
            );

            let error = journal
                .commit(&serde_json::json!({"b": 2}))
                .expect_err("the second record must remain uncommitted when its flush fails");

            assert!(
                matches!(
                    error,
                    JournalError::Sink {
                        operation: SinkOperation::Flush,
                        ..
                    }
                ),
                "a failed second flush must return a Flush sink error"
            );
            assert_eq!(
                journal.writer.accepted_bytes, b"{\"a\":1}\n{\"b\":2}\n",
                "both complete lines must remain visible in the sink's accepted bytes"
            );
            assert_eq!(
                journal.writer.committed_bytes(),
                b"{\"a\":1}\n",
                "a failed second flush must preserve the prior committed boundary"
            );
            assert_eq!(
                journal.writer.uncertain_suffix(),
                b"{\"b\":2}\n",
                "the complete line preceding a failed flush must be an uncertain suffix"
            );
            assert_eq!(
                journal.writer.calls,
                [
                    SinkCall::Write(b"{\"a\":1}\n".to_vec()),
                    SinkCall::Flush,
                    SinkCall::Write(b"{\"b\":2}\n".to_vec()),
                    SinkCall::Flush,
                ],
                "each record must be written before its corresponding flush"
            );
            assert!(
                journal.writer.results.is_empty(),
                "the boundary trace must consume exactly its four scripted results"
            );
            assert!(
                journal.is_poisoned(),
                "the failed second flush must poison the Journal"
            );
        }

        /// Invariant: a write failure in a later record cannot move the boundary
        /// past records whose flushes previously succeeded.
        #[test]
        fn partial_second_write_failure_preserves_the_prior_boundary() {
            let mut journal = scripted_journal(
                7,
                [
                    ScriptedResult::Write(Ok(b"{\"a\":1}\n".len())),
                    ScriptedResult::Flush(Ok(())),
                    ScriptedResult::Write(Ok(b"{\"b\":2}".len())),
                    ScriptedResult::Write(Err(io::Error::new(
                        io::ErrorKind::BrokenPipe,
                        "scripted final-byte failure",
                    ))),
                ],
            );
            journal
                .commit(&serde_json::json!({"a": 1}))
                .expect("the first record must establish a committed boundary");

            let error = journal
                .commit(&serde_json::json!({"b": 2}))
                .expect_err("the final byte of the second record must fail");

            assert!(
                matches!(
                    error,
                    JournalError::Sink {
                        operation: SinkOperation::Write,
                        ..
                    }
                ),
                "a final-byte write failure must remain classified as Write"
            );
            assert_eq!(
                journal.writer.committed_bytes(),
                b"{\"a\":1}\n",
                "a partial later write must not move the prior committed boundary"
            );
            assert_eq!(
                journal.writer.uncertain_suffix(),
                b"{\"b\":2}",
                "the accepted portion of the failed record must remain uncertain"
            );
            assert_eq!(
                journal.writer.calls,
                [
                    SinkCall::Write(b"{\"a\":1}\n".to_vec()),
                    SinkCall::Flush,
                    SinkCall::Write(b"{\"b\":2}\n".to_vec()),
                    SinkCall::Write(b"\n".to_vec()),
                ],
                "the failed record must stop after attempting its final remaining byte"
            );
            assert!(
                journal.is_poisoned(),
                "the partial second-record failure must poison the Journal"
            );
        }

        /// Invariant: a later record rejected by its first write leaves both the
        /// prior committed prefix intact and the uncertain suffix empty.
        #[test]
        fn first_write_failure_after_commit_leaves_no_uncertain_suffix() {
            let mut journal = scripted_journal(
                7,
                [
                    ScriptedResult::Write(Ok(b"{\"a\":1}\n".len())),
                    ScriptedResult::Flush(Ok(())),
                    ScriptedResult::Write(Err(io::Error::new(
                        io::ErrorKind::BrokenPipe,
                        "scripted clean second-record failure",
                    ))),
                ],
            );
            journal
                .commit(&serde_json::json!({"a": 1}))
                .expect("the first record must establish a committed boundary");

            let error = journal
                .commit(&serde_json::json!({"b": 2}))
                .expect_err("the first write of the second record must fail");

            assert!(
                matches!(
                    error,
                    JournalError::Sink {
                        operation: SinkOperation::Write,
                        ..
                    }
                ),
                "a clean second-record failure must remain classified as Write"
            );
            assert_eq!(
                journal.writer.accepted_bytes,
                b"{\"a\":1}\n",
                "a failed first write must contribute no bytes after the prior record"
            );
            assert_eq!(
                journal.writer.committed_bytes(),
                b"{\"a\":1}\n",
                "a failed first write must preserve the prior committed boundary"
            );
            assert!(
                journal.writer.uncertain_suffix().is_empty(),
                "a failed first write must leave no uncertain suffix"
            );
            assert_eq!(
                journal.writer.calls,
                [
                    SinkCall::Write(b"{\"a\":1}\n".to_vec()),
                    SinkCall::Flush,
                    SinkCall::Write(b"{\"b\":2}\n".to_vec()),
                ],
                "the clean failure must stop after the second record's first write call"
            );
            assert!(
                journal.writer.results.is_empty(),
                "the clean-failure trace must consume exactly its three scripted results"
            );
            assert!(
                journal.is_poisoned(),
                "the clean second-record failure must poison the Journal"
            );
        }

        /// Invariant: completing a line through short writes does not commit it
        /// when the immediately following flush fails.
        #[test]
        fn flush_failure_after_short_writes_leaves_a_complete_uncertain_line() {
            let mut journal = scripted_journal(
                2,
                [
                    ScriptedResult::Write(Ok(1)),
                    ScriptedResult::Write(Ok(1)),
                    ScriptedResult::Write(Ok(1)),
                    ScriptedResult::Flush(Err(io::Error::other(
                        "scripted flush after short writes",
                    ))),
                ],
            );

            let error = journal
                .commit(&serde_json::json!({}))
                .expect_err("the flush after complete short writes must fail");

            assert!(
                matches!(
                    error,
                    JournalError::Sink {
                        operation: SinkOperation::Flush,
                        ..
                    }
                ),
                "a flush failure after short writes must remain classified as Flush"
            );
            assert!(
                journal.writer.committed_bytes().is_empty(),
                "a first-record flush failure must leave the committed boundary at zero"
            );
            assert_eq!(
                journal.writer.uncertain_suffix(),
                b"{}\n",
                "the complete short-written line must remain an uncertain suffix"
            );
            assert_eq!(
                journal.writer.calls,
                [
                    SinkCall::Write(b"{}\n".to_vec()),
                    SinkCall::Write(b"}\n".to_vec()),
                    SinkCall::Write(b"\n".to_vec()),
                    SinkCall::Flush,
                ],
                "flush must occur once and only after every short write completes"
            );
            assert!(
                journal.writer.results.is_empty(),
                "the short-write flush failure must consume its complete script"
            );
            assert!(
                journal.is_poisoned(),
                "the failed flush after short writes must poison the Journal"
            );
        }
    }

    mod journal_sink_matrix {
        use super::*;

        struct FailureCase {
            name: &'static str,
            results: Vec<ScriptedResult>,
            expected_operation: SinkOperation,
            expected_kind: io::ErrorKind,
            expected_calls: Vec<SinkCall>,
            expected_accepted: Option<&'static [u8]>,
        }

        /// Invariant: every possible write or flush failure permanently poisons the
        /// Journal, and a later commit reaches neither operation a second time.
        /// Design Doc: JRN-POISON
        #[test]
        fn every_sink_failure_poisons_exactly_once() {
            let full_line = SinkCall::Write(b"{}\n".to_vec());
            let remaining_after_one = SinkCall::Write(b"}\n".to_vec());
            let cases = [
                FailureCase {
                    name: "returned write error",
                    results: vec![
                        ScriptedResult::Write(Err(io::Error::other("scripted write error"))),
                        ScriptedResult::Write(Ok(3)),
                    ],
                    expected_operation: SinkOperation::Write,
                    expected_kind: io::ErrorKind::Other,
                    expected_calls: vec![full_line.clone()],
                    expected_accepted: Some(b""),
                },
                FailureCase {
                    name: "interrupted write",
                    results: vec![
                        ScriptedResult::Write(Err(io::Error::new(
                            io::ErrorKind::Interrupted,
                            "scripted interruption",
                        ))),
                        ScriptedResult::Write(Ok(3)),
                    ],
                    expected_operation: SinkOperation::Write,
                    expected_kind: io::ErrorKind::Interrupted,
                    expected_calls: vec![full_line.clone()],
                    expected_accepted: Some(b""),
                },
                FailureCase {
                    name: "zero-progress write",
                    results: vec![ScriptedResult::Write(Ok(0)), ScriptedResult::Write(Ok(3))],
                    expected_operation: SinkOperation::Write,
                    expected_kind: io::ErrorKind::WriteZero,
                    expected_calls: vec![full_line.clone()],
                    expected_accepted: Some(b""),
                },
                FailureCase {
                    name: "over-reported first write",
                    results: vec![ScriptedResult::Write(Ok(4)), ScriptedResult::Write(Ok(3))],
                    expected_operation: SinkOperation::Write,
                    expected_kind: io::ErrorKind::InvalidData,
                    expected_calls: vec![full_line.clone()],
                    expected_accepted: None,
                },
                FailureCase {
                    name: "error after a partial write",
                    results: vec![
                        ScriptedResult::Write(Ok(1)),
                        ScriptedResult::Write(Err(io::Error::new(
                            io::ErrorKind::BrokenPipe,
                            "scripted error after progress",
                        ))),
                        ScriptedResult::Write(Ok(3)),
                    ],
                    expected_operation: SinkOperation::Write,
                    expected_kind: io::ErrorKind::BrokenPipe,
                    expected_calls: vec![full_line.clone(), remaining_after_one.clone()],
                    expected_accepted: Some(b"{"),
                },
                FailureCase {
                    name: "zero progress after a partial write",
                    results: vec![
                        ScriptedResult::Write(Ok(1)),
                        ScriptedResult::Write(Ok(0)),
                        ScriptedResult::Write(Ok(3)),
                    ],
                    expected_operation: SinkOperation::Write,
                    expected_kind: io::ErrorKind::WriteZero,
                    expected_calls: vec![full_line.clone(), remaining_after_one.clone()],
                    expected_accepted: Some(b"{"),
                },
                FailureCase {
                    name: "over-report after a partial write",
                    results: vec![
                        ScriptedResult::Write(Ok(1)),
                        ScriptedResult::Write(Ok(3)),
                        ScriptedResult::Write(Ok(3)),
                    ],
                    expected_operation: SinkOperation::Write,
                    expected_kind: io::ErrorKind::InvalidData,
                    expected_calls: vec![full_line.clone(), remaining_after_one.clone()],
                    expected_accepted: None,
                },
                FailureCase {
                    name: "returned flush error",
                    results: vec![
                        ScriptedResult::Write(Ok(3)),
                        ScriptedResult::Flush(Err(io::Error::other("scripted flush error"))),
                        ScriptedResult::Write(Ok(3)),
                    ],
                    expected_operation: SinkOperation::Flush,
                    expected_kind: io::ErrorKind::Other,
                    expected_calls: vec![full_line, SinkCall::Flush],
                    expected_accepted: Some(b"{}\n"),
                },
            ];

            for case in cases {
                let mut journal = scripted_journal(2, case.results);

                let error = journal
                    .commit(&serde_json::json!({}))
                    .expect_err("every scripted sink failure must fail its commit");
                match error {
                    JournalError::Sink { operation, error } => {
                        assert_eq!(
                            operation, case.expected_operation,
                            "{} must preserve the failing sink operation",
                            case.name
                        );
                        assert_eq!(
                            error.kind(),
                            case.expected_kind,
                            "{} must preserve or assign the required error kind",
                            case.name
                        );
                    }
                    _ => panic!("{} must return a typed sink error", case.name),
                }
                assert!(
                    journal.is_poisoned(),
                    "{} must poison the Journal before returning",
                    case.name
                );
                assert_eq!(
                    journal.writer.calls, case.expected_calls,
                    "{} must stop at exactly the failing sink call",
                    case.name
                );
                if let Some(expected_accepted) = case.expected_accepted {
                    assert_eq!(
                        journal.writer.accepted_bytes, expected_accepted,
                        "{} must retain exactly the valid reported write prefixes",
                        case.name
                    );
                }
                assert_eq!(
                    journal.writer.results.len(),
                    1,
                    "{} must leave the post-failure sentinel result untouched",
                    case.name
                );

                let accepted_before_retry = journal.writer.accepted_bytes.clone();
                let retry = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let _ = journal.commit(&serde_json::json!({}));
                }));

                assert!(
                    retry.is_err(),
                    "{} must make a later commit fail at the poison precondition",
                    case.name
                );
                assert_eq!(
                    journal.writer.calls, case.expected_calls,
                    "{} must permit no sink call after poisoning",
                    case.name
                );
                assert_eq!(
                    journal.writer.accepted_bytes, accepted_before_retry,
                    "{} must accept no additional bytes after poisoning",
                    case.name
                );
                assert_eq!(
                    journal.writer.results.len(),
                    1,
                    "{} must consume no scripted result after poisoning",
                    case.name
                );
            }
        }

        /// Invariant: the sink receives each complete encoded line followed by only
        /// its unwritten suffixes, in record order, with a flush after each line.
        /// Design Doc: JRN-SINK, TRUST-SINK
        #[test]
        fn the_sink_receives_exact_bytes_in_call_order() {
            let mut journal = scripted_journal(
                7,
                [
                    ScriptedResult::Write(Ok(2)),
                    ScriptedResult::Write(Ok(3)),
                    ScriptedResult::Write(Ok(3)),
                    ScriptedResult::Flush(Ok(())),
                    ScriptedResult::Write(Ok(8)),
                    ScriptedResult::Flush(Ok(())),
                ],
            );

            journal
                .commit(&serde_json::json!({"a": 1}))
                .expect("short writes followed by a flush must commit the first record");
            journal
                .commit(&serde_json::json!({"b": 2}))
                .expect("a complete write followed by a flush must commit the second record");

            assert_eq!(
                journal.writer.calls,
                [
                    SinkCall::Write(b"{\"a\":1}\n".to_vec()),
                    SinkCall::Write(b"a\":1}\n".to_vec()),
                    SinkCall::Write(b"1}\n".to_vec()),
                    SinkCall::Flush,
                    SinkCall::Write(b"{\"b\":2}\n".to_vec()),
                    SinkCall::Flush,
                ],
                "the sink must receive exact unwritten suffixes and flushes in call order"
            );
            assert_eq!(
                journal.writer.accepted_bytes, b"{\"a\":1}\n{\"b\":2}\n",
                "the memory sink must store exactly every valid reported prefix"
            );
            assert_eq!(
                journal.writer.committed_bytes(),
                b"{\"a\":1}\n{\"b\":2}\n",
                "both successfully flushed lines must lie within the committed boundary"
            );
            assert!(
                journal.writer.uncertain_suffix().is_empty(),
                "two successful flushes must leave no uncertain suffix"
            );
            assert!(
                journal.writer.results.is_empty(),
                "the exact-byte trace must consume every scripted result"
            );
            assert!(
                !journal.is_poisoned(),
                "successful exact-byte delivery must leave the Journal unpoisoned"
            );
        }
    }
}
