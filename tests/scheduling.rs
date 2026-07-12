use std::sync::{Arc, Mutex};

use kavod::time::Timestamp;
use kavod::{Engine, EngineConfig, EngineError, HandlerCtx, Message, ReducerCtx};

// ========================================================================
// Fixtures
// ========================================================================

#[derive(Debug, Clone, PartialEq)]
struct Bar(u64);

impl Message for Bar {}

#[derive(Debug, Clone, PartialEq)]
struct Signal(u64);

impl Message for Signal {}

#[derive(Debug, Clone, PartialEq)]
struct Done(u64);

impl Message for Done {}

// ========================================================================
// Cascades and send_at
// ========================================================================

/// Invariant: send_at schedules between surrounding future messages.
#[test]
fn test_send_at_inserts_between_future_messages() {
    let order = Arc::new(Mutex::new(Vec::new()));
    let o_bar = Arc::clone(&order);
    let o_sig = Arc::clone(&order);
    let o_done = Arc::clone(&order);

    let t0 = Timestamp::new(0);
    let t_mid = Timestamp::new(50);
    let t_late = Timestamp::new(100);

    let mut builder = Engine::builder(EngineConfig::backtest(t0));
    builder
        .on(move |ctx: &mut HandlerCtx<'_>, _bar: &Bar| {
            o_bar.lock().unwrap().push("bar");
            ctx.send_at(t_mid, Signal(1)).unwrap();
        })
        .produces::<Signal>();
    builder.on(move |_ctx: &mut HandlerCtx<'_>, _s: &Signal| {
        o_sig.lock().unwrap().push("sig");
    });
    builder.on(move |_ctx: &mut HandlerCtx<'_>, _d: &Done| {
        o_done.lock().unwrap().push("done");
    });

    let mut engine = builder.build().unwrap();
    engine.push_event(t0, Bar(0)).unwrap();
    engine.push_event(t_late, Done(0)).unwrap();
    engine.run().unwrap();

    assert_eq!(*order.lock().unwrap(), vec!["bar", "sig", "done"]);
}

/// Invariant: send_at before current dispatch time is rejected.
#[test]
fn test_past_send_at_is_rejected() {
    let mut builder = Engine::builder(EngineConfig::backtest(Timestamp::new(100)));
    builder
        .on(|ctx: &mut HandlerCtx<'_>, _bar: &Bar| {
            let err = ctx.send_at(Timestamp::new(50), Signal(1)).unwrap_err();
            assert!(matches!(err, kavod::HandlerOutputError::PastEvent { .. }));
        })
        .produces::<Signal>();
    // Signal needs a consumer for build to succeed even if never sent.
    builder.on(|_ctx: &mut HandlerCtx<'_>, _s: &Signal| {});

    let mut engine = builder.build().unwrap();
    engine.push_event(Timestamp::new(100), Bar(0)).unwrap();
    engine.run().unwrap();
}

/// Invariant: push_event before current logical time is rejected.
#[test]
fn test_past_push_event_is_rejected() {
    let mut builder = Engine::builder(EngineConfig::backtest(Timestamp::new(100)));
    builder.on(|_ctx: &mut HandlerCtx<'_>, _bar: &Bar| {});
    let mut engine = builder.build().unwrap();

    let err = engine.push_event(Timestamp::new(50), Bar(0)).unwrap_err();
    assert_eq!(
        err,
        EngineError::PastEvent {
            requested: Timestamp::new(50),
            current: Timestamp::new(100),
        }
    );
}

/// Invariant: reducers run before handlers for the same message.
#[test]
fn test_reducers_before_handlers() {
    let order = Arc::new(Mutex::new(Vec::new()));
    let o_r = Arc::clone(&order);
    let o_h = Arc::clone(&order);

    let mut builder = Engine::builder(EngineConfig::backtest(Timestamp::new(0)));
    builder.reduce(move |_ctx: &mut ReducerCtx<'_>, _bar: &Bar| {
        o_r.lock().unwrap().push("reducer");
    });
    builder.on(move |_ctx: &mut HandlerCtx<'_>, _bar: &Bar| {
        o_h.lock().unwrap().push("handler");
    });

    let mut engine = builder.build().unwrap();
    engine.push_event(Timestamp::new(0), Bar(0)).unwrap();
    engine.run().unwrap();

    assert_eq!(*order.lock().unwrap(), vec!["reducer", "handler"]);
}

/// Invariant: empty engine run exits cleanly.
#[test]
fn test_empty_run_exits_cleanly() {
    let mut engine = Engine::builder(EngineConfig::backtest(Timestamp::new(0)))
        .build()
        .unwrap();
    engine.run().unwrap();
}

/// Invariant: same-time handler production is processed after existing
/// same-time ingress (breadth-first).
#[test]
fn test_same_time_production_after_existing_ingress() {
    let order = Arc::new(Mutex::new(Vec::new()));
    let o_bar = Arc::clone(&order);
    let o_sig = Arc::clone(&order);
    let o_done = Arc::clone(&order);

    let mut builder = Engine::builder(EngineConfig::backtest(Timestamp::new(0)));
    builder
        .on(move |ctx: &mut HandlerCtx<'_>, bar: &Bar| {
            o_bar.lock().unwrap().push(format!("bar:{}", bar.0));
            ctx.send(Signal(bar.0 + 100)).unwrap();
        })
        .produces::<Signal>();
    builder.on(move |_ctx: &mut HandlerCtx<'_>, s: &Signal| {
        o_sig.lock().unwrap().push(format!("sig:{}", s.0));
    });
    builder.on(move |_ctx: &mut HandlerCtx<'_>, d: &Done| {
        o_done.lock().unwrap().push(format!("done:{}", d.0));
    });

    let mut engine = builder.build().unwrap();
    let t = Timestamp::new(10);
    engine.push_event(t, Bar(1)).unwrap();
    engine.push_event(t, Done(2)).unwrap();
    engine.run().unwrap();

    assert_eq!(
        *order.lock().unwrap(),
        vec![
            "bar:1".to_string(),
            "done:2".to_string(),
            "sig:101".to_string(),
        ]
    );
}
