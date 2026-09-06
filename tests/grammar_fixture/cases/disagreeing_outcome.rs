include!("../src/lib.rs");

reconstruct_engine! {
    fn continue_answer_cannot_request_stop() {
        let _stop_pending = continue_checkpointed().request_stop();
    }

    fn stop_answer_cannot_complete_continue() {
        let _between_turns = stop_checkpointed().complete_continue();
    }
}

fn main() {}
