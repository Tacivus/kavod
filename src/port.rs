use serde::Serialize;

pub trait PortContract {
    type Event: Serialize;
    type Command: Serialize;
}

#[derive(Debug, Clone, Copy)]
pub enum Never {}

impl Serialize for Never {
    fn serialize<S>(&self, _serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match *self {}
    }
}

/// Declares a Slot-qualified Event sum and Command sum for one Application.
///
/// The identifier after `enum` labels the declaration; no type is generated
/// with that name. The generated Event and Command enums derive only
/// [`serde::Serialize`] and use serde's default externally tagged
/// representation. Because the derive uses `::serde`, callers must have a
/// direct dependency named `serde` with its `derive` feature enabled.
/// Slot Contract types must be visible anywhere the generated variant
/// constructors are used.
///
/// ```
/// mod protocol {
///     pub struct MarketData;
///     impl kavod::PortContract for MarketData {
///         type Event = u32;
///         type Command = kavod::Never;
///     }
///
///     pub struct Execution;
///     impl kavod::PortContract for Execution {
///         type Event = u8;
///         type Command = u8;
///     }
///
///     kavod::ports!(
///         pub enum Trading<Event = TradingEvent, Command = TradingCommand> {
///             Primary(MarketData),
///             Secondary(MarketData),
///             Execution(Execution),
///         }
///     );
/// }
///
/// let event = protocol::TradingEvent::Primary(1);
/// assert_eq!(serde_json::to_string(&event).unwrap(), r#"{"Primary":1}"#);
/// ```
#[macro_export]
macro_rules! ports {
    (
        $vis:vis enum $_name:ident<Event = $event_name:ident, Command = $command_name:ident> {
            $($variant:ident($contract:ty)),* $(,)?
        }
    ) => {
        #[derive(::serde::Serialize)]
        $vis enum $event_name {
            $($variant(<$contract as $crate::PortContract>::Event)),*
        }

        #[derive(::serde::Serialize)]
        $vis enum $command_name {
            $($variant(<$contract as $crate::PortContract>::Command)),*
        }
    };
}

#[cfg(test)]
mod tests {
    // A Never-carrying Command variant can never be constructed by design;
    // allow dead_code module-wide rather than fight the lint.
    #![allow(dead_code)]

    use super::*;

    // Shared fixtures used by multiple test groups below.

    struct MarketData;
    impl PortContract for MarketData {
        type Event = MarketEvent;
        type Command = Never;
    }

    #[derive(Serialize)]
    struct MarketEvent(u32);

    struct Execution;
    impl PortContract for Execution {
        type Event = ExecutionEvent;
        type Command = ExecutionCommand;
    }

    #[derive(Serialize)]
    struct ExecutionEvent(u8);

    #[derive(Serialize)]
    struct ExecutionCommand(u16);

    ports!(
        enum Trading<Event = TradingEvent, Command = TradingCommand> {
            Primary(MarketData),
            Secondary(MarketData),
            Execution(Execution),
        }
    );

    mod ports_event_serialization {
        use super::*;

        /// Invariant: generated Event variants serialize with serde's default
        /// externally tagged representation, keyed by the Slot's name (§3.4).
        #[test]
        fn event_variant_serializes_tagged_by_slot_name() {
            assert_eq!(
                serde_json::to_string(&TradingEvent::Primary(MarketEvent(1))).unwrap(),
                r#"{"Primary":1}"#
            );
            assert_eq!(
                serde_json::to_string(&TradingEvent::Secondary(MarketEvent(2))).unwrap(),
                r#"{"Secondary":2}"#
            );
            assert_eq!(
                serde_json::to_string(&TradingEvent::Execution(ExecutionEvent(3))).unwrap(),
                r#"{"Execution":3}"#
            );
        }
    }

    mod ports_command_serialization {
        use super::*;

        /// Invariant: generated Command variants serialize with serde's default
        /// externally tagged representation, keyed by the Slot's name (§3.4).
        #[test]
        fn command_variant_serializes_tagged_by_slot_name() {
            assert_eq!(
                serde_json::to_string(&TradingCommand::Execution(ExecutionCommand(9))).unwrap(),
                r#"{"Execution":9}"#
            );
        }
    }

    mod ports_hand_written_equivalence {
        use super::*;

        #[derive(Serialize)]
        enum HandWrittenEvent {
            Execution(ExecutionEvent),
        }

        #[derive(Serialize)]
        enum HandWrittenCommand {
            Execution(ExecutionCommand),
        }

        /// Invariant: a hand-written enum pair with the same shape as the
        /// macro expansion is observationally identical to the
        /// macro-generated sums (§3.2).
        #[test]
        fn hand_written_enums_match_macro_generated_serialization() {
            let generated_event = TradingEvent::Execution(ExecutionEvent(5));
            let hand_written_event = HandWrittenEvent::Execution(ExecutionEvent(5));
            assert_eq!(
                serde_json::to_string(&generated_event).unwrap(),
                serde_json::to_string(&hand_written_event).unwrap()
            );

            let generated_command = TradingCommand::Execution(ExecutionCommand(9));
            let hand_written_command = HandWrittenCommand::Execution(ExecutionCommand(9));
            assert_eq!(
                serde_json::to_string(&generated_command).unwrap(),
                serde_json::to_string(&hand_written_command).unwrap()
            );
        }
    }

