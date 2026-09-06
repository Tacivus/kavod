include!("../src/lib.rs");

reconstruct_engine! {
    fn closed() -> Certificate<Vec<u8>, record::Closed> {
        let stop_pending = match stop_checkpointed().request_stop() {
            Ok(certificate) => certificate,
            Err(_) => panic!("the terminal fixture must commit StopRequested"),
        };
        match stop_pending.close::<_, ()>(CleanEnvironment) {
            Ok(certificate) => certificate,
            Err(_) => panic!("the terminal fixture must complete clean shutdown"),
        }
    }

    fn closed_cannot_run_started() {
        let _next = closed().run_started();
    }

    fn closed_cannot_classify() {
        let _next = closed().classify(TurnOutcome::Continue);
    }

    fn closed_cannot_take_the_empty_batch_edge() {
        let commands = BoundedBuffer::<u8>::new(0);
        let _next = closed().no_commands(&commands);
    }

    fn closed_cannot_dispatch_a_batch() {
        let mut environment = CleanEnvironment;
        let mut commands = BoundedBuffer::new(1);
        commands.try_push(1).expect("the attack command must fit");
        let _next = closed().dispatch_batch::<_, _, ()>(&mut environment, &mut commands);
    }

    fn closed_cannot_checkpoint() {
        let _next = closed().checkpoint::<_, ()>(&mut CleanEnvironment);
    }

    fn closed_cannot_complete_continue() {
        let _next = closed().complete_continue();
    }

    fn closed_cannot_accept_event() {
        let _next = closed().accept_event::<_, ()>(&mut CleanEnvironment);
    }

    fn closed_cannot_request_stop() {
        let _next = closed().request_stop();
    }

    fn closed_cannot_close_again() {
        let _next = closed().close::<_, ()>(CleanEnvironment);
    }
}

fn main() {}
