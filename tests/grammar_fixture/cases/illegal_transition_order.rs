include!("../src/lib.rs");

reconstruct_engine! {
    fn classify_before_run_started() {
        let _classified = initial().classify(TurnOutcome::Continue);
    }
}

fn main() {}