    mod ports_payload_projection {
        use super::*;

        /// Invariant: every generated Event variant carries exactly its
        /// Contract's Event type (`PORT-SUMS`).
        #[test]
        fn event_variant_carries_contract_event_type() {
            let event = TradingEvent::Execution(ExecutionEvent(7));

            let TradingEvent::Execution(ExecutionEvent(value)) = event else {
                panic!("constructed Event changed Slot")
            };
            assert_eq!(value, 7);
        }

        /// Invariant: every generated Command variant carries exactly its
        /// Contract's Command type (`PORT-SUMS`).
        #[test]
        fn command_variant_carries_contract_command_type() {
            let command = TradingCommand::Execution(ExecutionCommand(700));

            let TradingCommand::Execution(ExecutionCommand(value)) = command;
            assert_eq!(value, 700);
        }
    }

    mod ports_slot_distinctness {
        use super::*;

        /// Invariant: distinct Slots of one Contract are distinct variants (§3.2, `PORT-SUMS`).
        #[test]
        fn distinct_slots_of_one_contract_produce_distinct_variants() {
            let primary = TradingEvent::Primary(MarketEvent(7));
            let secondary = TradingEvent::Secondary(MarketEvent(7));
            assert_ne!(
                serde_json::to_string(&primary).unwrap(),
                serde_json::to_string(&secondary).unwrap()
            );
        }
    }

    mod ports_never_command_handling {
        use super::*;

        /// Invariant: a `Never` Command arm is discharged by matching the
        /// uninhabited value, so the generated Command sum stays exhaustively
        /// matchable even for an absent direction (§3.2).
        #[test]
        fn never_command_arm_is_exhaustively_matchable() {
            fn discharge(command: TradingCommand) {
                match command {
                    TradingCommand::Primary(never) => match never {},
                    TradingCommand::Secondary(never) => match never {},
                    TradingCommand::Execution(_) => {}
                }
            }

            discharge(TradingCommand::Execution(ExecutionCommand(1)));
        }
    }

    mod ports_never_event_handling {
        use super::*;

        struct CommandOnly;
        impl PortContract for CommandOnly {
            type Event = Never;
            type Command = ExecutionCommand;
        }

        ports!(
            enum CommandOnlyPorts<Event = CommandOnlyEvent, Command = CommandOnlyCommand> {
                Sink(CommandOnly),
            }
        );

        /// Invariant: a `Never` Event arm is discharged by matching the
        /// uninhabited value, so absent Event directions remain exhaustive
        /// (§3.2).
        #[test]
        fn never_event_arm_is_exhaustively_matchable() {
            fn discharge(event: CommandOnlyEvent) {
                match event {
                    CommandOnlyEvent::Sink(never) => match never {},
                }
            }

            let _discharge: fn(CommandOnlyEvent) = discharge;
            let _command = CommandOnlyCommand::Sink(ExecutionCommand(1));
        }
    }

    mod ports_visibility_propagation {
        use super::*;

        mod generated {
            use super::*;

            ports!(
                pub(super) enum Visible<Event = VisibleEvent, Command = VisibleCommand> {
                    Execution(Execution),
                }
            );

            ports!(
                enum Private<Event = PrivateEvent, Command = PrivateCommand> {
                    Execution(Execution),
                }
            );

            mod descendant {
                use super::*;

                /// Invariant: default (private) visibility still reaches the
                /// defining module's descendants, not just the defining
                /// module itself (§3.4).
                #[test]
                fn private_enums_are_usable_from_descendant_module() {
                    let event = PrivateEvent::Execution(ExecutionEvent(1));
                    let command = PrivateCommand::Execution(ExecutionCommand(2));

                    assert_eq!(serde_json::to_string(&event).unwrap(), r#"{"Execution":1}"#);
                    assert_eq!(
                        serde_json::to_string(&command).unwrap(),
                        r#"{"Execution":2}"#
                    );
                }
            }
        }

        /// Invariant: both generated enums carry the invocation's visibility
        /// and are accessible from every scope that visibility permits (§3.4).
        #[test]
        fn pub_super_enums_are_usable_from_parent_module() {
            let event = generated::VisibleEvent::Execution(ExecutionEvent(1));
            let command = generated::VisibleCommand::Execution(ExecutionCommand(2));

            assert_eq!(serde_json::to_string(&event).unwrap(), r#"{"Execution":1}"#);
            assert_eq!(
                serde_json::to_string(&command).unwrap(),
                r#"{"Execution":2}"#
            );
        }
    }

    mod ports_path_hygiene {
        use super::*;

