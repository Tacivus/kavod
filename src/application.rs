use core::fmt;

use serde::Serialize;

use crate::{
    bounded_buffer::BoundedBuffer,
    time::{EventIndex, Timestamp},
};

#[derive(Debug)]
pub enum Outcome<F> {
    Continue,
    Stop,
    Fatal(F),
}

pub trait Application {
    type State;
    type Event: Serialize;
    type Command: Serialize;
    type Fatal: Serialize;

    fn initial_state(&self) -> Self::State;

    fn on_start(
        &self,
        state: &mut Self::State,
        ctx: &mut Context<'_, Self::Command>,
    ) -> Outcome<Self::Fatal>;

    fn on_event(
        &self,
        state: &mut Self::State,
        event: &Self::Event,
        ctx: &mut Context<'_, Self::Command>,
    ) -> Outcome<Self::Fatal>;
}

/// Handler authority for one accepted turn.
pub struct Context<'a, C> {
    index: EventIndex,
    time: Timestamp,
    commands: &'a mut BoundedBuffer<C>,
    overflowed: bool,
}

impl<C> fmt::Debug for Context<'_, C> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Context")
            .field("index", &self.index)
            .field("time", &self.time)
            .field("len", &self.commands.len())
            .field("capacity", &self.commands.capacity())
            .field("overflowed", &self.overflowed)
            .finish()
    }
}

impl<'a, C> Context<'a, C> {
    /// Starts a turn with an empty, reusable command batch.
    pub(crate) fn new(
        index: EventIndex,
        time: Timestamp,
        commands: &'a mut BoundedBuffer<C>,
    ) -> Self {
        commands.clear();
        assert!(commands.is_empty(), "clear did not empty the cmd buffer");

        Self {
            index,
            time,
            commands,
            overflowed: false,
        }
    }

    /// The current accepted turn index.
    #[must_use]
    pub fn index(&self) -> EventIndex {
        self.index
    }

    /// The current accepted turn's logical time.
    #[must_use]
    pub fn logical_time(&self) -> Timestamp {
        self.time
    }

    /// Exact commands this turn can still stage, or zero after overflow.
    #[must_use]
    pub fn remaining(&self) -> usize {
        let remaining = self.commands.remaining_capacity();
        assert!(
            !self.overflowed || remaining == 0,
            "overflow marker set with capacity remaining"
        );
        remaining
    }

    /// Stages one command in call order until the turn exceeds its configured bound.
    pub fn emit(&mut self, cmd: C) {
        if self.overflowed {
            assert!(
                self.commands.is_full(),
                "overflow marker set with non-full buffer"
            );
            return;
        }

        if self.commands.push(cmd).is_err() {
            self.overflowed = true;
            assert!(self.commands.is_full(), "overflow marker set when not full");
        }
    }

    #[must_use]
    pub(crate) fn is_overflowed(&self) -> bool {
        self.overflowed
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, rc::Rc};

    use super::*;

    #[derive(Debug)]
    struct DropProbe(Rc<Cell<usize>>);

    impl Drop for DropProbe {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }

    mod context_construction {
        use super::*;

        /// Invariant: `Context` is the single authority for the turn's index and time.
        #[test]
        fn reports_the_index_and_time_it_was_built_with() {
            let mut buffer = BoundedBuffer::<i32>::new(3).unwrap();
            let ctx = Context::new(EventIndex::START, Timestamp::new(42), &mut buffer);

            assert_eq!(ctx.index(), EventIndex::START);
            assert_eq!(ctx.logical_time(), Timestamp::new(42));
        }

        /// Invariant: A fresh turn starts with the buffer's full capacity available and unoverflowed.
        #[test]
        fn starts_with_full_capacity_and_no_overflow() {
            let mut buffer = BoundedBuffer::<i32>::new(3).unwrap();
            let ctx = Context::new(EventIndex::START, Timestamp::new(0), &mut buffer);

            assert_eq!(ctx.remaining(), 3);
            assert!(!ctx.is_overflowed());
        }

        /// Invariant: `new` clears whatever the buffer held from a prior turn before the handler runs.
        #[test]
        fn clears_values_left_by_a_previous_turn() {
            let mut buffer = BoundedBuffer::new(2).unwrap();
            {
                let mut ctx = Context::new(EventIndex::START, Timestamp::new(0), &mut buffer);
                ctx.emit(1);
            }

            let ctx = Context::new(EventIndex::START, Timestamp::new(0), &mut buffer);

            assert_eq!(ctx.remaining(), 2);
            assert_eq!(buffer.as_slice(), &[] as &[i32]);
        }
    }

    mod context_emit_ordering {
        use super::*;

        /// Invariant: `emit` stages commands in call order (APP-EMIT).
        #[test]
        fn stages_commands_in_call_order() {
            let mut buffer = BoundedBuffer::new(3).unwrap();
            let mut ctx = Context::new(EventIndex::START, Timestamp::new(0), &mut buffer);

            ctx.emit(1);
            ctx.emit(2);
            ctx.emit(3);

            assert_eq!(buffer.as_slice(), [1, 2, 3]);
        }

        /// Invariant: `emit` accepts exactly the buffer's capacity without overflowing.
        #[test]
        fn accepts_exactly_capacity_commands_without_overflow() {
            let mut buffer = BoundedBuffer::new(3).unwrap();
            let mut ctx = Context::new(EventIndex::START, Timestamp::new(0), &mut buffer);

            ctx.emit(1);
            ctx.emit(2);
            ctx.emit(3);

            assert!(!ctx.is_overflowed());
            assert_eq!(ctx.remaining(), 0);
        }

