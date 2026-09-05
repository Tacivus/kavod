use serde::Serialize;

pub trait PortContract {
    type Event: Serialize;
    type Command: Serialize;
}

pub enum Never {}

impl Serialize for Never {
    fn serialize<S: serde::Serializer>(&self, _serializer: S) -> Result<S::Ok, S::Error> {
        match *self {}
    }
}

#[macro_export]
macro_rules! ports {
    (
        $vis:vis enum $name:ident<Event = $event:ident, Command = $command:ident> {
            $( $variant:ident($contract:ty) ),+ $(,)?
        }
    ) => {
        #[derive(::serde::Serialize)]
        $vis enum $event {
            $( $variant(<$contract as $crate::PortContract>::Event) ),+
        }
        #[derive(::serde::Serialize)]
        $vis enum $command {
            $( $variant(<$contract as $crate::PortContract>::Command) ),+
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(serde::Serialize)]
    struct EventPayload {
        event_value: u64,
    }

    #[derive(serde::Serialize)]
    struct CommandPayload {
        command_code: u64,
    }

    struct Duplex;

    impl PortContract for Duplex {
        type Event = EventPayload;
        type Command = CommandPayload;
    }

    crate::ports!(
        enum Reused<Event = ReusedEvent, Command = ReusedCommand> {
            Primary(Duplex),
            Secondary(Duplex),
        }
    );

    struct Reused(&'static str);

    #[derive(serde::Serialize)]
    enum HandWrittenEvent {
        Primary(<Duplex as PortContract>::Event),
        Secondary(<Duplex as PortContract>::Event),
    }

    #[derive(serde::Serialize)]
    enum HandWrittenCommand {
        Primary(<Duplex as PortContract>::Command),
        Secondary(<Duplex as PortContract>::Command),
    }

    #[allow(
        dead_code,
        reason = "the generated command variant is uninhabited by design"
    )]
    mod receive_only_fixture {
        use super::*;

        pub(super) struct EventOnly;

        impl PortContract for EventOnly {
            type Event = EventPayload;
            type Command = Never;
        }

        crate::ports!(
            pub(super) enum ReceiveOnly<Event = ReceiveOnlyEvent, Command = ReceiveOnlyCommand> {
                Feed(EventOnly)
            }
        );
    }

    use receive_only_fixture::{ReceiveOnlyCommand, ReceiveOnlyEvent};

    mod ports_macro_expansion {
        use super::*;

        fn serialized<T: Serialize>(value: &T) -> String {
            serde_json::to_string(value).expect("a generated port sum must serialize")
        }

        /// Invariant: generated event and command sums serialize the selected Slot
        /// as the sole outer JSON object key.
        /// Design Doc: the Port Mechanism's wire form, by name
        #[test]
        fn generated_sums_are_externally_tagged() {
            assert_eq!(
                serialized(&ReusedEvent::Secondary(EventPayload { event_value: 42 })),
                r#"{"Secondary":{"event_value":42}}"#,
                "a generated Event sum must use its Slot name as the external tag"
            );
            assert_eq!(
                serialized(&ReusedCommand::Primary(CommandPayload { command_code: 7 })),
                r#"{"Primary":{"command_code":7}}"#,
                "a generated Command sum must use its Slot name as the external tag"
            );
        }

        /// Invariant: generated and hand-written sums with the same variants and
        /// payloads produce exactly the same JSON bytes.
        /// Design Doc: PORT-SUMS
        #[test]
        fn hand_written_equivalent_is_byte_identical() {
            assert_eq!(
                serialized(&ReusedEvent::Primary(EventPayload { event_value: 1 })),
                serialized(&HandWrittenEvent::Primary(EventPayload { event_value: 1 })),
                "a generated primary Event variant must match its hand-written equivalent"
            );
            assert_eq!(
                serialized(&ReusedEvent::Secondary(EventPayload { event_value: 2 })),
                serialized(&HandWrittenEvent::Secondary(EventPayload { event_value: 2 })),
                "a generated secondary Event variant must match its hand-written equivalent"
            );
            assert_eq!(
                serialized(&ReusedCommand::Primary(CommandPayload { command_code: 3 })),
                serialized(&HandWrittenCommand::Primary(CommandPayload { command_code: 3 })),
                "a generated primary Command variant must match its hand-written equivalent"
            );
            assert_eq!(
                serialized(&ReusedCommand::Secondary(CommandPayload { command_code: 4 })),
                serialized(&HandWrittenCommand::Secondary(CommandPayload { command_code: 4 })),
                "a generated secondary Command variant must match its hand-written equivalent"
            );
        }

        /// Invariant: binding one contract at two Slots creates two distinct
        /// variants that remain separately selectable.
        /// Design Doc: PORT-SUMS
        #[test]
        fn contract_bound_at_two_slots_yields_two_variants() {
            fn slot(event: ReusedEvent) -> &'static str {
                match event {
                    ReusedEvent::Primary(_) => "primary",
                    ReusedEvent::Secondary(_) => "secondary",
                }
            }

            assert_eq!(
                slot(ReusedEvent::Primary(EventPayload { event_value: 0 })),
                "primary",
                "the first binding of a reused Contract must retain its own variant"
            );
            assert_eq!(
                slot(ReusedEvent::Secondary(EventPayload { event_value: 0 })),
                "secondary",
                "the second binding of a reused Contract must retain its own variant"
            );
        }

        /// Invariant: the generated Event and Command variants use their Contract's
        /// distinct associated payload types without crossing them.
        #[test]
        fn event_and_command_associated_payloads_remain_distinct() {
            fn event_value(event: ReusedEvent) -> u64 {
                match event {
                    ReusedEvent::Primary(EventPayload { event_value })
                    | ReusedEvent::Secondary(EventPayload { event_value }) => event_value,
                }
            }

            fn command_code(command: ReusedCommand) -> u64 {
                match command {
                    ReusedCommand::Primary(CommandPayload { command_code })
                    | ReusedCommand::Secondary(CommandPayload { command_code }) => command_code,
                }
            }

            assert_eq!(
                event_value(ReusedEvent::Primary(EventPayload { event_value: 11 })),
                11,
                "an Event variant must contain the Contract's Event payload"
            );
            assert_eq!(
                command_code(ReusedCommand::Secondary(CommandPayload { command_code: 13 })),
                13,
                "a Command variant must contain the Contract's Command payload"
            );
        }

        /// Invariant: the macro accepts its minimum one-Slot declaration without a
        /// trailing comma and produces a usable sum.
        #[test]
        fn single_slot_without_trailing_comma_expands() {
            assert_eq!(
                serialized(&ReceiveOnlyEvent::Feed(EventPayload { event_value: 1 })),
                r#"{"Feed":{"event_value":1}}"#,
                "a one-Slot declaration without a trailing comma must generate a usable Event sum"
            );
        }

        /// Invariant: the declaration name consumed by the macro remains available
        /// for an independent item because only the Event and Command sums are generated.
        #[test]
        fn declaration_name_is_available_for_an_independent_item() {
            let Reused(value) = Reused("independent item");

            assert_eq!(
                value, "independent item",
                "the macro declaration name must remain bound to the independent item"
            );
        }
    }

    mod never_direction {
        use super::*;

        /// Invariant: a command variant whose direction is absent can be handled
        /// exhaustively without constructing a fallback value.
        /// Design Doc: Never, by name
        #[test]
        fn never_command_arm_is_discharged_by_match() {
            fn fan_out(command: ReceiveOnlyCommand) {
                match command {
                    ReceiveOnlyCommand::Feed(never) => match never {},
                }
            }

            let _exhaustive_fan_out: fn(ReceiveOnlyCommand) = fan_out;
        }
    }
}