        trait PortContract {}

        struct HygienicContract;
        impl crate::PortContract for HygienicContract {
            type Event = ExecutionEvent;
            type Command = ExecutionCommand;
        }

        ports!(
            enum Hygienic<Event = HygienicEvent, Command = HygienicCommand> {
                Slot(HygienicContract),
            }
        );

        /// Invariant: generated payload projections resolve Kavod's
        /// `PortContract`, regardless of names in the invocation scope (§3.4).
        #[test]
        fn local_port_contract_name_does_not_shadow_kavod_trait() {
            let event = HygienicEvent::Slot(ExecutionEvent(1));
            let command = HygienicCommand::Slot(ExecutionCommand(2));

            assert_eq!(serde_json::to_string(&event).unwrap(), r#"{"Slot":1}"#);
            assert_eq!(serde_json::to_string(&command).unwrap(), r#"{"Slot":2}"#);
        }
    }

    mod ports_trailing_comma {
        use super::*;

        /// Invariant: the macro accepts an optional trailing comma after the
        /// last Slot.
        #[test]
        fn trailing_comma_after_last_slot_is_accepted() {
            ports!(
                enum WithTrailingComma<Event = TrailEvent, Command = TrailCommand> {
                    Primary(MarketData),
                }
            );

            let event = TrailEvent::Primary(MarketEvent(1));
            assert_eq!(serde_json::to_string(&event).unwrap(), r#"{"Primary":1}"#);
        }

        /// Invariant: the trailing comma after the final Slot is optional.
        #[test]
        fn trailing_comma_after_last_slot_may_be_omitted() {
            ports!(
                enum WithoutTrailingComma<Event = NoTrailEvent, Command = NoTrailCommand> {
                    Execution(Execution),
                }
            );

            let event = NoTrailEvent::Execution(ExecutionEvent(1));
            let command = NoTrailCommand::Execution(ExecutionCommand(2));

            assert_eq!(serde_json::to_string(&event).unwrap(), r#"{"Execution":1}"#);
            assert_eq!(
                serde_json::to_string(&command).unwrap(),
                r#"{"Execution":2}"#
            );
        }
    }

    mod ports_empty_slot_set {
        ports!(
            enum Empty<Event = EmptyEvent, Command = EmptyCommand> {}
        );

        /// Invariant: an Application with no Slots has closed, uninhabited
        /// Event and Command sums (`PORT-SUMS`).
        #[test]
        fn empty_slot_set_produces_exhaustively_empty_sums() {
            fn discharge_event(event: EmptyEvent) {
                match event {}
            }

            fn discharge_command(command: EmptyCommand) {
                match command {}
            }

            let _event_discharge: fn(EmptyEvent) = discharge_event;
            let _command_discharge: fn(EmptyCommand) = discharge_command;
        }
    }

    mod ports_payload_bounds {
        use super::*;

        #[derive(Serialize)]
        struct SerializeOnlyEvent;

        #[derive(Serialize)]
        struct SerializeOnlyCommand;

        struct SerializeOnlyContract;
        impl PortContract for SerializeOnlyContract {
            type Event = SerializeOnlyEvent;
            type Command = SerializeOnlyCommand;
        }

        ports!(
            enum MinimalBounds<Event = MinimalEvent, Command = MinimalCommand> {
                Slot(SerializeOnlyContract),
            }
        );

        /// Invariant: generated sums require only the `Serialize` bounds
        /// declared by `PortContract`; payloads need not implement other traits
        /// (§3.1, `PORT-SUMS`).
        #[test]
        fn serialize_only_payloads_are_accepted() {
            let event = MinimalEvent::Slot(SerializeOnlyEvent);
            let command = MinimalCommand::Slot(SerializeOnlyCommand);

            assert_eq!(serde_json::to_string(&event).unwrap(), r#"{"Slot":null}"#);
            assert_eq!(serde_json::to_string(&command).unwrap(), r#"{"Slot":null}"#);
        }
    }

    mod ports_contract_type_syntax {
        use std::marker::PhantomData;

        use super::*;

        mod fixture {
            use super::*;

            pub(super) struct GenericContract<T>(PhantomData<T>);

            impl<T: Serialize> crate::PortContract for GenericContract<T> {
                type Event = T;
                type Command = T;
            }
        }

        ports!(
            enum Generic<Event = GenericEvent, Command = GenericCommand> {
                Slot(fixture::GenericContract<u32>),
            }
        );

        /// Invariant: each Slot accepts a concrete Contract type, including a
        /// qualified generic type, and projects its associated payloads
        /// (`PORT-SUMS`).
        #[test]
        fn qualified_generic_contract_type_is_accepted() {
            let event = GenericEvent::Slot(3);
            let command = GenericCommand::Slot(4);

            assert_eq!(serde_json::to_string(&event).unwrap(), r#"{"Slot":3}"#);
            assert_eq!(serde_json::to_string(&command).unwrap(), r#"{"Slot":4}"#);
        }
    }
}
