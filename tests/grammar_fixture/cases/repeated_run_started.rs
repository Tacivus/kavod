include!("../src/lib.rs");

reconstruct_engine! {
    fn run_started_cannot_repeat() {
        let _second_start = turn_open().run_started();
    }
}

fn main() {}
