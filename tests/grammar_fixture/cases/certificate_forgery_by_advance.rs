include!("../src/lib.rs");

reconstruct_engine! {
    fn forge_closed_by_advance() {
        let _closed: Certificate<Vec<u8>, record::Closed> = initial().advance();
    }
}

fn main() {}
