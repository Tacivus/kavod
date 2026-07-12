use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};
use std::thread;

use kavod::time::Timestamp;
use kavod::{Engine, EngineConfig, HandlerCtx, Message, ReducerCtx, State};

// ========================================================================
// Fixtures
// ========================================================================

#[derive(Debug, Clone, PartialEq)]
struct Bar {
    close: u64,
}

impl Message for Bar {}

#[derive(Debug, Clone, PartialEq)]
struct Signal {
    buy: bool,
}

impl Message for Signal {}

#[derive(Debug, Clone, PartialEq)]
struct Config {
    threshold: u64,
}

impl State for Config {
    type Key = ();

    fn key(&self) -> Self::Key {}
}

#[derive(Debug, Clone, PartialEq)]
struct Hits {
    n: u64,
}

impl State for Hits {
    type Key = ();

    fn key(&self) -> Self::Key {}
}

// ========================================================================
// Multi-message cascade
// ========================================================================

/// Invariant: a multi-message cascade completes through reducer + handlers.
#[test]
fn test_multi_message_cascade() {
    let signals = Arc::new(Mutex::new(Vec::new()));
    let signals_c = Arc::clone(&signals);

    let mut builder = Engine::builder(EngineConfig::backtest(Timestamp::new(0)));
    builder.seed(Config { threshold: 10 }).unwrap();
    builder.seed(Hits { n: 0 }).unwrap();

    builder.reduce(|ctx: &mut ReducerCtx<'_>, bar: &Bar| {
        if bar.close > 0 {
            let hits = ctx.get_singleton_mut::<Hits>().unwrap();
            hits.n += 1;
        }
    });

    builder
        .on(|ctx: &mut HandlerCtx<'_>, bar: &Bar| {
            let cfg = ctx.get_singleton::<Config>().unwrap();
            if bar.close > cfg.threshold {
                ctx.send(Signal { buy: true }).unwrap();
            }
        })
        .produces::<Signal>();

    builder.on(move |_ctx: &mut HandlerCtx<'_>, s: &Signal| {
        signals_c.lock().unwrap().push(s.buy);
    });

    let mut engine = builder.build().unwrap();
    let t = Timestamp::new(0);
    engine.push_event(t, Bar { close: 5 }).unwrap(); // no signal
    engine.push_event(t, Bar { close: 20 }).unwrap(); // signal
    engine.run().unwrap();

    assert_eq!(*signals.lock().unwrap(), vec![true]);
}

// ========================================================================
// Isolation
// ========================================================================

/// Invariant: two independent engines do not share state.
#[test]
fn test_two_engines_do_not_share_state() {
    fn build_engine(threshold: u64, hits_out: Arc<AtomicU64>) -> Engine {
        let mut builder = Engine::builder(EngineConfig::backtest(Timestamp::new(0)));
        builder.seed(Config { threshold }).unwrap();
        builder.seed(Hits { n: 0 }).unwrap();
        builder.reduce(move |ctx: &mut ReducerCtx<'_>, bar: &Bar| {
            let cfg = ctx.get_singleton::<Config>().unwrap();
            if bar.close > cfg.threshold {
                let hits = ctx.get_singleton_mut::<Hits>().unwrap();
                hits.n += 1;
                hits_out.store(hits.n, Ordering::SeqCst);
            }
        });
        builder.build().unwrap()
    }

    let hits_a = Arc::new(AtomicU64::new(0));
    let hits_b = Arc::new(AtomicU64::new(0));

    let mut engine_a = build_engine(10, Arc::clone(&hits_a));
    let mut engine_b = build_engine(100, Arc::clone(&hits_b));

    let t = Timestamp::new(0);
    engine_a.push_event(t, Bar { close: 50 }).unwrap();
    engine_b.push_event(t, Bar { close: 50 }).unwrap();
    engine_a.run().unwrap();
    engine_b.run().unwrap();

    assert_eq!(hits_a.load(Ordering::SeqCst), 1);
    assert_eq!(hits_b.load(Ordering::SeqCst), 0);
}

// ========================================================================
// Send / thread move
// ========================================================================

/// Invariant: an engine can be moved to another thread and run there.
#[test]
fn test_engine_moves_to_worker_thread_and_runs() {
    let seen = Arc::new(Mutex::new(0u64));
    let seen_c = Arc::clone(&seen);

    let mut builder = Engine::builder(EngineConfig::backtest(Timestamp::new(0)));
    builder.on(move |_ctx: &mut HandlerCtx<'_>, bar: &Bar| {
        *seen_c.lock().unwrap() = bar.close;
    });
    let mut engine = builder.build().unwrap();
    engine
        .push_event(Timestamp::new(0), Bar { close: 99 })
        .unwrap();

    let handle = thread::spawn(move || {
        engine.run().unwrap();
    });
    handle.join().unwrap();

    assert_eq!(*seen.lock().unwrap(), 99);
}
