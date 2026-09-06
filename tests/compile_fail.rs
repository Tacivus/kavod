#[cfg(test)]
mod tests {
    mod grammar_compile_fail {
        /// Invariant: graph transitions cannot be called out of order, bypass a
        /// checkpoint, complete Stop early, commit dispatch completion directly,
        /// or contradict the answer fixed during classification.
        /// Design Doc: VERIFY-GRAMMAR
        #[test]
        fn illegal_transitions_do_not_compile() {
            let cases = trybuild::TestCases::new();
            cases.compile_fail("tests/grammar_fixture/cases/illegal_transition_order.rs");
            cases.compile_fail("tests/grammar_fixture/cases/skipped_checkpoint.rs");
            cases.compile_fail("tests/grammar_fixture/cases/premature_stop_completion.rs");
            cases.compile_fail("tests/grammar_fixture/cases/independent_commands_dispatched.rs");
            cases.compile_fail("tests/grammar_fixture/cases/disagreeing_outcome.rs");
        }

        /// Invariant: a certificate cannot be cloned, reused after being moved,
        /// or constructed through the Default trait.
        /// Design Doc: VERIFY-GRAMMAR
        #[test]
        fn certificate_duplication_does_not_compile() {
            let cases = trybuild::TestCases::new();
            cases.compile_fail("tests/grammar_fixture/cases/certificate_clone.rs");
            cases.compile_fail("tests/grammar_fixture/cases/certificate_copy.rs");
            cases.compile_fail("tests/grammar_fixture/cases/certificate_default.rs");
        }

        /// Invariant: the reconstructed private Engine module admits complete
        /// legal Continue and Stop paths, including ordered command dispatch.
        /// Design Doc: VERIFY-GRAMMAR
        #[test]
        fn the_fixture_reconstruction_itself_compiles() {
            let cases = trybuild::TestCases::new();
            cases.pass("tests/grammar_fixture/cases/legal.rs");
        }

        /// Invariant: phase-specific methods remain unavailable from additional
        /// source phases that would repeat startup, expose premature turn context,
        /// or accept an Event before Continue completion.
        #[test]
        fn phase_specific_methods_reject_additional_wrong_sources() {
            let cases = trybuild::TestCases::new();
            cases.compile_fail("tests/grammar_fixture/cases/repeated_run_started.rs");
            cases.compile_fail("tests/grammar_fixture/cases/initial_context_access.rs");
            cases.compile_fail("tests/grammar_fixture/cases/event_before_continue_completion.rs");
        }

        /// Invariant: the closed run is terminal, an unclassified certificate
        /// cannot take either batch edge or name the phases beyond them, and
        /// BetweenTurns permits only Event acceptance as its next transition.
        #[test]
        fn terminal_and_intermediate_phases_reject_wrong_transitions() {
            let cases = trybuild::TestCases::new();
            cases.compile_fail("tests/grammar_fixture/cases/closed_is_terminal.rs");
            cases.compile_fail("tests/grammar_fixture/cases/unclassified_batch_edge.rs");
            cases.compile_fail("tests/grammar_fixture/cases/between_turns_only_accepts_event.rs");
        }
    }
}
