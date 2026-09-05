use crate::bounded_buffer::BoundedBuffer;
use crate::{EventIndex, Timestamp};
use serde::Serialize;

pub enum Outcome<E> {
    Continue,
    Stop,
    Fatal(E),
}

pub trait Application {
    type State;
    type Event: Serialize;
    type Command: Serialize;
    type Error;

    fn initial_state(&self) -> Self::State;

    fn on_start(
        &self,
        state: &mut Self::State,
        ctx: &mut Context<'_, Self::Command>,
    ) -> Outcome<Self::Error>;

    fn on_event(
        &self,
        state: &mut Self::State,
        event: &Self::Event,
        ctx: &mut Context<'_, Self::Command>,
    ) -> Outcome<Self::Error>;
}

pub struct Context<'a, C> {
    buffer: &'a mut BoundedBuffer<C>,
    overflowed: bool,
    index: EventIndex,
    logical_time: Timestamp,
}

impl<'a, C> Context<'a, C> {
    #[allow(
        dead_code,
        reason = "the Engine constructs Contexts in a later build step"
    )]
    pub(crate) fn new(
        buffer: &'a mut BoundedBuffer<C>,
        index: EventIndex,
        logical_time: Timestamp,
    ) -> Self {
        buffer.clear();
        Self {
            buffer,
            overflowed: false,
            index,
            logical_time,
        }
    }

    /// Returns the current accepted turn.
    pub fn index(&self) -> EventIndex {
        self.index
    }

    /// Returns the current turn's accepted logical time.
    pub fn logical_time(&self) -> Timestamp {
        self.logical_time
    }

    /// Returns the exact number of commands the current batch can still store.
    pub fn remaining(&self) -> usize {
        if self.overflowed {
            return 0;
        }

        self.buffer
            .capacity()
            .checked_sub(self.buffer.len())
            .expect("a Context command buffer length must not exceed its logical capacity")
    }

    /// Transfers one command into the current batch when capacity remains.
    pub fn emit(&mut self, command: C) {
        if self.overflowed {
            return;
        }
        if self.buffer.try_push(command).is_err() {
            self.overflowed = true;
        }
    }

    #[allow(
        dead_code,
        reason = "the Engine reads the overflow marker in a later build step"
    )]
    pub(crate) fn overflowed(&self) -> bool {
        self.overflowed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod context_emit {
        use super::*;

        /// Invariant: commands emitted through the exact batch capacity are stored
        /// in the same order in which the handler emitted them.
        /// Design Doc: APP-EMIT
        #[test]
        fn commands_append_in_call_order_through_exact_capacity() {
            let mut buffer =
                BoundedBuffer::new(3).expect("a three-command batch must be reservable");
            let mut context =
                Context::new(&mut buffer, EventIndex::new(0), Timestamp::from_nanos(10));

            context.emit("first");
            context.emit("second");
            context.emit("third");

            assert_eq!(
                context.buffer.as_slice(),
                &["first", "second", "third"],
                "commands emitted through exact capacity must retain call order"
            );
        }

        /// Invariant: remaining capacity exactly tracks each successfully stored
        /// command and reaches zero when the batch is full.
        /// Design Doc: APP-CONTEXT
        #[test]
        fn remaining_reports_exact_free_capacity() {
            let mut buffer =
                BoundedBuffer::new(3).expect("a three-command batch must be reservable");
            let mut context =
                Context::new(&mut buffer, EventIndex::new(0), Timestamp::from_nanos(10));

            assert_eq!(
                context.remaining(),
                3,
                "a fresh Context must report every command slot as free"
            );
            context.emit(1);
            assert_eq!(
                context.remaining(),
                2,
                "one stored command must consume exactly one slot"
            );
            context.emit(2);
            assert_eq!(
                context.remaining(),
                1,
                "two stored commands must leave exactly one slot"
            );
            context.emit(3);
            assert_eq!(
                context.remaining(),
                0,
                "a full command batch must report no remaining slots"
            );
        }

        /// Invariant: a zero-capacity Context rejects its first command and records
        /// that the handler exceeded the command bound.
        #[test]
        fn zero_capacity_rejects_first_command_and_sets_marker() {
            let mut buffer = BoundedBuffer::new(0).expect("zero capacity must be reservable");
            let mut context =
                Context::new(&mut buffer, EventIndex::new(0), Timestamp::from_nanos(0));

            assert_eq!(
                context.remaining(),
                0,
                "a zero-capacity Context must initially report no free slots"
            );
            assert!(
                !context.overflowed(),
                "a fresh zero-capacity Context must begin with a clear overflow marker"
            );
            context.emit("rejected");
            assert!(
                context.buffer.is_empty(),
                "a zero-capacity Context must not store its first command"
            );
            assert!(
                context.overflowed(),
                "the first command offered at zero capacity must set the overflow marker"
            );
        }

        /// Invariant: a one-slot Context accepts exactly one command without
        /// marking the batch as overflowed.
        #[test]
        fn one_slot_capacity_accepts_one_command_without_overflow() {
            let mut buffer = BoundedBuffer::new(1).expect("one slot must be reservable");
            let mut context =
                Context::new(&mut buffer, EventIndex::new(0), Timestamp::from_nanos(0));

            context.emit("accepted");

            assert_eq!(
                context.buffer.as_slice(),
                &["accepted"],
                "a one-slot Context must retain its first command"
            );
            assert!(
                !context.overflowed(),
                "filling the only command slot must not set the overflow marker"
            );
        }
    }

    mod context_overflow {
        use super::*;

        /// Invariant: the first command emitted beyond the batch bound is not
        /// stored and permanently marks that handler invocation as overflowed.
        /// Design Doc: APP-OVERFLOW
        #[test]
        fn first_over_bound_emit_stores_nothing_and_sets_the_marker() {
            let mut buffer = BoundedBuffer::new(2).expect("two slots must be reservable");
            let mut context =
                Context::new(&mut buffer, EventIndex::new(0), Timestamp::from_nanos(10));
            context.emit(1);
            context.emit(2);

            context.emit(3);

            assert_eq!(
                context.buffer.as_slice(),
                &[1, 2],
                "the first over-bound command must leave the full batch unchanged"
            );
            assert!(
                context.overflowed(),
                "the first over-bound command must set the overflow marker"
            );
        }

        /// Invariant: after one command exceeds the batch bound, all subsequent
        /// commands from that handler invocation are discarded.
        /// Design Doc: APP-OVERFLOW
        #[test]
        fn every_later_emit_stores_nothing() {
            let mut buffer = BoundedBuffer::new(1).expect("one slot must be reservable");
            let mut context =
                Context::new(&mut buffer, EventIndex::new(0), Timestamp::from_nanos(10));
            context.emit("accepted");
            context.emit("first rejected");

            context.emit("second rejected");
            context.emit("third rejected");

            assert_eq!(
                context.buffer.as_slice(),
                &["accepted"],
                "commands emitted after overflow must leave the accepted batch unchanged"
            );
        }

        /// Invariant: once a handler exceeds its command bound, no capacity is
        /// reported for the rest of that invocation.
        /// Design Doc: APP-OVERFLOW
        #[test]
        fn remaining_is_zero_once_the_marker_is_set() {
            let mut buffer = BoundedBuffer::new(1).expect("one slot must be reservable");
            let mut context =
                Context::new(&mut buffer, EventIndex::new(0), Timestamp::from_nanos(10));
            context.emit("accepted");
            context.emit("rejected");

            assert_eq!(
                context.remaining(),
                0,
                "an overflowed Context must report no remaining capacity"
            );
        }

        /// Invariant: filling the batch exactly does not mark the handler as
        /// overflowed until another command is emitted.
        #[test]
        fn exact_capacity_keeps_overflow_marker_clear() {
            let mut buffer = BoundedBuffer::new(2).expect("two slots must be reservable");
            let mut context =
                Context::new(&mut buffer, EventIndex::new(0), Timestamp::from_nanos(0));

            context.emit(1);
            context.emit(2);

            assert!(
                !context.overflowed(),
                "accepting through exact capacity must leave the overflow marker clear"
            );
        }

        /// Invariant: each command rejected because of overflow is dropped during
        /// the emit call rather than retained by the Context.
        #[test]
        fn rejected_commands_are_dropped_immediately() {
            use std::cell::Cell;
            use std::rc::Rc;

            struct DropCommand(Rc<Cell<usize>>);

            impl Drop for DropCommand {
                fn drop(&mut self) {
                    self.0.set(self.0.get() + 1);
                }
            }

            let drops = Rc::new(Cell::new(0));
            let mut buffer = BoundedBuffer::new(0).expect("zero capacity must be reservable");
            let mut context =
                Context::new(&mut buffer, EventIndex::new(0), Timestamp::from_nanos(0));

            context.emit(DropCommand(Rc::clone(&drops)));
            assert_eq!(
                drops.get(),
                1,
                "the first over-bound command must be dropped before emit returns"
            );
            context.emit(DropCommand(Rc::clone(&drops)));
            assert_eq!(
                drops.get(),
                2,
                "a command rejected after overflow must be dropped before emit returns"
            );
        }
    }

    mod context_reuse {
        use super::*;

        /// Invariant: each handler invocation starts with no commands and no
        /// overflow indication, even when the previous invocation overflowed.
        /// Design Doc: APP-OVERFLOW
        #[test]
        fn fresh_invocation_starts_empty_with_a_clear_marker() {
            let mut buffer = BoundedBuffer::new(2).expect("two slots must be reservable");
            {
                let mut context =
                    Context::new(&mut buffer, EventIndex::new(0), Timestamp::from_nanos(10));
                context.emit(1);
                context.emit(2);
                context.emit(3);
                assert!(
                    context.overflowed(),
                    "the previous invocation must establish the overflow precondition"
                );
            }

            let context = Context::new(&mut buffer, EventIndex::new(1), Timestamp::from_nanos(20));

            assert!(
                context.buffer.is_empty(),
                "a fresh invocation must clear every command from the reused batch"
            );
            assert!(
                !context.overflowed(),
                "a fresh invocation must clear the previous overflow marker"
            );
            assert_eq!(
                context.remaining(),
                2,
                "a fresh invocation must make the full reused batch available"
            );
        }

        /// Invariant: beginning a new invocation clears commands from a successful
        /// prior invocation as well as from an overflowed one.
        #[test]
        fn fresh_invocation_clears_a_non_overflowed_batch() {
            let mut buffer = BoundedBuffer::new(2).expect("two slots must be reservable");
            {
                let mut context =
                    Context::new(&mut buffer, EventIndex::new(0), Timestamp::from_nanos(0));
                context.emit("prior");
                assert!(
                    !context.overflowed(),
                    "the prior invocation must remain within its command bound"
                );
            }

            let context = Context::new(&mut buffer, EventIndex::new(1), Timestamp::from_nanos(1));

            assert!(
                context.buffer.is_empty(),
                "a fresh invocation must clear a successful prior batch"
            );
            assert_eq!(
                context.remaining(),
                2,
                "clearing a successful prior batch must restore every slot"
            );
        }
    }

    mod context_observers {
        use super::*;

        /// Invariant: a Context reports exactly the accepted turn index and logical
        /// time supplied at construction, including both numeric domain boundaries.
        #[test]
        fn index_and_logical_time_report_exact_boundary_values() {
            let mut buffer = BoundedBuffer::<()>::new(1).expect("one slot must be reservable");

            for value in [0, u64::MAX] {
                let context = Context::new(
                    &mut buffer,
                    EventIndex::new(value),
                    Timestamp::from_nanos(value),
                );

                assert_eq!(
                    context.index().as_u64(),
                    value,
                    "Context must report its exact accepted turn index"
                );
                assert_eq!(
                    context.logical_time().as_nanos(),
                    value,
                    "Context must report its exact accepted logical time"
                );
            }
        }

        /// Invariant: emitting beyond capacity cannot alter the accepted turn index
        /// or logical time observed by the handler.
        #[test]
        fn index_and_logical_time_remain_stable_after_overflow() {
            let mut buffer = BoundedBuffer::new(0).expect("zero capacity must be reservable");
            let mut context =
                Context::new(&mut buffer, EventIndex::new(7), Timestamp::from_nanos(11));

            context.emit("rejected");

            assert_eq!(
                context.index().as_u64(),
                7,
                "overflow must not change the Context turn index"
            );
            assert_eq!(
                context.logical_time().as_nanos(),
                11,
                "overflow must not change the Context logical time"
            );
        }
    }
}
