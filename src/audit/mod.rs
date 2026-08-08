use crate::audit::buffer::AuditBuffer;

pub mod buffer;
pub mod log;
pub mod writer;

struct EncodedAuditRecord([u8]);

pub trait AuditEncode {
    fn encode(&self, out: &mut AuditBuffer) -> Result<(), ()>;
}

pub trait AuditRecord: AuditEncode {
    fn policy(&self) -> SyncPolicy;
}

/// `SyncPolicy` determines when the record is synced after entering the `AuditLog`.
#[derive(Debug, Clone, Copy)]
pub enum SyncPolicy {
    /// The new record (and all buffered records) are synced immediatly
    /// after the new record is added
    Immediate,

    /// The new record is added to the buffered queue of pending records whose sync
    /// was also deferred
    Deferred,
}
