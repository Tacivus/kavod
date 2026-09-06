include!("../src/lib.rs");

reconstruct_engine! {
    fn clone_certificate() {
        let certificate = initial();
        let _duplicate = certificate.clone();
    }
}

fn main() {}
