//! Phase 19: backtest actor integration and parity tests.
//!
//! Proves a minimal simulated venue (message-fed private book + fill latency)
//! and a snapshot-fed actor alternative through public APIs only.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread;

use kavod::time::{Duration, Timestamp};
use kavod::{ActorCtx, Engine, EngineConfig, HandlerCtx, Message, ReducerCtx, State};

// ========================================================================
// Fixtures
// ========================================================================

type InstrumentId = u64;

const INSTR: InstrumentId = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
struct MarketData {
    instrument: InstrumentId,
    price: u64,
}

impl Message for MarketData {}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SubmitOrder {
    instrument: InstrumentId,
    qty: u64,
}

impl Message for SubmitOrder {}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Fill {
    instrument: InstrumentId,
    price: u64,
    qty: u64,
}

impl Message for Fill {}

/// Owned snapshot for actors that do not subscribe to every market tick.
#[derive(Debug, Clone, PartialEq, Eq)]
struct MarketSnapshot {
    instrument: InstrumentId,
    last_price: u64,
}

/// Handler-built command carrying an owned market projection into an actor.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SimulateOrder {
    order: SubmitOrder,
    market: MarketSnapshot,
}

impl Message for SimulateOrder {}

/// Reducer-owned cache projection of the latest market price.
#[derive(Debug, Clone, PartialEq, Eq)]
struct MarketProjection {
    instrument: InstrumentId,
    last_price: u64,
}

impl State for MarketProjection {
    type Key = InstrumentId;

    fn key(&self) -> Self::Key {
        self.instrument
    }
}

/// Reducer-owned portfolio updated by fills.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct Portfolio {
    position: i64,
    last_fill_price: u64,
    fill_count: u64,
}

impl State for Portfolio {
    type Key = ();

    fn key(&self) -> Self::Key {}
}

/// Private simulated venue book; not stored in the global cache.
struct SimVenueState {
    book: HashMap<InstrumentId, u64>,
    fill_latency: Duration,
}

impl SimVenueState {
    fn new(fill_latency: Duration) -> Self {
        Self {
            book: HashMap::new(),
            fill_latency,
        }
    }
}

/// Snapshot-fed venue: no MarketData subscription.
struct SnapshotVenue;

#[derive(Debug, Clone, PartialEq, Eq)]
enum TraceEvent {
    /// Consumer phase for a MarketData message.
    MdPhase {
        phase: &'static str,
        price: u64,
        cache_price: Option<u64>,
        actor_book: Option<u64>,
        dispatch_time: i128,
    },
    /// Consumer phase for a Fill message.
    FillPhase {
        phase: &'static str,
        price: u64,
        portfolio_position: Option<i64>,
        dispatch_time: i128,
    },
    /// Ordered stream of message kinds as observed by terminal handlers.
    Observed {
        kind: &'static str,
        dispatch_time: i128,
        price: u64,
    },
}

fn fill_latency() -> Duration {
    Duration::from_nanos(5_000_000) // 5ms
}

