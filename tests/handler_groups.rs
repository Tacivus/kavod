use std::sync::{Arc, Mutex};

use kavod::time::Timestamp;
use kavod::{Engine, EngineConfig, HandlerCtx, Message};

// ========================================================================
// Fixtures
// ========================================================================

#[derive(Debug, Clone, PartialEq)]
struct Tick(u64);

impl Message for Tick {}

#[derive(Debug, Clone, PartialEq)]
struct Reset;

impl Message for Reset {}

struct Counter {
    n: u64,
}

// ========================================================================
// Group state
// ========================================================================

/// Invariant: handler-group state persists across multiple messages in one run.
#[test]
fn test_group_state_persists_across_messages() {
    let final_n = Arc::new(Mutex::new(0u64));
    let final_c = Arc::clone(&final_n);

    let mut builder = Engine::builder(EngineConfig::backtest(Timestamp::new(0)));
    builder.handler_group(Counter { n: 0 }, |group| {
        group.on(
            |state: &mut Counter, _ctx: &mut HandlerCtx<'_>, tick: &Tick| {
                state.n += tick.0;
            },
        );
        let final_c = Arc::clone(&final_c);
        group.on(
            move |state: &mut Counter, _ctx: &mut HandlerCtx<'_>, _r: &Reset| {
                *final_c.lock().unwrap() = state.n;
            },
        );
    });

    let mut engine = builder.build().unwrap();
    let t = Timestamp::new(0);
    engine.push_event(t, Tick(1)).unwrap();
    engine.push_event(t, Tick(2)).unwrap();
    engine.push_event(t, Tick(3)).unwrap();
    engine.push_event(t, Reset).unwrap();
    engine.run().unwrap();

    assert_eq!(*final_n.lock().unwrap(), 6);
}

/// Invariant: separate handler groups have isolated state.
#[test]
fn test_separate_groups_are_isolated() {
    let a_final = Arc::new(Mutex::new(0u64));
    let b_final = Arc::new(Mutex::new(0u64));
    let a_c = Arc::clone(&a_final);
    let b_c = Arc::clone(&b_final);

    let mut builder = Engine::builder(EngineConfig::backtest(Timestamp::new(0)));
    builder.handler_group(Counter { n: 0 }, |group| {
        let a_c = Arc::clone(&a_c);
        group.on(
            move |state: &mut Counter, _ctx: &mut HandlerCtx<'_>, tick: &Tick| {
                state.n += tick.0;
                *a_c.lock().unwrap() = state.n;
            },
        );
    });
    builder.handler_group(Counter { n: 0 }, |group| {
        let b_c = Arc::clone(&b_c);
        group.on(
            move |state: &mut Counter, _ctx: &mut HandlerCtx<'_>, tick: &Tick| {
                state.n += tick.0 * 10;
                *b_c.lock().unwrap() = state.n;
            },
        );
    });

    let mut engine = builder.build().unwrap();
    engine.push_event(Timestamp::new(0), Tick(1)).unwrap();
    engine.run().unwrap();

    assert_eq!(*a_final.lock().unwrap(), 1);
    assert_eq!(*b_final.lock().unwrap(), 10);
}

/// Invariant: a stateless handler engine builds and runs.
#[test]
fn test_stateless_handler_runs() {
    let seen = Arc::new(Mutex::new(None));
    let seen_c = Arc::clone(&seen);

    let mut builder = Engine::builder(EngineConfig::backtest(Timestamp::new(0)));
    builder.on(move |_ctx: &mut HandlerCtx<'_>, tick: &Tick| {
        *seen_c.lock().unwrap() = Some(tick.0);
    });

    let mut engine = builder.build().unwrap();
    engine.push_event(Timestamp::new(0), Tick(42)).unwrap();
    engine.run().unwrap();

    assert_eq!(*seen.lock().unwrap(), Some(42));
}

/// Invariant: handler-group state need not implement State.
#[test]
fn test_group_state_need_not_implement_state() {
    // Counter does not implement State; this must still compile and run.
    let seen = Arc::new(Mutex::new(false));
    let seen_c = Arc::clone(&seen);

    let mut builder = Engine::builder(EngineConfig::backtest(Timestamp::new(0)));
    builder.handler_group(Counter { n: 0 }, |group| {
        let seen_c = Arc::clone(&seen_c);
        group.on(
            move |state: &mut Counter, _ctx: &mut HandlerCtx<'_>, _t: &Tick| {
                state.n = 1;
                *seen_c.lock().unwrap() = state.n == 1;
            },
        );
    });

    let mut engine = builder.build().unwrap();
    engine.push_event(Timestamp::new(0), Tick(0)).unwrap();
    engine.run().unwrap();

    assert!(*seen.lock().unwrap());
}
