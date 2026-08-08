use crate::audit::EncodedAuditRecord;

/// `Writer` represents the main actor who actually writes the audit log.
pub trait Writer {
    type Fatal;

    /// Writer the encoded records
    fn write(&mut self, records: &[EncodedAuditRecord]) -> Result<(), Self::Fatal>;
}
