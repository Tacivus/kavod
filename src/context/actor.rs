use crate::{
    actor::{ActorEmission, ActorOutputError, ActorOutputSink, wrap_message},
    message::Message,
    output::ProductionSet,
    time::Timestamp,
};

/// Context passed to actor callbacks.
///
/// Provides dispatch time and restricted output. Actors cannot access the
/// global cache, scheduler, sequence allocator, clock, or mode.
///
/// # Capabilities
///
/// | Operation | Available |
/// |---|---|
/// | `dispatch_time()` | Yes |
/// | `send(msg)` | Yes |
/// | `send_at(ts, msg)` | Yes |
/// | Cache read/write | No |
/// | Clock, sequence, scheduler, mode | No |
pub struct ActorCtx<'a> {
    dispatch_time: Timestamp,
    output: &'a mut dyn ActorOutputSink,
    declared_productions: &'a ProductionSet,
}

impl<'a> ActorCtx<'a> {
    #[allow(dead_code)]
    pub(crate) fn new(
        dispatch_time: Timestamp,
        output: &'a mut dyn ActorOutputSink,
        declared_productions: &'a ProductionSet,
    ) -> Self {
        Self {
            dispatch_time,
            output,
            declared_productions,
        }
    }

    /// Scheduler timestamp of the message currently being handled.
    ///
    /// Stable for the duration of the callback; not wall time.
    pub fn dispatch_time(&self) -> Timestamp {
        self.dispatch_time
    }

    /// Emit `msg` for kernel ingress without a user-chosen final dispatch time.
    ///
    /// The kernel assigns receipt/dispatch time and sequence when accepting
    /// the emission. Type must be declared via `.produces::<M>()`.
    pub fn send<M: Message>(&mut self, msg: M) -> Result<(), ActorOutputError> {
        if !self.declared_productions.contains::<M>() {
            return Err(ActorOutputError::UndeclaredProduction {
                type_name: std::any::type_name::<M>(),
            });
        }
        self.output.emit(ActorEmission::Immediate {
            payload: wrap_message(msg),
        })
    }

