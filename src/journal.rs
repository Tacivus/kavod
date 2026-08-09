use std::{io::Write, num::NonZeroUsize};

use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JournalBuildError {
    MaxBytesTooLarge,
}

#[derive(Debug)]
pub enum JournalError {
    Encode(serde_json::Error),
    NotAnObject,
    BoundExceeded,
    Sink {
        operation: SinkOperation,
        error: std::io::Error,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SinkOperation {
    Write,
    Flush,
}

#[derive(Debug)]
pub struct Journal<W: std::io::Write> {
    writer: W,
    buffer: BoundedBuffer,
    is_poisoned: bool,
}

impl<W: std::io::Write> Journal<W> {
    /// Creates a Journal with one fixed-capacity reusable record buffer.
    ///
    /// `max_record_bytes` bounds the encoded JSON object. The JSON Lines newline
    /// is excluded from that limit, reserved in the same buffer, and written with
    /// the encoded object through one sink-writing loop.
    ///
    /// # Errors
    ///
    /// Returns `Err(JournalBuildError::MaxBytesTooLarge)` if `max_record_bytes`
    /// equals `usize::MAX`, which would leave no room for the reserved `\n` byte.
    pub fn new(writer: W, max_record_bytes: NonZeroUsize) -> Result<Self, JournalBuildError> {
        // +1 for the newline character
        let max_bytes = max_record_bytes
            .checked_add(1)
            .ok_or(JournalBuildError::MaxBytesTooLarge)?;

        Ok(Self {
            writer,
            buffer: BoundedBuffer::new(max_bytes),
            is_poisoned: false,
        })
    }

    /// Encodes `record` as one JSON object, writes its terminating newline, and
    /// flushes the sink.
    ///
    /// # Errors
    ///
    /// Encoding and bound failures (`Encode`, `NotAnObject`, `BoundExceeded`)
    /// write nothing and do not poison the Journal. A write or flush failure
    /// (`Sink`) permanently poisons it.
    ///
    /// # Panics
    ///
    /// Panics if the Journal is already poisoned.
    pub fn commit<R: Serialize>(&mut self, record: &R) -> Result<(), JournalError> {
        assert!(!self.is_poisoned, "commit called on a poisoned journal");

        Self::write_record_to_buffer(&mut self.buffer, record)?;

        if let Err(error) = Self::write_to_sink(&mut self.writer, self.buffer.written_bytes()) {
            return Err(self.poison(SinkOperation::Write, error));
        }

        if let Err(error) = self.writer.flush() {
            return Err(self.poison(SinkOperation::Flush, error));
        }

        Ok(())
    }

    /// Encodes one JSON object into the reusable fixed-capacity buffer
    fn write_record_to_buffer<R: Serialize>(
        buff: &mut BoundedBuffer,
        record: &R,
    ) -> Result<(), JournalError> {
        buff.clear();

        // Write the json
        let encode_result = serde_json::to_writer(&mut *buff, record);
        if buff.exceeded {
            return Err(JournalError::BoundExceeded);
        }
        if let Err(error) = encode_result {
            return Err(JournalError::Encode(error));
        }

        // Ensure the string looks like a json object
        let encoded = buff.written_bytes();
        if !(encoded.starts_with(b"{") && encoded.ends_with(b"}")) {
            return Err(JournalError::NotAnObject);
        }

        // Write the reserved newline byte. `BoundedBuffer::write` cannot fail for
        // any reason other than the bound check above, but this stays explicit so
        // a future change to that behavior fails loudly instead of silently
        // mislabeling itself as `BoundExceeded`.
        let newline_result = buff.write(b"\n");
        if buff.exceeded {
            return Err(JournalError::BoundExceeded);
        }
        match newline_result {
            Err(error) => panic!("memory write failed: {error}"),
            Ok(len) => assert!(len == 1, "`\n` wrote more than one byte: {len}"),
        }

        Ok(())
    }

    /// Writes all bytes to the sink without retrying interrupted writes.
    fn write_to_sink(writer: &mut W, bytes: &[u8]) -> std::io::Result<()> {
        let mut written = 0;

        while written < bytes.len() {
            match writer.write(&bytes[written..]) {
                Ok(0) => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::WriteZero,
                        "journal sink made no write progress",
                    ));
                }
                Ok(count) if count > bytes.len() - written => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "journal sink reported writing more bytes than supplied",
                    ));
                }
                Ok(count) => written += count,
                Err(error) => return Err(error),
            }
        }

        Ok(())
    }

    /// Returns whether a prior sink failure permanently disabled this Journal.
    pub fn is_poisoned(&self) -> bool {
        self.is_poisoned
    }

    /// Marks the Journal poisoned and packages the sink failure.
    fn poison(&mut self, operation: SinkOperation, error: std::io::Error) -> JournalError {
        self.is_poisoned = true;
        JournalError::Sink { operation, error }
    }
}