        /// Invariant: `remaining` decreases by exactly one per accepted emit.
        #[test]
        fn remaining_decreases_by_one_per_accepted_emit() {
            let mut buffer = BoundedBuffer::new(2).unwrap();
            let mut ctx = Context::new(EventIndex::START, Timestamp::new(0), &mut buffer);

            assert_eq!(ctx.remaining(), 2);
            ctx.emit(1);
            assert_eq!(ctx.remaining(), 1);
            ctx.emit(2);
            assert_eq!(ctx.remaining(), 0);
        }
    }

    mod context_overflow {
        use super::*;

        /// Invariant: The first over-bound emit sets the overflow marker and stores nothing.
        #[test]
        fn first_over_bound_emit_sets_the_marker_and_stores_nothing() {
            let mut buffer = BoundedBuffer::new(1).unwrap();
            let mut ctx = Context::new(EventIndex::START, Timestamp::new(0), &mut buffer);

            ctx.emit(1);
            ctx.emit(2);

            assert!(ctx.is_overflowed());
            assert_eq!(buffer.as_slice(), [1]);
        }

        /// Invariant: Every emit after the first overflow also stores nothing.
        #[test]
        fn later_emits_after_overflow_also_store_nothing() {
            let mut buffer = BoundedBuffer::new(1).unwrap();
            let mut ctx = Context::new(EventIndex::START, Timestamp::new(0), &mut buffer);

            ctx.emit(1);
            ctx.emit(2);
            ctx.emit(3);
            ctx.emit(4);

            assert!(ctx.is_overflowed());
            assert_eq!(buffer.as_slice(), [1]);
        }

        /// Invariant: `remaining` is zero once the overflow marker is set.
        #[test]
        fn remaining_is_zero_once_overflowed() {
            let mut buffer = BoundedBuffer::new(1).unwrap();
            let mut ctx = Context::new(EventIndex::START, Timestamp::new(0), &mut buffer);

            ctx.emit(1);
            ctx.emit(2);

            assert_eq!(ctx.remaining(), 0);
        }

        /// Invariant: A command rejected because the buffer is already full is dropped exactly once.
        #[test]
        fn command_rejected_at_full_capacity_is_dropped_exactly_once() {
            let drops = Rc::new(Cell::new(0));
            let mut buffer = BoundedBuffer::new(1).unwrap();
            let mut ctx = Context::new(EventIndex::START, Timestamp::new(0), &mut buffer);

            ctx.emit(DropProbe(Rc::clone(&drops)));
            assert_eq!(drops.get(), 0);

            ctx.emit(DropProbe(Rc::clone(&drops)));
            assert_eq!(drops.get(), 1);
        }

        /// Invariant: A command short-circuited by an already-set overflow marker is dropped exactly once.
        #[test]
        fn command_rejected_after_marker_is_already_set_is_dropped_exactly_once() {
            let drops = Rc::new(Cell::new(0));
            let mut buffer = BoundedBuffer::new(1).unwrap();
            let mut ctx = Context::new(EventIndex::START, Timestamp::new(0), &mut buffer);

            ctx.emit(DropProbe(Rc::clone(&drops)));
            ctx.emit(DropProbe(Rc::clone(&drops)));
            assert_eq!(drops.get(), 1);

            ctx.emit(DropProbe(Rc::clone(&drops)));
            assert_eq!(drops.get(), 2);
        }

        /// Invariant: A zero-capacity turn overflows on its very first emit.
        #[test]
        fn zero_capacity_context_overflows_on_first_emit() {
            let mut buffer = BoundedBuffer::new(0).unwrap();
            let mut ctx = Context::new(EventIndex::START, Timestamp::new(0), &mut buffer);

            ctx.emit(1);

            assert!(ctx.is_overflowed());
            assert_eq!(ctx.remaining(), 0);
        }
    }

    mod context_reuse_across_turns {
        use super::*;

        /// Invariant: The overflow marker lives on `Context`, not the buffer — a new turn
        /// over a previously overflowed buffer starts unoverflowed with full capacity.
        #[test]
        fn new_turn_over_a_previously_overflowed_buffer_starts_clean() {
            let mut buffer = BoundedBuffer::new(1).unwrap();
            {
                let mut ctx = Context::new(EventIndex::START, Timestamp::new(0), &mut buffer);
                ctx.emit(1);
                ctx.emit(2);
                assert!(ctx.is_overflowed());
            }

            let next_index = EventIndex::START.checked_next().unwrap();
            let ctx = Context::new(next_index, Timestamp::new(1), &mut buffer);

            assert!(!ctx.is_overflowed());
            assert_eq!(ctx.remaining(), 1);
        }
    }

    mod context_debug_format {
        use super::*;

        /// Invariant: `Context`'s Debug output does not require the command type to implement Debug.
        #[test]
        fn reports_state_without_requiring_command_debug() {
            struct NotDebug;

            let mut buffer = BoundedBuffer::new(2).unwrap();
            let mut ctx = Context::new(EventIndex::START, Timestamp::new(7), &mut buffer);
            ctx.emit(NotDebug);

            assert_eq!(
                format!("{ctx:?}"),
                "Context { index: EventIndex(0), time: Timestamp(7), len: 1, capacity: 2, overflowed: false }"
            );
        }
    }
}
