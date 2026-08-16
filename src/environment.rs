use crate::time::Timestamp;

pub trait Environment {
    type Event;
    type Command;
    type Error;

    fn start(&mut self) -> Result<Timestamp, Self::Error>;
    fn next_event(&mut self) -> Result<(Self::Event, Timestamp), Self::Error>;
    fn dispatch(&mut self, command: Self::Command) -> Result<(), Self::Error>;
    fn take_error(&mut self) -> Option<Self::Error>;
    fn shutdown(self) -> Quiescence;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Quiescence {
    Quiesced,
    Incomplete,
}
