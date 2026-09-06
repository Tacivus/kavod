include!("../src/lib.rs");

reconstruct_engine! {
    fn commit_commands_dispatched_directly() {
        let mut certificate = turn_open();
        let payload = record::CommandsDispatchedRecord {
            record_kind: record::Kind::new(),
            index: crate::time::EventIndex::new(0),
        };
        let _result = certificate.commit(&payload, None);
    }
}

fn main() {}
