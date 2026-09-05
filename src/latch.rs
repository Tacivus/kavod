use crate::{Quiescence, ShutdownReport};
use std::mem;

#[allow(dead_code, reason = "used by later Environment build steps")]
pub(crate) struct Latch<E> {
    state: State<E>,
}

enum State<E> {
    Empty,
    Pending(E),
    Reported,
    Closed,
}

#[allow(dead_code, reason = "used by later Environment build steps")]
impl<E> Latch<E> {
    pub(crate) fn new() -> Self {
        Self {
            state: State::Empty,
        }
    }

    pub(crate) fn publish(&mut self, error: E) {
        if let State::Empty = &self.state {
            self.state = State::Pending(error);
        }
    }

    pub(crate) fn take(&mut self) -> Option<E> {
        match mem::replace(&mut self.state, State::Reported) {
            State::Pending(error) => Some(error),
            State::Empty => {
                self.state = State::Empty;
                None
            }
            State::Reported => None,
            State::Closed => {
                self.state = State::Closed;
                None
            }
        }
    }

    pub(crate) fn close(&mut self) -> Option<E> {
        match mem::replace(&mut self.state, State::Closed) {
            State::Pending(error) => Some(error),
            State::Empty | State::Reported | State::Closed => None,
        }
    }

    pub(crate) fn is_pending(&self) -> bool {
        matches!(&self.state, State::Pending(_))
    }

    pub(crate) fn resolve_local_error(&mut self, local_error: E) -> E {
        self.take().unwrap_or(local_error)
    }

