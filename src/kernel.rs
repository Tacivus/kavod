use std::{any::Any, collections::HashSet};

use crate::{
    cache::{Cache, State},
    clock::Clock,
    context::Context,
    graph,
    handler::{HandlerEntry, HandlerRegistry, StateGroupHandle},
    log::{InboundLog, SeqNo},
    message::Message,
    reducer::ReducerRegistry,
    schedule::Scheduler,
    time::timestamp::Timestamp,
};

const DEFAULT_MAX_ITERATIONS_PER_INSTANT: usize = 10_000;

pub struct Kernel {
    scheduler: Scheduler,
    cache: Cache,
    clock: Box<dyn Clock>,
    seq: SeqNo,
    inbound_log: InboundLog,
    handler_reg: HandlerRegistry,
    reducer_reg: ReducerRegistry,
    max_iterations_per_instant: usize,
}

impl Kernel {
    pub fn new(clock: Box<dyn Clock>) -> Self {
        Self {
            scheduler: Scheduler::new(),
            cache: Cache::new(),
            clock,
            seq: SeqNo::initial(),
            inbound_log: InboundLog::new(),
            handler_reg: HandlerRegistry::new(),
            reducer_reg: ReducerRegistry::new(),
            max_iterations_per_instant: DEFAULT_MAX_ITERATIONS_PER_INSTANT,
        }
    }

    pub fn push_event<M: Message>(&mut self, ts: Timestamp, msg: M) {
        self.seq = self.seq.next();
        self.scheduler.push(ts, self.seq, msg);
    }

    pub fn on<M: Message>(
        &mut self,
        handler: impl Fn(&mut Context, &M) + 'static,
    ) -> &mut HandlerEntry {
        self.handler_reg.on(handler)
    }

    pub fn state<S: State + Send + 'static>(&mut self, init: S) -> StateGroupHandle<'_, S> {
        self.handler_reg.state(init)
    }

    pub fn reduce<M: Message>(&mut self, reduce: impl Fn(&mut Cache, &M) + 'static) {
        self.reducer_reg.register(reduce)
    }

