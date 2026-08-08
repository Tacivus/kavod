use crate::{audit::AuditEncode, engine::EngineEvent, time::Timestamp};

pub struct EventEnvelope<E> {
    pub index: u64,
    pub logical_timestamp: Timestamp,
    pub event: E,
}

pub enum AppEvent<E> {
    Engine(EngineEvent),
    Port(E),
}

pub enum Outcome<F> {
    Continue,
    Stop,
    Fatal(F),
}

pub trait Application {
    type State;
    type Event: AuditEncode;
    type Command: AuditEncode;
    type Fatal: AuditEncode;

    fn initial_state(&self) -> Self::State;

    fn on_event(
        &self,
        state: &mut Self::State,
        event: EventEnvelope<AppEvent<Self::Event>>,
    ) -> Outcome<Self::Fatal>;
}