/// Build a full sim-venue graph: MD cache + portfolio + sim actor + tracers.
fn build_sim_venue_engine(
    initial: Timestamp,
    latency: Duration,
    trace: Arc<Mutex<Vec<TraceEvent>>>,
) -> Engine {
    let t_md_r = Arc::clone(&trace);
    let t_md_h = Arc::clone(&trace);
    let t_fill_r = Arc::clone(&trace);
    let t_fill_h = Arc::clone(&trace);
    let t_obs_md = Arc::clone(&trace);
    let t_obs_fill = Arc::clone(&trace);
    let t_actor_md = Arc::clone(&trace);
    let t_actor_order = Arc::clone(&trace);

    let mut builder = Engine::builder(EngineConfig::backtest(initial));
    builder.seed(Portfolio::default()).unwrap();

    builder.reduce(move |ctx: &mut ReducerCtx<'_>, md: &MarketData| {
        ctx.insert(MarketProjection {
            instrument: md.instrument,
            last_price: md.price,
        });
        let cache_price = ctx
            .get::<MarketProjection>(&md.instrument)
            .map(|p| p.last_price);
        t_md_r.lock().unwrap().push(TraceEvent::MdPhase {
            phase: "reducer",
            price: md.price,
            cache_price,
            actor_book: None,
            dispatch_time: ctx.dispatch_time().raw(),
        });
    });

    builder.reduce(move |ctx: &mut ReducerCtx<'_>, fill: &Fill| {
        let portfolio = ctx.get_singleton_mut::<Portfolio>().unwrap();
        portfolio.position += fill.qty as i64;
        portfolio.last_fill_price = fill.price;
        portfolio.fill_count += 1;
        let pos = portfolio.position;
        t_fill_r.lock().unwrap().push(TraceEvent::FillPhase {
            phase: "reducer",
            price: fill.price,
            portfolio_position: Some(pos),
            dispatch_time: ctx.dispatch_time().raw(),
        });
    });

    builder.on(move |ctx: &mut HandlerCtx<'_>, md: &MarketData| {
        let cache_price = ctx
            .get::<MarketProjection>(&md.instrument)
            .map(|p| p.last_price);
        t_md_h.lock().unwrap().push(TraceEvent::MdPhase {
            phase: "handler",
            price: md.price,
            cache_price,
            actor_book: None,
            dispatch_time: ctx.dispatch_time().raw(),
        });
        t_obs_md.lock().unwrap().push(TraceEvent::Observed {
            kind: "market",
            dispatch_time: ctx.dispatch_time().raw(),
            price: md.price,
        });
    });

    builder.on(move |ctx: &mut HandlerCtx<'_>, fill: &Fill| {
        let portfolio = ctx.get_singleton::<Portfolio>().unwrap();
        t_fill_h.lock().unwrap().push(TraceEvent::FillPhase {
            phase: "handler",
            price: fill.price,
            portfolio_position: Some(portfolio.position),
            dispatch_time: ctx.dispatch_time().raw(),
        });
        t_obs_fill.lock().unwrap().push(TraceEvent::Observed {
            kind: "fill",
            dispatch_time: ctx.dispatch_time().raw(),
            price: fill.price,
        });
    });

    builder
        .actor("sim-venue", SimVenueState::new(latency), move |actor| {
            actor.on(move |venue, ctx: &mut ActorCtx<'_>, md: &MarketData| {
                venue.book.insert(md.instrument, md.price);
                let actor_book = venue.book.get(&md.instrument).copied();
                t_actor_md.lock().unwrap().push(TraceEvent::MdPhase {
                    phase: "actor",
                    price: md.price,
                    cache_price: None,
                    actor_book,
                    dispatch_time: ctx.dispatch_time().raw(),
                });
            });
            actor
                .on(move |venue, ctx: &mut ActorCtx<'_>, order: &SubmitOrder| {
                    let price = *venue.book.get(&order.instrument).unwrap_or(&0);
                    t_actor_order.lock().unwrap().push(TraceEvent::Observed {
                        kind: "order",
                        dispatch_time: ctx.dispatch_time().raw(),
                        price,
                    });
                    let fill_at = ctx
                        .dispatch_time()
                        .checked_add(venue.fill_latency)
                        .expect("fill latency must not overflow");
                    ctx.send_at(
                        fill_at,
                        Fill {
                            instrument: order.instrument,
                            price,
                            qty: order.qty,
                        },
                    )
                    .unwrap();
                })
                .produces::<Fill>();
        })
        .unwrap();

    builder.build().unwrap()
}

// ========================================================================
// Independent projections
// ========================================================================

