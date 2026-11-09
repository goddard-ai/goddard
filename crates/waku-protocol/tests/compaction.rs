use std::time::{Duration, Instant};
use uuid::Uuid;
use waku_protocol::compaction::CompactionHint;
use waku_protocol::model::{AgentSession, ContextUsage, DriverEvent, ProviderKind};

fn submit(id: Uuid) -> DriverEvent {
    DriverEvent::PromptSubmitted {
        message: String::new(),
        turn_id: id,
        message_id: Uuid::new_v4(),
        sent_by_task: None,
        hidden: false,
        report_trigger: None,
        reference_context: None,
    }
}

fn high_context() -> AgentSession {
    let mut session = AgentSession::new(Uuid::new_v4(), ProviderKind::Claude);
    session.context_usage = Some(ContextUsage {
        tokens: 180_000,
        window: Some(200_000),
    });
    session
}

#[test]
fn slow_high_context_wait_is_advisory_and_drop_strengthens_it() {
    let mut session = high_context();
    let start = Instant::now();
    let prompt = submit(Uuid::new_v4());
    session.observe_compaction(&prompt, start);
    // A duplicate echo, metadata and acceptance do not restart or end the wait.
    session.observe_compaction(&prompt, start + Duration::from_secs(10));
    session.observe_compaction(&DriverEvent::TurnStarted, start + Duration::from_secs(20));
    assert_eq!(
        session
            .compaction_monitor
            .hint(start + Duration::from_secs(59)),
        None
    );
    assert_eq!(
        session
            .compaction_monitor
            .hint(start + Duration::from_secs(60)),
        Some(CompactionHint::Possible)
    );
    session.observe_compaction(
        &DriverEvent::ReasoningDelta("working".into()),
        start + Duration::from_secs(65),
    );
    assert_eq!(
        session
            .compaction_monitor
            .hint(start + Duration::from_secs(65)),
        None
    );
    session.observe_compaction(
        &DriverEvent::UsageUpdated {
            context_tokens: Some(60_000),
            context_window: None,
        },
        start + Duration::from_secs(70),
    );
    assert_eq!(
        session
            .compaction_monitor
            .hint(start + Duration::from_secs(70)),
        Some(CompactionHint::Likely)
    );
    // Timing does not leak into persisted/wire state or synthesize a provider event.
    let saved = serde_json::to_value(&session).unwrap();
    assert!(saved.get("compaction_monitor").is_none());
    let restored: AgentSession = serde_json::from_value(saved).unwrap();
    assert_eq!(
        restored
            .compaction_monitor
            .hint(start + Duration::from_secs(80)),
        None
    );
}

#[test]
fn ordinary_work_unknown_usage_and_configuration_changes_do_not_imply_compaction() {
    let start = Instant::now();
    let late = start + Duration::from_secs(120);
    for usage in [
        None,
        Some(ContextUsage {
            tokens: 180_000,
            window: None,
        }),
        Some(ContextUsage {
            tokens: 201_000,
            window: Some(200_000),
        }),
        Some(ContextUsage {
            tokens: 80_000,
            window: Some(200_000),
        }),
    ] {
        let mut session = high_context();
        session.context_usage = usage;
        session.observe_compaction(&submit(Uuid::new_v4()), start);
        assert_eq!(session.compaction_monitor.hint(late), None);
    }
    let mut session = high_context();
    session.observe_compaction(&submit(Uuid::new_v4()), start);
    session.observe_compaction(
        &DriverEvent::TextDelta("reply".into()),
        start + Duration::from_secs(1),
    );
    // A long tool loop after a fast first response is not a slow prompt.
    session.observe_compaction(
        &DriverEvent::UsageUpdated {
            context_tokens: Some(50_000),
            context_window: None,
        },
        late,
    );
    assert_eq!(session.compaction_monitor.hint(late), None);
    session.observe_compaction(&submit(Uuid::new_v4()), late);
    session.model = Some("another-model".into());
    session.observe_compaction(&DriverEvent::TurnStarted, late + Duration::from_secs(65));
    assert_eq!(
        session
            .compaction_monitor
            .hint(late + Duration::from_secs(65)),
        None
    );
    let mut session = high_context();
    let turn = session.begin_turn("original");
    session.observe_prompt_dispatch(turn, start);
    session.observe_compaction(
        &DriverEvent::TextDelta("reply".into()),
        start + Duration::from_secs(1),
    );
    session.observe_compaction(&submit(Uuid::new_v4()), late);
    assert_eq!(
        session.compaction_hint(late + Duration::from_secs(65)),
        None
    );
}

#[test]
fn baseline_adapts_and_cancellation_clears_the_hint() {
    let mut session = high_context();
    let start = Instant::now();
    for i in 0..5 {
        let sent = start + Duration::from_secs(i * 30);
        session.observe_compaction(&submit(Uuid::new_v4()), sent);
        session.observe_compaction(
            &DriverEvent::TextDelta("reply".into()),
            sent + Duration::from_secs(15),
        );
    }
    let sent = start + Duration::from_secs(150);
    session.observe_compaction(&submit(Uuid::new_v4()), sent);
    assert_eq!(
        session
            .compaction_monitor
            .hint(sent + Duration::from_secs(44)),
        None
    );
    assert_eq!(
        session
            .compaction_monitor
            .hint(sent + Duration::from_secs(45)),
        Some(CompactionHint::Possible)
    );
    session.observe_compaction(&DriverEvent::TurnCancelled, sent + Duration::from_secs(46));
    assert_eq!(
        session
            .compaction_monitor
            .hint(sent + Duration::from_secs(60)),
        None
    );
}
#[test]
fn explicit_compaction_reconnects_and_window_changes_supersede_inference() {
    use waku_protocol::model::{ActivityItem, ActivityKind};
    let start = Instant::now();
    let late = start + Duration::from_secs(65);
    let mut explicit = ActivityItem::new(None, ActivityKind::Tool, "Compacting", None, false);
    explicit.title_i18n = Some(waku_protocol::WireTranslation::new("activity.compact", []));
    for event in [
        DriverEvent::RichActivity(explicit),
        DriverEvent::Connected {
            provider_cursor: None,
        },
        DriverEvent::TurnParked,
        DriverEvent::ProcessExited,
        DriverEvent::UsageUpdated {
            context_tokens: None,
            context_window: Some(1_000_000),
        },
    ] {
        let mut session = high_context();
        session.observe_prompt_dispatch(Uuid::new_v4(), start);
        assert_eq!(
            session.compaction_hint(late),
            Some(CompactionHint::Possible)
        );
        session.observe_compaction(&event, late);
        session.observe_compaction(
            &DriverEvent::UsageUpdated {
                context_tokens: Some(50_000),
                context_window: None,
            },
            late,
        );
        assert_eq!(session.compaction_hint(late), None, "{event:?}");
    }
    let mut session = high_context();
    session.observe_prompt_dispatch(Uuid::new_v4(), start);
    session.observe_compaction(
        &DriverEvent::UsageUpdated {
            context_tokens: Some(170_000),
            context_window: None,
        },
        late,
    );
    assert_ne!(session.compaction_hint(late), Some(CompactionHint::Likely));
    session.model = Some("changed".into());
    assert_eq!(session.compaction_hint(late), None);
    session.observe_prompt_dispatch(Uuid::new_v4(), late);
    assert_eq!(
        session.compaction_hint(late + Duration::from_secs(65)),
        None
    );
}
