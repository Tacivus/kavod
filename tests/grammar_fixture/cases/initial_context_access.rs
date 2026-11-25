include!("../src/lib.rs");

reconstruct_engine! {
    fn initial_has_no_index_getter() {
        let _index = initial().index();
    }

    fn initial_has_no_logical_time_getter() {
        let _logical_time = initial().logical_time();
    }
}

fn main() {}
