include!("../src/lib.rs");

reconstruct_engine! {
    fn default_certificate() {
        let _certificate: Certificate<Vec<u8>, Initial> = Default::default();
    }
}

fn main() {}