    pub(crate) fn close_into_report(&mut self, quiescence: Quiescence) -> ShutdownReport<E> {
        ShutdownReport {
            quiescence,
            error: self.close(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod latch_first_wins {
        use super::*;

        /// Invariant: the first error published to an unobserved latch is the only
        /// error that can later be returned.
        /// Design Doc: ENV-LATCH
        #[test]
        fn first_publication_is_kept_and_later_discarded() {
            let mut latch = Latch::new();

            latch.publish("first");
            latch.publish("later");

            assert_eq!(
                latch.take(),
                Some("first"),
                "the latch must retain its first publication and discard later ones"
            );
        }

        /// Invariant: once a pending error has been returned, it cannot be returned
        /// again or replaced by a later publication.
        /// Design Doc: ENV-LATCH
        #[test]
        fn take_marks_reported_forever() {
            let mut latch = Latch::new();
            latch.publish("first");

            assert_eq!(
                latch.take(),
                Some("first"),
                "taking a pending latch must return its stored error"
            );
            latch.publish("later");

            assert_eq!(
                latch.take(),
                None,
                "a reported latch must never return another error"
            );
            assert_eq!(
                latch.close(),
                None,
                "closing a reported latch must not return a later publication"
            );
        }
    }

    mod latch_close {
        use super::*;

        /// Invariant: closing a latch returns its pending error once and every
        /// subsequent observation proves that no error remains.
        /// Design Doc: ENV-LATCH
        #[test]
        fn close_returns_the_pending_error_exactly_once() {
            let mut latch = Latch::new();
            latch.publish("pending");

            assert_eq!(
                latch.close(),
                Some("pending"),
                "the first close must return the pending error"
            );
            assert_eq!(
                latch.close(),
                None,
                "a repeated close must not return the error again"
            );
            assert_eq!(
                latch.take(),
                None,
                "a closed latch must not expose an error through take"
            );
        }

        /// Invariant: an error published after the latch closes is discarded and
        /// cannot make the latch pending again.
        /// Design Doc: ENV-LATCH
        #[test]
        fn publication_after_close_is_discarded() {
            let mut latch = Latch::new();

            assert_eq!(
                latch.close(),
                None,
                "closing an empty latch must report no error"
            );
            latch.publish("too late");

            assert!(
                !latch.is_pending(),
                "a publication after close must not make the latch pending"
            );
            assert_eq!(
                latch.take(),
                None,
                "a publication after close must remain unobservable"
            );
        }

        /// Invariant: closing directly into a shutdown report preserves the chosen
        /// quiescence and emits a pending error only in the first report.
        #[test]
        fn close_into_report_preserves_quiescence_and_emits_once() {
            let mut latch = Latch::new();
            latch.publish("pending");

            let first = latch.close_into_report(Quiescence::Incomplete);
            let second = latch.close_into_report(Quiescence::Quiesced);

            assert_eq!(
                first.quiescence,
                Quiescence::Incomplete,
                "the first report must preserve incomplete quiescence"
            );
            assert_eq!(
                first.error,
                Some("pending"),
                "the first report must contain the pending error"
            );
            assert_eq!(
                second.quiescence,
                Quiescence::Quiesced,
                "a repeated report must preserve complete quiescence"
            );
            assert_eq!(
                second.error, None,
                "a repeated report must not contain the error again"
            );
        }
    }

    mod latch_precedence {
        use super::*;
        use std::cell::Cell;
        use std::rc::Rc;

        struct TrackedError {
            name: &'static str,
            drops: Rc<Cell<usize>>,
        }

        impl Drop for TrackedError {
            fn drop(&mut self) {
                self.drops.set(self.drops.get() + 1);
            }
        }

        /// Invariant: a pending published error is returned instead of a local
        /// failure, permanently reports the latch, and cannot be replaced by a
        /// later publication.
        /// Design Doc: ENV-LATCH, A4
        #[test]
        fn a_pending_error_wins_and_discards_the_local_error() {
            let pending_drops = Rc::new(Cell::new(0));
            let local_drops = Rc::new(Cell::new(0));
            let later_drops = Rc::new(Cell::new(0));
            let second_local_drops = Rc::new(Cell::new(0));
            let mut latch = Latch::new();
            latch.publish(TrackedError {
                name: "pending",
                drops: Rc::clone(&pending_drops),
            });

            let winner = latch.resolve_local_error(TrackedError {
                name: "local",
                drops: Rc::clone(&local_drops),
            });

            assert_eq!(
                winner.name, "pending",
                "a pending publication must win over the operation's local error"
            );
            assert_eq!(
                local_drops.get(),
                1,
                "the losing local error must be discarded immediately"
            );
            assert_eq!(
                pending_drops.get(),
                0,
                "the winning pending error must remain owned by the caller"
            );
            assert!(
                !latch.is_pending(),
                "returning the pending error must permanently report the latch"
            );

            latch.publish(TrackedError {
                name: "later",
                drops: Rc::clone(&later_drops),
            });

            assert_eq!(
                later_drops.get(),
                1,
                "a publication after reporting must be discarded immediately"
            );
            assert!(
                !latch.is_pending(),
                "a publication after reporting must not make the latch pending again"
            );

            let second_winner = latch.resolve_local_error(TrackedError {
                name: "second local",
                drops: Rc::clone(&second_local_drops),
            });

            assert_eq!(
                second_winner.name, "second local",
                "a reported latch must leave a later local error unchanged"
            );
            assert_eq!(
                second_local_drops.get(),
                0,
                "the later local error must remain owned by the caller"
            );
        }

        /// Invariant: when no published error is pending, an operation's local
        /// failure is returned unchanged.
        /// Design Doc: ENV-LATCH
        #[test]
        fn a_local_error_wins_when_the_latch_is_empty() {
            let mut latch = Latch::new();

            assert_eq!(
                latch.resolve_local_error("local"),
                "local",
                "an empty latch must leave the operation's local error unchanged"
            );
            assert!(
                !latch.is_pending(),
                "returning a local error must not make an empty latch pending"
            );
        }
    }

    mod latch_observation {
        use super::*;

        /// Invariant: observing an empty latch does not prevent its first later
        /// publication from becoming pending and observable.
        #[test]
        fn empty_take_keeps_the_latch_open() {
            let mut latch = Latch::new();

            assert_eq!(
                latch.take(),
                None,
                "taking an empty latch must report no error"
            );
            latch.publish("first later error");

            assert!(
                latch.is_pending(),
                "an empty observation must leave the latch open to publication"
            );
            assert_eq!(
                latch.take(),
                Some("first later error"),
                "the first publication after an empty observation must be returned"
            );
        }
    }

    mod latch_pending_state {
        use super::*;

        /// Invariant: a latch reports pending only while its first accepted error
        /// is waiting to be returned or closed into a report.
        #[test]
        fn pending_is_true_only_while_an_error_waits() {
            let empty = Latch::<&str>::new();
            let mut pending = Latch::new();
            pending.publish("pending");
            let mut reported = Latch::new();
            reported.publish("reported");
            assert_eq!(
                reported.take(),
                Some("reported"),
                "the reported-state fixture must observe its pending error"
            );
            let mut closed = Latch::<&str>::new();
            assert_eq!(
                closed.close(),
                None,
                "the closed-state fixture must close without an error"
            );

            assert!(
                !empty.is_pending(),
                "an empty latch must not report a pending error"
            );
            assert!(
                pending.is_pending(),
                "a published but unobserved error must report pending"
            );
            assert!(
                !reported.is_pending(),
                "a reported latch must not report a pending error"
            );
            assert!(
                !closed.is_pending(),
                "a closed latch must not report a pending error"
            );
        }
    }
}
