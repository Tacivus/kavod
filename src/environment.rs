use crate::Timestamp;

/// The Core's boundary for serial interaction with the outside world.
///
/// One caller invokes these operations one at a time: `start` first, if at all,
/// followed by event, command, and error operations, and finally at most one
/// consuming `shutdown`.
pub trait Environment {
    /// The external event type delivered to the Application.
    type Event;
    /// The command type handed off to the outside world.
    type Command;
    /// The failure type reported by Environment operations.
    type Error;

    /// Activates run-scoped activity and returns its frozen start time.
    ///
    /// An error leaves the Environment quiesced and safe to drop without a later
    /// shutdown call.
    fn start(&mut self) -> Result<Timestamp, Self::Error>;

    /// Waits for and consumes exactly one candidate Event on success.
    ///
    /// An error means no candidate was consumed by this call.
    fn next_event(&mut self) -> Result<(Self::Event, Timestamp), Self::Error>;

    /// Attempts a non-waiting handoff of one Command.
    ///
    /// Success transfers ownership of the Command; an error means it was not
    /// handed off.
    fn dispatch(&mut self, command: Self::Command) -> Result<(), Self::Error>;

    /// Takes the first currently latched Error without waiting for one.
    fn take_error(&mut self) -> Option<Self::Error>;

    /// Raises the shutdown signal and closes Event admission as its one
    /// initiating step. The latch remains open through the Environment's bounded
    /// graceful-shutdown window; the final observation fixes the report and
    /// closes the latch.
    fn shutdown(self) -> ShutdownReport<Self::Error>;
}

/// The Environment's final account of run-scoped activity and latched failure.
pub struct ShutdownReport<E> {
    /// Whether the Environment accounted every unit of run-scoped activity
    /// complete before its bounded shutdown wait ended.
    pub quiescence: Quiescence,
    /// The pending Error the latch held when it closed; `None` proves the latch
    /// was empty or already reported at the close.
    pub error: Option<E>,
}

/// Whether the Environment accounted all run-scoped activity complete.
#[derive(Debug, PartialEq, Eq)]
pub enum Quiescence {
    /// Every unit of run-scoped activity was accounted complete.
    Quiesced,
    /// At least one unit remained unaccounted when the bounded wait ended.
    Incomplete,
}

#[cfg(test)]
mod tests {
    use super::*;

    mod environment_contract_shape {
        use super::*;

        struct ScriptedEnvironment {
            next_call: usize,
        }

        impl Environment for ScriptedEnvironment {
            type Event = &'static str;
            type Command = u8;
            type Error = &'static str;

            fn start(&mut self) -> Result<Timestamp, Self::Error> {
                assert_eq!(
                    self.next_call, 0,
                    "start must be the first Environment operation"
                );
                self.next_call += 1;
                Ok(Timestamp::from_nanos(10))
            }

            fn next_event(&mut self) -> Result<(Self::Event, Timestamp), Self::Error> {
                assert_eq!(
                    self.next_call, 1,
                    "next_event must follow successful Environment startup"
                );
                self.next_call += 1;
                Ok(("event", Timestamp::from_nanos(11)))
            }

            fn dispatch(&mut self, command: Self::Command) -> Result<(), Self::Error> {
                assert_eq!(
                    self.next_call, 2,
                    "dispatch must follow the scripted Event delivery"
                );
                assert_eq!(command, 7, "dispatch must receive the scripted Command");
                self.next_call += 1;
                Ok(())
            }

            fn take_error(&mut self) -> Option<Self::Error> {
                assert_eq!(
                    self.next_call, 3,
                    "take_error must follow the scripted Command handoff"
                );
                self.next_call += 1;
                None
            }

            fn shutdown(self) -> ShutdownReport<Self::Error> {
                assert_eq!(
                    self.next_call, 4,
                    "shutdown must consume the Environment after its preceding operations"
                );
                ShutdownReport {
                    quiescence: Quiescence::Quiesced,
                    error: None,
                }
            }
        }

        /// Invariant: an Environment implementation can perform startup, receive
        /// an event, hand off a command, observe its error latch, and then be
        /// consumed by shutdown in serial order.
        /// Design Doc: the Environment API block, by name
        #[test]
        fn a_scripted_implementation_drives_all_five_operations() {
            let mut environment = ScriptedEnvironment { next_call: 0 };

            let start = environment.start().expect("scripted startup must succeed");
            let (event, time) = environment
                .next_event()
                .expect("scripted Event delivery must succeed");
            environment
                .dispatch(7)
                .expect("scripted Command handoff must succeed");
            let error = environment.take_error();
            let report = environment.shutdown();

            assert_eq!(
                start,
                Timestamp::from_nanos(10),
                "start must return the scripted frozen timestamp"
            );
            assert_eq!(event, "event", "next_event must return the scripted Event");
            assert_eq!(
                time,
                Timestamp::from_nanos(11),
                "next_event must return the scripted Event timestamp"
            );
            assert_eq!(error, None, "take_error must return the scripted empty latch");
            assert_eq!(
                report.quiescence,
                Quiescence::Quiesced,
                "shutdown must return the scripted quiescence"
            );
            assert_eq!(report.error, None, "shutdown must return the scripted empty latch");
        }
    }

    mod quiescence_variants {
        use super::*;

        /// Invariant: complete and incomplete shutdown accounts are distinct
        /// comparable states, and each state compares equal to itself.
        #[test]
        fn both_states_are_distinct_and_comparable() {
            assert_eq!(
                Quiescence::Quiesced,
                Quiescence::Quiesced,
                "the complete quiescence state must equal itself"
            );
            assert_eq!(
                Quiescence::Incomplete,
                Quiescence::Incomplete,
                "the incomplete quiescence state must equal itself"
            );
            assert_ne!(
                Quiescence::Quiesced,
                Quiescence::Incomplete,
                "complete and incomplete quiescence must remain distinct"
            );
        }
    }
}
