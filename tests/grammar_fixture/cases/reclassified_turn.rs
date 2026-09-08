include!("../src/lib.rs");

reconstruct_engine! {
    fn continue_turn_cannot_reclassify() {
        let _stop = continue_turn().classify(TurnOutcome::Stop);
    }

    fn stop_turn_cannot_reclassify() {
        let _continue = stop_turn().classify(TurnOutcome::Continue);
    }
}

fn main() {}
