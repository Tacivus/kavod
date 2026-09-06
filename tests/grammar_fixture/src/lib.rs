macro_rules! reconstruct_engine {
    ($($attack:item)*) => {
        #[allow(dead_code)]
        mod bounded_buffer {
            use std::vec::Drain;

            pub(crate) struct BoundedBuffer<T> {
                items: Vec<T>,
                capacity: usize,
            }

            impl<T> BoundedBuffer<T> {
                pub(crate) fn new(capacity: usize) -> Self {
                    Self {
                        items: Vec::with_capacity(capacity),
                        capacity,
                    }
                }

                pub(crate) fn try_push(&mut self, value: T) -> Result<(), T> {
                    if self.items.len() == self.capacity {
                        return Err(value);
                    }
                    self.items.push(value);
                    Ok(())
                }

                pub(crate) fn is_empty(&self) -> bool {
                    self.items.is_empty()
                }

                pub(crate) fn as_slice(&self) -> &[T] {
                    self.items.as_slice()
                }

                pub(crate) fn drain(&mut self) -> Drain<'_, T> {
                    self.items.drain(..)
                }
            }
        }

        #[allow(unused_imports)]
        mod application {
            pub(crate) use kavod::{Application, Context, Outcome};
        }

        mod environment {
            pub(crate) use kavod::{Environment, Quiescence, ShutdownReport};
        }

        mod journal {
            pub(crate) use kavod::{Journal, JournalError};
        }

        mod time {
            pub(crate) use kavod::Timestamp;

            #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
            pub(crate) struct EventIndex(u64);

            impl EventIndex {
                pub(crate) fn new(index: u64) -> Self {
                    Self(index)
                }

                pub(crate) fn as_u64(self) -> u64 {
                    self.0
                }
            }
        }

        #[allow(dead_code)]
        mod engine {
            use crate::bounded_buffer::BoundedBuffer;
            use crate::environment::{Environment, ShutdownReport};
            use crate::journal::Journal;
            use crate::time::Timestamp;
            use std::num::NonZeroUsize;

            pub(super) struct EnvironmentFatal<E> {
                pub error: E,
                pub operation: EnvironmentOperation,
            }

            pub(super) enum EnvironmentOperation {
                NextEvent,
                Dispatch { position: usize },
                Checkpoint,
                Shutdown,
            }

            pub(super) enum CoreError {
                CommandBoundExceeded,
                IndexExhausted,
                TimeRegression {
                    previous: Timestamp,
                    offered: Timestamp,
                },
                ShutdownIncomplete,
            }

            pub(super) enum FatalCause<AE, EE> {
                Application(AE),
                Environment(EnvironmentFatal<EE>),
                Journal(record::JournalFatal),
                Core(CoreError),
            }

            mod record {
                include!("../../../src/engine/record.rs");
            }

            use record::{
                Certificate, Checkpointed, ClassifiedTurn, EffectsComplete, Initial, TurnOpen,
                TurnOutcome, answer,
            };

            struct CleanEnvironment;

            impl Environment for CleanEnvironment {
                type Event = u8;
                type Command = u8;
                type Error = ();

                fn start(&mut self) -> Result<Timestamp, Self::Error> {
                    Ok(Timestamp::from_nanos(0))
                }

                fn next_event(&mut self) -> Result<(Self::Event, Timestamp), Self::Error> {
                    Ok((1, Timestamp::from_nanos(1)))
                }

                fn dispatch(&mut self, _command: Self::Command) -> Result<(), Self::Error> {
                    Ok(())
                }

                fn take_error(&mut self) -> Option<Self::Error> {
                    None
                }

                fn shutdown(self) -> ShutdownReport<Self::Error> {
                    ShutdownReport {
                        quiescence: crate::environment::Quiescence::Quiesced,
                        error: None,
                    }
                }
            }

            fn initial() -> Certificate<Vec<u8>, Initial> {
                let max_record_bytes = NonZeroUsize::new(512)
                    .expect("the grammar fixture record bound must be nonzero");
                let journal = Journal::new(Vec::new(), max_record_bytes)
                    .expect("the grammar fixture Journal must reserve its buffer");
                Certificate::mint(journal, Timestamp::from_nanos(0))
            }

            fn turn_open() -> Certificate<Vec<u8>, TurnOpen> {
                match initial().run_started() {
                    Ok(certificate) => certificate,
                    Err(_) => panic!("the grammar fixture must commit RunStarted"),
                }
            }

            fn continue_turn() -> Certificate<Vec<u8>, TurnOpen<answer::Continue>> {
                match turn_open().classify(TurnOutcome::Continue) {
                    ClassifiedTurn::Continue(certificate) => certificate,
                    ClassifiedTurn::Stop(_) => {
                        panic!("the grammar fixture must retain its Continue answer")
                    }
                }
            }

            fn stop_turn() -> Certificate<Vec<u8>, TurnOpen<answer::Stop>> {
                match turn_open().classify(TurnOutcome::Stop) {
                    ClassifiedTurn::Stop(certificate) => certificate,
                    ClassifiedTurn::Continue(_) => {
                        panic!("the grammar fixture must retain its Stop answer")
                    }
                }
            }

            fn continue_effects() -> Certificate<Vec<u8>, EffectsComplete<answer::Continue>> {
                let commands = BoundedBuffer::<u8>::new(0);
                continue_turn().no_commands(&commands)
            }

            fn stop_effects() -> Certificate<Vec<u8>, EffectsComplete<answer::Stop>> {
                let commands = BoundedBuffer::<u8>::new(0);
                stop_turn().no_commands(&commands)
            }

            fn continue_checkpointed() -> Certificate<Vec<u8>, Checkpointed<answer::Continue>> {
                match continue_effects().checkpoint::<_, ()>(&mut CleanEnvironment) {
                    Ok(certificate) => certificate,
                    Err(_) => panic!("the grammar fixture Continue checkpoint must succeed"),
                }
            }

            fn stop_checkpointed() -> Certificate<Vec<u8>, Checkpointed<answer::Stop>> {
                match stop_effects().checkpoint::<_, ()>(&mut CleanEnvironment) {
                    Ok(certificate) => certificate,
                    Err(_) => panic!("the grammar fixture Stop checkpoint must succeed"),
                }
            }

            $($attack)*
        }
    };
}