/// Invariant: market data updates reducer-owned cache and actor-private book
/// independently.
#[test]
fn test_market_data_updates_cache_and_actor_independently() {
    let cache_seen = Arc::new(Mutex::new(None::<u64>));
    let actor_seen = Arc::new(Mutex::new(None::<u64>));
    let cache_c = Arc::clone(&cache_seen);
    let actor_c = Arc::clone(&actor_seen);

    let t0 = Timestamp::new(0);
    let mut builder = Engine::builder(EngineConfig::backtest(t0));

    builder.reduce(|ctx: &mut ReducerCtx<'_>, md: &MarketData| {
        ctx.insert(MarketProjection {
            instrument: md.instrument,
            last_price: md.price,
        });
    });

    builder.on(move |ctx: &mut HandlerCtx<'_>, md: &MarketData| {
        let p = ctx.get::<MarketProjection>(&md.instrument).unwrap();
        *cache_c.lock().unwrap() = Some(p.last_price);
    });

    builder
        .actor("sim-venue", SimVenueState::new(Duration::ZERO), |actor| {
            actor.on(move |venue, _ctx, md: &MarketData| {
                venue.book.insert(md.instrument, md.price);
                *actor_c.lock().unwrap() = venue.book.get(&md.instrument).copied();
            });
            // Terminal consumer so MarketData is not orphaned if only actor
            // subscribed — actor already consumes MD; order path unused here.
            actor.on(|_v, _ctx, _o: &SubmitOrder| {}).produces::<Fill>();
        })
        .unwrap();
    // Fill needs a consumer if produced; unused path still needs graph validity
    // only when produces is declared — declare Fill consumer for orphan safety.
    builder.on(|_ctx: &mut HandlerCtx<'_>, _f: &Fill| {});

    let mut engine = builder.build().unwrap();
    engine
        .push_event(
            t0,
            MarketData {
                instrument: INSTR,
                price: 42,
            },
        )
        .unwrap();
    engine.run().unwrap();

    assert_eq!(*cache_seen.lock().unwrap(), Some(42));
    assert_eq!(*actor_seen.lock().unwrap(), Some(42));
}

// ========================================================================
// Dispatch order: reducers → handlers → actors
// ========================================================================

/// Invariant: handler sees reducer-updated cache before actor delivery.
#[test]
fn test_handler_sees_reducer_cache_before_actor() {
    let phases = Arc::new(Mutex::new(Vec::new()));
    let p_r = Arc::clone(&phases);
    let p_h = Arc::clone(&phases);
    let p_a = Arc::clone(&phases);

    let t0 = Timestamp::new(0);
    let mut builder = Engine::builder(EngineConfig::backtest(t0));

    builder.reduce(move |ctx: &mut ReducerCtx<'_>, md: &MarketData| {
        ctx.insert(MarketProjection {
            instrument: md.instrument,
            last_price: md.price,
        });
        p_r.lock().unwrap().push(("reducer", md.price));
    });

    builder.on(move |ctx: &mut HandlerCtx<'_>, md: &MarketData| {
        let cache = ctx
            .get::<MarketProjection>(&md.instrument)
            .map(|p| p.last_price)
            .unwrap();
        p_h.lock().unwrap().push(("handler", cache));
    });

    builder
        .actor("sim-venue", SimVenueState::new(Duration::ZERO), |actor| {
            actor.on(move |venue, _ctx, md: &MarketData| {
                venue.book.insert(md.instrument, md.price);
                p_a.lock().unwrap().push(("actor", md.price));
            });
        })
        .unwrap();

    let mut engine = builder.build().unwrap();
    engine
        .push_event(
            t0,
            MarketData {
                instrument: INSTR,
                price: 7,
            },
        )
        .unwrap();
    engine.run().unwrap();

    assert_eq!(
        *phases.lock().unwrap(),
        vec![("reducer", 7), ("handler", 7), ("actor", 7)]
    );
}

// ========================================================================
// Causal market book
// ========================================================================

