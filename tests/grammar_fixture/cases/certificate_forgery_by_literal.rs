include!("../src/lib.rs");

reconstruct_engine! {
    fn forge_closed_by_literal() {
        let max_record_bytes =
            NonZeroUsize::new(512).expect("the forgery record bound must be nonzero");
        let journal = Journal::new(Vec::new(), max_record_bytes)
            .expect("the forgery Journal must reserve its buffer");
        let _closed: Certificate<Vec<u8>, record::Closed> = Certificate {
            journal,
            index: crate::time::EventIndex::new(0),
            last_time: Timestamp::from_nanos(0),
            _phase: std::marker::PhantomData,
        };
    }
}

fn main() {}
