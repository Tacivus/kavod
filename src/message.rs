use std::{any::Any, fmt::Debug, sync::Arc};

/// Core message type that is passed around the engine
pub trait Message: Send + Sync + Debug + Any + 'static {}

pub(crate) type SharedMessage = Arc<dyn Message>;
