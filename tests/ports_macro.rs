#[cfg(test)]
mod tests {
    #[derive(serde::Serialize)]
    struct MarketEvent(u64);

    #[derive(serde::Serialize)]
    struct MarketCommand(u64);

    struct Market;

    impl kavod::PortContract for Market {
        type Event = MarketEvent;
        type Command = MarketCommand;
    }

    #[derive(serde::Serialize)]
    struct ExecutionEvent(u64);

    #[derive(serde::Serialize)]
    struct ExecutionCommand(u64);

    struct Execution;

    impl kavod::PortContract for Execution {
        type Event = ExecutionEvent;
        type Command = ExecutionCommand;
    }

    kavod::ports!(
        enum ConsumerPorts<Event = ConsumerEvent, Command = ConsumerCommand> {
            Market(Market),
            Execution(Execution),
        }
    );

    #[allow(
        dead_code,
        reason = "the generated receive-only command variant is uninhabited by design"
    )]
    mod receive_only_fixture {
        use super::MarketEvent;

        pub(super) struct ReceiveOnly;

        impl kavod::PortContract for ReceiveOnly {
            type Event = MarketEvent;
            type Command = kavod::Never;
        }

        kavod::ports!(
            pub(super) enum ReceiveOnlyPorts<Event = ReceiveOnlyEvent, Command = ReceiveOnlyCommand> {
                Feed(ReceiveOnly),
            }
        );
    }

    mod ports_macro_downstream {
        use super::*;
        use super::receive_only_fixture::{ReceiveOnlyCommand, ReceiveOnlyEvent};

        fn serialized<T: serde::Serialize>(value: &T) -> String {
            serde_json::to_string(value).expect("a downstream generated sum must serialize")
        }

        fn event_destination(event: ConsumerEvent) -> (&'static str, u64) {
            match event {
                ConsumerEvent::Market(MarketEvent(value)) => ("market", value),
                ConsumerEvent::Execution(ExecutionEvent(value)) => ("execution", value),
            }
        }

        fn command_destination(command: ConsumerCommand) -> (&'static str, u64) {
            match command {
                ConsumerCommand::Market(MarketCommand(value)) => ("market", value),
                ConsumerCommand::Execution(ExecutionCommand(value)) => ("execution", value),
            }
        }

        /// Invariant: a downstream crate can invoke the exported macro and serialize
        /// generated sums containing each Contract's associated payload type.
        /// Design Doc: PORT-SUMS
        #[test]
        fn consumer_invocation_compiles_and_serializes() {
            assert_eq!(
                serialized(&ConsumerEvent::Market(MarketEvent(7))),
                r#"{"Market":7}"#,
                "a downstream Event sum must serialize with its Slot tag"
            );
            assert_eq!(
                serialized(&ConsumerCommand::Execution(ExecutionCommand(11))),
                r#"{"Execution":11}"#,
                "a downstream Command sum must serialize with its Slot tag"
            );
        }

        /// Invariant: a downstream consumer can route every generated Event and
        /// Command variant exhaustively while preserving its typed payload.
        /// Design Doc: PORT-ROUTING
        #[test]
        fn the_fanout_match_is_exhaustive() {
            let market_fan_in: fn(MarketEvent) -> ConsumerEvent = ConsumerEvent::Market;
            let execution_fan_in: fn(ExecutionEvent) -> ConsumerEvent = ConsumerEvent::Execution;

            assert_eq!(
                event_destination(market_fan_in(MarketEvent(13))),
                ("market", 13),
                "the Market fan-in constructor must route its typed Event payload"
            );
            assert_eq!(
                event_destination(execution_fan_in(ExecutionEvent(17))),
                ("execution", 17),
                "the Execution fan-in constructor must route its typed Event payload"
            );
            assert_eq!(
                command_destination(ConsumerCommand::Market(MarketCommand(19))),
                ("market", 19),
                "the exhaustive fan-out must route the Market Command payload"
            );
            assert_eq!(
                command_destination(ConsumerCommand::Execution(ExecutionCommand(23))),
                ("execution", 23),
                "the exhaustive fan-out must route the Execution Command payload"
            );
        }

        /// Invariant: every generated variant keeps its own Slot name when serialized
        /// from a downstream crate.
        #[test]
        fn every_generated_variant_serializes_with_its_own_slot_tag() {
            assert_eq!(
                serialized(&ConsumerEvent::Market(MarketEvent(1))),
                r#"{"Market":1}"#,
                "the Market Event variant must retain its own Slot tag"
            );
            assert_eq!(
                serialized(&ConsumerEvent::Execution(ExecutionEvent(2))),
                r#"{"Execution":2}"#,
                "the Execution Event variant must retain its own Slot tag"
            );
            assert_eq!(
                serialized(&ConsumerCommand::Market(MarketCommand(3))),
                r#"{"Market":3}"#,
                "the Market Command variant must retain its own Slot tag"
            );
            assert_eq!(
                serialized(&ConsumerCommand::Execution(ExecutionCommand(4))),
                r#"{"Execution":4}"#,
                "the Execution Command variant must retain its own Slot tag"
            );
        }

        /// Invariant: a downstream receive-only Contract can use the crate-root
        /// uninhabited type and discharge its generated Command arm exhaustively.
        #[test]
        fn receive_only_never_arm_is_discharged_downstream() {
            fn fan_out(command: ReceiveOnlyCommand) {
                match command {
                    ReceiveOnlyCommand::Feed(never) => match never {},
                }
            }

            let _exhaustive_fan_out: fn(ReceiveOnlyCommand) = fan_out;
            assert_eq!(
                serialized(&ReceiveOnlyEvent::Feed(MarketEvent(29))),
                r#"{"Feed":29}"#,
                "the receive-only Event direction must remain usable downstream"
            );
        }
    }
}
