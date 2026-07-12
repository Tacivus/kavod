```rust 
#[cfg(tests)]
mod test {
    // ========================================================================
    // Test Section Identifier
    // ========================================================================

    /// Invariant: {invariant descriton on the property the test upholds}
    #[test]
    fn test_some_short_test_description() {}
    
    /// Invariant: {invariant descriton on the property the test upholds}
    #[test]
    fn test_another_test_in_the_same_test_section() {}
}
```
