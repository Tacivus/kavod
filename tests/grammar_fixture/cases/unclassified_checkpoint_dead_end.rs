include!("../src/lib.rs");

reconstruct_engine! {
    fn unclassified_checkpointed(
    ) -> Certificate<Vec<u8>, Checkpointed<record::Unclassified>> {
        let commands = BoundedBuffer::<u8>::new(0);
        let effects = turn_open().no_commands(&commands);
        match effects.checkpoint::<_, ()>(&mut CleanEnvironment) {
            Ok(certificate) => certificate,
            Err(_) => panic!("the unclassified fixture checkpoint must succeed"),
        }
    }

    fn unclassified_cannot_complete_continue() {
        let _next = unclassified_checkpointed().complete_continue();
    }

    fn unclassified_cannot_request_stop() {
        let _next = unclassified_checkpointed().request_stop();
    }
}

fn main() {}
