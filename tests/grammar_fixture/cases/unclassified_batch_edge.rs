include!("../src/lib.rs");

reconstruct_engine! {
    fn unclassified_cannot_take_the_recordless_edge() {
        let commands = BoundedBuffer::<u8>::new(0);
        let _effects = turn_open().no_commands(&commands);
    }

    fn unclassified_cannot_dispatch_a_batch() {
        let mut commands = BoundedBuffer::<u8>::new(1);
        let _ = commands.try_push(7);
        let _effects =
            turn_open().dispatch_batch::<_, _, ()>(&mut CleanEnvironment, &mut commands);
    }

    fn unclassified_effects_are_not_a_phase(
    ) -> Certificate<Vec<u8>, EffectsComplete<record::Unclassified>> {
        unreachable!()
    }
}

fn main() {}
