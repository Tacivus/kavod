# Test Design Pattern

All test modules in Kavod should follow this pattern:

- Keep tests in a `#[cfg(test)] mod tests` module in the source file they test.
- Create one nested module per **subject and behavior**.
- Name every group `<subject>_<behavior>`, even when the source file currently has one subject.
- Keep groups small and cohesive. Do not combine unrelated behavior merely because a subject has few tests.
- Put helpers and fixtures in the outer `tests` module only when shared by multiple groups.
- Document each test with the specific invariant it verifies.
- Name tests for the observable behavior being verified.

```rust
#[cfg(test)]
mod tests {
    use super::*;

    // Shared helpers and fixtures used by multiple test groups go here.

    mod order_input_validation {
        use super::*;

        /// Invariant: Values outside the permitted range are rejected.
        #[test]
        fn out_of_range_value_returns_error() {
            // ...
        }

        /// Invariant: Rejection leaves the existing state unchanged.
        #[test]
        fn rejected_value_does_not_mutate_state() {
            // ...
        }
    }

    mod order_capacity_enforcement {
        use super::*;

        /// Invariant: Adding an item beyond capacity fails without growing storage.
        #[test]
        fn capacity_overflow_returns_error() {
            // ...
        }
    }

    mod queue_ordering {
        use super::*;

        /// Invariant: Equal-priority items retain insertion order.
        #[test]
        fn equal_priority_items_are_stable() {
            // ...
        }
    }

    mod record_serialization {
        use super::*;

        /// Invariant: The serialized form preserves all required fields.
        #[test]
        fn serialized_output_preserves_required_fields() {
            // ...
        }
    }
}
