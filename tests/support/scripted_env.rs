use kavod::{Environment, Quiescence, ShutdownReport, Timestamp};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EnvCall<E, C> {
    Start(Result<Timestamp, ()>),
    NextEvent(Result<(E, Timestamp), ()>),
    Dispatch {
        command: C,
        result: Result<(), ()>,
    },
    TakeError {
        returned_error: bool,
    },
    Shutdown {
        quiescence: TraceQuiescence,
        returned_error: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TraceQuiescence {
    Quiesced,
    Incomplete,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct EnvTrace<E, C> {
    pub calls: Vec<EnvCall<E, C>>,
    pub handoffs: Vec<C>,
    pub shutdown_count: usize,
}

pub type SharedEnvTrace<E, C> = Rc<RefCell<EnvTrace<E, C>>>;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    BeforeStart,
    Turn,
    Checkpointed,
    MustShutdown,
    StartFailed,
}

pub struct ScriptedEnv<E, C, Err> {
    start: Option<Result<Timestamp, Err>>,
    next_events: VecDeque<Result<(E, Timestamp), Err>>,
    dispatches: VecDeque<Result<(), Err>>,
    checkpoints: VecDeque<Option<Err>>,
    shutdown: ShutdownReport<Err>,
    phase: Phase,
    trace: SharedEnvTrace<E, C>,
}

impl<E, C, Err> ScriptedEnv<E, C, Err> {
    pub fn new(
        start: Result<Timestamp, Err>,
        next_events: impl IntoIterator<Item = Result<(E, Timestamp), Err>>,
        dispatches: impl IntoIterator<Item = Result<(), Err>>,
        checkpoints: impl IntoIterator<Item = Option<Err>>,
        shutdown: ShutdownReport<Err>,
    ) -> (Self, SharedEnvTrace<E, C>) {
        let trace = Rc::new(RefCell::new(EnvTrace {
            calls: Vec::new(),
            handoffs: Vec::new(),
            shutdown_count: 0,
        }));
        (
            Self {
                start: Some(start),
                next_events: next_events.into_iter().collect(),
                dispatches: dispatches.into_iter().collect(),
                checkpoints: checkpoints.into_iter().collect(),
                shutdown,
                phase: Phase::BeforeStart,
                trace: Rc::clone(&trace),
            },
            trace,
        )
    }
}

impl<E: Clone, C: Clone, Err> Environment for ScriptedEnv<E, C, Err> {
    type Event = E;
    type Command = C;
    type Error = Err;

    fn start(&mut self) -> Result<Timestamp, Self::Error> {
        assert!(
            self.phase == Phase::BeforeStart,
            "Environment start must be the first operation and occur at most once"
        );
        let result = self
            .start
            .take()
            .expect("the Environment start script must be consumed exactly once");
        self.phase = if result.is_ok() {
            Phase::Turn
        } else {
            Phase::StartFailed
        };
        self.trace
            .borrow_mut()
            .calls
            .push(EnvCall::Start(result.as_ref().copied().map_err(|_| ())));
        result
    }

    fn next_event(&mut self) -> Result<(Self::Event, Timestamp), Self::Error> {
        assert!(
            self.phase == Phase::Checkpointed,
            "next_event must follow a successful checkpoint with no pending Error"
        );
        let result = self
            .next_events
            .pop_front()
            .expect("each next_event call must consume exactly one scripted result");
        self.phase = if result.is_ok() {
            Phase::Turn
        } else {
            Phase::MustShutdown
        };
        self.trace.borrow_mut().calls.push(EnvCall::NextEvent(
            result
                .as_ref()
                .map(|(event, time)| (event.clone(), *time))
                .map_err(|_| ()),
        ));
        result
    }

    fn dispatch(&mut self, command: Self::Command) -> Result<(), Self::Error> {
        assert!(
            self.phase == Phase::Turn,
            "dispatch must occur during an open turn before its checkpoint"
        );
        let result = self
            .dispatches
            .pop_front()
            .expect("each dispatch call must consume exactly one scripted result");
        if result.is_ok() {
            self.trace.borrow_mut().handoffs.push(command.clone());
        } else {
            self.phase = Phase::MustShutdown;
        }
        self.trace.borrow_mut().calls.push(EnvCall::Dispatch {
            command,
            result: result.as_ref().map(|_| ()).map_err(|_| ()),
        });
        result
    }

    fn take_error(&mut self) -> Option<Self::Error> {
        assert!(
            self.phase == Phase::Turn,
            "take_error must checkpoint an open turn after all dispatches"
        );
        let result = self
            .checkpoints
            .pop_front()
            .expect("each take_error call must consume exactly one scripted result");
        self.phase = if result.is_some() {
            Phase::MustShutdown
        } else {
            Phase::Checkpointed
        };
        self.trace.borrow_mut().calls.push(EnvCall::TakeError {
            returned_error: result.is_some(),
        });
        result
    }

    fn shutdown(self) -> ShutdownReport<Self::Error> {
        assert!(
            matches!(
                self.phase,
                Phase::Turn | Phase::Checkpointed | Phase::MustShutdown
            ),
            "shutdown must follow successful startup and be the final Environment operation"
        );
        let quiescence = match &self.shutdown.quiescence {
            Quiescence::Quiesced => TraceQuiescence::Quiesced,
            Quiescence::Incomplete => TraceQuiescence::Incomplete,
        };
        let mut trace = self.trace.borrow_mut();
        trace.shutdown_count += 1;
        trace.calls.push(EnvCall::Shutdown {
            quiescence,
            returned_error: self.shutdown.error.is_some(),
        });
        drop(trace);
        self.shutdown
    }
}