    /// Request appearance of `msg` at `ts`.
    ///
    /// Rejects `ts` strictly before the current dispatch time.
    /// Type must be declared via `.produces::<M>()`.
    /// The kernel remains authoritative for sequence and final scheduling.
    pub fn send_at<M: Message>(&mut self, ts: Timestamp, msg: M) -> Result<(), ActorOutputError> {
        if ts < self.dispatch_time {
            return Err(ActorOutputError::PastEvent {
                requested: ts,
                current: self.dispatch_time,
            });
        }
        if !self.declared_productions.contains::<M>() {
            return Err(ActorOutputError::UndeclaredProduction {
                type_name: std::any::type_name::<M>(),
            });
        }
        self.output.emit(ActorEmission::At {
            requested_time: ts,
            payload: wrap_message(msg),
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::actor::RecordingActorSink;

    use super::*;
    use std::any::Any;

    // ========================================================================
    // Test types
    // ========================================================================

    #[derive(Debug, Clone, PartialEq)]
    struct MyMsg(u64);

    impl Message for MyMsg {}

    #[derive(Debug, Clone, PartialEq)]
    struct OtherMsg(u64);

    impl Message for OtherMsg {}

    // ========================================================================
    // dispatch_time
    // ========================================================================

    /// Invariant: dispatch_time returns a stable copied value
    #[test]
    fn test_dispatch_time_returns_stable_value() {
        let ts = Timestamp::new(100);
        let mut sink = RecordingActorSink::new();
        let productions = ProductionSet::new();
        let ctx = ActorCtx::new(ts, &mut sink, &productions);
        assert_eq!(ctx.dispatch_time(), ts);
        assert_eq!(ctx.dispatch_time(), ts);
    }

    /// Invariant: dispatch_time is stable after successful send
    #[test]
    fn test_dispatch_time_stable_after_send() {
        let ts = Timestamp::new(42);
        let mut sink = RecordingActorSink::new();
        let mut productions = ProductionSet::new();
        productions.insert::<MyMsg>();
        let mut ctx = ActorCtx::new(ts, &mut sink, &productions);
        ctx.send(MyMsg(1)).unwrap();
        assert_eq!(ctx.dispatch_time(), ts);
    }

    // ========================================================================
    // send
    // ========================================================================

    /// Invariant: declared immediate output reaches the sink as Immediate
    #[test]
    fn test_send_declared_reaches_sink() {
        let ts = Timestamp::new(100);
        let mut sink = RecordingActorSink::new();
        let mut productions = ProductionSet::new();
        productions.insert::<MyMsg>();
        {
            let mut ctx = ActorCtx::new(ts, &mut sink, &productions);
            ctx.send(MyMsg(42)).unwrap();
        }
        assert_eq!(sink.emissions.len(), 1);
        match &sink.emissions[0] {
            ActorEmission::Immediate { payload } => {
                let p: &dyn Any = &**payload;
                assert_eq!(p.downcast_ref::<MyMsg>(), Some(&MyMsg(42)));
            }
            ActorEmission::At { .. } => panic!("expected Immediate"),
        }
    }

    /// Invariant: undeclared send fails and does not call the sink
    #[test]
    fn test_send_undeclared_fails_without_sink_call() {
        let ts = Timestamp::new(0);
        let mut sink = RecordingActorSink::new();
        let productions = ProductionSet::new();
        let mut ctx = ActorCtx::new(ts, &mut sink, &productions);
        let result = ctx.send(MyMsg(1));
        assert!(matches!(
            result,
            Err(ActorOutputError::UndeclaredProduction { .. })
        ));
        assert!(sink.emissions.is_empty());
    }

    // ========================================================================
    // send_at
    // ========================================================================

    /// Invariant: declared send_at reaches the sink with requested time
    #[test]
    fn test_send_at_declared_reaches_sink_with_time() {
        let dispatch_ts = Timestamp::new(100);
        let future_ts = Timestamp::new(200);
        let mut sink = RecordingActorSink::new();
        let mut productions = ProductionSet::new();
        productions.insert::<MyMsg>();
        {
            let mut ctx = ActorCtx::new(dispatch_ts, &mut sink, &productions);
            ctx.send_at(future_ts, MyMsg(7)).unwrap();
        }
        assert_eq!(sink.emissions.len(), 1);
        match &sink.emissions[0] {
            ActorEmission::At {
                requested_time,
                payload,
            } => {
                assert_eq!(*requested_time, future_ts);
                let p: &dyn Any = &**payload;
                assert_eq!(p.downcast_ref::<MyMsg>(), Some(&MyMsg(7)));
            }
            ActorEmission::Immediate { .. } => panic!("expected At"),
        }
    }

    /// Invariant: send_at at dispatch_time succeeds
    #[test]
    fn test_send_at_at_dispatch_time_succeeds() {
        let ts = Timestamp::new(100);
        let mut sink = RecordingActorSink::new();
        let mut productions = ProductionSet::new();
        productions.insert::<MyMsg>();
        {
            let mut ctx = ActorCtx::new(ts, &mut sink, &productions);
            assert!(ctx.send_at(ts, MyMsg(5)).is_ok());
        }
        assert_eq!(sink.emissions.len(), 1);
        match &sink.emissions[0] {
            ActorEmission::At { requested_time, .. } => assert_eq!(*requested_time, ts),
            ActorEmission::Immediate { .. } => panic!("expected At"),
        }
    }

    /// Invariant: send_at rejects timestamp strictly before dispatch_time
    #[test]
    fn test_send_at_rejects_past_timestamp() {
        let dispatch_ts = Timestamp::new(100);
        let past_ts = Timestamp::new(50);
        let mut sink = RecordingActorSink::new();
        let mut productions = ProductionSet::new();
        productions.insert::<MyMsg>();
        let mut ctx = ActorCtx::new(dispatch_ts, &mut sink, &productions);
        let result = ctx.send_at(past_ts, MyMsg(3));
        assert_eq!(
            result,
            Err(ActorOutputError::PastEvent {
                requested: past_ts,
                current: dispatch_ts,
            })
        );
        assert!(sink.emissions.is_empty());
    }

    /// Invariant: past check runs before production check
    #[test]
    fn test_send_at_rejects_past_before_checking_productions() {
        let dispatch_ts = Timestamp::new(100);
        let past_ts = Timestamp::new(50);
        let mut sink = RecordingActorSink::new();
        let productions = ProductionSet::new();
        let mut ctx = ActorCtx::new(dispatch_ts, &mut sink, &productions);
        let result = ctx.send_at(past_ts, MyMsg(1));
        assert!(matches!(result, Err(ActorOutputError::PastEvent { .. })));
        assert!(sink.emissions.is_empty());
    }

    /// Invariant: undeclared send_at fails and does not call the sink
    #[test]
    fn test_send_at_undeclared_fails_without_sink_call() {
        let ts = Timestamp::new(100);
        let mut sink = RecordingActorSink::new();
        let productions = ProductionSet::new();
        let mut ctx = ActorCtx::new(ts, &mut sink, &productions);
        let result = ctx.send_at(Timestamp::new(200), MyMsg(1));
        assert!(matches!(
            result,
            Err(ActorOutputError::UndeclaredProduction { .. })
        ));
        assert!(sink.emissions.is_empty());
    }

    // ========================================================================
    // Multiple emissions
    // ========================================================================

    /// Invariant: two declared sends both reach the sink in order
    #[test]
    fn test_two_sends_both_reach_sink() {
        let ts = Timestamp::new(0);
        let mut sink = RecordingActorSink::new();
        let mut productions = ProductionSet::new();
        productions.insert::<MyMsg>();
        {
            let mut ctx = ActorCtx::new(ts, &mut sink, &productions);
            ctx.send(MyMsg(0)).unwrap();
            ctx.send(MyMsg(1)).unwrap();
        }
        assert_eq!(sink.emissions.len(), 2);
    }

    /// Invariant: one handler's declarations do not authorize another type
    #[test]
    fn test_declared_type_does_not_authorize_other() {
        let ts = Timestamp::new(0);
        let mut sink = RecordingActorSink::new();
        let mut productions = ProductionSet::new();
        productions.insert::<MyMsg>();
        let mut ctx = ActorCtx::new(ts, &mut sink, &productions);
        assert!(ctx.send(MyMsg(1)).is_ok());
        let result = ctx.send(OtherMsg(2));
        assert!(matches!(
            result,
            Err(ActorOutputError::UndeclaredProduction { .. })
        ));
    }

    // ========================================================================
    // Capability isolation
    // ========================================================================

    /// Invariant: ActorCtx has no cache API (type-level; only dispatch_time/send/send_at)
    #[test]
    fn test_actor_ctx_exposes_only_output_capabilities() {
        let ts = Timestamp::new(0);
        let mut sink = RecordingActorSink::new();
        let productions = ProductionSet::new();
        let ctx = ActorCtx::new(ts, &mut sink, &productions);
        let _ = ctx.dispatch_time();
        // No get / get_mut / insert / seq / clock / mode methods exist on ActorCtx.
    }
}
