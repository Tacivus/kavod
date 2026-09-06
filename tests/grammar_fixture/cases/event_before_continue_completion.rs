include!("../src/lib.rs");

reconstruct_engine! {
    fn accept_event_before_continue_completion() {
        let _accepted = continue_checkpointed().accept_event::<_, ()>(&mut CleanEnvironment);
    }
}

fn main() {}
