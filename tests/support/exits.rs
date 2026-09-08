use kavod::{CoreError, EngineExit, EnvironmentFatal, FatalCause, JournalFatal, Quiescence};

pub fn stopped<S, AE, EE>(exit: EngineExit<S, AE, EE>) -> S {
    match exit {
        EngineExit::Stopped { state } => state,
        EngineExit::Fatal { .. } => panic!("a clean run must return Stopped"),
    }
}

pub fn expect_journal_fatal<S, AE, EE>(
    exit: EngineExit<S, AE, EE>,
) -> (S, JournalFatal, Quiescence) {
    match exit {
        EngineExit::Fatal {
            state,
            cause: FatalCause::Journal(fatal),
            quiescence,
        } => (state, fatal, quiescence),
        _ => panic!("a sink failure must produce a Journal fatal exit"),
    }
}

pub fn expect_environment_fatal<S, AE, EE>(
    exit: EngineExit<S, AE, EE>,
) -> (S, EnvironmentFatal<EE>, Quiescence) {
    match exit {
        EngineExit::Fatal {
            state,
            cause: FatalCause::Environment(fatal),
            quiescence,
        } => (state, fatal, quiescence),
        _ => panic!("an Environment failure must produce an Environment fatal exit"),
    }
}

pub fn expect_core_fatal<S, AE, EE>(exit: EngineExit<S, AE, EE>) -> (S, CoreError, Quiescence) {
    match exit {
        EngineExit::Fatal {
            state,
            cause: FatalCause::Core(error),
            quiescence,
        } => (state, error, quiescence),
        _ => panic!("a Core failure must produce a Core fatal exit"),
    }
}

pub fn expect_application_fatal<S, AE, EE>(exit: EngineExit<S, AE, EE>) -> (S, AE, Quiescence) {
    match exit {
        EngineExit::Fatal {
            state,
            cause: FatalCause::Application(error),
            quiescence,
        } => (state, error, quiescence),
        _ => panic!("an Application failure must produce an Application fatal exit"),
    }
}
