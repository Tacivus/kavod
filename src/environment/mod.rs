use crate::{audit::AuditEncode, time::Timestamp};

pub trait Environment {
    type Event: AuditEncode;
    type Command: AuditEncode;
    type Fatal: AuditEncode;

    fn start(&mut self) -> Result<Timestamp, Self::Fatal>;

    fn next_event(&mut self) -> Result<(Self::Event, Timestamp), Self::Fatal>;

    fn command_batch(&mut self, commands: CommandBatch<Self::Command>) -> Result<(), Self::Fatal>;

    fn stop(&mut self) -> Result<(), Self::Fatal>;

    fn abort(&mut self);
}

pub struct CommandBatch<T>([T]);
