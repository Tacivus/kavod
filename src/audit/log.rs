use crate::audit::{AuditRecord, buffer::AuditBuffer, writer::Writer};

#[derive(Debug, Clone, Copy)]
pub struct Bounds {}

/// `Log` is the main type that handles everything for the canotical Kavod audit log.
pub struct Log<W: Writer> {
    writer: W,
    bounds: Bounds,
}

impl<W: Writer> Log<W> {
    pub fn new(writer: W, bounds: &Bounds) {}

    /// Submits a new record to the audit queue.
    pub fn submit<R: AuditRecord>(&mut self, record: &R) -> Result<(), ()> {
        todo!()
    }
}
