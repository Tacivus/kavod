pub struct GoldenLines;

impl GoldenLines {
    pub fn split(bytes: &[u8]) -> Vec<&[u8]> {
        assert!(
            bytes.is_empty() || bytes.ends_with(b"\n"),
            "golden Journal bytes must be empty or end at a newline boundary"
        );
        bytes.split_inclusive(|byte| *byte == b'\n').collect()
    }
}
