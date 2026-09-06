include!("../src/lib.rs");

reconstruct_engine! {
    fn complete_continue_without_checkpoint() {
        let _between_turns = continue_effects().complete_continue();
    }
}

fn main() {}
