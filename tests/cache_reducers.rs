use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

use kavod::time::Timestamp;
use kavod::{Engine, EngineConfig, Message, ReducerCtx, State};

// ========================================================================
// Fixtures
// ========================================================================

#[derive(Debug, Clone, PartialEq)]
struct Fill {
    amount: u64,
}

impl Message for Fill {}

#[derive(Debug, Clone, PartialEq)]
struct Portfolio {
    total: u64,
}

impl State for Portfolio {
    type Key = ();

    fn key(&self) -> Self::Key {}
}

// ========================================================================
// Reducer-only engine
// ========================================================================

/// Invariant: a reducer-only engine updates seeded cache state on run.
#[test]
fn test_reducer_only_updates_seeded_state() {
    let seen = Arc::new(Mutex::new(0u64));
    let seen_c = Arc::clone(&seen);

    let mut builder = Engine::builder(EngineConfig::backtest(Timestamp::new(0)));
    builder.seed(Portfolio { total: 0 }).unwrap();
    builder.reduce(move |ctx: &mut ReducerCtx<'_>, fill: &Fill| {
        let portfolio = ctx
            .get_singleton_mut::<Portfolio>()
            .expect("portfolio must be seeded");
        portfolio.total += fill.amount;
        *seen_c.lock().unwrap() = portfolio.total;
    });

    let mut engine = builder.build().unwrap();
    engine
        .push_event(Timestamp::new(0), Fill { amount: 10 })
        .unwrap();
    engine
        .push_event(Timestamp::new(0), Fill { amount: 5 })
        .unwrap();
    engine.run().unwrap();

    assert_eq!(*seen.lock().unwrap(), 15);
}

/// Invariant: multiple reducers for one message run in registration order.
#[test]
fn test_multiple_reducers_run_in_registration_order() {
    let order = Arc::new(Mutex::new(Vec::new()));
    let o1 = Arc::clone(&order);
    let o2 = Arc::clone(&order);

    let mut builder = Engine::builder(EngineConfig::backtest(Timestamp::new(0)));
    builder.reduce(move |_ctx: &mut ReducerCtx<'_>, _fill: &Fill| {
        o1.lock().unwrap().push(1);
    });
    builder.reduce(move |_ctx: &mut ReducerCtx<'_>, _fill: &Fill| {
        o2.lock().unwrap().push(2);
    });

    let mut engine = builder.build().unwrap();
    engine
        .push_event(Timestamp::new(0), Fill { amount: 1 })
        .unwrap();
    engine.run().unwrap();

    assert_eq!(*order.lock().unwrap(), vec![1, 2]);
}

/// Invariant: a later reducer sees mutations from an earlier reducer.
#[test]
fn test_later_reducer_sees_prior_mutations() {
    let final_total = Arc::new(Mutex::new(0u64));
    let final_c = Arc::clone(&final_total);

    let mut builder = Engine::builder(EngineConfig::backtest(Timestamp::new(0)));
    builder.seed(Portfolio { total: 0 }).unwrap();
    builder.reduce(|ctx: &mut ReducerCtx<'_>, fill: &Fill| {
        let p = ctx.get_singleton_mut::<Portfolio>().unwrap();
        p.total += fill.amount;
    });
    builder.reduce(move |ctx: &mut ReducerCtx<'_>, _fill: &Fill| {
        let p = ctx.get_singleton::<Portfolio>().unwrap();
        *final_c.lock().unwrap() = p.total;
    });

    let mut engine = builder.build().unwrap();
    engine
        .push_event(Timestamp::new(0), Fill { amount: 7 })
        .unwrap();
    engine.run().unwrap();

    assert_eq!(*final_total.lock().unwrap(), 7);
}

/// Invariant: reducer receives the message dispatch time.
#[test]
fn test_reducer_receives_dispatch_time() {
    let seen = Arc::new(Mutex::new(None));
    let seen_c = Arc::clone(&seen);
    let t = Timestamp::new(1_000);

    let mut builder = Engine::builder(EngineConfig::backtest(Timestamp::new(0)));
    builder.reduce(move |ctx: &mut ReducerCtx<'_>, _fill: &Fill| {
        *seen_c.lock().unwrap() = Some(ctx.dispatch_time());
    });

    let mut engine = builder.build().unwrap();
    engine.push_event(t, Fill { amount: 1 }).unwrap();
    engine.run().unwrap();

    assert_eq!(*seen.lock().unwrap(), Some(t));
}

/// Invariant: reducer fire count matches number of dispatched messages.
#[test]
fn test_reducer_fires_once_per_message() {
    let count = Arc::new(AtomicUsize::new(0));
    let count_c = Arc::clone(&count);

    let mut builder = Engine::builder(EngineConfig::backtest(Timestamp::new(0)));
    builder.reduce(move |_ctx: &mut ReducerCtx<'_>, _fill: &Fill| {
        count_c.fetch_add(1, Ordering::SeqCst);
    });

    let mut engine = builder.build().unwrap();
    engine
        .push_event(Timestamp::new(0), Fill { amount: 1 })
        .unwrap();
    engine
        .push_event(Timestamp::new(1), Fill { amount: 2 })
        .unwrap();
    engine.run().unwrap();

    assert_eq!(count.load(Ordering::SeqCst), 2);
}
