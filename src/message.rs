use std::{any::Any, fmt::Debug};

/// Message is the core trait that makes the entire system work
pub trait Message: Send + Debug + Any + 'static {}