    pub fn run(&mut self) {
        graph::validate(&self.handler_reg, &self.reducer_reg, &HashSet::new());

        let mut instant_iter_count = 0;
        let mut current_instant = self.clock.now();

        while let Some(event) = self.scheduler.pop() {
            assert!(
                event.ts >= self.clock.now(),
                "event timestamp {:?} is before current clock {:?}",
                event.ts,
                self.clock.now(),
            );

            self.clock.set(event.ts);
            self.seq = self.seq.next();

            if event.ts != current_instant {
                current_instant = event.ts;
                instant_iter_count = 0;
            }
            instant_iter_count += 1;
            assert!(
                instant_iter_count <= self.max_iterations_per_instant,
                "exceeded max iterations ({}) at timestamp {:?}",
                self.max_iterations_per_instant,
                event.ts,
            );

            let type_id: std::any::TypeId = (&*event.payload as &dyn Any).type_id();

            self.reducer_reg.reduce(&mut self.cache, &*event.payload);

            let mut outbox: Vec<(Timestamp, Box<dyn Message>)> = Vec::new();
            self.handler_reg.dispatch(
                type_id,
                &*event.payload,
                &self.cache,
                &*self.clock,
                &self.seq,
                &mut outbox,
            );

            for (ts, msg) in outbox {
                self.seq = self.seq.next();
                self.scheduler.push_boxed(ts, self.seq, msg);
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn cache(&self) -> &Cache {
        &self.cache
    }

    #[cfg(test)]
    fn current_now(&self) -> Timestamp {
        self.clock.now()
    }

    #[cfg(test)]
    fn is_empty(&self) -> bool {
        self.scheduler.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{cache::State, clock::sim::SimClock, message::Message};

    #[derive(Clone, Debug, PartialEq)]
    struct Bar {
        instrument: u32,
        price: i64,
    }
    impl Message for Bar {}

    #[derive(Clone, Debug, PartialEq)]
    struct NewOrder {
        instrument: u32,
        quantity: i64,
    }
    impl Message for NewOrder {}

    #[derive(Clone, Debug, PartialEq)]
    struct ApprovedOrder {
        instrument: u32,
        quantity: i64,
    }
    impl Message for ApprovedOrder {}

    #[derive(Clone, Debug, PartialEq)]
    struct Signal {
        strength: f64,
    }
    impl Message for Signal {}

    #[derive(Clone, Debug, PartialEq)]
    struct CascadeLog {
        events: Vec<String>,
    }
    impl State for CascadeLog {
        type Key = ();
        fn key(&self) {}
    }

    #[derive(Clone, Debug, PartialEq)]
    struct OrderLog {
        count: u64,
    }
    impl State for OrderLog {
        type Key = ();
        fn key(&self) {}
    }

    #[derive(Clone, Debug, PartialEq)]
    struct ReducerFlag {
        touched: bool,
    }
    impl State for ReducerFlag {
        type Key = ();
        fn key(&self) {}
    }

    #[derive(Clone, Debug, PartialEq)]
    struct SmaCounter {
        count: u64,
    }
    impl State for SmaCounter {
        type Key = ();
        fn key(&self) {}
    }

    fn sim_kernel() -> Kernel {
        Kernel::new(Box::new(SimClock::new(Timestamp::new(0))))
    }

    // ==================================================================
    // Empty kernel
    // ==================================================================

    #[test]
    fn empty_kernel_run_clean_exit() {
        let mut kernel = sim_kernel();
        kernel.run();
    }

    #[test]
    fn new_kernel_cache_is_empty() {
        let kernel = sim_kernel();
        assert!(kernel.cache.is_empty());
    }

    // ==================================================================
    // End-to-end: Bar -> NewOrder -> ApprovedOrder
    // ==================================================================

    #[test]
    fn bar_to_order_to_approved_cascade() {
        let mut kernel = sim_kernel();

        kernel.cache.insert(CascadeLog { events: Vec::new() });

        kernel
            .on::<Bar>(|ctx, bar| {
                ctx.send(NewOrder {
                    instrument: bar.instrument,
                    quantity: bar.price as i64,
                });
            })
            .produces::<NewOrder>();

        kernel
            .on::<NewOrder>(|ctx, order| {
                ctx.send(ApprovedOrder {
                    instrument: order.instrument,
                    quantity: order.quantity,
                });
            })
            .produces::<ApprovedOrder>();

        kernel.reduce::<ApprovedOrder>(|cache, ao| {
            if let Some(log) = cache.get_mut::<CascadeLog>() {
                log.events.push(format!(
                    "approved: inst={}, qty={}",
                    ao.instrument, ao.quantity
                ));
            }
        });

        kernel.push_event(
            Timestamp::new(1000),
            Bar {
                instrument: 7,
                price: 50,
            },
        );

        kernel.run();

        assert!(kernel.is_empty());
        let log = kernel.cache().get::<CascadeLog>().unwrap();
        assert_eq!(log.events.len(), 1);
        assert!(log.events[0].contains("approved"));
    }

    // ==================================================================
    // Reducers run before handlers
    // ==================================================================

    #[test]
    fn reducer_runs_before_handler() {
        let mut kernel = sim_kernel();

        kernel.reduce::<Bar>(|cache, _bar| {
            cache.insert(ReducerFlag { touched: true });
        });

        kernel
            .on::<Bar>(|ctx, _bar| {
                let flag = ctx.get::<ReducerFlag>(&()).unwrap();
                ctx.send(Signal {
                    strength: if flag.touched { 1.0 } else { -1.0 },
                });
            })
            .produces::<Signal>();

        kernel.reduce::<Signal>(|cache, sig| {
            cache.insert(OrderLog {
                count: sig.strength as u64,
            });
        });

        kernel.push_event(
            Timestamp::new(0),
            Bar {
                instrument: 1,
                price: 0,
            },
        );

        kernel.run();

        assert_eq!(kernel.cache().get::<OrderLog>().unwrap().count, 1);
    }

    #[test]
    fn handler_sees_reducer_mutation_not_stale_state() {
        let mut kernel = sim_kernel();

        kernel.reduce::<Bar>(|cache, _bar| {
            cache.insert(OrderLog { count: 99 });
        });

        kernel
            .on::<Bar>(|ctx, _bar| {
                let log = ctx.get::<OrderLog>(&()).unwrap();
                ctx.send(Signal {
                    strength: log.count as f64 / 100.0,
                });
            })
            .produces::<Signal>();

        kernel.reduce::<Signal>(|_, _| {});

        kernel.push_event(
            Timestamp::new(0),
            Bar {
                instrument: 1,
                price: 0,
            },
        );

        kernel.run();

        assert_eq!(kernel.cache().get::<OrderLog>().unwrap().count, 99);
    }

    // ==================================================================
    // Multiple events — staggered timestamps
    // ==================================================================

    #[test]
    fn staggered_timestamps_process_in_order() {
        let mut kernel = sim_kernel();

        kernel.cache.insert(CascadeLog { events: Vec::new() });

        kernel
            .on::<Bar>(|ctx, bar| {
                ctx.send(Signal {
                    strength: bar.price as f64,
                });
            })
            .produces::<Signal>();

        kernel.reduce::<Signal>(|cache, sig| {
            if let Some(log) = cache.get_mut::<CascadeLog>() {
                log.events.push(format!("sig: s={}", sig.strength as i64));
            }
        });

        kernel.push_event(
            Timestamp::new(300),
            Bar {
                instrument: 1,
                price: 3,
            },
        );
        kernel.push_event(
            Timestamp::new(100),
            Bar {
                instrument: 1,
                price: 1,
            },
        );
        kernel.push_event(
            Timestamp::new(200),
            Bar {
                instrument: 1,
                price: 2,
            },
        );

        kernel.run();

        let log = kernel.cache().get::<CascadeLog>().unwrap();
        assert_eq!(&log.events, &["sig: s=1", "sig: s=2", "sig: s=3"]);
    }

    // ==================================================================
    // Same-instant BFS cascade
    // ==================================================================

    #[test]
    fn same_instant_cascade_before_time_advances() {
        let mut kernel = sim_kernel();

        kernel.cache.insert(CascadeLog { events: Vec::new() });

        kernel
            .on::<Bar>(|ctx, bar| {
                ctx.send(NewOrder {
                    instrument: bar.instrument,
                    quantity: 1,
                });
            })
            .produces::<NewOrder>();

        kernel
            .on::<NewOrder>(|ctx, order| {
                ctx.send(Signal {
                    strength: order.quantity as f64,
                });
            })
            .produces::<Signal>();

        kernel.reduce::<Signal>(|cache, sig| {
            if let Some(log) = cache.get_mut::<CascadeLog>() {
                log.events.push(format!("sig: s={}", sig.strength as i64));
            }
        });

        kernel.push_event(
            Timestamp::new(100),
            Bar {
                instrument: 1,
                price: 0,
            },
        );
        kernel.push_event(
            Timestamp::new(200),
            Bar {
                instrument: 1,
                price: 0,
            },
        );

        kernel.run();

        let log = kernel.cache().get::<CascadeLog>().unwrap();
        assert_eq!(log.events.len(), 2);
    }

    // ==================================================================
    // Graph validation panics at run()
    // ==================================================================

    #[test]
    #[should_panic(expected = "No consumer")]
    fn orphan_production_panics_at_run() {
        let mut kernel = sim_kernel();

        kernel
            .on::<Bar>(|ctx, _bar| {
                ctx.send(NewOrder {
                    instrument: 1,
                    quantity: 1,
                });
            })
            .produces::<NewOrder>();

        kernel.push_event(
            Timestamp::new(0),
            Bar {
                instrument: 1,
                price: 0,
            },
        );

        kernel.run();
    }

    #[test]
    fn valid_graph_run_does_not_panic() {
        let mut kernel = sim_kernel();

        kernel
            .on::<Bar>(|ctx, _bar| {
                ctx.send(Signal { strength: 1.0 });
            })
            .produces::<Signal>();

        kernel.reduce::<Signal>(|_, _| {});

        kernel.push_event(
            Timestamp::new(0),
            Bar {
                instrument: 1,
                price: 0,
            },
        );

        kernel.run();
    }

    // ==================================================================
    // Push event populates scheduler
    // ==================================================================

    #[test]
    fn push_event_populates_scheduler() {
        let mut kernel = sim_kernel();

        kernel.push_event(
            Timestamp::new(500),
            Bar {
                instrument: 1,
                price: 100,
            },
        );

        assert_eq!(kernel.scheduler.len(), 1);
        let event = kernel.scheduler.pop().unwrap();
        assert_eq!(event.ts, Timestamp::new(500));
        let payload: &dyn Any = &*event.payload;
        assert!(payload.downcast_ref::<Bar>().is_some());
    }

    #[test]
    fn push_event_increments_seq() {
        let mut kernel = sim_kernel();
        let s0 = kernel.seq;

        kernel.push_event(
            Timestamp::new(0),
            Bar {
                instrument: 1,
                price: 0,
            },
        );
        let s1 = kernel.seq;
        assert!(s1 > s0);

        kernel.push_event(
            Timestamp::new(0),
            Bar {
                instrument: 1,
                price: 0,
            },
        );
        let s2 = kernel.seq;
        assert!(s2 > s1);
    }

    // ==================================================================
    // Stateful handlers
    // ==================================================================

    #[test]
    fn stateful_handler_persists_state_through_kernel() {
        let mut kernel = sim_kernel();

        kernel.cache.insert(CascadeLog { events: Vec::new() });

        kernel
            .state(SmaCounter { count: 0 })
            .on::<Bar>(|state, ctx, _bar| {
                state.count += 1;
                ctx.send(Signal {
                    strength: state.count as f64,
                });
            })
            .produces::<Signal>();

        kernel.reduce::<Signal>(|cache, sig| {
            if let Some(log) = cache.get_mut::<CascadeLog>() {
                log.events.push(format!("sig: s={}", sig.strength as i64));
            }
        });

        kernel.push_event(
            Timestamp::new(0),
            Bar {
                instrument: 1,
                price: 0,
            },
        );
        kernel.push_event(
            Timestamp::new(1),
            Bar {
                instrument: 1,
                price: 0,
            },
        );

        kernel.run();

        let log = kernel.cache().get::<CascadeLog>().unwrap();
        assert_eq!(log.events.len(), 2);
        assert_eq!(log.events[0], "sig: s=1");
        assert_eq!(log.events[1], "sig: s=2");
    }

    // ==================================================================
    // Clock
    // ==================================================================

    #[test]
    fn kernel_respects_initial_clock() {
        let clock = Box::new(SimClock::new(Timestamp::new(999)));
        let kernel = Kernel::new(clock);
        assert_eq!(kernel.current_now(), Timestamp::new(999));
    }

    #[test]
    #[should_panic(expected = "before current clock")]
    fn past_event_panics() {
        let mut kernel = sim_kernel();

        kernel.push_event(
            Timestamp::new(1000),
            Bar {
                instrument: 1,
                price: 0,
            },
        );
        kernel.run();

        kernel.push_event(
            Timestamp::new(500),
            Bar {
                instrument: 1,
                price: 0,
            },
        );
        kernel.run();
    }

    // ==================================================================
    // Multiple reducers
    // ==================================================================

    #[test]
    fn multiple_reducers_run_in_order() {
        let mut kernel = sim_kernel();

        kernel.reduce::<Bar>(|cache, _bar| {
            cache.insert(OrderLog { count: 1 });
        });

        kernel.reduce::<Bar>(|cache, _bar| {
            if let Some(log) = cache.get_mut::<OrderLog>() {
                log.count *= 10;
            }
        });

        kernel.reduce::<Bar>(|cache, _bar| {
            if let Some(log) = cache.get_mut::<OrderLog>() {
                log.count += 5;
            }
        });

        kernel.push_event(
            Timestamp::new(0),
            Bar {
                instrument: 1,
                price: 0,
            },
        );

        kernel.run();

        assert_eq!(kernel.cache().get::<OrderLog>().unwrap().count, 15);
    }

    // ==================================================================
    // Edge cases
    // ==================================================================

    #[test]
    fn no_handler_for_type_is_noop() {
        let mut kernel = sim_kernel();

        kernel.push_event(
            Timestamp::new(0),
            Bar {
                instrument: 1,
                price: 0,
            },
        );

        kernel.run();
    }

    #[test]
    fn handler_without_produces_generates_no_messages() {
        let mut kernel = sim_kernel();

        kernel.on::<Bar>(|_ctx, _bar| {});

        kernel.push_event(
            Timestamp::new(0),
            Bar {
                instrument: 1,
                price: 0,
            },
        );

        kernel.run();
    }

    #[test]
    fn send_at_schedules_future_message() {
        let mut kernel = sim_kernel();

        kernel.cache.insert(CascadeLog { events: Vec::new() });

        kernel
            .on::<Bar>(|ctx, _bar| {
                ctx.send_at(Timestamp::new(5000), Signal { strength: 1.0 });
            })
            .produces::<Signal>();

        kernel
            .on::<Signal>(|ctx, _sig| {
                ctx.send(NewOrder {
                    instrument: 1,
                    quantity: 1,
                });
            })
            .produces::<NewOrder>();

        kernel.reduce::<NewOrder>(|cache, order| {
            if let Some(log) = cache.get_mut::<CascadeLog>() {
                log.events.push(format!("order: inst={}", order.instrument));
            }
        });

        kernel.push_event(
            Timestamp::new(1000),
            Bar {
                instrument: 1,
                price: 0,
            },
        );

        kernel.run();

        let log = kernel.cache().get::<CascadeLog>().unwrap();
        assert_eq!(log.events.len(), 1);
    }
}