/// Invariant: sim venue sees market events in scheduler order.
#[test]
fn test_sim_venue_sees_market_in_scheduler_order() {
    let book_at_order = Arc::new(Mutex::new(0u64));
    let seen = Arc::clone(&book_at_order);

    let t0 = Timestamp::new(0);
    let t1 = Timestamp::new(10);
    let t2 = Timestamp::new(20);

    let mut builder = Engine::builder(EngineConfig::backtest(t0));
    builder
        .actor("sim-venue", SimVenueState::new(Duration::ZERO), |actor| {
            actor.on(|venue, _ctx, md: &MarketData| {
                venue.book.insert(md.instrument, md.price);
            });
            actor
                .on(move |venue, ctx, order: &SubmitOrder| {
                    let px = *venue.book.get(&order.instrument).unwrap_or(&0);
                    *seen.lock().unwrap() = px;
                    ctx.send(Fill {
                        instrument: order.instrument,
                        price: px,
                        qty: order.qty,
                    })
                    .unwrap();
                })
                .produces::<Fill>();
        })
        .unwrap();
    builder.on(|_ctx: &mut HandlerCtx<'_>, _f: &Fill| {});

    let mut engine = builder.build().unwrap();
    engine
        .push_event(
            t0,
            MarketData {
                instrument: INSTR,
                price: 100,
            },
        )
        .unwrap();
    engine
        .push_event(
            t1,
            MarketData {
                instrument: INSTR,
                price: 110,
            },
        )
        .unwrap();
    engine
        .push_event(
            t2,
            SubmitOrder {
                instrument: INSTR,
                qty: 1,
            },
        )
        .unwrap();
    engine.run().unwrap();

    assert_eq!(*book_at_order.lock().unwrap(), 110);
}

/// Invariant: order executes against the latest causally preceding market state.
#[test]
fn test_order_executes_against_causally_preceding_market() {
    let fill_price = Arc::new(Mutex::new(0u64));
    let fp = Arc::clone(&fill_price);

    let t0 = Timestamp::new(0);

    let mut builder = Engine::builder(EngineConfig::backtest(t0));
    builder
        .actor("sim-venue", SimVenueState::new(Duration::ZERO), |actor| {
            actor.on(|venue, _ctx, md: &MarketData| {
                venue.book.insert(md.instrument, md.price);
            });
            actor
                .on(|venue, ctx, order: &SubmitOrder| {
                    let px = *venue.book.get(&order.instrument).unwrap_or(&0);
                    ctx.send(Fill {
                        instrument: order.instrument,
                        price: px,
                        qty: order.qty,
                    })
                    .unwrap();
                })
                .produces::<Fill>();
        })
        .unwrap();
    builder.on(move |_ctx: &mut HandlerCtx<'_>, f: &Fill| {
        *fp.lock().unwrap() = f.price;
    });

    let mut engine = builder.build().unwrap();
    // Same-time: MD seq before Order seq → book is A when order runs.
    engine
        .push_event(
            t0,
            MarketData {
                instrument: INSTR,
                price: 50,
            },
        )
        .unwrap();
    engine
        .push_event(
            t0,
            SubmitOrder {
                instrument: INSTR,
                qty: 2,
            },
        )
        .unwrap();
    // Later market must not rewrite the already-produced fill price.
    engine
        .push_event(
            Timestamp::new(1),
            MarketData {
                instrument: INSTR,
                price: 999,
            },
        )
        .unwrap();
    engine.run().unwrap();

    assert_eq!(*fill_price.lock().unwrap(), 50);
}

// ========================================================================
// Fill latency placement
// ========================================================================

