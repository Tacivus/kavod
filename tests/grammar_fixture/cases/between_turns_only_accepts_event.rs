include!("../src/lib.rs");

reconstruct_engine! {
    fn between_turns() -> Certificate<Vec<u8>, record::BetweenTurns> {
        match continue_checkpointed().complete_continue() {
            Ok(certificate) => certificate,
            Err(_) => panic!("the BetweenTurns fixture must complete Continue"),
        }
    }

    fn between_turns_cannot_run_started() {
        let _next = between_turns().run_started();
    }

    fn between_turns_cannot_classify() {
        let _next = between_turns().classify(TurnOutcome::Continue);
    }

    fn between_turns_cannot_take_the_empty_batch_edge() {
        let commands = BoundedBuffer::<u8>::new(0);
        let _next = between_turns().no_commands(&commands);
    }

    fn between_turns_cannot_dispatch_a_batch() {
        let mut environment = CleanEnvironment;
        let mut commands = BoundedBuffer::new(1);
        commands.try_push(1).expect("the attack command must fit");
        let _next = between_turns().dispatch_batch::<_, _, ()>(&mut environment, &mut commands);
    }

    fn between_turns_cannot_checkpoint() {
        let _next = between_turns().checkpoint::<_, ()>(&mut CleanEnvironment);
    }

    fn between_turns_cannot_complete_continue() {
        let _next = between_turns().complete_continue();
    }

    fn between_turns_cannot_request_stop() {
        let _next = between_turns().request_stop();
    }

    fn between_turns_cannot_close() {
        let _next = between_turns().close::<_, ()>(CleanEnvironment);
    }
}

fn main() {}
