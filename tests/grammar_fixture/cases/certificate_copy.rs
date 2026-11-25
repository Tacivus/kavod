include!("../src/lib.rs");

reconstruct_engine! {
    fn reuse_certificate_after_move() {
        let certificate = initial();
        let moved = certificate;
        drop(moved);
        drop(certificate);
    }
}

fn main() {}