/// Invariant: fill latency places a fill between surrounding market events.
#[test]
fn test_fill_latency_places_fill_between_market_events() {
    let observed = Arc::new(Mutex::new(Vec::new()));
    let o_md = Arc::clone(&observed);
    let o_fill = Arc::clone(&observed);

    let t0 = Timestamp::new(0);
    let latency = fill_latency();
    let t_mid = t0.checked_add(Duration::from_nanos(2_000_000)).unwrap(); // 2ms
    let t_late = t0.checked_add(Duration::from_nanos(10_000_000)).unwrap(); // 10ms

    let mut builder = Engine::builder(EngineConfig::backtest(t0));
    builder.on(move |ctx: &mut HandlerCtx<'_>, md: &MarketData| {
        o_md.lock()
            .unwrap()
            .push(("market", ctx.dispatch_time().raw(), md.price));
    });
    builder.on(move |ctx: &mut HandlerCtx<'_>, f: &Fill| {
        o_fill
            .lock()
            .unwrap()
            .push(("fill", ctx.dispatch_time().raw(), f.price));
    });
    builder
        .actor("sim-venue", SimVenueState::new(latency), |actor| {
            actor.on(|venue, _ctx, md: &MarketData| {
                venue.book.insert(md.instrument, md.price);
            });
            actor
                .on(|venue, ctx, order: &SubmitOrder| {
                    let px = *venue.book.get(&order.instrument).unwrap_or(&0);
                    let fill_at = ctx.dispatch_time().checked_add(venue.fill_latency).unwrap();
                    ctx.send_at(
                        fill_at,
                        Fill {
                            instrument: order.instrument,
                            price: px,
                            qty: order.qty,
                        },
                    )
                    .unwrap();
                })
                .produces::<Fill>();
        })
        .unwrap();

    let mut engine = builder.build().unwrap();
    engine
        .push_event(
            t0,
            MarketData {
                instrument: INSTR,
                price: 100,
            },
        )
        .unwrap();
    engine
        .push_event(
            t0,
            SubmitOrder {
                instrument: INSTR,
                qty: 1,
            },
        )
        .unwrap();
    engine
        .push_event(
            t_mid,
            MarketData {
                instrument: INSTR,
                price: 110,
            },
        )
        .unwrap();
    engine
        .push_event(
            t_late,
            MarketData {
                instrument: INSTR,
                price: 120,
            },
        )
        .unwrap();
    engine.run().unwrap();

    let events = observed.lock().unwrap().clone();
    assert_eq!(
        events,
        vec![
            ("market", 0, 100),
            ("market", 2_000_000, 110),
            ("fill", 5_000_000, 100), // causal price from order time
            ("market", 10_000_000, 120),
        ]
    );
}

// ========================================================================
// Fill reducer before fill handlers
// ========================================================================

/// Invariant: fill reducer updates portfolio before fill handlers run.
#[test]
fn test_fill_reducer_updates_portfolio_before_handlers() {
    let phases = Arc::new(Mutex::new(Vec::new()));
    let p_r = Arc::clone(&phases);
    let p_h = Arc::clone(&phases);

    let t0 = Timestamp::new(0);
    let mut builder = Engine::builder(EngineConfig::backtest(t0));
    builder.seed(Portfolio::default()).unwrap();

    builder.reduce(move |ctx: &mut ReducerCtx<'_>, fill: &Fill| {
        let portfolio = ctx.get_singleton_mut::<Portfolio>().unwrap();
        portfolio.position += fill.qty as i64;
        portfolio.last_fill_price = fill.price;
        portfolio.fill_count += 1;
        p_r.lock()
            .unwrap()
            .push(("reducer", portfolio.position, portfolio.last_fill_price));
    });

    builder.on(move |ctx: &mut HandlerCtx<'_>, fill: &Fill| {
        let portfolio = ctx.get_singleton::<Portfolio>().unwrap();
        p_h.lock()
            .unwrap()
            .push(("handler", portfolio.position, portfolio.last_fill_price));
        assert_eq!(portfolio.last_fill_price, fill.price);
        assert_eq!(portfolio.position, fill.qty as i64);
    });

    // Direct fill ingress (no actor) — isolates reducer-before-handler on Fill.
    let mut engine = builder.build().unwrap();
    engine
        .push_event(
            t0,
            Fill {
                instrument: INSTR,
                price: 33,
                qty: 4,
            },
        )
        .unwrap();
    engine.run().unwrap();

    assert_eq!(
        *phases.lock().unwrap(),
        vec![("reducer", 4, 33), ("handler", 4, 33)]
    );
}

