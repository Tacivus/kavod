use kavod::{Application, Context, Outcome};
use serde::Serialize;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AppCall<E> {
    InitialState,
    OnStart {
        index: u64,
        logical_time: u64,
    },
    OnEvent {
        event: E,
        index: u64,
        logical_time: u64,
    },
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct AppTrace<E> {
    pub calls: Vec<AppCall<E>>,
}

pub type SharedAppTrace<E> = Rc<RefCell<AppTrace<E>>>;

pub enum ScriptedAnswer<Err> {
    Continue,
    Stop,
    Fatal(Err),
}

pub struct ScriptedTurn<C, Err> {
    mutation: u8,
    commands: Vec<C>,
    answer: ScriptedAnswer<Err>,
}

impl<C, Err> ScriptedTurn<C, Err> {
    pub fn new(mutation: u8, commands: Vec<C>, answer: ScriptedAnswer<Err>) -> Self {
        Self {
            mutation,
            commands,
            answer,
        }
    }
}

pub struct RecordingApp<E, C, Err> {
    initial_state: Vec<u8>,
    turns: RefCell<VecDeque<ScriptedTurn<C, Err>>>,
    trace: SharedAppTrace<E>,
}

impl<E, C, Err> RecordingApp<E, C, Err> {
    pub fn new(
        initial_state: Vec<u8>,
        turns: impl IntoIterator<Item = ScriptedTurn<C, Err>>,
    ) -> (Self, SharedAppTrace<E>) {
        let trace = Rc::new(RefCell::new(AppTrace { calls: Vec::new() }));
        (
            Self {
                initial_state,
                turns: RefCell::new(turns.into_iter().collect()),
                trace: Rc::clone(&trace),
            },
            trace,
        )
    }

    fn handle(&self, state: &mut Vec<u8>, context: &mut Context<'_, C>) -> Outcome<Err> {
        let turn = self
            .turns
            .borrow_mut()
            .pop_front()
            .expect("each Application handler call must consume exactly one scripted turn");
        state.push(turn.mutation);
        for command in turn.commands {
            context.emit(command);
        }
        match turn.answer {
            ScriptedAnswer::Continue => Outcome::Continue,
            ScriptedAnswer::Stop => Outcome::Stop,
            ScriptedAnswer::Fatal(error) => Outcome::Fatal(error),
        }
    }
}

impl<E: Clone + Serialize, C: Serialize, Err> Application for RecordingApp<E, C, Err> {
    type State = Vec<u8>;
    type Event = E;
    type Command = C;
    type Error = Err;

    fn initial_state(&self) -> Self::State {
        self.trace.borrow_mut().calls.push(AppCall::InitialState);
        self.initial_state.clone()
    }

    fn on_start(
        &self,
        state: &mut Self::State,
        context: &mut Context<'_, Self::Command>,
    ) -> Outcome<Self::Error> {
        self.trace.borrow_mut().calls.push(AppCall::OnStart {
            index: context.index().as_u64(),
            logical_time: context.logical_time().as_nanos(),
        });
        self.handle(state, context)
    }

    fn on_event(
        &self,
        state: &mut Self::State,
        event: &Self::Event,
        context: &mut Context<'_, Self::Command>,
    ) -> Outcome<Self::Error> {
        self.trace.borrow_mut().calls.push(AppCall::OnEvent {
            event: event.clone(),
            index: context.index().as_u64(),
            logical_time: context.logical_time().as_nanos(),
        });
        self.handle(state, context)
    }
}
