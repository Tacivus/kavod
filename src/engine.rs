use crate::{
    builder::EngineBuilder, cache::Cache, clock::Clock, config::EngineConfig,
    graph::ValidatedGraph, handler::HandlerRegistry, reducer::ReducerRegistry, schedule::Scheduler,
    sequence::Sequencer, time::timestamp::Timestamp,
};

/// Runtime engine after topology freeze.
///
/// Constructed only via [`EngineBuilder::build`]. Registration methods are not
/// available on this type. Ingress (`push_event`) and the event loop (`run`)
/// are added in later phases.
///
/// Field layout is flat for now; Phase 14 may group mutable runtime fields
/// into an internal `Runtime` for borrow splitting. Actors are Phase 16+.
pub struct Engine {
    pub(crate) config: EngineConfig,
    pub(crate) scheduler: Scheduler,
    pub(crate) sequence: Sequencer,
    pub(crate) cache: Cache,
    pub(crate) reducers: ReducerRegistry,
    pub(crate) handlers: HandlerRegistry,
    pub(crate) graph: ValidatedGraph,
    pub(crate) dispatch_time: Timestamp,
    pub(crate) clock: Box<dyn Clock>,
}

impl Engine {
    /// Start configuration with the given engine config.
    pub fn builder(config: EngineConfig) -> EngineBuilder {
        EngineBuilder::new(config)
    }
}

#[cfg(test)]
impl Engine {
    pub(crate) fn cache(&self) -> &Cache {
        &self.cache
    }

    pub(crate) fn dispatch_time(&self) -> Timestamp {
        self.dispatch_time
    }

    pub(crate) fn scheduler_len(&self) -> usize {
        self.scheduler.len()
    }

    pub(crate) fn clock_now(&self) -> Timestamp {
        self.clock.now()
    }

    pub(crate) fn config(&self) -> &EngineConfig {
        &self.config
    }

    pub(crate) fn has_consumer(&self, type_id: std::any::TypeId) -> bool {
        self.graph.has_consumer(type_id)
    }
}