// ========================================================================
// Determinism and isolation
// ========================================================================

/// Invariant: repeating the same run produces identical output ordering.
#[test]
fn test_same_run_is_deterministic() {
    fn run_once() -> Vec<(i128, &'static str, u64)> {
        let observed = Arc::new(Mutex::new(Vec::new()));
        let o_md = Arc::clone(&observed);
        let o_fill = Arc::clone(&observed);
        let o_order = Arc::clone(&observed);

        let t0 = Timestamp::new(0);
        let latency = fill_latency();
        let mut builder = Engine::builder(EngineConfig::backtest(t0));
        builder.seed(Portfolio::default()).unwrap();

        builder.reduce(|ctx: &mut ReducerCtx<'_>, md: &MarketData| {
            ctx.insert(MarketProjection {
                instrument: md.instrument,
                last_price: md.price,
            });
        });
        builder.reduce(|ctx: &mut ReducerCtx<'_>, fill: &Fill| {
            let p = ctx.get_singleton_mut::<Portfolio>().unwrap();
            p.position += fill.qty as i64;
            p.last_fill_price = fill.price;
            p.fill_count += 1;
        });

        builder.on(move |ctx: &mut HandlerCtx<'_>, md: &MarketData| {
            o_md.lock()
                .unwrap()
                .push((ctx.dispatch_time().raw(), "market", md.price));
        });
        builder.on(move |ctx: &mut HandlerCtx<'_>, f: &Fill| {
            o_fill
                .lock()
                .unwrap()
                .push((ctx.dispatch_time().raw(), "fill", f.price));
        });
        builder.on(move |ctx: &mut HandlerCtx<'_>, o: &SubmitOrder| {
            o_order
                .lock()
                .unwrap()
                .push((ctx.dispatch_time().raw(), "order", o.qty));
        });

        builder
            .actor("sim-venue", SimVenueState::new(latency), |actor| {
                actor.on(|venue, _ctx, md: &MarketData| {
                    venue.book.insert(md.instrument, md.price);
                });
                actor
                    .on(|venue, ctx, order: &SubmitOrder| {
                        let px = *venue.book.get(&order.instrument).unwrap_or(&0);
                        let fill_at = ctx.dispatch_time().checked_add(venue.fill_latency).unwrap();
                        ctx.send_at(
                            fill_at,
                            Fill {
                                instrument: order.instrument,
                                price: px,
                                qty: order.qty,
                            },
                        )
                        .unwrap();
                    })
                    .produces::<Fill>();
            })
            .unwrap();

        let mut engine = builder.build().unwrap();
        engine
            .push_event(
                t0,
                MarketData {
                    instrument: INSTR,
                    price: 100,
                },
            )
            .unwrap();
        engine
            .push_event(
                t0,
                SubmitOrder {
                    instrument: INSTR,
                    qty: 3,
                },
            )
            .unwrap();
        engine
            .push_event(
                Timestamp::new(2_000_000),
                MarketData {
                    instrument: INSTR,
                    price: 110,
                },
            )
            .unwrap();
        engine.run().unwrap();
        observed.lock().unwrap().clone()
    }

    let a = run_once();
    let b = run_once();
    assert_eq!(a, b);
    assert_eq!(
        a,
        vec![
            (0, "market", 100),
            (0, "order", 3),
            (2_000_000, "market", 110),
            (5_000_000, "fill", 100),
        ]
    );
}

