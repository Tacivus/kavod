use std::{io::Write, num::NonZeroUsize};

use serde::Serialize;

pub enum JournalBuildError {
    MaxBytesTooLarge,
}

pub enum JournalError {
    Encode(serde_json::Error),
    NotAnObject,
    BoundExceeded,
    Sink {
        operation: SinkOperation,
        error: std::io::Error,
    },
}

pub enum SinkOperation {
    Write,
    Flush,
}

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
    /// Returns `Err(JournalBuildError::MaxBytesTooLarge)` if the `max_record_bytes`
    /// are equal to `usize::MAX` to ensure at least a single byte is reserved for
    /// the `\n` byte.
    pub fn new(writer: W, max_record_bytes: NonZeroUsize) -> Result<Self, JournalBuildError> {
        if max_record_bytes.get() == usize::MAX {
            return Err(JournalBuildError::MaxBytesTooLarge);
        }

        Ok(Self {
            writer,
            buffer: BoundedBuffer::new(max_record_bytes.saturating_add(1)), // +1 for the newline character
            is_poisoned: false,
        })
    }

    /// Encodes `record` as one JSON object, writes its terminating newline, and
    /// flushes the sink.
    ///
    /// Encoding and bound failures write nothing and do not poison the Journal.
    /// A write or flush failure permanently poisons it. Panics if already poisoned.
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
        if !(buff.written_bytes().starts_with(b"{") && buff.written_bytes().ends_with(b"}")) {
            return Err(JournalError::NotAnObject);
        }

        // Write the reserved newline byte.
        let newline_result = buff.write(b"\n");
        if buff.exceeded {
            return Err(JournalError::BoundExceeded);
        }
        match newline_result {
            Err(error) => panic!("memory write failed: {}", error),
            Ok(len) => assert!(len == 1, "`\n` wrote more than one byte: {}", len),
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
