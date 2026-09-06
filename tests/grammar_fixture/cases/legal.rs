include!("../src/lib.rs");

reconstruct_engine! {
    fn legal_continue_path() {
        let mut environment = CleanEnvironment;
        let mut commands = BoundedBuffer::new(1);
        commands
            .try_push(7)
            .expect("the legal fixture command must fit");
        let effects = match continue_turn().dispatch_batch::<_, _, ()>(
            &mut environment,
            &mut commands,
        ) {
            Ok(certificate) => certificate,
            Err(_) => panic!("the legal fixture dispatch must succeed"),
        };
        let checkpointed = match effects.checkpoint::<_, ()>(&mut environment) {
            Ok(certificate) => certificate,
            Err(_) => panic!("the legal fixture Continue checkpoint must succeed"),
        };
        let between_turns = match checkpointed.complete_continue() {
            Ok(certificate) => certificate,
            Err(_) => panic!("the legal fixture Continue completion must commit"),
        };
        match between_turns.accept_event::<_, ()>(&mut environment) {
            Ok((_certificate, _event)) => {}
            Err(_) => panic!("the legal fixture must accept its next Event"),
        }
    }

    fn legal_stop_path() {
        let checkpointed = stop_checkpointed();
        let stop_pending = match checkpointed.request_stop() {
            Ok(certificate) => certificate,
            Err(_) => panic!("the legal fixture StopRequested record must commit"),
        };
        match stop_pending.close::<_, ()>(CleanEnvironment) {
            Ok(_certificate) => {}
            Err(_) => panic!("the legal fixture Stop completion must succeed"),
        }
    }
}

fn main() {}
