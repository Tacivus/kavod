use kavod::time::Timestamp;
use kavod::{BuildError, Engine, EngineConfig, EngineError, HandlerCtx, Message, State};

// ========================================================================
// Fixtures
// ========================================================================

#[derive(Debug)]
struct MsgA;

impl Message for MsgA {}

#[derive(Debug)]
struct Orphan;

impl Message for Orphan {}

#[derive(Debug, Clone, PartialEq)]
struct KeyedNum {
    key: u32,
    value: u64,
}

impl State for KeyedNum {
    type Key = u32;

    fn key(&self) -> Self::Key {
        self.key
    }
}

// ========================================================================
// Build success / modes
// ========================================================================

/// Invariant: an empty backtest builder builds successfully.
#[test]
fn test_empty_builder_builds() {
    let _engine = Engine::builder(EngineConfig::backtest(Timestamp::new(0)))
        .build()
        .unwrap();
}

/// Invariant: live and replay config constructors report unsupported mode.
#[test]
fn test_unsupported_modes_fail_at_config() {
    assert!(matches!(
        EngineConfig::live(Timestamp::new(0)),
        Err(BuildError::UnsupportedMode { mode: "Live" })
    ));
    assert!(matches!(
        EngineConfig::replay(Timestamp::new(0)),
        Err(BuildError::UnsupportedMode { mode: "Replay" })
    ));
}

// ========================================================================
// Seeding
// ========================================================================

/// Invariant: duplicate seeded (type, key) returns BuildError.
#[test]
fn test_duplicate_seed_returns_build_error() {
    let mut builder = Engine::builder(EngineConfig::backtest(Timestamp::new(0)));
    builder.seed(KeyedNum { key: 1, value: 10 }).unwrap();
    let err = match builder.seed(KeyedNum { key: 1, value: 99 }) {
        Err(e) => e,
        Ok(_) => panic!("expected duplicate seed error"),
    };
    assert!(matches!(
        err,
        BuildError::DuplicateSeededState { type_name }
            if type_name.contains("KeyedNum")
    ));
}

// ========================================================================
// Graph validation
// ========================================================================

/// Invariant: a terminal handler with no productions builds.
#[test]
fn test_terminal_handler_builds() {
    let mut builder = Engine::builder(EngineConfig::backtest(Timestamp::new(0)));
    builder.on(|_ctx: &mut HandlerCtx<'_>, _msg: &MsgA| {});
    builder.build().unwrap();
}

/// Invariant: orphan production fails at build with a readable type name.
#[test]
fn test_orphan_production_prevents_build() {
    let mut builder = Engine::builder(EngineConfig::backtest(Timestamp::new(0)));
    builder
        .on(|_ctx: &mut HandlerCtx<'_>, _msg: &MsgA| {})
        .produces::<Orphan>();
    let err = match builder.build() {
        Err(e) => e,
        Ok(_) => panic!("expected orphan production build error"),
    };
    assert!(matches!(
        err,
        BuildError::MissingConsumer { message_type }
            if message_type.contains("Orphan")
    ));
    let msg = err.to_string();
    assert!(
        msg.contains("Orphan") || msg.contains("no consumer"),
        "expected readable diagnostic, got: {msg}"
    );
}

/// Invariant: unconsumed external input is rejected before run.
#[test]
fn test_unconsumed_ingress_rejected() {
    let mut engine = Engine::builder(EngineConfig::backtest(Timestamp::new(0)))
        .build()
        .unwrap();
    let err = engine.push_event(Timestamp::new(0), MsgA).unwrap_err();
    assert!(matches!(
        err,
        EngineError::UnconsumedIngress { message_type }
            if message_type.contains("MsgA")
    ));
}

// ========================================================================
// Actors (metadata / graph only)
// ========================================================================

struct VenueState;

/// Invariant: actor subscription satisfies handler orphan production at build.
#[test]
fn test_actor_satisfies_orphan_handler_production() {
    #[derive(Debug)]
    struct Produced;
    impl Message for Produced {}

    let mut builder = Engine::builder(EngineConfig::backtest(Timestamp::new(0)));
    builder
        .on(|_ctx: &mut HandlerCtx<'_>, _msg: &MsgA| {})
        .produces::<Produced>();
    builder
        .actor("venue", VenueState, |actor| {
            actor.on(|_s, _ctx, _msg: &Produced| {});
        })
        .unwrap();
    builder.build().unwrap();
}

/// Invariant: duplicate actor names fail with DuplicateRegistrationIdentity.
#[test]
fn test_duplicate_actor_name_fails() {
    let mut builder = Engine::builder(EngineConfig::backtest(Timestamp::new(0)));
    builder.actor("venue", VenueState, |_| {}).unwrap();
    let err = match builder.actor("venue", VenueState, |_| {}) {
        Err(e) => e,
        Ok(_) => panic!("expected duplicate actor name error"),
    };
    assert!(matches!(
        err,
        BuildError::DuplicateRegistrationIdentity { name: "venue" }
    ));
    let msg = err.to_string();
    assert!(
        msg.contains("venue") || msg.contains("duplicate"),
        "expected readable diagnostic, got: {msg}"
    );
}

/// Invariant: actor orphan production fails build with readable type name.
#[test]
fn test_actor_orphan_production_prevents_build() {
    let mut builder = Engine::builder(EngineConfig::backtest(Timestamp::new(0)));
    builder
        .actor("venue", VenueState, |actor| {
            actor.on(|_s, _ctx, _msg: &MsgA| {}).produces::<Orphan>();
        })
        .unwrap();
    let err = match builder.build() {
        Err(e) => e,
        Ok(_) => panic!("expected orphan production build error"),
    };
    assert!(matches!(
        err,
        BuildError::MissingConsumer { message_type }
            if message_type.contains("Orphan")
    ));
}
