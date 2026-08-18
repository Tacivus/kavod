# Test Design Pattern

All test modules in Kavod follow this pattern:

- Keep unit tests in a `#[cfg(test)] mod tests` module in the source file they test.
- Suites that span files live in `tests/`: the conformance trace suite (both
  Environments), golden-Journal suites, and live lifecycle tests. Compile-fail proofs
  (illegal transitions, missing witnesses) use `trybuild` under `tests/`.
- Create one nested module per **subject and behavior**.
- Name every group `<subject>_<behavior>`, even when the source file currently has one
  subject.
- Keep groups small and cohesive. Do not combine unrelated behavior merely because a
  subject has few tests.
- Put helpers and fixtures in the outer `tests` module only when shared by multiple
  groups.
- Document each test with the specific invariant it verifies, citing its ID
  (`APP-EMIT`, `JRN-POISON`, …) — never a section number. Facts without an ID (API
  shapes, wire bytes) are cited by name.
- Name tests for the observable behavior being verified.

```rust
#[cfg(test)]
mod tests {
    use super::*;

    // Shared helpers and fixtures used by multiple test groups go here.

    mod order_input_validation {
        use super::*;

        /// Invariant: ORD-RANGE — values outside the permitted range are rejected.
        #[test]
        fn out_of_range_value_returns_error() {
            // ...
        }

        /// Invariant: ORD-RANGE — rejection leaves the existing state unchanged.
        #[test]
        fn rejected_value_does_not_mutate_state() {
            // ...
        }
    }

    mod order_capacity_enforcement {
        use super::*;

        /// Invariant: ORD-BOUND — adding an item beyond capacity fails without
        /// growing storage.
        #[test]
        fn capacity_overflow_returns_error() {
            // ...
        }
    }
}
```
