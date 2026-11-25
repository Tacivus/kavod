include!("../src/lib.rs");

reconstruct_engine! {
    fn close_before_stop_requested() {
        let _closed = stop_checkpointed().close::<_, ()>(CleanEnvironment);
    }
}

fn main() {}