/// Invariant: two backtests on separate threads produce isolated state.
#[test]
fn test_two_backtests_on_threads_are_isolated() {
    fn build_and_run(seed_price: u64, out: Arc<Mutex<u64>>) {
        let t0 = Timestamp::new(0);
        let mut builder = Engine::builder(EngineConfig::backtest(t0));
        builder.seed(Portfolio::default()).unwrap();

        builder.reduce(|ctx: &mut ReducerCtx<'_>, md: &MarketData| {
            ctx.insert(MarketProjection {
                instrument: md.instrument,
                last_price: md.price,
            });
        });
        builder.reduce(|ctx: &mut ReducerCtx<'_>, fill: &Fill| {
            let p = ctx.get_singleton_mut::<Portfolio>().unwrap();
            p.last_fill_price = fill.price;
            p.fill_count += 1;
        });

        let out_c = Arc::clone(&out);
        builder.on(move |_ctx: &mut HandlerCtx<'_>, f: &Fill| {
            *out_c.lock().unwrap() = f.price;
        });

        builder
            .actor("sim-venue", SimVenueState::new(Duration::ZERO), |actor| {
                actor.on(|venue, _ctx, md: &MarketData| {
                    venue.book.insert(md.instrument, md.price);
                });
                actor
                    .on(|venue, ctx, order: &SubmitOrder| {
                        let px = *venue.book.get(&order.instrument).unwrap_or(&0);
                        ctx.send(Fill {
                            instrument: order.instrument,
                            price: px,
                            qty: order.qty,
                        })
                        .unwrap();
                    })
                    .produces::<Fill>();
            })
            .unwrap();

        let mut engine = builder.build().unwrap();
        engine
            .push_event(
                t0,
                MarketData {
                    instrument: INSTR,
                    price: seed_price,
                },
            )
            .unwrap();
        engine
            .push_event(
                t0,
                SubmitOrder {
                    instrument: INSTR,
                    qty: 1,
                },
            )
            .unwrap();
        engine.run().unwrap();
    }

    let out_a = Arc::new(Mutex::new(0u64));
    let out_b = Arc::new(Mutex::new(0u64));
    let a = Arc::clone(&out_a);
    let b = Arc::clone(&out_b);

    let h1 = thread::spawn(move || build_and_run(11, a));
    let h2 = thread::spawn(move || build_and_run(22, b));
    h1.join().unwrap();
    h2.join().unwrap();

    assert_eq!(*out_a.lock().unwrap(), 11);
    assert_eq!(*out_b.lock().unwrap(), 22);
}

// ========================================================================
// Snapshot-fed actor
// ========================================================================

/// Invariant: snapshot actor consumes an owned message; no cache borrow into
/// the actor.
#[test]
fn test_snapshot_actor_no_cache_borrow() {
    let fill_price = Arc::new(Mutex::new(0u64));
    let fp = Arc::clone(&fill_price);

    let t0 = Timestamp::new(0);
    let mut builder = Engine::builder(EngineConfig::backtest(t0));
    builder.seed(Portfolio::default()).unwrap();

    // Cache projection updated by reducers; actor never reads cache.
    builder.reduce(|ctx: &mut ReducerCtx<'_>, md: &MarketData| {
        ctx.insert(MarketProjection {
            instrument: md.instrument,
            last_price: md.price,
        });
    });
    builder.reduce(|ctx: &mut ReducerCtx<'_>, fill: &Fill| {
        let p = ctx.get_singleton_mut::<Portfolio>().unwrap();
        p.last_fill_price = fill.price;
        p.fill_count += 1;
    });

    // Handler clones cache into an owned snapshot message for the actor.
    builder
        .on(|ctx: &mut HandlerCtx<'_>, order: &SubmitOrder| {
            let proj = ctx
                .get::<MarketProjection>(&order.instrument)
                .expect("market projection must exist");
            ctx.send(SimulateOrder {
                order: order.clone(),
                market: MarketSnapshot {
                    instrument: proj.instrument,
                    last_price: proj.last_price,
                },
            })
            .unwrap();
        })
        .produces::<SimulateOrder>();

    // Snapshot venue: only SimulateOrder — no MarketData subscription.
    builder
        .actor("snapshot-venue", SnapshotVenue, |actor| {
            actor
                .on(|_venue, ctx: &mut ActorCtx<'_>, cmd: &SimulateOrder| {
                    ctx.send(Fill {
                        instrument: cmd.order.instrument,
                        price: cmd.market.last_price,
                        qty: cmd.order.qty,
                    })
                    .unwrap();
                })
                .produces::<Fill>();
        })
        .unwrap();

    builder.on(move |_ctx: &mut HandlerCtx<'_>, f: &Fill| {
        *fp.lock().unwrap() = f.price;
    });
    // MarketData needs a consumer (reducer already counts).
    // SubmitOrder consumed by handler above.

    let mut engine = builder.build().unwrap();
    engine
        .push_event(
            t0,
            MarketData {
                instrument: INSTR,
                price: 77,
            },
        )
        .unwrap();
    engine
        .push_event(
            t0,
            SubmitOrder {
                instrument: INSTR,
                qty: 5,
            },
        )
        .unwrap();
    engine.run().unwrap();

    assert_eq!(*fill_price.lock().unwrap(), 77);
}