#[derive(Debug)]
struct BoundedBuffer {
    bytes: Box<[u8]>,
    len: usize,
    exceeded: bool,
}

impl BoundedBuffer {
    fn new(max_record_bytes: NonZeroUsize) -> Self {
        Self {
            bytes: vec![0_u8; max_record_bytes.get()].into_boxed_slice(),
            len: 0,
            exceeded: false,
        }
    }

    fn clear(&mut self) {
        self.len = 0;
        self.exceeded = false;
    }

    fn written_bytes(&self) -> &[u8] {
        assert!(
            self.len <= self.bytes.len(),
            "bounded buffer cursor exceeds its capacity"
        );

        &self.bytes[..self.len]
    }
}

impl std::io::Write for BoundedBuffer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        assert!(
            self.len <= self.bytes.len(),
            "bounded buffer cursor exceeds its capacity"
        );

        let remaining = self.bytes.len() - self.len;

        if bytes.len() > remaining {
            self.exceeded = true;
            return Err(std::io::Error::new(
                std::io::ErrorKind::WriteZero,
                "journal record exceeds configured bound",
            ));
        }

        let end = self
            .len
            .checked_add(bytes.len())
            .expect("bounded buffer cursor overflow");
        self.bytes[self.len..end].copy_from_slice(bytes);
        self.len = end;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{
        cell::RefCell,
        collections::{BTreeMap, VecDeque},
        io::{self, ErrorKind, Write},
        num::NonZeroUsize,
        panic::{AssertUnwindSafe, catch_unwind},
        rc::Rc,
    };

    use serde::{
        Serialize, Serializer,
        ser::{Error as _, SerializeMap},
    };

    use super::{BoundedBuffer, Journal, JournalBuildError, JournalError, SinkOperation};

    // Setup

    #[derive(Serialize)]
    struct ObjectRecord<'a> {
        value: &'a str,
    }

    /// Writes a partial JSON object before returning a serializer error.
    struct FailsAfterPartialEncoding;

    impl Serialize for FailsAfterPartialEncoding {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            let mut map = serializer.serialize_map(Some(1))?;
            map.serialize_entry("partial", "was buffered")?;
            Err(S::Error::custom("intentional serializer failure"))
        }
    }

    #[derive(Clone, Copy)]
    enum WriteAction {
        Accept(usize),
        Error(ErrorKind),
    }

    #[derive(Clone, Copy)]
    enum FlushAction {
        Ok,
        Error(ErrorKind),
    }

    #[derive(Default)]
    struct SinkState {
        bytes: Vec<u8>,
        write_inputs: Vec<Vec<u8>>,
        flush_calls: usize,
    }

    struct ScriptedWriter {
        state: Rc<RefCell<SinkState>>,
        write_actions: VecDeque<WriteAction>,
        flush_actions: VecDeque<FlushAction>,
    }

    impl ScriptedWriter {
        fn new(
            state: Rc<RefCell<SinkState>>,
            write_actions: impl IntoIterator<Item = WriteAction>,
            flush_actions: impl IntoIterator<Item = FlushAction>,
        ) -> Self {
            Self {
                state,
                write_actions: write_actions.into_iter().collect(),
                flush_actions: flush_actions.into_iter().collect(),
            }
        }
    }

    impl Write for ScriptedWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            let action = self
                .write_actions
                .pop_front()
                .unwrap_or(WriteAction::Accept(bytes.len()));

            let mut state = self.state.borrow_mut();
            state.write_inputs.push(bytes.to_vec());

            match action {
                WriteAction::Accept(count) => {
                    if count <= bytes.len() {
                        state.bytes.extend_from_slice(&bytes[..count]);
                    }
                    Ok(count)
                }
                WriteAction::Error(kind) => Err(io::Error::from(kind)),
            }
        }

        fn flush(&mut self) -> io::Result<()> {
            let action = self.flush_actions.pop_front().unwrap_or(FlushAction::Ok);

            self.state.borrow_mut().flush_calls += 1;

            match action {
                FlushAction::Ok => Ok(()),
                FlushAction::Error(kind) => Err(io::Error::from(kind)),
            }
        }
    }

    fn nonzero(value: usize) -> NonZeroUsize {
        NonZeroUsize::new(value).expect("test bounds must be nonzero")
    }

    fn journal(
        max_record_bytes: usize,
        write_actions: impl IntoIterator<Item = WriteAction>,
        flush_actions: impl IntoIterator<Item = FlushAction>,
    ) -> (Journal<ScriptedWriter>, Rc<RefCell<SinkState>>) {
        let state = Rc::new(RefCell::new(SinkState::default()));
        let writer = ScriptedWriter::new(Rc::clone(&state), write_actions, flush_actions);

        (
            Journal::new(writer, nonzero(max_record_bytes)).unwrap(),
            state,
        )
    }

    fn assert_no_sink_effect(state: &Rc<RefCell<SinkState>>) {
        let state = state.borrow();

        assert!(
            state.bytes.is_empty(),
            "failure unexpectedly wrote bytes to the sink"
        );
        assert!(
            state.write_inputs.is_empty(),
            "failure unexpectedly called sink.write"
        );
        assert_eq!(
            state.flush_calls, 0,
            "failure unexpectedly called sink.flush"
        );
    }

    fn assert_sink_error(
        result: Result<(), JournalError>,
        expected_write_operation: bool,
        expected_kind: ErrorKind,
    ) {
        match result {
            Err(JournalError::Sink { operation, error }) => {
                assert_eq!(error.kind(), expected_kind);

                assert_eq!(
                    matches!(operation, SinkOperation::Write),
                    expected_write_operation,
                    "unexpected sink operation"
                );
            }
            _ => panic!("expected JournalError::Sink"),
        }
    }

    fn assert_not_an_object<R: Serialize>(record: R) {
        let (mut journal, state) = journal(128, [], []);

        assert!(matches!(
            journal.commit(&record),
            Err(JournalError::NotAnObject)
        ));
        assert!(!journal.is_poisoned());
        assert_no_sink_effect(&state);

        journal
            .commit(&ObjectRecord { value: "recovery" })
            .expect("format rejection must not disable the journal");

        let state = state.borrow();
        assert_eq!(state.bytes, b"{\"value\":\"recovery\"}\n".to_vec());
        assert_eq!(state.write_inputs.len(), 1);
        assert_eq!(state.flush_calls, 1);
    }

    mod journal_construction_and_capacity_validation {
        use super::*;

        // ========================================================================
        // Journal::new Construction and Capacity Validation
        // ========================================================================

        /// Invariant: A freshly constructed Journal is usable and unpoisoned.
        #[test]
        fn new_journal_starts_unpoisoned() {
            let (journal, state) = journal(1, [], []);

            assert!(!journal.is_poisoned());
            assert_no_sink_effect(&state);
        }

        /// Invariant: max_record_bytes must reserve one extra byte for newline.
        #[test]
        fn new_rejects_max_record_bytes_that_overflow_with_newline() {
            let writer = ScriptedWriter::new(Rc::new(RefCell::new(SinkState::default())), [], []);

            assert!(matches!(
                Journal::new(writer, nonzero(usize::MAX)),
                Err(JournalBuildError::MaxBytesTooLarge)
            ));
        }
    }

    mod journal_jsonl_encoding_and_successful_commit_protocol {
        use super::*;

        // ========================================================================
        // Journal::commit JSONL Encoding and Successful Commit Protocol
        // ========================================================================

        /// Invariant: One successful commit writes one JSON object, one newline, and flushes once.
        #[test]
        fn commit_writes_json_object_newline_and_flushes() {
            let (mut journal, state) = journal(128, [], []);
            let record = ObjectRecord { value: "hello" };

            journal.commit(&record).unwrap();

            let state = state.borrow();
            assert_eq!(state.bytes, b"{\"value\":\"hello\"}\n".to_vec());
            assert_eq!(state.write_inputs.len(), 1);
            assert_eq!(state.flush_calls, 1);
            assert!(!journal.is_poisoned());
        }

        /// Invariant: Record order is sink order, with exactly one newline per record.
        #[test]
        fn multiple_commits_preserve_order_without_blank_lines() {
            let (mut journal, state) = journal(128, [], []);

            journal.commit(&ObjectRecord { value: "first" }).unwrap();
            journal.commit(&ObjectRecord { value: "second" }).unwrap();

            let state = state.borrow();
            assert_eq!(
                state.bytes,
                b"{\"value\":\"first\"}\n{\"value\":\"second\"}\n".to_vec()
            );
            assert_eq!(state.flush_calls, 2);
            assert_eq!(state.write_inputs.len(), 2);
        }

        /// Invariant: Reusing the encode buffer cannot leak bytes from a prior longer record.
        #[test]
        fn short_record_after_long_record_has_no_stale_bytes() {
            let (mut journal, state) = journal(256, [], []);

            journal
                .commit(&ObjectRecord {
                    value: "this record is deliberately much longer than the next",
                })
                .unwrap();
            journal.commit(&ObjectRecord { value: "x" }).unwrap();

            assert_eq!(
            state.borrow().bytes,
            b"{\"value\":\"this record is deliberately much longer than the next\"}\n{\"value\":\"x\"}\n"
                .to_vec()
        );
        }

        /// Invariant: max_record_bytes bounds the object only; the newline is additional storage.
        #[test]
        fn object_exactly_at_payload_bound_is_accepted() {
            let record = ObjectRecord { value: "exact" };
            let payload = serde_json::to_vec(&record).unwrap();
            let (mut journal, state) = journal(payload.len(), [], []);

            journal.commit(&record).unwrap();

            let mut expected = payload;
            expected.push(b'\n');

            let state = state.borrow();
            assert_eq!(state.bytes, expected);
            assert_eq!(state.flush_calls, 1);
        }

        /// Invariant: A JSON map is an accepted JSON object record.
        #[test]
        fn map_object_is_accepted() {
            let mut record = BTreeMap::new();
            record.insert("alpha", 7_u8);

            let (mut journal, state) = journal(128, [], []);
            journal.commit(&record).unwrap();

            assert_eq!(state.borrow().bytes, b"{\"alpha\":7}\n".to_vec());
        }

        /// Invariant: Short successful writes are retried until the prepared record is complete.
        #[test]
        fn partial_writes_complete_the_record_before_flush() {
            let record = ObjectRecord { value: "partial" };
            let expected = b"{\"value\":\"partial\"}\n";

            let (mut journal, state) =
                journal(128, [WriteAction::Accept(1), WriteAction::Accept(2)], []);

            journal.commit(&record).unwrap();

            let state = state.borrow();
            assert_eq!(state.bytes, expected.to_vec());
            assert_eq!(
                state.write_inputs,
                vec![
                    expected.to_vec(),
                    expected[1..].to_vec(),
                    expected[3..].to_vec(),
                ]
            );
            assert_eq!(state.flush_calls, 1);
        }
    }

    mod journal_record_shape_and_serde_encoding_rejection {
        use super::*;

        // ========================================================================
        // Journal::commit Record Shape and Serde Encoding Rejection
        // ========================================================================

        /// Invariant: Scalar JSON values are not journal records and reach no sink operation.
        #[test]
        fn scalar_payloads_are_rejected_as_not_an_object() {
            assert_not_an_object(true);
            assert_not_an_object(42_i64);
            assert_not_an_object(String::from("not an object"));
        }

        /// Invariant: Arrays and null are not journal records and reach no sink operation.
        #[test]
        fn array_and_unit_payloads_are_rejected_as_not_an_object() {
            assert_not_an_object(vec![1_u8, 2_u8, 3_u8]);
            assert_not_an_object(());
        }

        /// Invariant: A serializer error after partial buffer output writes nothing to the sink.
        #[test]
        fn partial_encoding_error_writes_nothing_and_does_not_poison() {
            let (mut journal, state) = journal(128, [], []);

            assert!(matches!(
                journal.commit(&FailsAfterPartialEncoding),
                Err(JournalError::Encode(_))
            ));
            assert!(!journal.is_poisoned());
            assert_no_sink_effect(&state);

            journal
                .commit(&ObjectRecord { value: "recovery" })
                .expect("encode failure must not disable the journal");

            let state = state.borrow();
            assert_eq!(state.bytes, b"{\"value\":\"recovery\"}\n".to_vec());
            assert_eq!(state.write_inputs.len(), 1);
            assert_eq!(state.flush_calls, 1);
        }

        /// Invariant: Map keys not representable as JSON strings are Encode failures.
        #[test]
        fn unsupported_map_key_is_an_encode_error_without_sink_effect() {
            let mut record = BTreeMap::new();
            record.insert((1_u8, 2_u8), 3_u8);

            let (mut journal, state) = journal(128, [], []);

            assert!(matches!(
                journal.commit(&record),
                Err(JournalError::Encode(_))
            ));
            assert!(!journal.is_poisoned());
            assert_no_sink_effect(&state);

            journal
                .commit(&ObjectRecord { value: "recovery" })
                .expect("encode failure must not disable the journal");

            let state = state.borrow();
            assert_eq!(state.bytes, b"{\"value\":\"recovery\"}\n".to_vec());
            assert_eq!(state.write_inputs.len(), 1);
            assert_eq!(state.flush_calls, 1);
        }
    }

    mod journal_payload_bound_enforcement {
        use super::*;

        // ========================================================================
        // Journal::commit Payload Bound Enforcement
        // ========================================================================

        /// Invariant: An object one byte over max_record_bytes writes and flushes nothing.
        #[test]
        fn oversized_object_is_rejected_without_sink_effect_or_poisoning() {
            let record = ObjectRecord {
                value: "one byte over",
            };
            let payload = serde_json::to_vec(&record).unwrap();
            let (mut journal, state) = journal(payload.len() - 1, [], []);

            assert!(matches!(
                journal.commit(&record),
                Err(JournalError::BoundExceeded)
            ));
            assert!(!journal.is_poisoned());
            assert_no_sink_effect(&state);

            journal
                .commit(&ObjectRecord { value: "recovery" })
                .expect("bound failure must not disable the original journal");

            let state = state.borrow();
            assert_eq!(state.bytes, b"{\"value\":\"recovery\"}\n".to_vec());
            assert_eq!(state.write_inputs.len(), 1);
            assert_eq!(state.flush_calls, 1);
        }

        /// Invariant: Overflow during JSON encoding is reported as BoundExceeded,
        /// rather than as serde_json's underlying buffer write error.
        #[test]
        fn object_overflow_during_encoding_is_rejected_as_bound_exceeded() {
            let record = ObjectRecord {
                value: "two bytes over",
            };
            let payload = serde_json::to_vec(&record).unwrap();
            let (mut journal, state) = journal(payload.len() - 2, [], []);

            assert!(matches!(
                journal.commit(&record),
                Err(JournalError::BoundExceeded)
            ));
            assert!(!journal.is_poisoned());
            assert_no_sink_effect(&state);
        }

        /// Invariant: A bound below the two-byte `{}` payload rejects every object.
        #[test]
        fn one_byte_payload_bound_rejects_smallest_object() {
            let record: BTreeMap<&str, u8> = BTreeMap::new();
            let (mut journal, state) = journal(1, [], []);

            assert!(matches!(
                journal.commit(&record),
                Err(JournalError::BoundExceeded)
            ));
            assert!(!journal.is_poisoned());
            assert_no_sink_effect(&state);
        }

        /// Invariant: Bounds apply to JSON's encoded bytes, including escapes and UTF-8.
        #[test]
        fn payload_bound_counts_escaped_and_utf8_bytes() {
            let record = ObjectRecord {
                value: "quote: \"\n\u{00e9}",
            };
            let payload = b"{\"value\":\"quote: \\\"\\n\xc3\xa9\"}";

            assert_eq!(serde_json::to_vec(&record).unwrap(), payload);

            let (mut exact_journal, exact_state) = journal(payload.len(), [], []);
            exact_journal.commit(&record).unwrap();

            let mut expected = payload.to_vec();
            expected.push(b'\n');
            assert_eq!(exact_state.borrow().bytes, expected);

            let (mut short_journal, short_state) = journal(payload.len() - 1, [], []);
            assert!(matches!(
                short_journal.commit(&record),
                Err(JournalError::BoundExceeded)
            ));
            assert!(!short_journal.is_poisoned());
            assert_no_sink_effect(&short_state);
        }

        /// Invariant: Objects are accepted iff their encoded length is at most max_record_bytes.
        #[test]
        fn payload_bound_size_sweep() {
            for value_len in 0..=64 {
                let value = "x".repeat(value_len);
                let record = ObjectRecord { value: &value };
                let payload = serde_json::to_vec(&record).unwrap();

                for max_record_bytes in [payload.len() - 1, payload.len(), payload.len() + 1] {
                    let (mut journal, state) = journal(max_record_bytes, [], []);
                    let result = journal.commit(&record);

                    if payload.len() <= max_record_bytes {
                        assert!(result.is_ok());
                        let mut expected = payload.clone();
                        expected.push(b'\n');
                        assert_eq!(state.borrow().bytes, expected);
                    } else {
                        assert!(matches!(result, Err(JournalError::BoundExceeded)));
                        assert!(!journal.is_poisoned());
                        assert_no_sink_effect(&state);
                    }
                }
            }
        }

        /// Invariant: A rejected record leaves no residue in the reused buffer for a
        /// later successful commit, even when the rejection left the buffer full.
        #[test]
        fn bound_exceeded_commit_leaves_no_residue_in_next_commit() {
            let long_record = ObjectRecord {
                value: "this value is deliberately too long to fit the configured bound",
            };
            let payload = serde_json::to_vec(&long_record).unwrap();
            let (mut journal, state) = journal(payload.len() - 1, [], []);

            assert!(matches!(
                journal.commit(&long_record),
                Err(JournalError::BoundExceeded)
            ));
            assert_no_sink_effect(&state);

            journal.commit(&ObjectRecord { value: "x" }).unwrap();

            assert_eq!(state.borrow().bytes, b"{\"value\":\"x\"}\n".to_vec());
        }

        /// Invariant: The smallest possible JSON object commits at the smallest bound
        /// that can hold it plus the reserved newline byte.
        #[test]
        fn smallest_possible_object_commits_at_minimal_bound() {
            let record: BTreeMap<&str, u8> = BTreeMap::new();
            let (mut journal, state) = journal(2, [], []);

            journal.commit(&record).unwrap();

            assert_eq!(state.borrow().bytes, b"{}\n".to_vec());
        }
    }

    mod journal_sink_write_loop_and_write_failures {
        use super::*;

        // ========================================================================
        // Journal Sink Write Loop: Partial Progress and Write Failures
        // ========================================================================

        /// Invariant: A write failure before progress poisons the Journal and reports Write.
        #[test]
        fn immediate_write_error_poison_journal() {
            let (mut journal, state) =
                journal(128, [WriteAction::Error(ErrorKind::PermissionDenied)], []);

            assert_sink_error(
                journal.commit(&ObjectRecord { value: "record" }),
                true,
                ErrorKind::PermissionDenied,
            );
            assert!(journal.is_poisoned());

            let state = state.borrow();
            assert_eq!(state.bytes, Vec::<u8>::new());
            assert_eq!(state.write_inputs.len(), 1);
            assert_eq!(state.flush_calls, 0);
        }

        /// Invariant: A write failure after partial progress poisons the Journal.
        #[test]
        fn partial_write_then_error_poison_journal() {
            let expected = b"{\"value\":\"record\"}\n";
            let (mut journal, state) = journal(
                128,
                [
                    WriteAction::Accept(4),
                    WriteAction::Error(ErrorKind::BrokenPipe),
                ],
                [],
            );

            assert_sink_error(
                journal.commit(&ObjectRecord { value: "record" }),
                true,
                ErrorKind::BrokenPipe,
            );
            assert!(journal.is_poisoned());

            let state = state.borrow();
            assert_eq!(state.bytes, expected[..4].to_vec());
            assert_eq!(state.write_inputs.len(), 2);
            assert_eq!(state.flush_calls, 0);
        }

        /// Invariant: Ok(0) becomes WriteZero and permanently poisons the Journal.
        #[test]
        fn zero_progress_write_poison_journal_with_write_zero() {
            let (mut journal, state) = journal(128, [WriteAction::Accept(0)], []);

            assert_sink_error(
                journal.commit(&ObjectRecord { value: "record" }),
                true,
                ErrorKind::WriteZero,
            );
            assert!(journal.is_poisoned());

            let state = state.borrow();
            assert_eq!(state.write_inputs.len(), 1);
            assert_eq!(state.flush_calls, 0);
        }

        /// Invariant: Ok(0) poisons with WriteZero even after earlier partial progress,
        /// not only when it is the very first write.
        #[test]
        fn zero_progress_write_after_partial_progress_poison_journal_with_write_zero() {
            let (mut journal, state) =
                journal(128, [WriteAction::Accept(4), WriteAction::Accept(0)], []);

            assert_sink_error(
                journal.commit(&ObjectRecord { value: "record" }),
                true,
                ErrorKind::WriteZero,
            );
            assert!(journal.is_poisoned());

            let state = state.borrow();
            assert_eq!(state.write_inputs.len(), 2);
            assert_eq!(state.flush_calls, 0);
        }

        /// Invariant: Interrupted writes are not retried and permanently poison the Journal.
        #[test]
        fn interrupted_write_is_not_retried() {
            let (mut journal, state) =
                journal(128, [WriteAction::Error(ErrorKind::Interrupted)], []);

            assert_sink_error(
                journal.commit(&ObjectRecord { value: "record" }),
                true,
                ErrorKind::Interrupted,
            );
            assert!(journal.is_poisoned());

            let state = state.borrow();
            assert_eq!(
                state.write_inputs.len(),
                1,
                "Interrupted writes must not be retried"
            );
            assert_eq!(state.flush_calls, 0);
        }

        /// Invariant: An Interrupted write is not retried even mid-loop, after progress.
        #[test]
        fn interrupted_write_after_partial_progress_is_not_retried() {
            let (mut journal, state) = journal(
                128,
                [
                    WriteAction::Accept(4),
                    WriteAction::Error(ErrorKind::Interrupted),
                ],
                [],
            );

            assert_sink_error(
                journal.commit(&ObjectRecord { value: "record" }),
                true,
                ErrorKind::Interrupted,
            );
            assert!(journal.is_poisoned());

            let state = state.borrow();
            assert_eq!(
                state.write_inputs.len(),
                2,
                "a mid-loop Interrupted write must not be retried"
            );
            assert_eq!(state.flush_calls, 0);
        }

        /// Invariant: A sink cannot claim to have written more bytes than it received.
        #[test]
        fn oversized_write_count_poison_journal_with_invalid_data() {
            let (mut journal, state) = journal(128, [WriteAction::Accept(usize::MAX)], []);

            assert_sink_error(
                journal.commit(&ObjectRecord { value: "record" }),
                true,
                ErrorKind::InvalidData,
            );
            assert!(journal.is_poisoned());

            let state = state.borrow();
            assert_eq!(state.bytes, Vec::<u8>::new());
            assert_eq!(state.write_inputs.len(), 1);
            assert_eq!(state.flush_calls, 0);
        }

        /// Invariant: An oversized write-count claim is judged against the bytes still
        /// owed, not the record's total length, so it is caught even mid-loop.
        #[test]
        fn oversized_write_count_after_partial_progress_poison_journal_with_invalid_data() {
            let (mut journal, state) =
                journal(128, [WriteAction::Accept(4), WriteAction::Accept(17)], []);

            assert_sink_error(
                journal.commit(&ObjectRecord { value: "record" }),
                true,
                ErrorKind::InvalidData,
            );
            assert!(journal.is_poisoned());

            let state = state.borrow();
            assert_eq!(state.write_inputs.len(), 2);
            assert_eq!(state.flush_calls, 0);
        }

        /// Invariant: A write failure short-circuits before flush is ever attempted,
        /// even when a flush failure is also scripted to occur.
        #[test]
        fn write_error_short_circuits_before_flush_is_attempted() {
            let (mut journal, state) = journal(
                128,
                [WriteAction::Error(ErrorKind::PermissionDenied)],
                [FlushAction::Error(ErrorKind::BrokenPipe)],
            );

            assert_sink_error(
                journal.commit(&ObjectRecord { value: "record" }),
                true,
                ErrorKind::PermissionDenied,
            );
            assert!(journal.is_poisoned());

            let state = state.borrow();
            assert_eq!(state.write_inputs.len(), 1);
            assert_eq!(
                state.flush_calls, 0,
                "a write failure must never reach flush"
            );
        }

        /// Invariant: The write-retry loop completes correctly no matter how finely
        /// the sink fragments its progress, one byte at a time.
        #[test]
        fn single_byte_writes_complete_the_record_before_flush() {
            let record = ObjectRecord { value: "record" };
            let expected = b"{\"value\":\"record\"}\n";

            let write_actions = std::iter::repeat(WriteAction::Accept(1)).take(expected.len());
            let (mut journal, state) = journal(128, write_actions, []);

            journal.commit(&record).unwrap();

            let state = state.borrow();
            assert_eq!(state.bytes, expected.to_vec());
            assert_eq!(state.write_inputs.len(), expected.len());
            assert_eq!(state.flush_calls, 1);
        }
    }

    mod journal_sink_flush_commitment_and_flush_failures {
        use super::*;

        // ========================================================================
        // Journal Sink Flush Commitment and Flush Failures
        // ========================================================================

        /// Invariant: A flush error poisons after all record bytes were offered to the sink.
        #[test]
        fn flush_error_poison_journal_and_reports_flush_operation() {
            let expected = b"{\"value\":\"record\"}\n";
            let (mut journal, state) =
                journal(128, [], [FlushAction::Error(ErrorKind::BrokenPipe)]);

            assert_sink_error(
                journal.commit(&ObjectRecord { value: "record" }),
                false,
                ErrorKind::BrokenPipe,
            );
            assert!(journal.is_poisoned());

            let state = state.borrow();
            assert_eq!(state.bytes, expected.to_vec());
            assert_eq!(state.write_inputs.len(), 1);
            assert_eq!(state.flush_calls, 1);
        }
    }

    mod journal_poison_state_and_post_failure_call_guard {
        use super::*;

        // ========================================================================
        // Journal Poison State and Post-Failure Call Guard
        // ========================================================================

        /// Invariant: Once poisoned, Journal performs no later explicit write or flush operation.
        #[test]
        fn poisoned_journal_panics_before_any_later_sink_operation() {
            let (mut journal, state) =
                journal(128, [WriteAction::Error(ErrorKind::BrokenPipe)], []);

            assert_sink_error(
                journal.commit(&ObjectRecord { value: "first" }),
                true,
                ErrorKind::BrokenPipe,
            );
            assert!(journal.is_poisoned());

            let panic = catch_unwind(AssertUnwindSafe(|| {
                let _ = journal.commit(&ObjectRecord { value: "second" });
            }));

            assert!(panic.is_err());

            let state = state.borrow();
            assert_eq!(state.write_inputs.len(), 1);
            assert_eq!(state.flush_calls, 0);
        }

        /// Invariant: Poisoning via a flush failure also blocks all later commits,
        /// not only poisoning caused by a write failure.
        #[test]
        fn flush_poisoned_journal_panics_before_any_later_sink_operation() {
            let (mut journal, state) =
                journal(128, [], [FlushAction::Error(ErrorKind::BrokenPipe)]);

            assert_sink_error(
                journal.commit(&ObjectRecord { value: "first" }),
                false,
                ErrorKind::BrokenPipe,
            );
            assert!(journal.is_poisoned());

            let panic = catch_unwind(AssertUnwindSafe(|| {
                let _ = journal.commit(&ObjectRecord { value: "second" });
            }));

            assert!(panic.is_err());

            let state = state.borrow();
            assert_eq!(state.write_inputs.len(), 1);
            assert_eq!(state.flush_calls, 1);
        }
    }

    mod bounded_buffer_capacity_and_reset_behavior {
        use super::*;

        // ========================================================================
        // BoundedBuffer Internal Capacity and Reset Behavior
        // ========================================================================

        /// Invariant: A rejected BoundedBuffer write leaves existing bytes unchanged.
        #[test]
        fn bounded_buffer_rejects_oversized_write_without_extending() {
            let mut buffer = BoundedBuffer::new(nonzero(3));

            assert_eq!(buffer.write(b"abc").unwrap(), 3);

            let error = buffer.write(b"d").unwrap_err();
            assert_eq!(error.kind(), ErrorKind::WriteZero);
            assert_eq!(buffer.written_bytes(), b"abc");
            assert!(buffer.exceeded);
        }

        /// Invariant: Clearing BoundedBuffer resets both cursor and sticky overflow state.
        #[test]
        fn bounded_buffer_clear_resets_record_state() {
            let mut buffer = BoundedBuffer::new(nonzero(3));

            buffer.write(b"abc").unwrap();
            assert!(buffer.write(b"d").is_err());
            assert!(buffer.exceeded);

            buffer.clear();

            assert_eq!(buffer.written_bytes(), b"");
            assert!(!buffer.exceeded);

            buffer.write(b"xy").unwrap();
            assert_eq!(buffer.written_bytes(), b"xy");
        }

        /// Invariant: A zero-length write is always accepted, even at full capacity.
        #[test]
        fn bounded_buffer_zero_length_write_at_full_capacity_is_accepted() {
            let mut buffer = BoundedBuffer::new(nonzero(3));

            assert_eq!(buffer.write(b"abc").unwrap(), 3);
            assert_eq!(buffer.write(b"").unwrap(), 0);

            assert_eq!(buffer.written_bytes(), b"abc");
            assert!(!buffer.exceeded);
        }

        /// Invariant: Successive writes without an intervening clear accumulate in order.
        #[test]
        fn bounded_buffer_accumulates_across_multiple_writes() {
            let mut buffer = BoundedBuffer::new(nonzero(5));

            assert_eq!(buffer.write(b"ab").unwrap(), 2);
            assert_eq!(buffer.write(b"cd").unwrap(), 2);

            assert_eq!(buffer.written_bytes(), b"abcd");
        }
    }
}