// ========================================================================
// End-to-end sim venue graph (latency + portfolio)
// ========================================================================

/// Invariant: full sim-venue graph fills against causal book with latency and
/// portfolio updates.
#[test]
fn test_full_sim_venue_graph_with_latency_and_portfolio() {
    let trace = Arc::new(Mutex::new(Vec::new()));
    let t0 = Timestamp::new(0);
    let mut engine = build_sim_venue_engine(t0, fill_latency(), Arc::clone(&trace));

    engine
        .push_event(
            t0,
            MarketData {
                instrument: INSTR,
                price: 100,
            },
        )
        .unwrap();
    engine
        .push_event(
            t0,
            SubmitOrder {
                instrument: INSTR,
                qty: 2,
            },
        )
        .unwrap();
    engine
        .push_event(
            Timestamp::new(2_000_000),
            MarketData {
                instrument: INSTR,
                price: 110,
            },
        )
        .unwrap();
    engine.run().unwrap();

    let events = trace.lock().unwrap().clone();

    // MD@0: reducer → handler → actor with cache/book 100.
    let md0: Vec<_> = events
        .iter()
        .filter(|e| {
            matches!(
                e,
                TraceEvent::MdPhase {
                    dispatch_time: 0,
                    ..
                }
            )
        })
        .collect();
    assert_eq!(md0.len(), 3);
    assert!(matches!(
        md0[0],
        TraceEvent::MdPhase {
            phase: "reducer",
            cache_price: Some(100),
            ..
        }
    ));
    assert!(matches!(
        md0[1],
        TraceEvent::MdPhase {
            phase: "handler",
            cache_price: Some(100),
            ..
        }
    ));
    assert!(matches!(
        md0[2],
        TraceEvent::MdPhase {
            phase: "actor",
            actor_book: Some(100),
            ..
        }
    ));

    // Fill@5ms: reducer before handler; portfolio reflects fill.
    let fills: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, TraceEvent::FillPhase { .. }))
        .collect();
    assert_eq!(fills.len(), 2);
    assert!(matches!(
        fills[0],
        TraceEvent::FillPhase {
            phase: "reducer",
            price: 100,
            portfolio_position: Some(2),
            dispatch_time: 5_000_000,
        }
    ));
    assert!(matches!(
        fills[1],
        TraceEvent::FillPhase {
            phase: "handler",
            price: 100,
            portfolio_position: Some(2),
            dispatch_time: 5_000_000,
        }
    ));

    let observed: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            TraceEvent::Observed {
                kind,
                dispatch_time,
                price,
            } => Some((*kind, *dispatch_time, *price)),
            _ => None,
        })
        .collect();
    assert_eq!(
        observed,
        vec![
            ("market", 0, 100),
            ("order", 0, 100),
            ("market", 2_000_000, 110),
            ("fill", 5_000_000, 100),
        ]
    );
}
