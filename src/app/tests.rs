use super::autocomplete::session_mention_candidate;
use super::close_dialog::busy_owned_session_counts;
use super::composer::{
    ComposerAtomKind, ComposerInlineAtom, ComposerSubmitAction, atom_display_content,
    atom_payload_content, composer_submit_action, dropped_file_mention, merged_submission,
    pasted_text_preview, remap_marker_seats, splice_inline_atoms, visible_branch_entries,
    workspace_subject_for,
};
use super::model_picker::{
    PickerRow, PolicyRowId, next_picker_highlight, picker_rows, supports_reasoning_default_reset,
};
use super::runtime::{
    merge_remote_session_catalog, session_accepts_immediate_steer, session_has_active_provider_turn,
};
use super::sessions::{
    dormant_session_ids, next_attention_target, next_idle_session, next_non_busy_session,
    next_unread_completion,
};
use super::settings::{filter_archived_sessions, visible_settings_pages};
use super::sidebar::SidebarRow;
use super::streaming::session_accepts_steer_result;
use super::transcript_view::changed_files_diff_file_lines;
use super::{
    ESCAPE_STOP_CONFIRMATION_TIMEOUT, EscapeStopConfirmation, EscapeStopPress, EscapeStopTarget,
    NAVIGATION_RAIL_TICK_HEIGHT, NAVIGATION_RAIL_TURN_HEIGHT, NavigationLocation, PendingUserInput,
    SessionNavigation, SettingsHistoryEntry, SettingsNavigation, StreamDeltaKind,
    TranscriptLanding, TranscriptRowKind::*, TranscriptScrollPosition, WORKING_INDICATOR_FADE_OUT,
    WorkingIndicatorFade, active_navigation_turn_index, activity_group_is_live,
    activity_header_title, append_text_delta_to_session, assistant_response_footer,
    assistant_response_footer_index, assistant_response_footer_time, compact_driver_error,
    disclosure_leading_space, fenced_code, fitted_file_tree_width, fitted_panel_widths,
    folded_transcript_row_kinds, format_worked_duration, format_working_elapsed,
    maintain_transcript_anchor, message_opens_turn, message_starts_followup_turn,
    navigation_preview_snippet, navigation_rail_fade_visibility, navigation_rail_height,
    navigation_rail_scale, next_navigation_turn_index, paused_toast_duration, pop_stream_batch,
    previous_navigation_turn_index, prompt_answer_index, push_reasoning_delta,
    push_transcript_activity,
    response_footer_message_index, response_row_turn_id, retain_fading_working_indicator,
    row_starts_followup_turn, session_accepts_turn_output, session_is_reapable,
    settle_stream_segment, should_refresh_branch_after_activity, should_show_navigation_rail,
    should_show_scroll_to_bottom, task_id_from_notification_tag, task_notification_tag,
    transcript_anchor_end_space, transcript_navigation_turns, transcript_position_landing,
    transcript_rests_at_tail, transcript_row_kinds, transcript_row_splice,
    transcript_rows_fingerprint, update_transcript_activity, widened_panel_width_for_file_editor,
    widened_panel_width_for_review,
};
use crate::git_branch::BranchEntry;
use crate::model::{
    ActivityItem, ActivityKind, AgentSession, Checkpoint, CheckpointFile, CheckpointStatus,
    DriverEvent, Message, MessageAttachment, MessageRole, Project, ProviderKind, QueuedMessage,
    ReasoningBlock, RuntimeEventCursor, SessionStatus, SessionWorkspace, TranscriptBlock,
    TranscriptNotice, TranscriptNoticeStatus, TurnStatus, UserInputOption, UserInputQuestion,
    unix_time,
};

#[test]
fn structured_user_input_preserves_question_order_and_custom_answer_precedence() {
    let questions = vec![
        UserInputQuestion {
            id: "environment".into(),
            header: "Environment".into(),
            question: "Where should this deploy?".into(),
            options: vec![UserInputOption {
                label: "Preview".into(),
                description: None,
            }],
            multi_select: false,
        },
        UserInputQuestion {
            id: "notes".into(),
            header: "Notes".into(),
            question: "Anything else?".into(),
            options: Vec::new(),
            multi_select: false,
        },
    ];
    let mut pending = PendingUserInput::new("request-1".into(), questions);
    pending
        .selections
        .insert("environment".into(), vec!["Preview".into()]);
    pending
        .selections
        .insert("notes".into(), vec!["stale choice".into()]);
    pending
        .custom_answers
        .insert("notes".into(), "Use the EU region".into());

    let answers = pending.answers();
    assert_eq!(answers[0].question_id, "environment");
    assert_eq!(answers[0].answers, ["Preview"]);
    assert_eq!(answers[1].question_id, "notes");
    assert_eq!(answers[1].answers, ["Use the EU region"]);
}
use gpui::{ListAlignment, ListOffset, ListState, Pixels, px};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    time::{Duration, Instant},
};
use uuid::Uuid;

fn attach_changed_files(session: &mut AgentSession, files: Vec<CheckpointFile>) {
    let turn = session.turns.last_mut().expect("the test has a turn");
    turn.checkpoint = Some(Checkpoint {
        turn_count: turn.turn_count,
        git_ref: format!("refs/waku/test-turn-{}", turn.turn_count),
        status: CheckpointStatus::Ready,
        files,
        additions: 0,
        deletions: 0,
        created_at: 1,
    });
    turn.checkpoint
        .as_mut()
        .expect("checkpoint was just attached")
        .refresh_totals();
}

#[test]
fn remote_task_catalog_adds_web_tasks_without_replacing_hydrated_detail() {
    let project_id = Uuid::new_v4();
    let mut local = AgentSession::new(project_id, ProviderKind::Codex);
    local.title = "Local title".into();
    local
        .messages
        .push(Message::new(MessageRole::User, "keep this transcript"));
    let local_id = local.id;

    let mut local_projection = local.list_projection();
    local_projection.title = "Renamed elsewhere".into();
    local_projection.status = SessionStatus::Waiting;
    local_projection.updated_at += 10;

    let mut web_task = AgentSession::new(project_id, ProviderKind::Claude).list_projection();
    web_task.title = "Created in Web".into();
    let web_task_id = web_task.id;

    let mut catalog = vec![local];
    let removed = merge_remote_session_catalog(
        &mut catalog,
        vec![local_projection, web_task],
        |_| false,
        |_| false,
        false,
    );

    assert!(removed.is_empty());
    assert_eq!(catalog.len(), 2);
    let merged_local = catalog
        .iter()
        .find(|session| session.id == local_id)
        .unwrap();
    assert_eq!(merged_local.title, "Renamed elsewhere");
    assert_eq!(merged_local.status, SessionStatus::Waiting);
    assert_eq!(merged_local.messages.len(), 1);
    assert_eq!(merged_local.messages[0].content, "keep this transcript");
    assert!(catalog.iter().any(|session| session.id == web_task_id));
}

#[test]
fn remote_task_catalog_adopts_workspace_for_skeletons_only() {
    let project_id = Uuid::new_v4();
    let worktree = SessionWorkspace::Worktree {
        path: std::path::PathBuf::from("/tmp/worktrees/task"),
        name: "task".into(),
        branch: Some("waku/task".into()),
        base_branch: None,
    };

    // A skeleton row has no workspace of its own beyond what the daemon
    // stored — the projection's column is authoritative for it.
    let skeleton = AgentSession::new(project_id, ProviderKind::Codex).list_projection();
    assert_eq!(skeleton.workspace, SessionWorkspace::Local);
    let skeleton_id = skeleton.id;
    let mut remote = skeleton.clone();
    remote.workspace = worktree.clone();

    // A hydrated session may hold an unsaved move; the projection must not
    // clobber it.
    let mut hydrated = AgentSession::new(project_id, ProviderKind::Codex);
    hydrated.workspace = worktree.clone();
    let hydrated_id = hydrated.id;
    let mut stale_remote = hydrated.list_projection();
    stale_remote.workspace = SessionWorkspace::Local;

    let mut catalog = vec![skeleton, hydrated];
    merge_remote_session_catalog(
        &mut catalog,
        vec![remote, stale_remote],
        |_| false,
        |_| false,
        false,
    );

    assert_eq!(
        catalog
            .iter()
            .find(|session| session.id == skeleton_id)
            .map(|session| &session.workspace),
        Some(&worktree)
    );
    assert_eq!(
        catalog
            .iter()
            .find(|session| session.id == hydrated_id)
            .map(|session| &session.workspace),
        Some(&worktree)
    );
}

#[test]
fn fresh_catalog_keeps_incognito_sessions_but_still_removes_ordinary_ones() {
    let project_id = Uuid::new_v4();
    // A daemon restart: the fresh catalog knows only what the store held,
    // so both of these rows are absent from it.
    let mut incognito = AgentSession::new(project_id, ProviderKind::Codex);
    incognito.incognito = true;
    incognito.detail_loaded = true;
    incognito.begin_turn("off the record");
    let incognito_id = incognito.id;

    let mut ordinary = AgentSession::new(project_id, ProviderKind::Codex);
    ordinary.detail_loaded = true;
    ordinary.begin_turn("on the record");
    let ordinary_id = ordinary.id;

    // The incognito skeleton another client learned about is not kept — it
    // lacks the transcript a re-push needs and returns with its owner's.
    let mut skeleton = incognito.list_projection();
    skeleton.id = Uuid::new_v4();
    let skeleton_id = skeleton.id;

    let mut catalog = vec![incognito, ordinary, skeleton];
    let removed = merge_remote_session_catalog(&mut catalog, Vec::new(), |_| true, |_| false, true);

    assert_eq!(removed, vec![ordinary_id, skeleton_id]);
    assert_eq!(catalog.len(), 1);
    assert!(catalog.iter().any(|session| session.id == incognito_id));
}

#[test]
fn non_fresh_catalog_removes_incognito_sessions_like_any_other() {
    let project_id = Uuid::new_v4();
    // Same-daemon revision: a deleted incognito task stays deleted — the
    // restart survival is not a resurrection.
    let mut incognito = AgentSession::new(project_id, ProviderKind::Codex);
    incognito.incognito = true;
    incognito.detail_loaded = true;
    incognito.begin_turn("off the record");
    let incognito_id = incognito.id;

    let mut catalog = vec![incognito];
    let removed =
        merge_remote_session_catalog(&mut catalog, Vec::new(), |_| true, |_| false, false);

    assert_eq!(removed, vec![incognito_id]);
    assert!(catalog.is_empty());
}

#[test]
fn composer_only_offers_stop_after_submission_preparation() {
    let mut idle = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
    idle.status = SessionStatus::Idle;
    assert_eq!(
        composer_submit_action(Some(&idle), false, false),
        ComposerSubmitAction::Send
    );

    let mut connecting = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
    connecting.status = SessionStatus::Connecting;
    assert_eq!(
        composer_submit_action(Some(&connecting), true, false),
        ComposerSubmitAction::Preparing
    );
    assert_eq!(
        composer_submit_action(Some(&connecting), false, false),
        ComposerSubmitAction::Stop
    );

    let mut working = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
    working.status = SessionStatus::Working;
    assert_eq!(
        composer_submit_action(Some(&working), false, false),
        ComposerSubmitAction::Stop
    );

    let mut failed = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
    failed.status = SessionStatus::Failed;
    assert_eq!(
        composer_submit_action(Some(&failed), false, false),
        ComposerSubmitAction::Send
    );
}

#[test]
fn composer_offers_continue_only_for_an_empty_composer_on_a_stopped_turn() {
    let mut stopped = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
    stopped.begin_turn("do the thing");
    stopped.finish_active_turn(TurnStatus::Interrupted);
    stopped.status = SessionStatus::Idle;

    // Stopped via Stop, an app quit, or an orphaned runtime — all land on
    // Idle with the last turn Interrupted, and all offer Continue.
    assert_eq!(
        composer_submit_action(Some(&stopped), false, false),
        ComposerSubmitAction::Continue
    );
    // A draft is a send, not a continue.
    assert_eq!(
        composer_submit_action(Some(&stopped), false, true),
        ComposerSubmitAction::Send
    );
    // Preparation still wins over everything.
    assert_eq!(
        composer_submit_action(Some(&stopped), true, false),
        ComposerSubmitAction::Preparing
    );

    // A failed last turn — a provider error, or the runtime dying with its
    // daemon — is equally resumable, so it offers Continue too.
    let mut failed_turn = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
    failed_turn.begin_turn("do the thing");
    failed_turn.finish_active_turn(TurnStatus::Failed);
    failed_turn.status = SessionStatus::Failed;
    assert_eq!(
        composer_submit_action(Some(&failed_turn), false, false),
        ComposerSubmitAction::Continue
    );

    // A settled turn does not qualify — only an unsettled one.
    let mut finished = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
    finished.begin_turn("done");
    finished.finish_active_turn(TurnStatus::Completed);
    finished.status = SessionStatus::Idle;
    assert_eq!(
        composer_submit_action(Some(&finished), false, false),
        ComposerSubmitAction::Send
    );

    // A running turn's interrupted predecessor does not qualify either.
    let mut running = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
    running.begin_turn("first");
    running.finish_active_turn(TurnStatus::Interrupted);
    running.begin_turn("second");
    running.status = SessionStatus::Working;
    assert_eq!(
        composer_submit_action(Some(&running), false, false),
        ComposerSubmitAction::Stop
    );
}

#[test]
fn connecting_status_does_not_hide_a_started_provider_turn() {
    let mut session = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
    session.begin_turn("inspect the project");
    session.mark_active_turn_provider_started();
    session.status = SessionStatus::Connecting;

    assert!(session_has_active_provider_turn(&session));
}

#[test]
fn foreground_output_recovers_a_missed_provider_turn_start() {
    let mut session = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
    session.begin_turn("inspect the project");
    session.status = SessionStatus::Connecting;

    assert!(!session_has_active_provider_turn(&session));
    assert!(session_accepts_turn_output(&mut session));
    assert_eq!(session.status, SessionStatus::Working);
    assert!(session_has_active_provider_turn(&session));
}

#[test]
fn steer_waits_during_assistant_text_but_not_reasoning_or_tools() {
    let mut session = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
    session.begin_turn("inspect the project");
    session.mark_active_turn_provider_started();

    session.status = SessionStatus::Working;
    assert!(session_accepts_immediate_steer(&session));

    session.push_message(MessageRole::Assistant, "Final reply");
    session.messages.last_mut().unwrap().streaming = true;
    assert!(!session_accepts_immediate_steer(&session));
    session.messages.last_mut().unwrap().streaming = false;
    assert!(session_accepts_immediate_steer(&session));

    session.status = SessionStatus::Waiting;
    assert!(session_accepts_immediate_steer(&session));

    session.status = SessionStatus::Connecting;
    assert!(!session_accepts_immediate_steer(&session));

    session.status = SessionStatus::Background;
    assert!(session_accepts_immediate_steer(&session));

    // A backgrounded status without a live provider turn is not steerable.
    session.turns.last_mut().unwrap().status = TurnStatus::Completed;
    assert!(!session_accepts_immediate_steer(&session));
}

#[test]
fn steer_results_are_only_accepted_into_a_running_turn() {
    let mut session = AgentSession::new(Uuid::new_v4(), ProviderKind::Claude);
    assert!(!session_accepts_steer_result(&session));

    session.begin_turn("inspect the project");
    assert!(session_accepts_steer_result(&session));

    session.turns.last_mut().unwrap().status = TurnStatus::Interrupted;
    assert!(!session_accepts_steer_result(&session));

    session.begin_turn("follow up");
    assert!(session_accepts_steer_result(&session));
    session.turns.last_mut().unwrap().status = TurnStatus::Completed;
    assert!(!session_accepts_steer_result(&session));
}

#[test]
fn completed_mutating_activities_refresh_git_status() {
    assert!(should_refresh_branch_after_activity(
        ActivityKind::FileChange,
        true
    ));
    assert!(should_refresh_branch_after_activity(
        ActivityKind::Command,
        true
    ));
    assert!(!should_refresh_branch_after_activity(
        ActivityKind::FileChange,
        false
    ));
    assert!(!should_refresh_branch_after_activity(
        ActivityKind::FileRead,
        true
    ));
}

#[test]
fn escape_stop_requires_a_matching_second_press_and_expires() {
    let target = EscapeStopTarget {
        session_id: Uuid::new_v4(),
        turn_id: Some(Uuid::new_v4()),
    };
    let other_turn = EscapeStopTarget {
        session_id: target.session_id,
        turn_id: Some(Uuid::new_v4()),
    };
    let mut confirmation = EscapeStopConfirmation::default();
    let now = Instant::now();

    let first_arm = match confirmation.press(target, now) {
        EscapeStopPress::Arm(arm) => arm,
        EscapeStopPress::Stop => panic!("the first press must arm Stop"),
    };
    assert!(confirmation.is_armed_for(target, now + Duration::from_secs(2)));
    assert_eq!(
        confirmation.press(target, now + Duration::from_secs(2)),
        EscapeStopPress::Stop
    );
    assert!(!confirmation.is_armed_for(target, now + Duration::from_secs(2)));

    assert_eq!(ESCAPE_STOP_CONFIRMATION_TIMEOUT, Duration::from_secs(3));
    let second_arm = match confirmation.press(target, now) {
        EscapeStopPress::Arm(arm) => arm,
        EscapeStopPress::Stop => panic!("an unarmed confirmation must arm Stop"),
    };
    assert!(!confirmation.is_armed_for(target, now + Duration::from_secs(3)));
    let replacement_arm = match confirmation.press(other_turn, now + Duration::from_secs(3)) {
        EscapeStopPress::Arm(arm) => arm,
        EscapeStopPress::Stop => panic!("an expired or different target must arm Stop again"),
    };
    assert!(!confirmation.expire(first_arm));
    assert!(!confirmation.expire(second_arm));
    assert!(confirmation.expire(replacement_arm));
}

#[test]
fn toast_pause_preserves_time_with_a_readable_minimum() {
    assert_eq!(
        paused_toast_duration(Duration::from_secs(10), Duration::from_secs(3)),
        Duration::from_secs(7)
    );
    assert_eq!(
        paused_toast_duration(Duration::from_secs(1), Duration::from_secs(5)),
        Duration::from_millis(800)
    );
}

#[test]
fn dropped_files_mention_project_relative_paths() {
    let root = std::path::Path::new("/work/repo");
    assert_eq!(
        dropped_file_mention(
            Some(root),
            std::path::Path::new("/work/repo/src/main.rs"),
            false
        ),
        "src/main.rs"
    );
    assert_eq!(
        dropped_file_mention(Some(root), std::path::Path::new("/tmp/shot.png"), false),
        "/tmp/shot.png"
    );
    assert_eq!(
        dropped_file_mention(Some(root), std::path::Path::new("/work/repo/src"), true),
        "src/"
    );
    // The project root itself relativizes to nothing; keep it absolute.
    assert_eq!(
        dropped_file_mention(Some(root), std::path::Path::new("/work/repo"), true),
        "/work/repo/"
    );
    assert_eq!(
        dropped_file_mention(None, std::path::Path::new("/tmp/no project.png"), false),
        "/tmp/no project.png"
    );
}

fn file_attachment(mention: &str) -> MessageAttachment {
    MessageAttachment {
        path: std::path::PathBuf::from(mention),
        mention: mention.to_owned(),
        name: mention.to_owned(),
        is_dir: false,
        is_image: false,
        blob_reference: None,
        pasted_text_preview: None,
        session_id: None,
    }
}

#[test]
fn submissions_append_attachment_mentions_after_the_prompt() {
    let attachments = vec![file_attachment("src/a.rs"), file_attachment("shot.png")];
    assert_eq!(
        merged_submission("fix this", &attachments).as_deref(),
        Some("fix this @src/a.rs @shot.png")
    );
    // Attachments alone are a valid submission; blank text contributes
    // nothing but whitespace-trimming.
    assert_eq!(
        merged_submission("  ", &attachments).as_deref(),
        Some("@src/a.rs @shot.png")
    );
    assert_eq!(merged_submission(" plain ", &[]).as_deref(), Some("plain"));
    assert_eq!(merged_submission("   ", &[]), None);
}

#[test]
fn session_attachments_submit_as_task_references() {
    let session_id = Uuid::new_v4();
    let attachments = vec![MessageAttachment {
        session_id: Some(session_id),
        name: "Fix flake".to_owned(),
        ..file_attachment("session:unused")
    }];
    assert_eq!(
        merged_submission("ask it", &attachments).as_deref(),
        Some(format!("ask it [session \"Fix flake\" (task_id: {session_id})]").as_str())
    );
}

#[test]
fn pasted_text_preview_caps_at_two_hundred_characters() {
    let short = "  first line\nsecond line  ";
    assert_eq!(pasted_text_preview(short), "first line\nsecond line");
    let long = "x".repeat(300);
    let preview = pasted_text_preview(&long);
    assert_eq!(preview, format!("{}…", "x".repeat(200)));
    assert_eq!(pasted_text_preview("   "), "");
}

fn pasted_atom(marker: usize, text: &str) -> ComposerInlineAtom {
    ComposerInlineAtom {
        marker,
        revision: Uuid::new_v4(),
        paste_category: None,
        kind: ComposerAtomKind::PastedText(text.to_owned()),
    }
}

fn session_atom(marker: usize) -> ComposerInlineAtom {
    ComposerInlineAtom {
        marker,
        revision: Uuid::new_v4(),
        paste_category: None,
        kind: ComposerAtomKind::SessionRef {
            session_id: Uuid::nil(),
            title: "Big refactor".into(),
        },
    }
}

#[test]
fn inline_atoms_splice_back_at_their_markers() {
    use crate::input::INLINE_ATOM_MARKER as M;
    // Each marker splices to its atom's payload in marker order — pasted
    // text verbatim, a session as its token — wherever the kinds interleave.
    let content = format!("fix this\n{M}\nand {M} then\n{M}");
    let atoms = vec![
        pasted_atom(0, "first\nblock"),
        session_atom(0),
        pasted_atom(0, " second\nblock "),
    ];
    assert_eq!(
        splice_inline_atoms(&content, &atoms),
        "fix this\nfirst\nblock\nand [session \"Big refactor\" (task_id: 00000000-0000-0000-0000-000000000000)] then\nsecond\nblock"
    );
    // A marker without an atom splices to nothing; an atom without a
    // marker folds onto the end, split off by a blank line.
    assert_eq!(
        splice_inline_atoms(&format!("fix this\n{M}"), &[pasted_atom(0, "  ")]),
        "fix this"
    );
    assert_eq!(
        splice_inline_atoms("fix this", &[pasted_atom(0, "a\nb"), session_atom(0)]),
        "fix this\n\na\nb\n\n[session \"Big refactor\" (task_id: 00000000-0000-0000-0000-000000000000)]"
    );
    assert_eq!(splice_inline_atoms("fix this", &[]), "fix this");
}

#[test]
fn atom_display_content_keeps_chips_and_payloads_round_trip() {
    use crate::input::INLINE_ATOM_MARKER as M;
    use waku_protocol::model::{
        MESSAGE_ATOM_END as END, MESSAGE_ATOM_OPEN as OPEN, atom_visible_text,
        encode_atom_session_id,
    };
    // What a send paints: the chip labels, not the provider payloads.
    let content = format!("fix {M} and {M} done");
    let atoms = vec![pasted_atom(0, "first\nblock"), session_atom(0)];
    let display = atom_display_content(&content, &atoms);
    let session_id = Uuid::nil();
    assert_eq!(
        display,
        format!(
            "fix {OPEN}Pasted text (2 lines){END} and {OPEN}{id}session:Big refactor{END} done",
            id = encode_atom_session_id(session_id),
        )
    );
    assert_eq!(
        atom_visible_text(&display),
        "fix Pasted text (2 lines) and session:Big refactor done"
    );

    // The wire atoms carry the payload an edit splices back inline.
    let wire: Vec<_> = atoms.iter().map(ComposerInlineAtom::message_atom).collect();
    assert_eq!(wire[0].label, "Pasted text (2 lines)");
    assert_eq!(wire[0].payload, "first\nblock");
    assert_eq!(wire[0].session_id, None);
    assert_eq!(wire[1].label, "session:Big refactor");
    assert_eq!(wire[1].session_id, Some(session_id));
    assert_eq!(
        atom_payload_content(&display, &wire),
        "fix first\nblock and [session \"Big refactor\" (task_id: 00000000-0000-0000-0000-000000000000)] done"
    );
}

#[test]
fn atom_display_content_escapes_markdown_in_labels() {
    use crate::input::INLINE_ATOM_MARKER as M;
    use waku_protocol::model::atom_visible_text;
    let mut atom = session_atom(0);
    atom.kind = ComposerAtomKind::SessionRef {
        session_id: Uuid::nil(),
        title: "use `x` *now*".into(),
    };
    let display = atom_display_content(&format!("{M}"), &[atom]);
    // The title's markdown-active characters stay literal inside the span.
    assert!(display.contains("session:use \\`x\\` \\*now\\*"));
    assert_eq!(atom_visible_text(&display), "session:use `x` *now*");
    // A paste category rides the label the composer showed.
    let mut bug = pasted_atom(0, "stack trace\nmore");
    bug.paste_category = Some("Bug report".to_owned());
    let display = atom_display_content(&format!("{M}"), &[bug]);
    assert!(display.contains("Bug report (2 lines)"));
}

#[test]
fn session_mentions_offer_only_the_composers_project() {
    let project = Uuid::new_v4();
    let mut same = started_session(Uuid::new_v4());
    same.project_id = project;
    let mut foreign = started_session(Uuid::new_v4());
    foreign.project_id = Uuid::new_v4();
    // In-project rows still have to earn their place: archived, side
    // chats, and drafts that never began stay out.
    let mut archived = started_session(Uuid::new_v4());
    archived.project_id = project;
    archived.archived_at = Some(1);
    let mut side_chat = started_session(Uuid::new_v4());
    side_chat.project_id = project;
    side_chat.side_chat_of = Some(Uuid::new_v4());
    let unstarted = AgentSession::new(project, ProviderKind::Codex);

    assert!(session_mention_candidate(&same, Some(project)));
    assert!(!session_mention_candidate(&foreign, Some(project)));
    assert!(!session_mention_candidate(&archived, Some(project)));
    assert!(!session_mention_candidate(&side_chat, Some(project)));
    assert!(!session_mention_candidate(&unstarted, Some(project)));
    // A composer with no draft slot has no project to scope to.
    assert!(session_mention_candidate(&foreign, None));
}

#[test]
fn marker_remap_keeps_the_seat_an_atom_just_seated() {
    use crate::input::INLINE_ATOM_MARKER as M;
    // The splice an atom's own marker insertion produces has an empty
    // removed range, and the atom already holds the marker's post-splice
    // offset — the seat is inside the inserted range, so it must not be
    // mistaken for a suffix atom and left without a position.
    let seats = remap_marker_seats(&[0], &[0], &(0..0), M.len_utf8());
    assert_eq!(seats, [Some(0)]);

    // Same story mid-field: the splice may carry a surrounding space too.
    let seats = remap_marker_seats(&[10], &[10], &(9..9), M.len_utf8() + 1);
    assert_eq!(seats, [Some(10)]);

    // A marker over a selection seats the atom the same way.
    let seats = remap_marker_seats(&[4], &[4], &(4..9), M.len_utf8());
    assert_eq!(seats, [Some(4)]);

    // A second atom joins the first without dropping either.
    let seats = remap_marker_seats(&[0, 8], &[0, 8], &(8..8), M.len_utf8());
    assert_eq!(seats, [Some(0), Some(8)]);
}

#[test]
fn marker_remap_shifts_and_drops_around_edits() {
    use crate::input::INLINE_ATOM_MARKER as M;
    // Typing before a marker re-seats the atom on the moved glyph.
    let seats = remap_marker_seats(&[8], &[10], &(2..2), 2);
    assert_eq!(seats, [Some(10)]);

    // Deleting a marker's range drops its atom; a survivor keeps its seat.
    let seats = remap_marker_seats(&[0, 8], &[0], &(8..11), 0);
    assert_eq!(seats, [Some(0), None]);

    // An undo step whose inserted text brings a marker back rebinds an atom
    // whose marker the removed range held.
    let seats = remap_marker_seats(&[5], &[5], &(4..6), 5);
    assert_eq!(seats, [Some(5)]);

    // A splice that leaves a stray marker — one no atom owns — still keeps
    // the trailing atoms aligned to their own markers.
    let seats = remap_marker_seats(&[12], &[4, 12], &(4..4), M.len_utf8());
    assert_eq!(seats, [Some(12)]);
}

#[test]
fn branch_picker_pins_selection_and_filters_by_name() {
    const NOW: u64 = 1_800_000_000;
    let branches = vec![
        BranchEntry {
            name: "topic/zebra".into(),
            checked_out_elsewhere: false,
            last_commit_at: Some(NOW - 90 * 86_400),
        },
        BranchEntry {
            name: "main".into(),
            checked_out_elsewhere: false,
            last_commit_at: Some(NOW - 30 * 86_400),
        },
        BranchEntry {
            name: "topic/apple".into(),
            checked_out_elsewhere: true,
            last_commit_at: Some(NOW - 86_400),
        },
    ];
    assert_eq!(
        visible_branch_entries(&branches, "main", "", NOW)
            .iter()
            .map(|branch| branch.name.as_str())
            .collect::<Vec<_>>(),
        vec!["main", "topic/apple", "topic/zebra"]
    );
    assert_eq!(
        visible_branch_entries(&branches, "main", "TOPIC APPLE", NOW)
            .iter()
            .map(|branch| branch.name.as_str())
            .collect::<Vec<_>>(),
        vec!["topic/apple"]
    );
}

#[test]
fn branch_picker_prefers_exact_matches() {
    const NOW: u64 = 1_800_000_000;
    let branches = vec![
        BranchEntry {
            name: "mainline".into(),
            checked_out_elsewhere: false,
            last_commit_at: Some(NOW - 86_400),
        },
        BranchEntry {
            name: "topic/main".into(),
            checked_out_elsewhere: false,
            last_commit_at: Some(NOW - 86_400),
        },
        BranchEntry {
            name: "main".into(),
            checked_out_elsewhere: false,
            last_commit_at: Some(NOW - 86_400),
        },
    ];
    // The exact match leads even when another branch is selected, and the
    // query's case does not matter.
    assert_eq!(
        visible_branch_entries(&branches, "topic/main", "MAIN", NOW)
            .iter()
            .map(|branch| branch.name.as_str())
            .collect::<Vec<_>>(),
        vec!["main", "topic/main", "mainline"]
    );
    // A partial query keeps the selection pinned; equally close matches fall
    // back to name order when their commits are equally fresh.
    assert_eq!(
        visible_branch_entries(&branches, "topic/main", "mai", NOW)
            .iter()
            .map(|branch| branch.name.as_str())
            .collect::<Vec<_>>(),
        vec!["topic/main", "main", "mainline"]
    );
}

#[test]
fn branch_picker_blends_match_fit_with_recency() {
    const NOW: u64 = 1_800_000_000;
    const DAY: u64 = 86_400;
    let branches = vec![
        BranchEntry {
            name: "topic/fix".into(),
            checked_out_elsewhere: false,
            last_commit_at: Some(NOW - DAY),
        },
        BranchEntry {
            name: "fix-old".into(),
            checked_out_elsewhere: false,
            last_commit_at: Some(NOW - 2 * 365 * DAY),
        },
        BranchEntry {
            name: "x-fix".into(),
            checked_out_elsewhere: false,
            last_commit_at: Some(NOW - DAY),
        },
    ];
    // A fresh branch outranks a stale one with a marginally better fit, while
    // a large fit gap still beats recency: "fix-old" starts with the query but
    // is two years old, "x-fix" is one day old, and "topic/fix" is fresh but
    // the worst fit of the three.
    assert_eq!(
        visible_branch_entries(&branches, "", "fix", NOW)
            .iter()
            .map(|branch| branch.name.as_str())
            .collect::<Vec<_>>(),
        vec!["x-fix", "fix-old", "topic/fix"]
    );
    // Without a query the ranking reduces to last-committed order.
    assert_eq!(
        visible_branch_entries(&branches, "", "", NOW)
            .iter()
            .map(|branch| branch.name.as_str())
            .collect::<Vec<_>>(),
        vec!["topic/fix", "x-fix", "fix-old"]
    );
}

#[test]
fn driver_errors_are_bounded_before_rendering() {
    let error = (0..20)
        .map(|line| format!("provider diagnostic line {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    let compact = compact_driver_error(&error);

    assert_eq!(compact.lines().count(), 7);
    assert!(compact.ends_with('…'));
    assert!(!compact.contains("provider diagnostic line 6"));

    let long = compact_driver_error(&"x".repeat(2_000));
    assert_eq!(long.chars().count(), 800);
    assert!(long.ends_with('…'));
}

#[test]
fn session_navigation_tracks_back_forward_and_new_branches() {
    let first = NavigationLocation::Task(Uuid::new_v4());
    let second = NavigationLocation::Task(Uuid::new_v4());
    let third = NavigationLocation::Task(Uuid::new_v4());
    let branch = NavigationLocation::Terminal(Uuid::new_v4());
    let mut navigation = SessionNavigation::default();

    navigation.visit(Some(first), second);
    navigation.visit(Some(second), third);
    assert_eq!(navigation.go_back(third), Some(second));
    assert_eq!(navigation.go_back(second), Some(first));
    assert_eq!(navigation.go_forward(first), Some(second));

    navigation.visit(Some(second), branch);
    assert_eq!(navigation.go_forward(branch), None);
    assert_eq!(navigation.go_back(branch), Some(second));
}

#[test]
fn session_navigation_interleaves_tasks_and_the_projects_page() {
    let task = NavigationLocation::Task(Uuid::new_v4());
    let project_a = NavigationLocation::ProjectsPage(Uuid::new_v4());
    let project_b = NavigationLocation::ProjectsPage(Uuid::new_v4());
    let mut navigation = SessionNavigation::default();

    // Task -> Projects A -> Projects B: back walks the page's own project
    // hops first, then the task the page was opened on.
    navigation.visit(Some(task), project_a);
    navigation.visit(Some(project_a), project_b);
    assert_eq!(navigation.go_back(project_b), Some(project_a));
    assert_eq!(navigation.go_back(project_a), Some(task));
    assert_eq!(navigation.go_forward(task), Some(project_a));
    assert_eq!(navigation.back_target(), Some(task));
}

#[test]
fn session_navigation_prunes_deleted_tasks() {
    let first = Uuid::new_v4();
    let second = Uuid::new_v4();
    let third = Uuid::new_v4();
    let terminal = Uuid::new_v4();
    let mut navigation = SessionNavigation::default();

    navigation.visit(
        Some(NavigationLocation::Task(first)),
        NavigationLocation::Terminal(terminal),
    );
    navigation.visit(
        Some(NavigationLocation::Terminal(terminal)),
        NavigationLocation::Task(second),
    );
    navigation.visit(
        Some(NavigationLocation::Task(second)),
        NavigationLocation::Task(third),
    );
    assert_eq!(
        navigation.go_back(NavigationLocation::Task(third)),
        Some(NavigationLocation::Task(second))
    );

    navigation.remove(first);
    navigation.remove(third);
    navigation.remove_terminal(terminal);
    assert_eq!(navigation.go_back(NavigationLocation::Task(second)), None);
    assert_eq!(
        navigation.go_forward(NavigationLocation::Task(second)),
        None
    );
}

#[test]
fn session_navigation_never_targets_the_current_location() {
    let task = NavigationLocation::Task(Uuid::new_v4());
    let first = Uuid::new_v4();
    let second = Uuid::new_v4();
    let mut navigation = SessionNavigation::default();

    // Task -> T1 -> T2, then T2 closes while viewed: the app lands on
    // T1, so it can no longer sit in the stacks — Back reaches the task.
    navigation.visit(Some(task), NavigationLocation::Terminal(first));
    navigation.visit(
        Some(NavigationLocation::Terminal(first)),
        NavigationLocation::Terminal(second),
    );
    navigation.remove_terminal(second);
    navigation.visit(None, NavigationLocation::Terminal(first));
    assert_eq!(
        navigation.go_back(NavigationLocation::Terminal(first)),
        Some(task)
    );

    // The forward stack gets the same treatment: T1 -> T2 -> Back ->
    // close T1 lands on T2, and Forward has nowhere to point but the
    // terminal already on screen.
    let mut navigation = SessionNavigation::default();
    navigation.visit(Some(task), NavigationLocation::Terminal(first));
    navigation.visit(
        Some(NavigationLocation::Terminal(first)),
        NavigationLocation::Terminal(second),
    );
    assert_eq!(
        navigation.go_back(NavigationLocation::Terminal(second)),
        Some(NavigationLocation::Terminal(first))
    );
    navigation.remove_terminal(first);
    navigation.visit(None, NavigationLocation::Terminal(second));
    assert_eq!(
        navigation.go_forward(NavigationLocation::Terminal(second)),
        None
    );
    assert_eq!(
        navigation.go_back(NavigationLocation::Terminal(second)),
        Some(task)
    );
}

fn settings_entry(page: super::SettingsPage, y: f32) -> SettingsHistoryEntry {
    SettingsHistoryEntry {
        page,
        offset: gpui::point(gpui::px(0.0), gpui::px(y)),
    }
}

#[test]
fn settings_navigation_tracks_pane_back_forward() {
    use super::SettingsPage::*;
    let mut navigation = SettingsNavigation::default();

    // General -> Appearance -> Git: back walks the hops in reverse,
    // carrying the scroll offset each pane was abandoned at; forward then
    // replays them.
    navigation.visit(Some(settings_entry(General, 0.0)), Appearance);
    navigation.visit(Some(settings_entry(Appearance, 120.0)), Git);
    assert_eq!(
        navigation.go_back(settings_entry(Git, 40.0)),
        Some(settings_entry(Appearance, 120.0))
    );
    assert_eq!(
        navigation.go_back(settings_entry(Appearance, 8.0)),
        Some(settings_entry(General, 0.0))
    );
    assert_eq!(navigation.back_target(), None);
    assert_eq!(
        navigation.go_forward(settings_entry(General, 0.0)),
        Some(settings_entry(Appearance, 8.0))
    );
}

#[test]
fn settings_navigation_folds_revisits_and_drops_gated_pages() {
    use super::SettingsPage::*;
    let mut navigation = SettingsNavigation::default();

    // General -> Appearance -> Git -> Appearance: the earlier Appearance
    // entry folds away, so back reaches Git then General without repeating
    // a pane.
    navigation.visit(Some(settings_entry(General, 0.0)), Appearance);
    navigation.visit(Some(settings_entry(Appearance, 0.0)), Git);
    navigation.visit(Some(settings_entry(Git, 0.0)), Appearance);
    assert_eq!(
        navigation.go_back(settings_entry(Appearance, 0.0)),
        Some(settings_entry(Git, 0.0))
    );
    assert_eq!(
        navigation.go_back(settings_entry(Git, 0.0)),
        Some(settings_entry(General, 0.0))
    );

    // A fresh visit clears forward; a gate closing drops the page from
    // both stacks.
    navigation.visit(Some(settings_entry(General, 0.0)), Appearance);
    navigation.visit(Some(settings_entry(Appearance, 0.0)), Friends);
    assert_eq!(
        navigation.go_back(settings_entry(Friends, 0.0)),
        Some(settings_entry(Appearance, 0.0))
    );
    navigation.remove(Friends);
    assert_eq!(navigation.go_forward(settings_entry(Appearance, 0.0)), None);
}

/// A session skeleton, as the session list holds them: stored rows report
/// started without their transcript detail loaded.
fn started_session(id: Uuid) -> AgentSession {
    let mut session = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
    session.id = id;
    session.detail_loaded = false;
    session
}

/// Pinned rows sort by activity recency, so the timestamp is the sidebar
/// order — `timestamp` bigger means higher in the pinned group.
fn pinned_session(id: Uuid, timestamp: u64) -> AgentSession {
    let mut session = started_session(id);
    session.pinned_at = Some(1);
    session.last_reply_at = Some(timestamp);
    session
}

#[test]
fn next_unread_completion_returns_the_topmost_unread_row() {
    let first = Uuid::new_v4();
    let second = Uuid::new_v4();
    let third = Uuid::new_v4();
    let sessions = vec![
        started_session(first),
        started_session(second),
        started_session(third),
    ];
    let rows = vec![
        SidebarRow::Search,
        SidebarRow::Session(first),
        SidebarRow::Session(second),
        SidebarRow::GroupSpacer,
        SidebarRow::Session(third),
    ];
    let unseen = HashMap::from([(first, 100), (third, 300)]);

    // Sidebar order is the importance order: the topmost unread row wins
    // regardless of where the current session sits, and non-session rows
    // never matter.
    for selected in [None, Some(second), Some(third)] {
        assert_eq!(
            next_unread_completion(
                &sessions,
                &unseen,
                &rows,
                selected,
                None,
                &HashSet::new(),
                None,
                None,
            ),
            Some(first)
        );
    }
    // The selected session never targets itself, even while unread.
    assert_eq!(
        next_unread_completion(
            &sessions,
            &unseen,
            &rows,
            Some(first),
            None,
            &HashSet::new(),
            None,
            None,
        ),
        Some(third)
    );
    // A pending activation is treated as on-screen too.
    assert_eq!(
        next_unread_completion(
            &sessions,
            &unseen,
            &rows,
            Some(third),
            Some(first),
            &HashSet::new(),
            None,
            None
        ),
        None
    );
    // No candidates: the caller falls to the idle rotation.
    assert_eq!(
        next_unread_completion(
            &sessions,
            &HashMap::new(),
            &rows,
            None,
            None,
            &HashSet::new(),
            None,
            None
        ),
        None
    );
}

#[test]
fn next_unread_completion_lets_pinned_rows_lead_by_position() {
    let pinned_top = Uuid::new_v4();
    let pinned_bottom = Uuid::new_v4();
    let current = Uuid::new_v4();
    let below = Uuid::new_v4();
    // Pinned order is activity recency: pinned_top over pinned_bottom.
    let sessions = vec![
        pinned_session(pinned_top, 300),
        pinned_session(pinned_bottom, 200),
        started_session(current),
        started_session(below),
    ];
    let rows = vec![
        SidebarRow::Session(pinned_top),
        SidebarRow::Session(pinned_bottom),
        SidebarRow::Session(current),
        SidebarRow::Session(below),
    ];

    // Pinned rows sit at the top of the sidebar, so an unread one leads
    // without any special-casing — even over an unread row at the anchor.
    let unseen = HashMap::from([(pinned_bottom, 100), (below, 400)]);
    for selected in [None, Some(current), Some(below)] {
        assert_eq!(
            next_unread_completion(
                &sessions,
                &unseen,
                &rows,
                selected,
                None,
                &HashSet::new(),
                None,
                None,
            ),
            Some(pinned_bottom)
        );
    }
    // Two unread pinned tasks take their sidebar order.
    let both_pinned = HashMap::from([(pinned_top, 50), (pinned_bottom, 500)]);
    assert_eq!(
        next_unread_completion(
            &sessions,
            &both_pinned,
            &rows,
            None,
            None,
            &HashSet::new(),
            None,
            None
        ),
        Some(pinned_top)
    );
    // A blocked pinned task counts as unread too.
    let mut sessions = sessions;
    sessions[0].status = SessionStatus::Waiting;
    assert_eq!(
        next_unread_completion(
            &sessions,
            &HashMap::new(),
            &rows,
            Some(current),
            None,
            &HashSet::new(),
            None,
            None
        ),
        Some(pinned_top)
    );
}

#[test]
fn next_unread_completion_skips_ineligible_sessions() {
    let queued_id = Uuid::new_v4();
    let archived_id = Uuid::new_v4();
    let unstarted_id = Uuid::new_v4();
    let settled_id = Uuid::new_v4();
    let mut queued = started_session(queued_id);
    queued.queued_messages.push(QueuedMessage::new("follow up"));
    let mut archived = started_session(archived_id);
    archived.archived_at = Some(1);
    // AgentSession::new is an unstarted draft: no row, no candidacy.
    let unstarted = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
    let settled = started_session(settled_id);
    let sessions = vec![queued, archived, unstarted, settled];
    let rows = vec![
        SidebarRow::Session(queued_id),
        SidebarRow::Session(settled_id),
    ];
    let unseen = HashMap::from([
        (queued_id, 400),
        (archived_id, 300),
        (unstarted_id, 200),
        (settled_id, 100),
    ]);
    assert_eq!(
        next_unread_completion(
            &sessions,
            &unseen,
            &rows,
            None,
            None,
            &HashSet::new(),
            None,
            None
        ),
        Some(settled_id)
    );

    // A blocked session with queued prompts is skipped the same way, and
    // when every candidate is ineligible there is no target.
    let mut blocked = started_session(Uuid::new_v4());
    blocked.status = SessionStatus::Waiting;
    blocked
        .queued_messages
        .push(QueuedMessage::new("and then this"));
    let blocked_id = blocked.id;
    let mut queued = started_session(queued_id);
    queued.queued_messages.push(QueuedMessage::new("follow up"));
    let sessions = vec![blocked, queued];
    let rows = vec![
        SidebarRow::Session(blocked_id),
        SidebarRow::Session(queued_id),
    ];
    let queued_only = HashMap::from([(queued_id, 300), (blocked_id, 400)]);
    assert_eq!(
        next_unread_completion(
            &sessions,
            &queued_only,
            &rows,
            None,
            None,
            &HashSet::new(),
            None,
            None
        ),
        None
    );
}

#[test]
fn next_unread_completion_skips_chain_seen_sessions() {
    let seen = Uuid::new_v4();
    let unread = Uuid::new_v4();
    let idle = Uuid::new_v4();
    let sessions = vec![
        started_session(seen),
        started_session(unread),
        started_session(idle),
    ];
    let rows = vec![
        SidebarRow::Session(seen),
        SidebarRow::Session(unread),
        SidebarRow::Session(idle),
    ];
    let unseen = HashMap::from([(seen, 100), (unread, 200)]);
    let sweep = HashSet::from([seen]);

    // The ⌘⇧D chain and the departure fallback pass a seen row over for the
    // next genuinely unread one, even though it sits higher in the sidebar.
    assert_eq!(
        next_unread_completion(
            &sessions,
            &unseen,
            &rows,
            None,
            None,
            &HashSet::new(),
            Some(&sweep),
            None
        ),
        Some(unread)
    );
    // With every unread row seen there is no unread target, so the caller
    // falls to the idle rotation.
    let only_seen = HashMap::from([(seen, 100)]);
    assert_eq!(
        next_unread_completion(
            &sessions,
            &only_seen,
            &rows,
            None,
            None,
            &HashSet::new(),
            Some(&sweep),
            None
        ),
        None
    );
    // ⌘D proper keeps seen rows as candidates.
    assert_eq!(
        next_unread_completion(
            &sessions,
            &unseen,
            &rows,
            None,
            None,
            &HashSet::new(),
            None,
            None
        ),
        Some(seen)
    );
}

#[test]
fn next_non_busy_session_walks_down_from_the_start_row_and_wraps() {
    let top = Uuid::new_v4();
    let busy = Uuid::new_v4();
    let middle = Uuid::new_v4();
    let bottom = Uuid::new_v4();
    let mut busy_session = started_session(busy);
    busy_session.status = SessionStatus::Working;
    let sessions = vec![
        started_session(top),
        busy_session,
        started_session(middle),
        started_session(bottom),
    ];
    let rows = vec![
        SidebarRow::Session(top),
        SidebarRow::Session(busy),
        SidebarRow::Session(middle),
        SidebarRow::Session(bottom),
    ];

    // ⌘⇧D's jump: below the marked session's row, busy rows skipped.
    assert_eq!(
        next_non_busy_session(
            &sessions,
            &rows,
            Some(top),
            None,
            1,
            &HashSet::new(),
            None,
            None,
            None
        ),
        Some(middle)
    );
    // It wraps to the top at the bottom of the list — including rows above
    // the anchor — but never lands on the selected session itself.
    assert_eq!(
        next_non_busy_session(
            &sessions,
            &rows,
            Some(bottom),
            None,
            4,
            &HashSet::new(),
            None,
            None,
            None
        ),
        Some(top)
    );
    assert_eq!(
        next_non_busy_session(
            &sessions,
            &rows,
            Some(top),
            None,
            4,
            &HashSet::new(),
            None,
            None,
            None
        ),
        Some(middle)
    );
    // A pending activation counts as on-screen.
    assert_eq!(
        next_non_busy_session(
            &sessions,
            &rows,
            Some(top),
            Some(middle),
            1,
            &HashSet::new(),
            None,
            None,
            None
        ),
        Some(bottom)
    );
    // The chain's seen set is skipped on the wrap: the jump can never land
    // back on a session it has already shown, including the one it started
    // from.
    let seen = HashSet::from([top, middle]);
    assert_eq!(
        next_non_busy_session(
            &sessions,
            &rows,
            Some(bottom),
            None,
            4,
            &HashSet::new(),
            Some(&seen),
            None,
            None
        ),
        None
    );
    assert_eq!(
        next_non_busy_session(
            &sessions,
            &rows,
            Some(bottom),
            None,
            4,
            &HashSet::new(),
            Some(&HashSet::from([top])),
            None,
            None,
        ),
        Some(middle)
    );
    // Everything busy or claimed: no target, the caller lands on New task.
    let mut all_busy = sessions;
    for session in &mut all_busy {
        session.status = SessionStatus::Working;
    }
    assert_eq!(
        next_non_busy_session(
            &all_busy,
            &rows,
            Some(top),
            None,
            1,
            &HashSet::new(),
            None,
            None,
            None
        ),
        None
    );
}

#[test]
fn next_idle_session_enters_at_the_top_and_walks_down_positionally() {
    let top = Uuid::new_v4();
    let middle = Uuid::new_v4();
    let bottom = Uuid::new_v4();
    let sessions = vec![
        started_session(top),
        started_session(middle),
        started_session(bottom),
    ];
    let rows = vec![
        SidebarRow::Session(top),
        SidebarRow::Session(middle),
        SidebarRow::Session(bottom),
    ];

    // With no current session — the New task page, or just after an archive —
    // the rotation enters at the topmost non-busy row.
    assert_eq!(
        next_idle_session(
            &sessions,
            &rows,
            None,
            None,
            &HashSet::new(),
            None,
            None,
            None
        ),
        Some(top)
    );
    // On an idle session the walk continues below it.
    assert_eq!(
        next_idle_session(
            &sessions,
            &rows,
            Some(top),
            None,
            &HashSet::new(),
            None,
            None,
            None
        ),
        Some(middle)
    );
    // …and wraps to the top at the bottom of the list.
    assert_eq!(
        next_idle_session(
            &sessions,
            &rows,
            Some(bottom),
            None,
            &HashSet::new(),
            None,
            None,
            None
        ),
        Some(top)
    );
    // …but never onto a row the ⌘⇧D chain has already shown.
    let seen = HashSet::from([top]);
    assert_eq!(
        next_idle_session(
            &sessions,
            &rows,
            Some(bottom),
            None,
            &HashSet::new(),
            Some(&seen),
            None,
            None
        ),
        Some(middle)
    );
    let seen = HashSet::from([top, middle, bottom]);
    assert_eq!(
        next_idle_session(
            &sessions,
            &rows,
            Some(bottom),
            None,
            &HashSet::new(),
            Some(&seen),
            None,
            None
        ),
        None
    );

    // A busy current session is not in the rotation, so the entry point is
    // again the topmost non-busy row rather than whatever sits below it.
    let mut sessions = sessions;
    sessions[0].status = SessionStatus::Working;
    assert_eq!(
        next_idle_session(
            &sessions,
            &rows,
            Some(top),
            None,
            &HashSet::new(),
            None,
            None,
            None
        ),
        Some(middle)
    );
    // Busy rows are skipped outright: wrapping from the bottom lands past
    // the busy top row on middle.
    assert_eq!(
        next_idle_session(
            &sessions,
            &rows,
            Some(bottom),
            None,
            &HashSet::new(),
            None,
            None,
            None
        ),
        Some(middle)
    );
    // A pending activation counts as on-screen, and when every other row is
    // busy or claimed there is no target.
    assert_eq!(
        next_idle_session(
            &sessions,
            &rows,
            Some(bottom),
            Some(middle),
            &HashSet::new(),
            None,
            None,
            None
        ),
        None
    );
    sessions[1].status = SessionStatus::Waiting;
    assert_eq!(
        next_idle_session(
            &sessions,
            &rows,
            None,
            None,
            &HashSet::new(),
            None,
            None,
            None
        ),
        Some(bottom)
    );
    assert_eq!(
        next_idle_session(
            &sessions,
            &rows,
            Some(bottom),
            None,
            &HashSet::new(),
            None,
            None,
            None
        ),
        None
    );

    // A failed turn still counts as idle — the user should be able to land on
    // it — and pinned rows join the same flat positional rotation.
    let pinned_top = Uuid::new_v4();
    let unpinned = Uuid::new_v4();
    let mut failed = started_session(unpinned);
    failed.status = SessionStatus::Failed;
    let sessions = vec![pinned_session(pinned_top, 300), failed];
    let rows = vec![
        SidebarRow::Session(pinned_top),
        SidebarRow::Session(unpinned),
    ];
    assert_eq!(
        next_idle_session(
            &sessions,
            &rows,
            Some(pinned_top),
            None,
            &HashSet::new(),
            None,
            None,
            None
        ),
        Some(unpinned)
    );
    assert_eq!(
        next_idle_session(
            &sessions,
            &rows,
            Some(unpinned),
            None,
            &HashSet::new(),
            None,
            None,
            None
        ),
        Some(pinned_top)
    );
}

/// A session skeleton bound to a project, as the session list holds them.
fn project_session(id: Uuid, project_id: Uuid) -> AgentSession {
    let mut session = AgentSession::new(project_id, ProviderKind::Codex);
    session.id = id;
    session.detail_loaded = false;
    session
}

fn project(project_id: Uuid, starred: bool) -> Project {
    let mut project = Project::from_path(std::path::PathBuf::from("/tmp/waku-test"));
    project.id = project_id;
    project.starred = starred;
    project
}

#[test]
fn next_attention_target_ranks_unread_above_starred_idle() {
    let starred_project = Uuid::new_v4();
    let plain_project = Uuid::new_v4();
    let projects = vec![
        project(starred_project, true),
        project(plain_project, false),
    ];
    let starred_idle = Uuid::new_v4();
    let starred_unseen = Uuid::new_v4();
    let plain_unseen = Uuid::new_v4();
    let sessions = vec![
        project_session(plain_unseen, plain_project),
        project_session(starred_idle, starred_project),
        project_session(starred_unseen, starred_project),
    ];
    // The unstarred unread sits at the top of the sidebar, then the starred
    // project's seen idle task, then its unseen one.
    let rows = vec![
        SidebarRow::Session(plain_unseen),
        SidebarRow::Session(starred_idle),
        SidebarRow::Session(starred_unseen),
    ];
    let unseen = HashMap::from([(plain_unseen, 100), (starred_unseen, 200)]);

    // Starred unseen beats everything.
    assert_eq!(
        next_attention_target(
            &sessions,
            &projects,
            &unseen,
            &rows,
            None,
            None,
            &HashSet::new(),
            None,
            None
        ),
        Some(starred_unseen)
    );
    // Genuinely new activity outranks the starred project's already-seen
    // idle task — the star leads inside each attention tier, not above it.
    let only_plain_unseen = HashMap::from([(plain_unseen, 100)]);
    assert_eq!(
        next_attention_target(
            &sessions,
            &projects,
            &only_plain_unseen,
            &rows,
            None,
            None,
            &HashSet::new(),
            None,
            None
        ),
        Some(plain_unseen)
    );
    // Once the unseen queue is drained the starred project leads the idle
    // rotation.
    assert_eq!(
        next_attention_target(
            &sessions,
            &projects,
            &HashMap::new(),
            &rows,
            None,
            None,
            &HashSet::new(),
            None,
            None
        ),
        Some(starred_idle)
    );
    // With the starred project's sessions gone the unstarred unread leads.
    let starred_rows = vec![SidebarRow::Session(plain_unseen)];
    let only_plain = vec![project_session(plain_unseen, plain_project)];
    assert_eq!(
        next_attention_target(
            &only_plain,
            &projects,
            &unseen,
            &starred_rows,
            None,
            None,
            &HashSet::new(),
            None,
            None
        ),
        Some(plain_unseen)
    );
    // Nothing starred: today's unread-then-idle order is unchanged.
    let unstarred = vec![
        project(starred_project, false),
        project(plain_project, false),
    ];
    assert_eq!(
        next_attention_target(
            &sessions,
            &unstarred,
            &unseen,
            &rows,
            None,
            None,
            &HashSet::new(),
            None,
            None
        ),
        Some(plain_unseen)
    );
}

#[test]
fn next_attention_target_sweep_skips_shown_idle_but_never_unread() {
    let busy = Uuid::new_v4();
    let first = Uuid::new_v4();
    let second = Uuid::new_v4();
    let mut busy_session = started_session(busy);
    busy_session.status = SessionStatus::Working;
    let mut sessions = vec![
        busy_session,
        started_session(first),
        started_session(second),
    ];
    let rows = vec![
        SidebarRow::Session(busy),
        SidebarRow::Session(first),
        SidebarRow::Session(second),
    ];
    let unseen = HashMap::new();

    // A busy current session enters the rotation at the top, but the ⌘D
    // sweep's seen set carries the walk past the row it just showed —
    // repeated presses visit each non-busy task once.
    let visited = HashSet::from([first]);
    assert_eq!(
        next_attention_target(
            &sessions,
            &[],
            &unseen,
            &rows,
            Some(busy),
            None,
            &HashSet::new(),
            None,
            Some(&visited)
        ),
        Some(second)
    );
    // The whole rotation shown: no target, so the caller restarts the
    // sweep clean and lets the plain rotation answer.
    let visited = HashSet::from([first, second]);
    assert_eq!(
        next_attention_target(
            &sessions,
            &[],
            &unseen,
            &rows,
            Some(busy),
            None,
            &HashSet::new(),
            None,
            Some(&visited)
        ),
        None
    );
    assert_eq!(
        next_attention_target(
            &sessions,
            &[],
            &unseen,
            &rows,
            Some(busy),
            None,
            &HashSet::new(),
            None,
            None
        ),
        Some(first)
    );
    // A session the sweep already showed still leads the moment it carries
    // fresh attention — the seen set only filters the idle rotation.
    let unseen = HashMap::from([(first, 100)]);
    assert_eq!(
        next_attention_target(
            &sessions,
            &[],
            &unseen,
            &rows,
            Some(busy),
            None,
            &HashSet::new(),
            None,
            Some(&visited)
        ),
        Some(first)
    );
    // A task blocked on its user likewise outranks the filter — it is still
    // waiting for an answer.
    sessions[1].status = SessionStatus::Waiting;
    assert_eq!(
        next_attention_target(
            &sessions,
            &[],
            &HashMap::new(),
            &rows,
            Some(busy),
            None,
            &HashSet::new(),
            None,
            Some(&visited)
        ),
        Some(first)
    );
}

#[test]
fn next_unread_completion_scopes_to_a_starred_tier() {
    let starred_project = Uuid::new_v4();
    let plain_project = Uuid::new_v4();
    let starred_set = HashSet::from([starred_project]);
    let starred_id = Uuid::new_v4();
    let plain_id = Uuid::new_v4();
    let sessions = vec![
        project_session(plain_id, plain_project),
        project_session(starred_id, starred_project),
    ];
    let rows = vec![
        SidebarRow::Session(plain_id),
        SidebarRow::Session(starred_id),
    ];
    let unseen = HashMap::from([(plain_id, 100), (starred_id, 200)]);

    // The starred tier ignores a higher unstarred row; the unstarred tier
    // returns it; no tier keeps the original topmost-unread answer.
    assert_eq!(
        next_unread_completion(
            &sessions,
            &unseen,
            &rows,
            None,
            None,
            &HashSet::new(),
            None,
            Some((&starred_set, true)),
        ),
        Some(starred_id)
    );
    assert_eq!(
        next_unread_completion(
            &sessions,
            &unseen,
            &rows,
            None,
            None,
            &HashSet::new(),
            None,
            Some((&starred_set, false)),
        ),
        Some(plain_id)
    );
    assert_eq!(
        next_unread_completion(
            &sessions,
            &unseen,
            &rows,
            None,
            None,
            &HashSet::new(),
            None,
            None
        ),
        Some(plain_id)
    );
}

#[test]
fn dormant_sessions_are_never_keyboard_jump_targets() {
    let shelved = Uuid::new_v4();
    let live = Uuid::new_v4();
    let sessions = vec![started_session(shelved), started_session(live)];
    // A revealed dormant row sits in the list like any other — even
    // topmost, even holding an unread stamp — and is still skipped.
    let rows = vec![SidebarRow::Session(shelved), SidebarRow::Session(live)];
    let dormant = HashSet::from([shelved]);
    let unseen = HashMap::from([(shelved, 300)]);

    assert_eq!(
        next_unread_completion(&sessions, &unseen, &rows, None, None, &dormant, None, None),
        None
    );
    assert_eq!(
        next_idle_session(&sessions, &rows, None, None, &dormant, None, None, None),
        Some(live)
    );
    assert_eq!(
        next_attention_target(
            &sessions,
            &[],
            &unseen,
            &rows,
            None,
            None,
            &dormant,
            None,
            None
        ),
        Some(live)
    );
    // An all-dormant list has no target — the caller lands on New task.
    let all_dormant = HashSet::from([shelved, live]);
    assert_eq!(
        next_attention_target(
            &sessions,
            &[],
            &unseen,
            &rows,
            None,
            None,
            &all_dormant,
            None,
            None
        ),
        None
    );
}

#[test]
fn dormant_session_ids_marks_swept_and_stale_sessions() {
    let swept = Uuid::new_v4();
    let live = Uuid::new_v4();
    let mut swept_session = started_session(swept);
    // An explicit sweep stays dormant until a mutation overtakes it.
    swept_session.dormant_at = Some(u64::MAX);
    let sessions = vec![swept_session, started_session(live)];

    // The swept session is dormant even with the auto-dormancy threshold
    // off; the untouched session is not.
    let dormant = dormant_session_ids(&sessions, None);
    assert!(dormant.contains(&swept));
    assert!(!dormant.contains(&live));

    // Staleness follows the configured threshold: a reply older than the
    // window is dormant, a fresh one is not.
    let mut stale = started_session(live);
    stale.last_reply_at = Some(unix_time().saturating_sub(8 * 86_400));
    let dormant = dormant_session_ids(&[stale], Some(7));
    assert!(dormant.contains(&live));
}

#[test]
fn task_notification_tags_route_to_the_corresponding_task() {
    let session_id = Uuid::new_v4();
    let tag = task_notification_tag(session_id);

    assert_eq!(task_id_from_notification_tag(&tag), Some(session_id));
    assert_eq!(task_id_from_notification_tag("waku-task:not-a-uuid"), None);
    assert_eq!(task_id_from_notification_tag(&session_id.to_string()), None);
}

#[test]
fn prompt_answers_normalize_to_the_button_index() {
    assert_eq!(prompt_answer_index(0), 0);
    assert_eq!(prompt_answer_index(1), 1);
    // macOS resolves with `NSAlertFirstButtonReturn + index`.
    assert_eq!(prompt_answer_index(1000), 0);
    assert_eq!(prompt_answer_index(1001), 1);
}

#[test]
fn conversation_navigation_rail_visibility_uses_all_three_gates() {
    assert!(should_show_navigation_rail(true, 2, 872.0));
    assert!(!should_show_navigation_rail(false, 2, 872.0));
    assert!(!should_show_navigation_rail(true, 1, 872.0));
    assert!(!should_show_navigation_rail(true, 2, 871.0));
}

#[test]
fn conversation_navigation_rail_height_caps_at_eighty_percent() {
    assert_eq!(navigation_rail_height(10, 600.0), 120.0);
    // Every turn keeps its full 12px scroll position; only the viewport is
    // capped, so the remaining turns are reached by scrolling the rail.
    assert_eq!(navigation_rail_height(100, 600.0), 480.0);
    assert!(navigation_rail_height(100, 600.0) <= 600.0 * 0.80);
    assert_eq!(
        NAVIGATION_RAIL_TURN_HEIGHT - NAVIGATION_RAIL_TICK_HEIGHT,
        10.0
    );
}

#[test]
fn conversation_navigation_rail_fades_point_toward_hidden_turns() {
    assert_eq!(
        navigation_rail_fade_visibility(px(0.0), px(120.0)),
        (false, true)
    );
    assert_eq!(
        navigation_rail_fade_visibility(px(-40.0), px(120.0)),
        (true, true)
    );
    assert_eq!(
        navigation_rail_fade_visibility(px(-120.0), px(120.0)),
        (true, false)
    );
    assert_eq!(
        navigation_rail_fade_visibility(px(0.0), px(0.0)),
        (false, false)
    );
}

#[test]
fn conversation_navigation_tick_scale_follows_hover_falloff() {
    assert_eq!(navigation_rail_scale(0, None), 0.25);
    assert_eq!(navigation_rail_scale(4, Some(4)), 1.0);
    assert_eq!(navigation_rail_scale(3, Some(4)), 0.68);
    assert_eq!(navigation_rail_scale(2, Some(4)), 0.44);
    assert_eq!(navigation_rail_scale(1, Some(4)), 0.25);
}

#[test]
fn conversation_navigation_active_turn_follows_the_scroll_top_and_tail() {
    let turn_rows = [0, 4, 9];
    assert_eq!(active_navigation_turn_index(&turn_rows, 0, false), Some(0));
    assert_eq!(active_navigation_turn_index(&turn_rows, 3, false), Some(0));
    assert_eq!(active_navigation_turn_index(&turn_rows, 4, false), Some(1));
    assert_eq!(active_navigation_turn_index(&turn_rows, 8, false), Some(1));
    assert_eq!(active_navigation_turn_index(&turn_rows, 4, true), Some(2));
    assert_eq!(active_navigation_turn_index(&[], 0, false), None);
}

#[test]
fn conversation_navigation_turn_stepping_walks_boundaries() {
    let turn_rows = [2, 5, 9];
    // Parked exactly on a prompt, previous steps past it.
    assert_eq!(previous_navigation_turn_index(&turn_rows, 5, true), Some(0));
    // Inside a turn, previous lands on that turn's own prompt first —
    // whether the scroll top sits mid-turn or a few pixels into the row
    // that opens it.
    assert_eq!(previous_navigation_turn_index(&turn_rows, 7, true), Some(1));
    assert_eq!(
        previous_navigation_turn_index(&turn_rows, 5, false),
        Some(1)
    );
    // Above the first prompt or on it, previous clamps to the first turn.
    assert_eq!(previous_navigation_turn_index(&turn_rows, 0, true), Some(0));
    assert_eq!(previous_navigation_turn_index(&turn_rows, 2, true), Some(0));
    // From the tail, previous lands on the last prompt.
    assert_eq!(
        previous_navigation_turn_index(&turn_rows, 12, true),
        Some(2)
    );
    assert_eq!(previous_navigation_turn_index(&[], 0, true), None);

    // Next always moves strictly forward, even parked on a boundary.
    assert_eq!(next_navigation_turn_index(&turn_rows, 0), Some(0));
    assert_eq!(next_navigation_turn_index(&turn_rows, 2), Some(1));
    assert_eq!(next_navigation_turn_index(&turn_rows, 7), Some(2));
    // On or past the last prompt there is no next turn; the action
    // re-pins the tail instead.
    assert_eq!(next_navigation_turn_index(&turn_rows, 9), None);
    assert_eq!(next_navigation_turn_index(&turn_rows, 30), None);
    assert_eq!(next_navigation_turn_index(&[], 0), None);
}

#[test]
fn conversation_navigation_preview_uses_each_prompt_and_latest_response() {
    let project_id = Uuid::new_v4();
    let mut session = AgentSession::new(project_id, ProviderKind::Codex);
    session.begin_turn("  First\n\nprompt  ");
    session.push_message(MessageRole::Assistant, "Interim update");
    session.push_message(MessageRole::Assistant, "Final answer");
    session.finish_active_turn(TurnStatus::Completed);
    session.begin_turn("Second prompt");
    let rows = [Message(0), Message(1), Message(2), Message(3)];

    let turns = transcript_navigation_turns(&session, &rows);
    assert_eq!(turns.len(), 2);
    assert_eq!(turns[0].message_index, 0);
    assert_eq!(turns[0].row_index, 0);
    assert_eq!(turns[0].prompt, "First prompt");
    assert_eq!(turns[0].response, "Final answer");
    assert_eq!(turns[1].row_index, 3);
    assert!(turns[1].response.is_empty());
    assert_eq!(
        navigation_preview_snippet("one   two\nthree", 20),
        "one two three"
    );
}

#[test]
fn conversation_navigation_preview_does_not_change_during_a_running_turn() {
    let project_id = Uuid::new_v4();
    let mut session = AgentSession::new(project_id, ProviderKind::Codex);
    let session_id = session.id;
    session.begin_turn("Streaming prompt");
    append_text_delta_to_session(
        std::slice::from_mut(&mut session),
        session_id,
        false,
        "Partial".to_owned(),
    );
    let rows = [Message(0), Message(1)];
    let before = transcript_navigation_turns(&session, &rows);

    append_text_delta_to_session(
        std::slice::from_mut(&mut session),
        session_id,
        true,
        " response".to_owned(),
    );
    let during = transcript_navigation_turns(&session, &rows);

    assert_eq!(before, during);
    assert_eq!(during[0].response, "");

    session.finish_active_turn(TurnStatus::Completed);
    let completed = transcript_navigation_turns(&session, &rows);
    assert_eq!(completed[0].response, "Partial response");
}

#[test]
fn panel_widths_preserve_main_content_when_the_window_narrows() {
    let (sidebar, right_panel) = fitted_panel_widths(980.0, true, true, 420.0, 720.0);

    assert_eq!(sidebar, 340.0);
    assert_eq!(right_panel, 280.0);
    assert_eq!(980.0 - sidebar - right_panel, 360.0);
}

#[test]
fn hidden_panels_do_not_consume_layout_width() {
    let (sidebar, right_panel) = fitted_panel_widths(980.0, false, true, 420.0, 720.0);

    assert_eq!(sidebar, 0.0);
    assert_eq!(right_panel, 620.0);
}

#[test]
fn file_tree_width_preserves_a_usable_editor() {
    assert_eq!(fitted_file_tree_width(460.0, 184.0), 184.0);
    assert_eq!(fitted_file_tree_width(460.0, 400.0), 320.0);
    assert_eq!(fitted_file_tree_width(280.0, 184.0), 140.0);
    assert_eq!(fitted_file_tree_width(280.0, f32::NAN), 140.0);
}

#[test]
fn first_file_editor_opening_reserves_500_pixels() {
    assert_eq!(widened_panel_width_for_file_editor(460.0, 184.0), 684.0);
    assert_eq!(widened_panel_width_for_file_editor(720.0, 184.0), 720.0);
    assert_eq!(widened_panel_width_for_file_editor(460.0, 360.0), 860.0);
}

#[test]
fn first_review_opening_reserves_diff_and_tree_space() {
    assert_eq!(widened_panel_width_for_review(460.0), 820.0);
    assert_eq!(widened_panel_width_for_review(920.0), 920.0);
}

#[test]
fn anchor_end_space_keeps_a_short_new_turn_at_the_viewport_top() {
    assert_eq!(
        transcript_anchor_end_space(gpui::px(700.0), gpui::px(180.0)),
        gpui::px(520.0)
    );
    assert_eq!(
        transcript_anchor_end_space(gpui::px(700.0), gpui::px(900.0)),
        gpui::px(0.0)
    );
}

#[test]
fn scroll_to_bottom_only_appears_while_the_tail_is_below_the_viewport() {
    let viewport_bottom = px(700.0);

    assert_eq!(
        should_show_scroll_to_bottom(false, false, true, viewport_bottom, None, Pixels::ZERO),
        Some(false)
    );
    assert_eq!(
        should_show_scroll_to_bottom(
            true,
            true,
            true,
            viewport_bottom,
            Some(px(900.0)),
            Pixels::ZERO,
        ),
        Some(false)
    );
    // Disclosure pinning keeps `is_scrolled` true and a splice can leave the
    // tail temporarily unmeasured, but a collapsed transcript that fits the
    // viewport has nowhere to scroll back to.
    assert_eq!(
        should_show_scroll_to_bottom(true, false, false, viewport_bottom, None, Pixels::ZERO),
        Some(false)
    );
    assert_eq!(
        should_show_scroll_to_bottom(
            true,
            false,
            true,
            viewport_bottom,
            Some(px(701.0)),
            Pixels::ZERO,
        ),
        Some(true)
    );
    assert_eq!(
        should_show_scroll_to_bottom(
            true,
            false,
            true,
            viewport_bottom,
            Some(px(500.0)),
            px(200.0),
        ),
        Some(false)
    );
    // A stream commit remeasures the tail rows, so the frame after each one has
    // no bounds to read. Answering "show" there strobes the button against the
    // measured frames between commits; the caller holds its last answer instead.
    assert_eq!(
        should_show_scroll_to_bottom(true, false, true, viewport_bottom, None, Pixels::ZERO),
        None
    );
}

#[test]
fn scrolling_back_onto_the_tail_is_told_apart_from_an_unmeasured_tail() {
    let viewport_bottom = px(700.0);

    // Landed on the tail: the reply's last row ends at the viewport bottom,
    // with or without the anchor's reserved end space below it.
    assert_eq!(
        transcript_rests_at_tail(viewport_bottom, Some(px(700.0)), Pixels::ZERO),
        Some(true)
    );
    assert_eq!(
        transcript_rests_at_tail(viewport_bottom, Some(px(500.0)), px(200.0)),
        Some(true)
    );
    // Stopped short of it, so the stream must not reclaim the view.
    assert_eq!(
        transcript_rests_at_tail(viewport_bottom, Some(px(701.0)), Pixels::ZERO),
        Some(false)
    );
    // Unmeasured this frame: unknown, not "stopped short" — concluding the
    // latter would drop the re-engage whenever a scroll settles on a commit.
    assert_eq!(
        transcript_rests_at_tail(viewport_bottom, None, Pixels::ZERO),
        None
    );
}

#[test]
fn pending_expansion_reasserts_the_user_message_anchor() {
    let rows = gpui::ListState::new(3, gpui::ListAlignment::Bottom, gpui::px(0.0));
    rows.scroll_to(gpui::ListOffset {
        item_ix: 0,
        offset_in_item: gpui::px(42.0),
    });

    assert!(maintain_transcript_anchor(&rows, 0, true, gpui::px(320.0),));
    let anchored = rows.logical_scroll_top();
    assert_eq!(anchored.item_ix, 0);
    assert_eq!(anchored.offset_in_item, gpui::Pixels::ZERO);
    assert!(!maintain_transcript_anchor(
        &rows,
        0,
        true,
        gpui::Pixels::ZERO,
    ));
}

#[test]
fn settling_an_anchored_turn_splices_without_resetting_its_prompt() {
    let mut session = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
    session.begin_turn("hi");
    session.push_message(MessageRole::Assistant, "Hello.");
    session.finish_active_turn(TurnStatus::Completed);

    let turn_id = session.begin_turn("give me a quick overview");
    session.status = SessionStatus::Working;
    session.transcript_blocks.push(TranscriptBlock {
        after_message: session.messages.len(),
        turn_id: Some(turn_id),
        activities: vec![ActivityItem::from_reasoning(
            ReasoningBlock {
                content: "Inspecting the project".into(),
                started_at_ms: 1_000,
                finished_at_ms: 2_000,
            },
            true,
        )],
    });
    session.push_message(MessageRole::Assistant, "Here is the overview.");

    let running = folded_transcript_row_kinds(&session, &HashSet::new(), None);
    let anchor_row = running
        .iter()
        .position(|kind| *kind == Message(2))
        .expect("the second prompt is visible");
    let rows = ListState::new(running.len(), ListAlignment::Top, px(2048.0));
    rows.scroll_to(gpui::ListOffset {
        item_ix: anchor_row,
        offset_in_item: Pixels::ZERO,
    });

    session.status = SessionStatus::Idle;
    session.finish_active_turn(TurnStatus::Completed);
    let settled = folded_transcript_row_kinds(&session, &HashSet::new(), None);
    let (range, new_count) = transcript_row_splice(&running, &settled)
        .expect("settlement folds the live work and removes its working row");

    assert!(
        range.start > anchor_row,
        "only rows after the anchored prompt should be folded"
    );
    rows.splice(range, new_count);
    assert_eq!(rows.item_count(), settled.len());
    let retained_anchor = rows.logical_scroll_top();
    assert_eq!(
        retained_anchor.item_ix, anchor_row,
        "an exact settlement splice must retain the sent-row anchor"
    );
    assert_eq!(retained_anchor.offset_in_item, Pixels::ZERO);
}

#[test]
fn only_later_user_messages_start_followup_turns() {
    let messages = vec![
        Message::new(MessageRole::User, "first"),
        Message::new(MessageRole::Assistant, "answer"),
        Message::new(MessageRole::User, "follow-up"),
        Message::new(MessageRole::Assistant, "answer"),
    ];
    assert!(!message_starts_followup_turn(&messages, 0));
    assert!(!message_starts_followup_turn(&messages, 1));
    assert!(message_starts_followup_turn(&messages, 2));
    assert!(!message_starts_followup_turn(&messages, 3));
}

#[test]
fn a_prompt_directly_behind_another_skips_the_followup_gap() {
    let mut session = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
    session.begin_turn("first prompt");
    // A steer the provider folded into the live turn lands directly behind
    // the prompt that opened it.
    session.push_user_message_with_presentation(
        "actually, also this",
        None,
        Vec::new(),
        Vec::new(),
        None,
    );
    session.push_message(MessageRole::Assistant, "answer");
    session.finish_active_turn(TurnStatus::Completed);
    session.begin_turn("second prompt");

    let rows = folded_transcript_row_kinds(&session, &HashSet::new(), None);
    let row_index = |message_index: usize| {
        rows.iter()
            .position(|kind| *kind == Message(message_index))
            .unwrap()
    };

    assert!(!row_starts_followup_turn(&session, &rows, 0));
    assert!(
        !row_starts_followup_turn(&session, &rows, row_index(1)),
        "two consecutive prompts cluster — no row between them to separate"
    );
    assert!(
        row_starts_followup_turn(&session, &rows, row_index(3)),
        "a prompt behind a settled response still opens the turn gap"
    );
}

#[test]
fn only_the_turn_opening_prompt_is_a_rewind_boundary() {
    let mut session = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
    session.begin_turn("first prompt");
    session.push_message(MessageRole::Assistant, "working on it");
    // A steer the provider folded into the live turn.
    session.push_user_message_with_presentation(
        "actually, also this",
        None,
        Vec::new(),
        Vec::new(),
        None,
    );
    session.push_message(MessageRole::Assistant, "answer");
    session.finish_active_turn(TurnStatus::Interrupted);
    session.begin_turn("second prompt");

    assert!(message_opens_turn(&session.messages, 0));
    assert!(!message_opens_turn(&session.messages, 1));
    assert!(
        !message_opens_turn(&session.messages, 2),
        "a steer shares the turn's checkpoint, so it cannot be rewound to on its own"
    );
    assert!(!message_opens_turn(&session.messages, 3));
    assert!(message_opens_turn(&session.messages, 4));
}

/// A steer lands mid-turn behind whatever was still streaming, which drops
/// that block out of the live tail. Its header then comes from the
/// summary, so the aborted command and thinking have to read "Ran" — an
/// unsettled `complete` flag would leave the collapsed group claiming the
/// work still runs for the rest of the turn.
#[test]
fn an_accepted_steer_settles_the_stream_segment_its_message_cuts_off() {
    let mut session = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
    session.begin_turn("Build it");
    session.status = SessionStatus::Working;
    push_transcript_activity(
        &mut session,
        ActivityItem::from_reasoning(
            ReasoningBlock {
                content: "Inspecting history".into(),
                started_at_ms: 1_000,
                finished_at_ms: 2_000,
            },
            false,
        ),
        false,
    );
    push_transcript_activity(
        &mut session,
        ActivityItem::new(None, ActivityKind::Command, "Run tests", None, false),
        true,
    );
    session.push_message(MessageRole::Assistant, "Working on it");
    session.messages.last_mut().unwrap().streaming = true;

    assert_eq!(
        activity_header_title(&session.transcript_blocks[0].activities, false, None),
        "Running 1 thought · 1 command"
    );

    // `SteerAccepted` settles the segment, then appends the folded-in
    // message to the running turn.
    settle_stream_segment(&mut session);
    session.push_user_message_with_presentation(
        "actually, also this",
        None,
        Vec::new(),
        Vec::new(),
        None,
    );

    let block = &session.transcript_blocks[0];
    assert!(block.activities.iter().all(|activity| activity.complete));
    assert!(!session.messages[1].streaming);
    assert!(!activity_group_is_live(
        session.active_turn_id() == block.turn_id,
        true,
        block.after_message,
        session.messages.len(),
    ));
    assert_eq!(
        activity_header_title(&block.activities, false, None),
        "Ran 1 thought · 1 command"
    );
}

#[test]
fn fenced_code_collects_all_blocks_without_languages() {
    let markdown = "Before\n```rust\nfn main() {}\n```\nAfter\n```\ncargo test\n```";
    assert_eq!(
        fenced_code(markdown).as_deref(),
        Some("fn main() {}\n\ncargo test")
    );
    assert_eq!(fenced_code("No code here"), None);
}

#[test]
fn stream_batches_commit_full_adjacent_text_and_preserve_event_order() {
    let runtime_id = Uuid::new_v4();
    let epoch = Uuid::new_v4();
    let mut events = VecDeque::from([
        DriverEvent::TextDelta("first ".into()),
        DriverEvent::RuntimeEventCursorAdvanced(RuntimeEventCursor {
            runtime_id,
            epoch,
            sequence: 1,
        }),
        DriverEvent::TextDelta("line\nsecond line".into()),
        DriverEvent::RuntimeEventCursorAdvanced(RuntimeEventCursor {
            runtime_id,
            epoch,
            sequence: 2,
        }),
        DriverEvent::Activity {
            id: None,
            kind: ActivityKind::Tool,
            title: "Tool".into(),
            detail: None,
            complete: true,
        },
        DriverEvent::TextDelta("after tool".into()),
    ]);

    assert!(matches!(
        pop_stream_batch(&mut events, StreamDeltaKind::Text, &mut 0),
        Some(DriverEvent::TextDelta(text)) if text == "first line\nsecond line"
    ));
    assert!(matches!(
        events.pop_front(),
        Some(DriverEvent::RuntimeEventCursorAdvanced(cursor)) if cursor.sequence == 2
    ));
    assert!(matches!(events.front(), Some(DriverEvent::Activity { .. })));
    assert!(matches!(
        events.get(1),
        Some(DriverEvent::TextDelta(text)) if text == "after tool"
    ));
}

#[test]
fn stream_parts_keep_targeting_the_running_session_after_selection_changes() {
    let project_id = uuid::Uuid::new_v4();
    let mut running = AgentSession::new(project_id, ProviderKind::Codex);
    running.begin_turn("background task");
    running.status = SessionStatus::Working;
    let running_id = running.id;
    let visible = AgentSession::new(project_id, ProviderKind::Claude);
    let visible_id = visible.id;
    let mut sessions = vec![running, visible];

    append_text_delta_to_session(&mut sessions, running_id, false, "first".into());
    // Navigation changes only which task is rendered. The runtime keeps
    // emitting with its own task ID while another task is visible.
    let selected_session = visible_id;
    append_text_delta_to_session(&mut sessions, running_id, true, " second".into());

    assert_eq!(selected_session, visible_id);
    assert_eq!(sessions[0].messages[1].content, "first second");
    assert!(sessions[0].messages[1].streaming);
    assert!(sessions[1].messages.is_empty());
}

#[test]
fn an_activity_update_rewrites_its_row_without_opening_a_new_one() {
    let mut session = AgentSession::new(Uuid::new_v4(), ProviderKind::Devin);
    session.begin_turn("Rebase");
    push_transcript_activity(
        &mut session,
        ActivityItem::new(
            Some("call-1".to_owned()),
            ActivityKind::Tool,
            "Read file",
            None,
            false,
        ),
        false,
    );

    // A status heartbeat for the row already on screen.
    let heartbeat = ActivityItem::new(
        Some("call-1".to_owned()),
        ActivityKind::Tool,
        "Read file",
        Some("still running".into()),
        true,
    );
    let (activity_id, _) = update_transcript_activity(&mut session, heartbeat)
        .expect("the heartbeat matches the pushed row");

    assert_eq!(session.transcript_blocks.len(), 1);
    assert_eq!(session.transcript_blocks[0].activities.len(), 1);
    let activity = &session.transcript_blocks[0].activities[0];
    assert_eq!(activity.id, activity_id);
    assert!(activity.complete);
    assert_eq!(activity.detail.as_deref(), Some("still running"));

    // An update for work that never pushed a row comes back for a fresh push.
    let unrelated = ActivityItem::new(
        Some("call-2".to_owned()),
        ActivityKind::Tool,
        "Edit file",
        None,
        false,
    );
    assert!(update_transcript_activity(&mut session, unrelated).is_err());
}

#[test]
fn a_text_delta_rejoins_the_message_an_interleaved_activity_cut_off() {
    let project_id = Uuid::new_v4();
    let mut session = AgentSession::new(project_id, ProviderKind::Devin);
    session.begin_turn("Rebase");
    let session_id = session.id;
    let mut sessions = vec![session];

    append_text_delta_to_session(
        &mut sessions,
        session_id,
        false,
        "All conflicts are adjacent".into(),
    );
    // The provider dispatched a tool mid-sentence; its own transcript keeps
    // the narration as one message.
    push_transcript_activity(
        &mut sessions[0],
        ActivityItem::new(
            Some("call-1".to_owned()),
            ActivityKind::Tool,
            "Read file",
            None,
            false,
        ),
        false,
    );
    append_text_delta_to_session(
        &mut sessions,
        session_id,
        false,
        " additions — union resolutions".into(),
    );

    assert_eq!(sessions[0].messages.len(), 2);
    assert_eq!(
        sessions[0].messages[1].content,
        "All conflicts are adjacent additions — union resolutions"
    );
    assert!(sessions[0].messages[1].streaming);
}

#[test]
fn a_text_delta_opens_a_fresh_row_after_a_completed_sentence() {
    let project_id = Uuid::new_v4();
    let mut session = AgentSession::new(project_id, ProviderKind::Devin);
    session.begin_turn("Rebase");
    let session_id = session.id;
    let mut sessions = vec![session];

    append_text_delta_to_session(
        &mut sessions,
        session_id,
        false,
        "The rebase is done.".into(),
    );
    push_transcript_activity(
        &mut sessions[0],
        ActivityItem::new(None, ActivityKind::Command, "Ran tests", None, true),
        false,
    );
    append_text_delta_to_session(
        &mut sessions,
        session_id,
        false,
        "Everything passed.".into(),
    );

    assert_eq!(sessions[0].messages.len(), 3);
    assert_eq!(sessions[0].messages[1].content, "The rebase is done.");
    assert_eq!(sessions[0].messages[2].content, "Everything passed.");
}

#[test]
fn a_text_delta_does_not_rejoin_across_turns_or_notices() {
    let project_id = Uuid::new_v4();
    let mut session = AgentSession::new(project_id, ProviderKind::Devin);
    session.begin_turn("Rebase");
    let session_id = session.id;
    let mut sessions = vec![session];

    // A delta that reads as a fresh sentence keeps its own row even when the
    // previous message ended without terminal punctuation.
    append_text_delta_to_session(
        &mut sessions,
        session_id,
        false,
        "Let me check the file".into(),
    );
    push_transcript_activity(
        &mut sessions[0],
        ActivityItem::new(None, ActivityKind::Command, "Ran ls", None, true),
        false,
    );
    append_text_delta_to_session(&mut sessions, session_id, false, "Now I see it.".into());
    assert_eq!(sessions[0].messages.len(), 3);
    assert_eq!(sessions[0].messages[1].content, "Let me check the file");
    assert_eq!(sessions[0].messages[2].content, "Now I see it.");

    // Once the turn settles, nothing stitches new text onto it.
    sessions[0].finish_active_turn(TurnStatus::Completed);
    append_text_delta_to_session(&mut sessions, session_id, false, "stray tail".into());
    assert_eq!(sessions[0].messages.len(), 4);
    assert_eq!(sessions[0].messages[3].content, "stray tail");
    assert!(sessions[0].messages[3].turn_id.is_none());
}

#[test]
fn reasoning_and_tools_share_one_ordered_activity_block() {
    let mut session = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
    session.begin_turn("Build it");

    push_transcript_activity(
        &mut session,
        ActivityItem::from_reasoning(
            ReasoningBlock {
                content: "Inspecting the project".into(),
                started_at_ms: 1_000,
                finished_at_ms: 2_000,
            },
            true,
        ),
        false,
    );
    push_transcript_activity(
        &mut session,
        ActivityItem::new(None, ActivityKind::Command, "Ran tests", None, false),
        true,
    );

    assert_eq!(session.transcript_blocks.len(), 1);
    assert_eq!(session.transcript_blocks[0].activities.len(), 2);
    assert_eq!(
        session.transcript_blocks[0]
            .activities
            .iter()
            .map(|activity| activity.kind)
            .collect::<Vec<_>>(),
        [ActivityKind::Reasoning, ActivityKind::Command]
    );

    session.push_message(MessageRole::Assistant, "Interim update");
    push_transcript_activity(
        &mut session,
        ActivityItem::from_reasoning(
            ReasoningBlock {
                content: "Checking the result".into(),
                started_at_ms: 3_000,
                finished_at_ms: 4_000,
            },
            false,
        ),
        false,
    );
    assert_eq!(
        session.transcript_blocks.len(),
        2,
        "assistant text keeps later work at its own transcript position"
    );
}

#[test]
fn line_delimited_reasoning_deltas_join_as_prose() {
    let fold = |deltas: &[&str]| {
        let mut content = String::new();
        let mut pending = 0;
        for delta in deltas {
            push_reasoning_delta(&mut content, &mut pending, delta);
        }
        content
    };

    // GLM via OpenRouter alternates a text chunk with a newline-only chunk;
    // spacing lives inside the text chunks, so the newlines drop out — even a
    // mid-word split ("upstream" + "/main") rejoins cleanly.
    assert_eq!(
        fold(&[
            "The",
            "\n",
            " tracking",
            "\n",
            " got",
            "\n",
            " set",
            "\n",
            " to",
            "\n",
            " upstream",
            "\n",
            "/main",
            "\n",
        ]),
        "The tracking got set to upstream/main"
    );

    // A run of newline-only chunks collapses to one paragraph break instead
    // of accumulating blank lines, and a trailing run goes away entirely.
    assert_eq!(
        fold(&["first", "\n", "\n", "\n", "second", "\n", "\n"]),
        "first\n\nsecond"
    );

    // Newlines inside a real chunk are kept, and a whitespace delta is still
    // real spacing rather than a collapsible break.
    assert_eq!(
        fold(&["one\ntwo", "\n", " three", " ", "four"]),
        "one\ntwo three four"
    );

    // A run at the head of a batch folds to a leading paragraph break;
    // `append_reasoning_delta` trims it when it would open a fresh block.
    assert_eq!(fold(&["\n", "\n", "text"]), "\n\ntext");
}

#[test]
fn idle_reaping_releases_finished_sessions_but_never_a_running_turn() {
    let project_id = uuid::Uuid::new_v4();
    let fresh = Duration::from_secs(60);
    let stale = Duration::from_secs(60 * 60);

    let idle = AgentSession::new(project_id, ProviderKind::Codex);
    assert!(session_is_reapable(Some(&idle), stale, false));
    assert!(!session_is_reapable(Some(&idle), fresh, false));
    assert!(!session_is_reapable(Some(&idle), stale, true));

    let mut working = AgentSession::new(project_id, ProviderKind::Codex);
    working.begin_turn("a long tool call");
    working.status = SessionStatus::Working;
    assert!(!session_is_reapable(Some(&working), stale, false));

    // An approval can sit unanswered far longer than the idle window; its agent
    // is blocked on the user, not abandoned.
    let mut waiting = AgentSession::new(project_id, ProviderKind::Codex);
    waiting.begin_turn("needs approval");
    waiting.status = SessionStatus::Waiting;
    assert!(!session_is_reapable(Some(&waiting), stale, false));

    let mut failed = AgentSession::new(project_id, ProviderKind::Codex);
    failed.begin_turn("failed turn");
    failed.finish_active_turn(TurnStatus::Failed);
    failed.status = SessionStatus::Failed;
    assert!(session_is_reapable(Some(&failed), stale, false));

    // A runtime whose session is already gone is pure leak.
    assert!(session_is_reapable(None, stale, false));
}

#[test]
fn turn_blocks_keep_their_message_boundaries() {
    // user, assistant text, tool row, assistant text, reasoning row,
    // assistant text
    let rows = transcript_row_kinds(4, &[2, 3]);
    assert_eq!(
        rows,
        vec![
            Message(0),
            Message(1),
            TurnBlock(0),
            Message(2),
            TurnBlock(1),
            Message(3)
        ]
    );
}

#[test]
fn blocks_follow_the_latest_message_without_a_reply() {
    let rows = transcript_row_kinds(2, &[2]);
    assert_eq!(rows, vec![Message(0), Message(1), TurnBlock(0)]);
}

#[test]
fn plain_transcript_maps_one_to_one() {
    let rows = transcript_row_kinds(4, &[]);
    assert_eq!(rows, vec![Message(0), Message(1), Message(2), Message(3)]);
}

#[test]
fn multiple_blocks_at_one_boundary_preserve_event_order() {
    let rows = transcript_row_kinds(2, &[1, 1]);
    assert_eq!(
        rows,
        vec![Message(0), TurnBlock(0), TurnBlock(1), Message(1)]
    );
}

/// Row *kinds* and row *count* are derived from the same list, and
/// `transcript_row` looks a row up by index in the cached kinds. If the two
/// ever disagree — or the cache is left empty — every row silently falls back
/// to `Message(n)` and all reasoning and tool activity vanish from the
/// transcript. That is exactly the bug this guards.
/// Expanding a disclosure pins a short transcript to the bottom of its
/// viewport. Doing that needs the document's real height — treating an
/// unmeasured list as zero-height asks for a leading space of the entire
/// viewport, which pushes every row off screen and leaves the transcript blank
/// until the reader scrolls it back.
#[test]
fn a_disclosure_never_forces_a_scroll_it_cannot_measure() {
    // Unmeasured: no scroll at all.
    assert_eq!(disclosure_leading_space(px(718.0), None), None);

    // Short content sits at the bottom, with the remainder as leading space.
    assert_eq!(
        disclosure_leading_space(px(718.0), Some(px(200.0))),
        Some(px(518.0))
    );

    // Content taller than the viewport needs no leading space, and never a
    // negative one.
    assert_eq!(
        disclosure_leading_space(px(718.0), Some(px(5_000.0))),
        Some(Pixels::ZERO)
    );
}

/// Expanding a disclosure re-measures exactly one row by splicing it in place.
/// That splice went dead while the row-kind cache was empty, so this pins the
/// behaviour it depends on: replacing one row with one row must not disturb the
/// list's contents.
#[test]
fn splicing_one_row_in_place_preserves_the_list() {
    let list = ListState::new(6, ListAlignment::Bottom, px(2048.0));
    assert_eq!(list.item_count(), 6);

    list.splice(3..4, 1);
    assert_eq!(
        list.item_count(),
        6,
        "a 1-for-1 splice must keep the row count"
    );

    // The tail remeasure path splices a trailing window.
    list.splice(3..6, 3);
    assert_eq!(list.item_count(), 6);

    // A range at the very end is still valid.
    list.splice(5..6, 1);
    assert_eq!(list.item_count(), 6);
}

#[test]
fn row_kinds_and_row_count_describe_the_same_rows() {
    let project_id = Uuid::new_v4();
    let mut session = AgentSession::new(project_id, ProviderKind::Codex);
    let turn_id = session.begin_turn("Build it");
    session.transcript_blocks.push(TranscriptBlock {
        after_message: 1,
        turn_id: Some(turn_id),
        activities: vec![
            ActivityItem::from_reasoning(
                ReasoningBlock {
                    content: "Looking around".into(),
                    started_at_ms: 1_000,
                    finished_at_ms: 2_000,
                },
                true,
            ),
            ActivityItem::new(None, ActivityKind::Command, "Ran tests", None, true),
        ],
    });
    session.push_message(MessageRole::Assistant, "Done.");
    session.finish_active_turn(TurnStatus::Completed);

    let kinds = folded_transcript_row_kinds(&session, &HashSet::from([turn_id]), None);
    // The work the agent did must be reachable by index, not just counted.
    assert!(
        kinds.iter().any(|kind| matches!(kind, TurnBlock(_))),
        "reasoning and activity must survive into the rendered rows: {kinds:?}"
    );
    assert_eq!(
        kinds
            .iter()
            .filter(|kind| matches!(kind, TurnBlock(_)))
            .count(),
        1,
        "reasoning and tools share one activity cluster: {kinds:?}"
    );
    // Every index below the count resolves to a real row.
    for index in 0..kinds.len() {
        assert!(kinds.get(index).is_some());
    }
    // Collapsed, that same work is one fold row — reachable, not lost.
    assert_eq!(
        folded_transcript_row_kinds(&session, &HashSet::new(), None),
        vec![
            Message(0),
            TurnFold(turn_id),
            Message(1),
            ResponseFooter(turn_id, 1),
        ]
    );
}

#[test]
fn changed_files_attach_to_the_response_footer() {
    let mut session = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
    let first_turn = session.begin_turn("Build it");
    session.push_message(MessageRole::Assistant, "Done.");
    session.finish_active_turn(TurnStatus::Completed);
    attach_changed_files(
        &mut session,
        vec![CheckpointFile {
            path: "src/app.rs".into(),
            additions: 12,
            deletions: 3,
        }],
    );

    session.begin_turn("One more thing");
    session.status = SessionStatus::Connecting;

    assert_eq!(
        folded_transcript_row_kinds(&session, &HashSet::new(), None),
        vec![
            Message(0),
            Message(1),
            ResponseFooter(first_turn, 1),
            Message(2),
            WorkingIndicator,
        ],
        "the footer containing the card must remain before the next prompt"
    );
    assert_eq!(response_footer_message_index(&session, first_turn), Some(1));
}

#[test]
fn response_hover_owns_every_response_row_but_not_the_prompt() {
    let mut session = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
    let turn_id = session.begin_turn("Build it");
    session.transcript_blocks.push(TranscriptBlock {
        after_message: 1,
        turn_id: Some(turn_id),
        activities: vec![ActivityItem::new(
            None,
            ActivityKind::Command,
            "Inspected the project",
            None,
            true,
        )],
    });
    session.push_message(MessageRole::Assistant, "Done.");
    session.finish_active_turn(TurnStatus::Completed);

    assert_eq!(response_row_turn_id(&session, Message(0)), None);
    for row in [
        TurnBlock(0),
        TurnFold(turn_id),
        Message(1),
        ResponseFooter(turn_id, 1),
        ChangedFiles(turn_id),
    ] {
        assert_eq!(response_row_turn_id(&session, row), Some(turn_id));
    }
    assert_eq!(response_row_turn_id(&session, WorkingIndicator), None);
}

#[test]
fn changed_files_remain_visible_when_an_interrupted_turn_has_no_answer() {
    let mut session = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
    let turn_id = session.begin_turn("Make the change");
    session.transcript_blocks.push(TranscriptBlock {
        after_message: 1,
        turn_id: Some(turn_id),
        activities: vec![ActivityItem::new(
            None,
            ActivityKind::Command,
            "Edited a file",
            None,
            true,
        )],
    });
    session.push_message(MessageRole::Assistant, "");
    session.finish_active_turn(TurnStatus::Interrupted);
    attach_changed_files(
        &mut session,
        vec![CheckpointFile {
            path: "src/lib.rs".into(),
            additions: 1,
            deletions: 0,
        }],
    );

    assert_eq!(
        folded_transcript_row_kinds(&session, &HashSet::new(), None),
        vec![Message(0), TurnFold(turn_id), ChangedFiles(turn_id)]
    );
    assert_eq!(
        folded_transcript_row_kinds(&session, &HashSet::from([turn_id]), None),
        vec![
            Message(0),
            TurnFold(turn_id),
            TurnBlock(0),
            Message(1),
            ChangedFiles(turn_id),
        ]
    );
    assert_eq!(response_footer_message_index(&session, turn_id), None);
}

#[test]
fn changed_files_surface_appears_only_for_a_ready_nonempty_checkpoint() {
    let mut session = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
    let turn_id = session.begin_turn("Build it");
    session.push_message(MessageRole::Assistant, "Done.");
    session.finish_active_turn(TurnStatus::Completed);

    attach_changed_files(&mut session, Vec::new());
    assert!(
        !folded_transcript_row_kinds(&session, &HashSet::new(), None)
            .contains(&ChangedFiles(turn_id))
    );

    attach_changed_files(
        &mut session,
        vec![CheckpointFile {
            path: "src/main.rs".into(),
            additions: 2,
            deletions: 1,
        }],
    );
    assert_eq!(response_footer_message_index(&session, turn_id), Some(1));
    assert!(
        !folded_transcript_row_kinds(&session, &HashSet::new(), None)
            .contains(&ChangedFiles(turn_id)),
        "a response with visible text hosts the card inside its footer"
    );
    session.turns[0]
        .checkpoint
        .as_mut()
        .expect("checkpoint")
        .status = CheckpointStatus::Unavailable;
    assert!(
        !folded_transcript_row_kinds(&session, &HashSet::new(), None)
            .contains(&ChangedFiles(turn_id))
    );
    assert_eq!(response_footer_message_index(&session, turn_id), Some(1));
}

#[test]
fn the_diff_preview_indexes_each_files_rows_without_headers() {
    use crate::model::ActivityFileChange;
    use crate::review_diff::{LineKind, from_file_changes};

    let change = |path: &str, diff: &str| ActivityFileChange {
        path: path.into(),
        additions: Some(1),
        deletions: Some(1),
        status: None,
        diff: Some(diff.into()),
    };
    let snapshot = from_file_changes(&[
        change("src/one.rs", "@@ -1,2 +1,2 @@\n kept\n-old\n+new\n"),
        change("src/two.rs", "@@\n+added\n"),
    ]);
    let file_lines = changed_files_diff_file_lines(&snapshot);

    assert_eq!(file_lines.len(), 2);
    for (file_index, indexes) in file_lines.iter().enumerate() {
        assert!(!indexes.is_empty());
        for &line_index in indexes {
            let line = &snapshot.lines[line_index];
            assert_eq!(line.file_index, file_index);
            assert_ne!(line.kind, LineKind::FileHeader);
        }
    }
    let indexed: usize = file_lines.iter().map(Vec::len).sum();
    let expected = snapshot
        .lines
        .iter()
        .filter(|line| line.kind != LineKind::FileHeader)
        .count();
    assert_eq!(indexed, expected);
}

#[test]
fn checkpoint_completion_invalidates_the_cached_transcript_rows() {
    let mut session = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
    let turn_id = session.begin_turn("Build it");
    session.push_message(MessageRole::Assistant, "Done.");
    session.finish_active_turn(TurnStatus::Completed);
    let before = transcript_rows_fingerprint(&session, &HashSet::new(), None);

    attach_changed_files(
        &mut session,
        vec![CheckpointFile {
            path: "src/main.rs".into(),
            additions: 2,
            deletions: 1,
        }],
    );

    assert_ne!(
        transcript_rows_fingerprint(&session, &HashSet::new(), None),
        before
    );
    assert_eq!(response_footer_message_index(&session, turn_id), Some(1));
    assert!(
        !folded_transcript_row_kinds(&session, &HashSet::new(), None)
            .contains(&ChangedFiles(turn_id)),
        "checkpoint completion changes the existing footer row's height"
    );
}

#[test]
fn an_inline_checkpoint_keeps_followup_row_identity() {
    let turn_id = Uuid::new_v4();
    let previous = vec![
        Message(0),
        Message(1),
        ResponseFooter(turn_id, 1),
        Message(2),
        WorkingIndicator,
    ];
    let with_checkpoint = previous.clone();

    assert_eq!(
        transcript_row_splice(&previous, &with_checkpoint),
        None,
        "the card remeasures its footer instead of shifting the following prompt"
    );
}

/// `refresh_transcript_row_kinds` skips the fold while this fingerprint holds
/// still, so anything that moves the rows has to move the fingerprint too.
/// Missing one leaves the transcript rendering stale rows, which drops every
/// reasoning block and tool activity from the session.
#[test]
fn the_row_fingerprint_moves_whenever_the_fold_does() {
    let mut base = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
    let turn_id = base.begin_turn("Build it");
    base.transcript_blocks.push(TranscriptBlock {
        after_message: 1,
        turn_id: Some(turn_id),
        activities: vec![ActivityItem::from_reasoning(
            ReasoningBlock {
                content: "Looking around".into(),
                started_at_ms: 1_000,
                finished_at_ms: 2_000,
            },
            true,
        )],
    });
    base.push_message(MessageRole::Assistant, "I found the relevant code.");
    base.transcript_blocks.push(TranscriptBlock {
        after_message: 2,
        turn_id: Some(turn_id),
        activities: vec![ActivityItem::new(
            None,
            ActivityKind::Command,
            "Ran tests",
            None,
            true,
        )],
    });
    base.push_message(MessageRole::Assistant, "Done. The change is ready.");
    base.finish_active_turn(TurnStatus::Completed);

    let settled = HashSet::new();
    // Both states, because a mutation inside the fold only moves rows once the
    // reader opens it — and those rows have to be right when they do.
    let rows = |session: &AgentSession| {
        (
            folded_transcript_row_kinds(session, &settled, None),
            folded_transcript_row_kinds(session, &HashSet::from([turn_id]), None),
        )
    };
    let baseline_rows = rows(&base);
    let baseline_fingerprint = transcript_rows_fingerprint(&base, &settled, None);

    // Expansion lives outside the session, so check it against the same base.
    assert_ne!(
        transcript_rows_fingerprint(&base, &HashSet::from([turn_id]), None),
        baseline_fingerprint,
        "expanding a turn fold"
    );

    let mutations: Vec<(&str, fn(&mut AgentSession))> = vec![
        ("a new message", |session| {
            session.push_message(MessageRole::User, "One more thing");
        }),
        ("a new transcript block", |session| {
            session.transcript_blocks.push(TranscriptBlock {
                after_message: session.messages.len(),
                turn_id: session.turns.first().map(|turn| turn.id),
                activities: vec![ActivityItem::new(
                    None,
                    ActivityKind::Command,
                    "Ran tests",
                    None,
                    true,
                )],
            });
        }),
        // `update_activity` re-anchors the block it is still appending to, so
        // the anchor cannot be treated as fixed at insertion.
        ("an existing block re-anchored", |session| {
            session.transcript_blocks[0].after_message = 2;
        }),
        ("a turn returning to running", |session| {
            session.turns[0].status = TurnStatus::Running;
        }),
        ("a message reassigned to no turn", |session| {
            session.messages[1].turn_id = None;
        }),
        // A blank part is work, not answer, so where the answer starts — and
        // with it everything the fold swallows — turns on the content itself.
        ("the final text part left blank", |session| {
            session.messages[2].content.clear();
        }),
    ];

    for (description, mutate) in mutations {
        let mut session = base.clone();
        mutate(&mut session);
        assert_ne!(
            rows(&session),
            baseline_rows,
            "{description} should change the rows — the case no longer proves anything"
        );
        assert_ne!(
            transcript_rows_fingerprint(&session, &settled, None),
            baseline_fingerprint,
            "{description} changed the rows but not the fingerprint, so the \
             cached rows would go stale"
        );
    }

    // The point of the guard: streamed text lands in an existing message
    // without moving a single row, and must not trigger a refold.
    let mut streamed = base.clone();
    streamed.messages[2].content.push_str(" Let me know.");
    assert_eq!(
        transcript_rows_fingerprint(&streamed, &settled, None),
        baseline_fingerprint,
        "appending to a message leaves the rows exactly where they were"
    );
}

#[test]
fn a_settled_turn_folds_all_of_its_work_above_the_answer() {
    let project_id = Uuid::new_v4();
    let mut session = AgentSession::new(project_id, ProviderKind::Codex);
    let turn_id = session.begin_turn("Build it");
    session.transcript_blocks.push(TranscriptBlock {
        after_message: 1,
        turn_id: Some(turn_id),
        activities: vec![ActivityItem::from_reasoning(
            ReasoningBlock {
                content: "Looking around".into(),
                started_at_ms: 1_000,
                finished_at_ms: 2_000,
            },
            true,
        )],
    });
    session.push_message(MessageRole::Assistant, "I found the relevant code.");
    session.transcript_blocks.push(TranscriptBlock {
        after_message: 2,
        turn_id: Some(turn_id),
        activities: vec![ActivityItem::new(
            None,
            ActivityKind::Command,
            "Ran tests",
            None,
            true,
        )],
    });
    session.push_message(MessageRole::Assistant, "Done. The change is ready.");
    session.finish_active_turn(TurnStatus::Completed);

    // The summary opens the turn and everything the agent did before its
    // answer sits behind it — reasoning, tool activity and interim commentary
    // alike. A divider between two pieces of work would read as a cut-off
    // response.
    assert_eq!(
        folded_transcript_row_kinds(&session, &HashSet::new(), None),
        vec![
            Message(0),
            TurnFold(turn_id),
            Message(2),
            ResponseFooter(turn_id, 2),
        ]
    );
    // Expanding restores the turn's real order in place.
    assert_eq!(
        folded_transcript_row_kinds(&session, &HashSet::from([turn_id]), None),
        vec![
            Message(0),
            TurnFold(turn_id),
            TurnBlock(0),
            Message(1),
            TurnBlock(1),
            Message(2),
            ResponseFooter(turn_id, 2),
        ]
    );
}

/// A continue's hidden prompt stays in `session.messages` — every client
/// projects the same ids — but it renders no row, no rail turn, and its turn
/// folds and footers exactly like a prompted one.
#[test]
fn a_hidden_prompt_renders_no_row_but_keeps_its_turn() {
    let mut session = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
    session.begin_turn("Build it");
    session.push_message(MessageRole::Assistant, "Built it.");
    session.finish_active_turn(TurnStatus::Interrupted);

    let continued = session.begin_hidden_turn("Continue the current task if able.");
    session.push_message(MessageRole::Assistant, "Kept going.");
    session.finish_active_turn(TurnStatus::Completed);

    assert!(session.messages[2].hidden);
    assert_eq!(
        folded_transcript_row_kinds(&session, &HashSet::new(), None),
        vec![
            Message(0),
            Message(1),
            ResponseFooter(session.turns[0].id, 1),
            Message(3),
            ResponseFooter(continued, 3),
        ],
        "the hidden prompt at index 2 produces no row"
    );
    // The rail offers only the prompts a human typed, and the hidden prompt
    // still ends the previous turn's preview — "Kept going." must not leak
    // into "Build it"'s row.
    let row_kinds = folded_transcript_row_kinds(&session, &HashSet::new(), None);
    let nav = transcript_navigation_turns(&session, &row_kinds);
    assert_eq!(nav.len(), 1);
    assert_eq!(nav[0].message_index, 0);
    assert_eq!(nav[0].response, "Built it.");
}

/// Continue on a turn the provider never confirmed resends the undelivered
/// prompt — a canned nudge would reach a provider session with no context
/// for it. A confirmed turn still gets the hidden continue instead.
#[test]
fn continue_resends_a_prompt_the_provider_never_saw() {
    use super::composer::undelivered_turn_resend;

    let mut session = AgentSession::new(Uuid::new_v4(), ProviderKind::Devin);
    let turn_id = session.begin_turn("fix the model picker");
    session.push_notice_message(
        MessageRole::Assistant,
        "Could not start the agent",
        TranscriptNotice::Status {
            kind: TranscriptNoticeStatus::StartFailed,
        },
    );
    session.finish_active_turn(TurnStatus::Failed);
    session.status = SessionStatus::Idle;

    let (dead_turn, dead_message, submission) =
        undelivered_turn_resend(&session).expect("the undelivered prompt is resent");
    assert_eq!(dead_turn, turn_id);
    assert_eq!(dead_message, session.messages[0].id);
    assert_eq!(submission.prompt, "fix the model picker");
    assert!(!submission.hidden);

    // The retry unwinds the dead turn, so the resent prompt takes its slot.
    session.unwind_unstarted_turn(dead_turn);
    assert!(session.turns.is_empty());
    assert!(session.messages.is_empty());

    // A turn the provider confirmed keeps the canned-continue path.
    let confirmed = session.begin_turn("real work");
    session.mark_active_turn_provider_started();
    session.finish_active_turn(TurnStatus::Interrupted);
    assert!(undelivered_turn_resend(&session).is_none());
    let _ = confirmed;
}

/// A daemon restart auto-resumes only a session whose turn the provider
/// actually began and whose cursor can reload it — idle sessions, unconfirmed
/// turns, and cursorless providers keep the ordinary loss path.
#[test]
fn daemon_restart_resume_requires_a_started_turn_and_a_cursor() {
    use super::runtime::session_resume_eligible;
    use crate::model::ProviderResumeCursor;

    let eligible = || {
        let mut session = AgentSession::new(Uuid::new_v4(), ProviderKind::Devin);
        session.begin_turn("run the suite");
        session.mark_active_turn_provider_started();
        session.status = SessionStatus::Working;
        session.provider_cursor = Some(ProviderResumeCursor::Devin {
            session_id: "devin-session".into(),
        });
        session
    };
    assert!(session_resume_eligible(&eligible()));

    let mut waiting = eligible();
    waiting.status = SessionStatus::Waiting;
    assert!(session_resume_eligible(&waiting));

    let mut idle = eligible();
    idle.finish_active_turn(TurnStatus::Completed);
    idle.status = SessionStatus::Idle;
    assert!(!session_resume_eligible(&idle));

    let mut cursorless = eligible();
    cursorless.provider_cursor = None;
    assert!(!session_resume_eligible(&cursorless));

    // Connecting without the provider's TurnStarted: the prompt may never
    // have arrived, so "continue" has nothing to resume.
    let mut unconfirmed = eligible();
    unconfirmed.turns.last_mut().unwrap().provider_turn_started = false;
    unconfirmed.status = SessionStatus::Connecting;
    assert!(!session_resume_eligible(&unconfirmed));
}

/// Providers split one answer across several text parts. They arrive with no
/// work between them, so they are all answer and none of them folds.
#[test]
fn consecutive_trailing_text_parts_all_stay_out_of_the_fold() {
    let mut session = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
    let turn_id = session.begin_turn("Build it");
    session.transcript_blocks.push(TranscriptBlock {
        after_message: 1,
        turn_id: Some(turn_id),
        activities: vec![ActivityItem::from_reasoning(
            ReasoningBlock {
                content: "Looking around".into(),
                started_at_ms: 1_000,
                finished_at_ms: 2_000,
            },
            true,
        )],
    });
    session.push_message(MessageRole::Assistant, "First half of the answer.");
    session.push_message(MessageRole::Assistant, "Second half of the answer.");
    session.finish_active_turn(TurnStatus::Completed);

    assert_eq!(
        folded_transcript_row_kinds(&session, &HashSet::new(), None),
        vec![
            Message(0),
            TurnFold(turn_id),
            Message(1),
            Message(2),
            ResponseFooter(turn_id, 2),
        ]
    );
}

/// An interrupted turn that never produced text has nothing to stay visible,
/// so the whole turn folds behind its summary rather than spilling raw work.
#[test]
fn a_turn_without_an_answer_folds_completely() {
    let mut session = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
    let turn_id = session.begin_turn("Build it");
    session.transcript_blocks.push(TranscriptBlock {
        after_message: 1,
        turn_id: Some(turn_id),
        activities: vec![ActivityItem::new(
            None,
            ActivityKind::Command,
            "Ran tests",
            None,
            true,
        )],
    });
    // The streaming placeholder never received any text.
    session.push_message(MessageRole::Assistant, "");
    session.finish_active_turn(TurnStatus::Interrupted);

    assert_eq!(
        folded_transcript_row_kinds(&session, &HashSet::new(), None),
        vec![Message(0), TurnFold(turn_id)]
    );
    assert_eq!(
        folded_transcript_row_kinds(&session, &HashSet::from([turn_id]), None),
        vec![Message(0), TurnFold(turn_id), TurnBlock(0), Message(1)]
    );
}

#[test]
fn assistant_response_footer_is_owned_by_the_terminal_part_and_copies_the_visible_answer() {
    let project_id = Uuid::new_v4();
    let mut session = AgentSession::new(project_id, ProviderKind::Codex);
    let turn_id = session.begin_turn("Build it");
    session.transcript_blocks.push(TranscriptBlock {
        after_message: 1,
        turn_id: Some(turn_id),
        activities: vec![ActivityItem::from_reasoning(
            ReasoningBlock {
                content: "Looking around".into(),
                started_at_ms: 1_000,
                finished_at_ms: 2_000,
            },
            true,
        )],
    });
    session.push_message(MessageRole::Assistant, "Interim commentary.");
    session.transcript_blocks.push(TranscriptBlock {
        after_message: 2,
        turn_id: Some(turn_id),
        activities: vec![ActivityItem::new(
            None,
            ActivityKind::Command,
            "Ran tests",
            None,
            true,
        )],
    });
    session.push_message(MessageRole::Assistant, "First half of the answer.");
    session.push_message(MessageRole::Assistant, "Second half of the answer.");
    session.finish_active_turn(TurnStatus::Completed);
    session.messages[3].created_at = 100;
    session.turns.last_mut().unwrap().completed_at = Some(200);

    assert_eq!(assistant_response_footer_index(&session, 1), Some(3));
    assert_eq!(assistant_response_footer_index(&session, 3), Some(3));
    assert_eq!(assistant_response_footer(&session, 1), None);
    // The interim commentary hides behind the "Worked for X" fold, so copying
    // the message must skip it and combine only the trailing answer parts.
    assert_eq!(
        assistant_response_footer(&session, 3).as_deref(),
        Some("First half of the answer.\n\nSecond half of the answer.")
    );
    assert_eq!(assistant_response_footer_time(&session, 1), None);
    assert_eq!(assistant_response_footer_time(&session, 3), Some(200));
    assert!(
        session.messages[1..]
            .iter()
            .all(|message| message.turn_id == Some(turn_id))
    );
}

#[test]
fn response_footer_follows_trailing_tool_activity() {
    let mut session = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
    let turn_id = session.begin_turn("Inspect it");
    session.push_message(MessageRole::Assistant, "I’ll inspect the implementation.");
    session.transcript_blocks.push(TranscriptBlock {
        after_message: 2,
        turn_id: Some(turn_id),
        activities: vec![ActivityItem::new(
            None,
            ActivityKind::Command,
            "Searched the source",
            None,
            true,
        )],
    });
    session.finish_active_turn(TurnStatus::Interrupted);

    assert_eq!(
        folded_transcript_row_kinds(&session, &HashSet::new(), None),
        vec![
            Message(0),
            Message(1),
            TurnBlock(0),
            ResponseFooter(turn_id, 1),
        ]
    );
}

/// A blank part breaks the answer run the same way work does — the fold hides
/// the text before it, so the copied message must leave that text out too.
#[test]
fn assistant_response_footer_treats_a_blank_part_as_work() {
    let mut session = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
    let turn_id = session.begin_turn("Build it");
    session.push_message(MessageRole::Assistant, "First text part.");
    session.push_message(MessageRole::Assistant, "  ");
    session.push_message(MessageRole::Assistant, "Final text part.");
    session.finish_active_turn(TurnStatus::Completed);

    assert_eq!(
        folded_transcript_row_kinds(&session, &HashSet::new(), None),
        vec![
            Message(0),
            TurnFold(turn_id),
            Message(3),
            ResponseFooter(turn_id, 3),
        ]
    );
    assert_eq!(
        assistant_response_footer(&session, 3).as_deref(),
        Some("Final text part.")
    );
}

#[test]
fn running_assistant_response_withholds_its_footer() {
    let project_id = Uuid::new_v4();
    let mut session = AgentSession::new(project_id, ProviderKind::Codex);
    session.begin_turn("Keep going");
    session.push_message(MessageRole::Assistant, "Interim text.");

    assert_eq!(assistant_response_footer_index(&session, 1), None);
    assert_eq!(assistant_response_footer(&session, 1), None);
}

#[test]
fn unkeyed_assistant_message_keeps_a_standalone_footer() {
    let project_id = Uuid::new_v4();
    let mut session = AgentSession::new(project_id, ProviderKind::Codex);
    session
        .messages
        .push(Message::new(MessageRole::Assistant, "Standalone response."));
    session.messages[0].created_at = 300;

    assert_eq!(assistant_response_footer_index(&session, 0), Some(0));
    assert_eq!(
        assistant_response_footer(&session, 0).as_deref(),
        Some("Standalone response.")
    );
    assert_eq!(assistant_response_footer_time(&session, 0), Some(300));
}

#[test]
fn turn_fold_visibility_splice_preserves_surrounding_message_rows() {
    let turn_id = Uuid::new_v4();
    let collapsed = vec![
        Message(0),
        TurnFold(turn_id),
        Message(2),
        ResponseFooter(turn_id, 2),
    ];
    let expanded = vec![
        Message(0),
        TurnFold(turn_id),
        TurnBlock(0),
        Message(1),
        TurnBlock(1),
        Message(2),
        ResponseFooter(turn_id, 2),
    ];

    let expand_splice = transcript_row_splice(&collapsed, &expanded);
    assert_eq!(expand_splice, Some((2..2, 3)));
    assert_eq!(
        transcript_row_splice(&expanded, &collapsed),
        Some((2..5, 0))
    );
    assert_eq!(transcript_row_splice(&collapsed, &collapsed), None);
}

#[test]
fn running_turn_keeps_its_ordered_work_visible() {
    let project_id = Uuid::new_v4();
    let mut session = AgentSession::new(project_id, ProviderKind::Codex);
    let turn_id = session.begin_turn("Keep going");
    session.transcript_blocks.push(TranscriptBlock {
        after_message: 1,
        turn_id: Some(turn_id),
        activities: vec![ActivityItem::from_reasoning(
            ReasoningBlock {
                content: "Still thinking".into(),
                started_at_ms: 1_000,
                finished_at_ms: 2_000,
            },
            false,
        )],
    });
    session.push_message(MessageRole::Assistant, "Interim update");

    assert_eq!(
        folded_transcript_row_kinds(&session, &HashSet::new(), None),
        vec![Message(0), TurnBlock(0), Message(1)]
    );
}

#[test]
fn plain_settled_response_does_not_add_an_empty_work_fold() {
    let project_id = Uuid::new_v4();
    let mut session = AgentSession::new(project_id, ProviderKind::Codex);
    let turn_id = session.begin_turn("Answer directly");
    session.push_message(MessageRole::Assistant, "The answer.");
    session.finish_active_turn(TurnStatus::Completed);

    assert_eq!(
        folded_transcript_row_kinds(&session, &HashSet::new(), None),
        vec![Message(0), Message(1), ResponseFooter(turn_id, 1)]
    );
}

#[test]
fn worked_duration_uses_readable_units() {
    assert_eq!(format_worked_duration(1), "1 second");
    assert_eq!(format_worked_duration(28), "28 seconds");
    assert_eq!(format_worked_duration(60), "1 minute");
    assert_eq!(format_worked_duration(88), "1 minute 28 seconds");
    assert_eq!(format_worked_duration(7_320), "2 hours 2 minutes");
}

#[test]
fn sidebar_time_labels_show_reply_age_during_a_live_turn() {
    use super::sidebar::{format_time_ago, session_time_label};

    assert_eq!(format_time_ago(0), "just now");
    assert_eq!(format_time_ago(59), "just now");
    assert_eq!(format_time_ago(300), "5m");
    assert_eq!(format_time_ago(7_200), "2h");
    assert_eq!(format_time_ago(420 * 86_400), "420d");

    // Never replied, nothing running: the row stays quiet.
    let mut session = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
    assert_eq!(session_time_label(&session, 1_000), None);

    // A live turn keeps the reply age — the worktree icon sits beside it
    // rather than replacing it.
    session.begin_turn("go");
    session.status = SessionStatus::Working;
    session.turns[0].started_at = 100;
    session.last_reply_at = Some(40);
    assert_eq!(session_time_label(&session, 109).as_deref(), Some("1m"));

    // Settled again: still how long ago the agent last replied.
    session.finish_active_turn(TurnStatus::Completed);
    session.status = SessionStatus::Idle;
    session.last_reply_at = Some(500);
    assert_eq!(session_time_label(&session, 800).as_deref(), Some("5m"));
}

/// The time-label wake-up chain arms exactly one timer, aimed at the next
/// instant a visible label rolls over. Firing early leaves a stale label on
/// screen for a unit; firing often burns wake-ups an idle window shouldn't
/// pay — so the boundary math is pinned here.
#[test]
fn time_label_wakes_land_exactly_on_label_boundaries() {
    use super::next_time_label_change;

    // Nothing on the clock: no sessions, or none that ever replied.
    assert_eq!(next_time_label_change(&[], 1_000), None);
    let mut session = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
    assert_eq!(
        next_time_label_change(std::slice::from_ref(&session), 1_000),
        None
    );

    // "just now" becomes "1m" sixty seconds after the reply.
    session.last_reply_at = Some(1_000);
    let sessions = [session];
    assert_eq!(next_time_label_change(&sessions, 1_030), Some(30));
    // "1m" → "2m" at the next minute multiple, not on a fixed cadence.
    assert_eq!(next_time_label_change(&sessions, 1_090), Some(30));
    // Hours-old labels wake hourly…
    assert_eq!(
        next_time_label_change(&sessions, 1_000 + 3 * 3_600 + 1_200),
        Some(2_400)
    );
    // …and day-old labels daily.
    assert_eq!(
        next_time_label_change(&sessions, 1_000 + 2 * 86_400 + 3_600),
        Some(82_800)
    );

    // The earliest boundary across sessions wins.
    let mut fresher = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
    fresher.last_reply_at = Some(1_000 + 2 * 86_400 + 3_550);
    let sessions = [&sessions[0], &fresher]
        .into_iter()
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(
        next_time_label_change(&sessions, 1_000 + 2 * 86_400 + 3_600),
        Some(10)
    );

    // A live turn follows the same boundary math — the row shows the reply
    // age, so there is no per-second counter to feed.
    let mut busy = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
    busy.begin_turn("go");
    busy.status = SessionStatus::Working;
    busy.last_reply_at = Some(1_000);
    let sessions = [busy];
    assert_eq!(next_time_label_change(&sessions, 1_030), Some(30));
}

#[test]
fn working_elapsed_stays_compact() {
    assert_eq!(format_working_elapsed(0), "0s");
    assert_eq!(format_working_elapsed(9), "9s");
    assert_eq!(format_working_elapsed(59), "59s");
    assert_eq!(format_working_elapsed(60), "1m");
    assert_eq!(format_working_elapsed(65), "1m 5s");
    assert_eq!(format_working_elapsed(3_600), "1h");
    assert_eq!(format_working_elapsed(3_720), "1h 2m");
}

/// The working indicator is on screen for the whole live turn: from before
/// the first chunk, below streamed content once chunks arrive, through the
/// permission pause — and gone the moment the session stops being busy.
#[test]
fn a_busy_turn_pins_the_working_indicator_after_the_last_row() {
    let mut session = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
    let turn_id = session.begin_turn("Build it");
    session.status = SessionStatus::Working;

    // No chunks yet: the indicator alone follows the prompt.
    assert_eq!(
        folded_transcript_row_kinds(&session, &HashSet::new(), None),
        vec![Message(0), WorkingIndicator]
    );

    // Streamed content pushes it down, never off.
    session.push_message(MessageRole::Assistant, "Starting on it.");
    assert_eq!(
        folded_transcript_row_kinds(&session, &HashSet::new(), None),
        vec![Message(0), Message(1), WorkingIndicator]
    );

    // A pending permission keeps the turn — and the indicator — alive.
    session.status = SessionStatus::Waiting;
    assert_eq!(
        folded_transcript_row_kinds(&session, &HashSet::new(), None),
        vec![Message(0), Message(1), WorkingIndicator]
    );

    // A driver error can fail the session while its last turn is still
    // marked running. Busy-ness is the only input that moved, so the
    // fingerprint must move with it or the stale indicator lingers.
    session.status = SessionStatus::Working;
    let busy_fingerprint = transcript_rows_fingerprint(&session, &HashSet::new(), None);
    session.status = SessionStatus::Failed;
    assert_eq!(
        folded_transcript_row_kinds(&session, &HashSet::new(), None),
        vec![Message(0), Message(1)]
    );
    assert_ne!(
        transcript_rows_fingerprint(&session, &HashSet::new(), None),
        busy_fingerprint,
        "dropping the busy status changed the rows but not the fingerprint"
    );

    // A settled turn swaps the indicator for its final transcript shape.
    session.status = SessionStatus::Working;
    session.finish_active_turn(TurnStatus::Completed);
    session.status = SessionStatus::Idle;
    assert_eq!(
        folded_transcript_row_kinds(&session, &HashSet::new(), None),
        vec![Message(0), Message(1), ResponseFooter(turn_id, 1)]
    );
}

/// Leaving a busy session while the live turn is on screen parks a
/// tail-while-busy position: reselecting it rejoins the stream while the
/// session still works, and falls back to the parked spot once it settles.
#[test]
fn a_parked_position_tails_only_while_the_session_still_works() {
    let offset = ListOffset {
        item_ix: 4,
        offset_in_item: px(12.0),
    };
    let watching = TranscriptScrollPosition {
        offset,
        tail_while_busy: true,
    };
    assert!(matches!(
        transcript_position_landing(watching, true),
        TranscriptLanding::Tail
    ));
    assert!(matches!(
        transcript_position_landing(watching, false),
        TranscriptLanding::Position(stored)
            if stored.item_ix == offset.item_ix
                && stored.offset_in_item == offset.offset_in_item
    ));

    // An ordinary reading spot restores verbatim either way.
    let reading = TranscriptScrollPosition {
        offset,
        tail_while_busy: false,
    };
    for busy in [true, false] {
        assert!(matches!(
            transcript_position_landing(reading, busy),
            TranscriptLanding::Position(stored)
                if stored.item_ix == offset.item_ix
                    && stored.offset_in_item == offset.offset_in_item
        ));
    }
}

/// A settled turn whose checkpoint capture is still queued or in flight
/// shows its changed-files card pending: inside the footer row when the turn
/// has one, as its own row when it does not. The flag lives in the capture
/// queues rather than the session, so the fingerprint has to carry it or the
/// card would appear and fill in a fold late.
#[test]
fn a_pending_checkpoint_shows_a_pending_card_after_the_settled_turn() {
    let mut session = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
    let turn_id = session.begin_turn("Build it");
    session.push_message(MessageRole::Assistant, "Done.");
    session.finish_active_turn(TurnStatus::Completed);
    session.status = SessionStatus::Idle;

    // A footer turn hosts the pending card inside its footer row, so the
    // rows hold still — only the fingerprint moves.
    let settled = vec![Message(0), Message(1), ResponseFooter(turn_id, 1)];
    assert_eq!(
        folded_transcript_row_kinds(&session, &HashSet::new(), None),
        settled
    );
    assert_eq!(
        folded_transcript_row_kinds(&session, &HashSet::new(), Some(turn_id)),
        settled
    );
    assert_ne!(
        transcript_rows_fingerprint(&session, &HashSet::new(), Some(turn_id)),
        transcript_rows_fingerprint(&session, &HashSet::new(), None),
        "the pending turn moved the footer's content but not the fingerprint"
    );

    // A turn without a footer — tool activity but no copyable answer — gets
    // the standalone pending card after its fold.
    let mut session = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
    let turn_id = session.begin_turn("Build it");
    session.transcript_blocks.push(TranscriptBlock {
        after_message: session.messages.len(),
        turn_id: Some(turn_id),
        activities: vec![ActivityItem::from_reasoning(
            ReasoningBlock {
                content: "Inspecting the project".into(),
                started_at_ms: 1_000,
                finished_at_ms: 2_000,
            },
            true,
        )],
    });
    session.finish_active_turn(TurnStatus::Completed);
    session.status = SessionStatus::Idle;

    assert_eq!(
        folded_transcript_row_kinds(&session, &HashSet::new(), None),
        vec![Message(0), TurnFold(turn_id)]
    );
    assert_eq!(
        folded_transcript_row_kinds(&session, &HashSet::new(), Some(turn_id)),
        vec![Message(0), TurnFold(turn_id), ChangedFiles(turn_id)]
    );
}

/// `retain_fading_working_indicator` holds the settled turn's indicator row
/// for one 300ms fade: the same session must have shown it, a fresh live
/// turn or a session switch ends it, and once retired it must not re-arm —
/// that last transition regressed once and looped the fade forever.
#[test]
fn the_working_indicator_fades_out_once_then_retires() {
    let session_id = Uuid::new_v4();
    let turn_id = Uuid::new_v4();
    let armed_at = Instant::now();

    // The settle dropped the indicator the same session was showing: the
    // fade arms and the row stays.
    let mut settled_rows = vec![Message(0)];
    let fade = retain_fading_working_indicator(
        &mut settled_rows,
        Some(session_id),
        Some(turn_id),
        Some(session_id),
        None,
        armed_at,
    )
    .expect("the settle transition arms the fade");
    assert_eq!(settled_rows, vec![Message(0), WorkingIndicator]);
    assert_eq!(fade.turn_id, turn_id);
    assert!(!fade.removal_scheduled);

    // Mid-fade refolds keep the ghost row until the window closes.
    let mut mid_fade = vec![Message(0)];
    let kept = retain_fading_working_indicator(
        &mut mid_fade,
        Some(session_id),
        Some(turn_id),
        Some(session_id),
        Some(fade),
        armed_at + WORKING_INDICATOR_FADE_OUT / 2,
    );
    assert_eq!(kept, Some(fade));
    assert_eq!(mid_fade, vec![Message(0), WorkingIndicator]);

    // Past the window the row retires for real…
    let mut expired = vec![Message(0)];
    assert_eq!(
        retain_fading_working_indicator(
            &mut expired,
            Some(session_id),
            Some(turn_id),
            Some(session_id),
            Some(fade),
            armed_at + WORKING_INDICATOR_FADE_OUT,
        ),
        None,
    );
    assert_eq!(expired, vec![Message(0)]);

    // …and with the indicator marked retired, the next refold must not read
    // "same session, indicator dropped" and arm a second fade.
    let mut after_retire = vec![Message(0)];
    assert_eq!(
        retain_fading_working_indicator(
            &mut after_retire,
            Some(session_id),
            Some(turn_id),
            None,
            None,
            armed_at + WORKING_INDICATOR_FADE_OUT,
        ),
        None,
    );
    assert_eq!(after_retire, vec![Message(0)]);
}

#[test]
fn the_working_indicator_fade_does_not_follow_a_session_switch() {
    let busy_session = Uuid::new_v4();
    let other_session = Uuid::new_v4();
    let now = Instant::now();

    // The previous fold showed the indicator — but for another session.
    let mut rows = vec![Message(0)];
    assert_eq!(
        retain_fading_working_indicator(
            &mut rows,
            Some(other_session),
            Some(Uuid::new_v4()),
            Some(busy_session),
            None,
            now,
        ),
        None,
    );
    assert_eq!(rows, vec![Message(0)]);

    // A fade armed in one session is dropped, not kept, in another.
    let fade = WorkingIndicatorFade {
        session_id: busy_session,
        turn_id: Uuid::new_v4(),
        started: now,
        removal_scheduled: false,
    };
    let mut rows = vec![Message(0)];
    assert_eq!(
        retain_fading_working_indicator(
            &mut rows,
            Some(other_session),
            Some(Uuid::new_v4()),
            None,
            Some(fade),
            now,
        ),
        None,
    );
    assert_eq!(rows, vec![Message(0)]);
}

#[test]
fn a_new_turn_cancels_the_working_indicator_fade() {
    let session_id = Uuid::new_v4();
    let now = Instant::now();
    let fade = WorkingIndicatorFade {
        session_id,
        turn_id: Uuid::new_v4(),
        started: now,
        removal_scheduled: true,
    };
    // The fold has a live indicator again — the fade retires instead of
    // stacking a ghost row on the new turn's real one.
    let mut rows = vec![Message(0), WorkingIndicator];
    assert_eq!(
        retain_fading_working_indicator(
            &mut rows,
            Some(session_id),
            Some(Uuid::new_v4()),
            Some(session_id),
            Some(fade),
            now,
        ),
        None,
    );
    assert_eq!(rows, vec![Message(0), WorkingIndicator]);
}

#[test]
fn model_picker_highlight_wraps_at_both_ends() {
    // Nothing highlighted yet: down opens on the first row, up on the last.
    assert_eq!(next_picker_highlight(None, 3, "down"), Some(0));
    assert_eq!(next_picker_highlight(None, 3, "up"), Some(2));

    assert_eq!(next_picker_highlight(Some(0), 3, "down"), Some(1));
    assert_eq!(next_picker_highlight(Some(2), 3, "down"), Some(0));
    assert_eq!(next_picker_highlight(Some(0), 3, "up"), Some(2));

    // Keys the filter field owns must not move the cursor.
    assert_eq!(next_picker_highlight(Some(1), 3, "home"), None);
    assert_eq!(next_picker_highlight(Some(1), 3, "enter"), None);

    // An empty result list has nothing to land on.
    assert_eq!(next_picker_highlight(None, 0, "down"), None);
}

#[test]
fn model_picker_highlight_seeds_from_the_selected_model() {
    use super::model_picker::{PickerGranularity, picker_selected_row_index};

    let probes = [
        picker_probe(ProviderKind::Claude, "claude-a", &[], false),
        picker_probe(ProviderKind::Claude, "claude-b", &[], false),
        picker_probe(ProviderKind::Claude, "claude-c", &[], false),
    ];
    let rows = visible_picker_rows(
        &probes,
        &[],
        &[],
        &[],
        None,
        "",
        false,
        PickerGranularity::Combos,
    );

    // The first arrow moves relative to the session's combo — the row the
    // reveal scrolled into view — rather than jumping to an end.
    let selection = (ProviderKind::Claude, "claude-b".to_owned(), None, false);
    let seed = picker_selected_row_index(Some(&selection), false, &rows);
    assert_eq!(seed, Some(1));
    assert_eq!(next_picker_highlight(seed, rows.len(), "down"), Some(2));
    assert_eq!(next_picker_highlight(seed, rows.len(), "up"), Some(0));

    // A combo the list does not contain — another provider's row — leaves
    // the cursor unseeded so the arrows open on an edge as before.
    let other = (ProviderKind::Codex, "claude-b".to_owned(), None, false);
    assert_eq!(picker_selected_row_index(Some(&other), false, &rows), None);
    assert_eq!(picker_selected_row_index(None, false, &rows), None);
}

#[test]
fn auto_route_seed_and_filter_follow_the_picker_row() {
    use super::model_picker::{PickerGranularity, picker_selected_row_index};
    use crate::model::ProviderModel;
    use crate::model::ProviderProbe;

    let probe = ProviderProbe {
        provider: ProviderKind::Codex,
        installed: true,
        path: Some(std::path::PathBuf::from("/bin/codex")),
        models: vec![ProviderModel::new("gpt-5.6-sol", "gpt-5.6-sol")],
        agent_presets: Vec::new(),
    };
    let probes = [probe];
    let is_auto = |row: &PickerRow| matches!(row, PickerRow::Policy(PolicyRowId::Auto));

    // Auto heads the unfiltered list when the draft may route, and the
    // routed selection seeds on it — never on a concrete model row.
    let rows = visible_picker_rows(
        &probes,
        &[],
        &[],
        &[],
        None,
        "",
        true,
        PickerGranularity::Combos,
    );
    assert!(is_auto(&rows[0]));
    assert_eq!(rows.len(), 2);
    assert_eq!(picker_selected_row_index(None, true, &rows), Some(0));

    // The same query rules as models: "auto" or "jev" keeps the row,
    // "sonnet" drops it.
    let rows = visible_picker_rows(
        &probes,
        &[],
        &[],
        &[],
        None,
        "aut",
        true,
        PickerGranularity::Combos,
    );
    assert!(is_auto(&rows[0]));
    let rows = visible_picker_rows(
        &probes,
        &[],
        &[],
        &[],
        None,
        "jev",
        true,
        PickerGranularity::Combos,
    );
    assert!(is_auto(&rows[0]));
    let rows = visible_picker_rows(
        &probes,
        &[],
        &[],
        &[],
        None,
        "sonnet",
        true,
        PickerGranularity::Combos,
    );
    assert!(rows.iter().all(|row| !is_auto(row)));

    // Without the flag the list is only concrete models.
    let rows = visible_picker_rows(
        &probes,
        &[],
        &[],
        &[],
        None,
        "",
        false,
        PickerGranularity::Combos,
    );
    assert!(rows.iter().all(|row| !is_auto(row)));
}

#[test]
fn only_opencode_providers_offer_an_explicit_reasoning_default_reset() {
    for provider in ProviderKind::ALL {
        assert_eq!(
            supports_reasoning_default_reset(provider),
            matches!(provider, ProviderKind::OpenCode | ProviderKind::OpenCode2),
            "{provider:?} should{} offer the Default row",
            if matches!(provider, ProviderKind::OpenCode | ProviderKind::OpenCode2) {
                ""
            } else {
                " not"
            }
        );
    }
}

#[test]
fn route_class_rows_lead_with_no_override_then_providers_then_models() {
    use super::model_picker::{PickerGranularity, PickerRowSpec};
    use crate::model::{ProviderModel, ProviderProbe};

    let probe = ProviderProbe {
        provider: ProviderKind::Claude,
        installed: true,
        path: Some(std::path::PathBuf::from("/bin/claude")),
        models: vec![ProviderModel::new("claude-sonnet-5", "Claude Sonnet 5")],
        agent_presets: Vec::new(),
    };
    // The class picker's spec: "No override" leads, each provider's own
    // default heads its block, combos name model + effort.
    let kinds = |query: &str| {
        picker_rows(
            &[probe.clone()],
            &PickerRowSpec {
                leading: &[PolicyRowId::NoOverride],
                provider_defaults: true,
                granularity: PickerGranularity::Efforts,
                favorites: &[],
                pinned: &[],
                recents: &[],
                disabled_providers: &[],
                locked_provider: None,
                normalized_query: query,
            },
        )
        .iter()
        .map(|row| match row {
            PickerRow::Policy(PolicyRowId::NoOverride) => "no-override".to_owned(),
            PickerRow::ProviderDefault(provider) => provider.id().to_owned(),
            PickerRow::Combo(row) => format!("{}:{}", row.provider.id(), row.model.id),
            PickerRow::Policy(PolicyRowId::Auto) => unreachable!("the class spec offers no auto"),
        })
        .collect::<Vec<_>>()
    };

    // The unmapped route leads, then the provider's own default and its
    // catalog models.
    assert_eq!(
        kinds(""),
        ["no-override", "claude", "claude:claude-sonnet-5"]
    );

    // The same token rule as the model picker: "claude" keeps the provider's
    // rows and drops the unmapped lead.
    assert_eq!(kinds("claude"), ["claude", "claude:claude-sonnet-5"]);
    assert_eq!(kinds("sonnet"), ["claude:claude-sonnet-5"]);
}

#[test]
fn settings_search_filters_pages_for_arrow_cycling() {
    use super::SettingsPage;

    let pages = |query: &str| {
        visible_settings_pages(query, true, true, true, true)
            .map(|(page, ..)| page)
            .collect::<Vec<_>>()
    };

    // An empty query keeps every page in sidebar order, so the arrows cycle
    // the full navigation even before anything is typed.
    let mut all_pages = vec![
        SettingsPage::General,
        SettingsPage::Appearance,
        SettingsPage::Keybindings,
    ];
    all_pages.extend([
        SettingsPage::Providers,
        SettingsPage::Skills,
        SettingsPage::Commands,
        SettingsPage::Terminal,
        SettingsPage::Git,
        SettingsPage::Memory,
        SettingsPage::Usage,
        SettingsPage::Archived,
        SettingsPage::Daemon,
        SettingsPage::Friends,
        SettingsPage::ComputerUse,
        SettingsPage::Jev,
        SettingsPage::Integrations,
        SettingsPage::Experiments,
        SettingsPage::Diagnostics,
    ]);
    assert_eq!(pages(""), all_pages);

    // General joins through the local workspace accent's "theme" keyword.
    assert_eq!(
        pages("theme"),
        vec![SettingsPage::General, SettingsPage::Appearance]
    );
    assert_eq!(pages("skill"), vec![SettingsPage::Skills]);

    // A keyword shared across pages keeps them all reachable.
    let codex_pages = vec![
        SettingsPage::Providers,
        SettingsPage::Skills,
        SettingsPage::Usage,
        SettingsPage::ComputerUse,
    ];
    assert_eq!(pages("codex"), codex_pages);

    assert_eq!(pages("no such setting"), vec![]);
    // Friends is experimental: with the opt-in off its row leaves the
    // navigation and the cycle skips it.
    assert!(
        !visible_settings_pages("", true, false, true, true)
            .any(|(page, ..)| page == SettingsPage::Friends)
    );
    // Jev likewise leaves the navigation when no eval-backed experiment is
    // on — its gate is the union of those opt-ins, not one flag.
    assert!(
        !visible_settings_pages("", true, true, false, true)
            .any(|(page, ..)| page == SettingsPage::Jev)
    );
    // Integrations is experimental the same way.
    assert!(
        !visible_settings_pages("", true, true, true, false)
            .any(|(page, ..)| page == SettingsPage::Integrations)
    );
}

#[test]
fn archived_filter_matches_titles_and_projects() {
    let project_a = Uuid::new_v4();
    let project_b = Uuid::new_v4();
    let names: HashMap<Uuid, String> = [
        (project_a, "goddard".to_string()),
        (project_b, "waku".to_string()),
    ]
    .into_iter()
    .collect();

    let mut alpha = AgentSession::new(project_a, ProviderKind::Codex);
    alpha.title = "fix the sidebar".into();
    let mut beta = AgentSession::new(project_b, ProviderKind::Claude);
    beta.title = "usage chart".into();
    let sessions = vec![&alpha, &beta];

    // No query and no project filter keeps every row in the given order.
    assert_eq!(
        filter_archived_sessions(&sessions, "", None, &names, None),
        vec![alpha.id, beta.id]
    );
    // Titles match a normalized query; the project name matches too.
    assert_eq!(
        filter_archived_sessions(&sessions, "sidebar", None, &names, None),
        vec![alpha.id]
    );
    assert_eq!(
        filter_archived_sessions(&sessions, "waku", None, &names, None),
        vec![beta.id]
    );
    // The project filter narrows before the query runs.
    assert_eq!(
        filter_archived_sessions(&sessions, "", Some(project_a), &names, None),
        vec![alpha.id]
    );
    assert_eq!(
        filter_archived_sessions(&sessions, "sidebar", Some(project_b), &names, None),
        Vec::<Uuid>::new()
    );
}

#[test]
fn archived_filter_matches_transcript_hits() {
    use crate::persistence::SessionMessageMatch;

    let project = Uuid::new_v4();
    let names: HashMap<Uuid, String> = [(project, "goddard".to_string())].into_iter().collect();

    let mut alpha = AgentSession::new(project, ProviderKind::Codex);
    alpha.title = "fix the sidebar".into();
    let mut beta = AgentSession::new(project, ProviderKind::Claude);
    beta.title = "usage chart".into();
    let sessions = vec![&alpha, &beta];

    // A transcript match surfaces a row whose title and project miss.
    let matches: HashMap<Uuid, SessionMessageMatch> = [(
        beta.id,
        SessionMessageMatch {
            session_id: beta.id,
            source: MessageRole::User,
            snippet: "a needle in the transcript".into(),
        },
    )]
    .into_iter()
    .collect();
    assert_eq!(
        filter_archived_sessions(&sessions, "needle", None, &names, Some(&matches)),
        vec![beta.id]
    );

    // The project filter still gates content matches.
    let other_project = Uuid::new_v4();
    assert_eq!(
        filter_archived_sessions(
            &sessions,
            "needle",
            Some(other_project),
            &names,
            Some(&matches)
        ),
        Vec::<Uuid>::new()
    );
}

#[test]
fn computer_use_navigation_follows_the_experiment_opt_in() {
    use super::SettingsPage;

    assert!(SettingsPage::General.is_visible_in_navigation(false, false, false, false));
    assert!(!SettingsPage::ComputerUse.is_visible_in_navigation(false, false, false, false));
    assert!(SettingsPage::ComputerUse.is_visible_in_navigation(true, false, false, false));
    assert!(!SettingsPage::Jev.is_visible_in_navigation(false, false, false, false));
    assert!(SettingsPage::Jev.is_visible_in_navigation(false, false, true, false));
    assert!(!SettingsPage::Integrations.is_visible_in_navigation(false, false, false, false));
    assert!(SettingsPage::Integrations.is_visible_in_navigation(false, false, false, true));

    // The experiment flag also removes the page from search results.
    let pages = |query: &str, enabled: bool| {
        visible_settings_pages(query, enabled, true, true, true)
            .map(|(page, ..)| page)
            .collect::<Vec<_>>()
    };
    assert!(!pages("", false).contains(&SettingsPage::ComputerUse));
    assert!(pages("", true).contains(&SettingsPage::ComputerUse));
    assert!(!pages("codex", false).contains(&SettingsPage::ComputerUse));
    assert!(pages("codex", true).contains(&SettingsPage::ComputerUse));
}

#[test]
fn switched_off_providers_leave_the_picker_except_for_their_locked_session() {
    use super::model_picker::PickerGranularity;
    use crate::model::{FavoriteModel, ProviderModel, ProviderProbe};

    let probe = |provider: ProviderKind, model: &str| ProviderProbe {
        provider,
        installed: true,
        path: Some(std::path::PathBuf::from(format!("/bin/{}", provider.id()))),
        models: vec![ProviderModel::new(model, model)],
        agent_presets: Vec::new(),
    };
    let probes = [
        probe(ProviderKind::Claude, "claude-sonnet-5"),
        probe(ProviderKind::Codex, "gpt-5.6-sol"),
    ];
    let favorites = [FavoriteModel {
        provider: ProviderKind::Claude,
        model: "claude-sonnet-5".into(),
        effort: None,
        fast: false,
    }];
    let disabled = [ProviderKind::Claude];

    // The merged list offers only the provider left switched on — its star
    // or its search index cannot resurface the other one's rows.
    let rows = visible_picker_rows(
        &probes,
        &favorites,
        &[],
        &disabled,
        None,
        "",
        false,
        PickerGranularity::Combos,
    );
    assert_eq!(rows.len(), 1);
    assert_eq!(combo(&rows[0]).provider, ProviderKind::Codex);
    let rows = visible_picker_rows(
        &probes,
        &favorites,
        &[],
        &disabled,
        None,
        "claude",
        false,
        PickerGranularity::Combos,
    );
    assert!(rows.is_empty());

    // A session already locked to the provider keeps its models.
    let rows = visible_picker_rows(
        &probes,
        &favorites,
        &[],
        &disabled,
        Some(ProviderKind::Claude),
        "",
        false,
        PickerGranularity::Combos,
    );
    assert_eq!(rows.len(), 1);
    assert_eq!(combo(&rows[0]).provider, ProviderKind::Claude);
}

#[test]
fn model_picker_subtitle_deduplicates_the_provider_name() {
    use super::model_picker::model_picker_subtitle;

    assert_eq!(
        model_picker_subtitle(ProviderKind::DeepSeek, Some("DeepSeek")),
        "DeepSeek"
    );
    assert_eq!(
        model_picker_subtitle(ProviderKind::DeepSeek, Some("OpenAI")),
        "OpenAI · DeepSeek"
    );
}

/// The picker probes a catalog where `model` expands into its own (effort,
/// tier) rows: `efforts` advertises the ladder, `has_fast` the fast tier.
fn picker_probe(
    provider: ProviderKind,
    model: &str,
    efforts: &[&str],
    has_fast: bool,
) -> crate::model::ProviderProbe {
    use crate::model::{ProviderModel, ProviderModelOption, ProviderProbe};
    ProviderProbe {
        provider,
        installed: true,
        path: Some(std::path::PathBuf::from(format!("/bin/{}", provider.id()))),
        models: vec![ProviderModel {
            id: model.into(),
            name: model.into(),
            name_i18n: None,
            sub_provider: None,
            is_default: false,
            reasoning_efforts: efforts
                .iter()
                .map(|effort| ProviderModelOption::new(*effort, *effort))
                .collect(),
            default_reasoning_effort: None,
            service_tiers: has_fast
                .then(|| ProviderModelOption::new("fast", "Fast"))
                .into_iter()
                .collect(),
            default_service_tier: None,
            context_windows: Vec::new(),
            default_context_window: None,
        }],
        agent_presets: Vec::new(),
    }
}

/// A test's view of a combo row — panics on the other variants, which the
/// specs below only produce where the test explicitly checks them.
fn combo(row: &PickerRow) -> &super::model_picker::ModelPickerRow {
    match row {
        PickerRow::Combo(row) => row,
        _ => panic!("expected a combo row"),
    }
}

/// The composer's picker list for tests — the spec the composer passes,
/// minus the parked unstars most tests do not exercise.
#[allow(clippy::too_many_arguments)]
fn visible_picker_rows(
    probes: &[crate::model::ProviderProbe],
    favorites: &[crate::model::FavoriteModel],
    recents: &[crate::persistence::RecentModelUse],
    disabled: &[ProviderKind],
    locked: Option<ProviderKind>,
    normalized_query: &str,
    auto: bool,
    granularity: super::model_picker::PickerGranularity,
) -> Vec<PickerRow> {
    visible_picker_rows_with_pins(
        probes,
        favorites,
        &[],
        recents,
        disabled,
        locked,
        normalized_query,
        auto,
        granularity,
    )
}

/// The composer's picker list for tests — the spec the composer passes.
#[allow(clippy::too_many_arguments)]
fn visible_picker_rows_with_pins(
    probes: &[crate::model::ProviderProbe],
    favorites: &[crate::model::FavoriteModel],
    pinned: &[super::model_picker::PinnedUnfavorite],
    recents: &[crate::persistence::RecentModelUse],
    disabled: &[ProviderKind],
    locked: Option<ProviderKind>,
    normalized_query: &str,
    auto: bool,
    granularity: super::model_picker::PickerGranularity,
) -> Vec<PickerRow> {
    picker_rows(
        probes,
        &super::model_picker::PickerRowSpec {
            leading: if auto { &[PolicyRowId::Auto] } else { &[] },
            provider_defaults: false,
            granularity,
            favorites,
            pinned,
            recents,
            disabled_providers: disabled,
            locked_provider: locked,
            normalized_query,
        },
    )
}

#[test]
fn picker_rows_expand_models_into_effort_and_fast_combos() {
    use super::model_picker::PickerGranularity;

    let probes = [
        picker_probe(ProviderKind::Codex, "gpt", &["low", "high"], true),
        picker_probe(ProviderKind::Claude, "opus", &["max"], false),
        // A model with no effort metadata contributes exactly one row.
        picker_probe(ProviderKind::Cursor, "auto", &[], false),
    ];
    let rows = visible_picker_rows(
        &probes,
        &[],
        &[],
        &[],
        None,
        "",
        false,
        PickerGranularity::Combos,
    );

    assert_eq!(rows.len(), 6);
    // Provider order first (Claude before Codex in `ProviderKind::ALL`),
    // then a model's rows in ladder order, standard before fast.
    let combos: Vec<_> = rows
        .iter()
        .map(|row| (combo(row).effort.as_deref(), combo(row).fast))
        .collect();
    assert_eq!(
        combos,
        [
            (Some("max"), false),
            (Some("low"), false),
            (Some("low"), true),
            (Some("high"), false),
            (Some("high"), true),
            (None, false),
        ]
    );
    assert!(
        rows.iter().any(|row| {
            combo(row).provider == ProviderKind::Cursor && combo(row).effort.is_none()
        })
    );

    // Effort ids and the fast flag are searchable.
    let fast_rows = visible_picker_rows(
        &probes,
        &[],
        &[],
        &[],
        None,
        "fast",
        false,
        PickerGranularity::Combos,
    );
    assert_eq!(fast_rows.len(), 2);
    assert!(fast_rows.iter().all(|row| combo(row).fast));
    let high_rows = visible_picker_rows(
        &probes,
        &[],
        &[],
        &[],
        None,
        "high",
        false,
        PickerGranularity::Combos,
    );
    assert_eq!(high_rows.len(), 2);
}

#[test]
fn picker_structured_tokens_filter_on_row_fields() {
    use super::model_picker::PickerGranularity;

    let probes = [
        picker_probe(ProviderKind::Codex, "gpt", &["low", "high"], true),
        picker_probe(ProviderKind::Claude, "opus", &["max"], false),
        picker_probe(ProviderKind::Pi, "pi-model", &["high"], false),
    ];

    // provider: narrows to one provider's rows.
    let rows = visible_picker_rows(
        &probes,
        &[],
        &[],
        &[],
        None,
        "provider:codex",
        false,
        PickerGranularity::Combos,
    );
    assert_eq!(rows.len(), 4);
    assert!(
        rows.iter()
            .all(|row| combo(row).provider == ProviderKind::Codex)
    );

    // effort: matches the row's effort id exactly.
    let rows = visible_picker_rows(
        &probes,
        &[],
        &[],
        &[],
        None,
        "effort:high",
        false,
        PickerGranularity::Combos,
    );
    assert_eq!(rows.len(), 3);
    assert!(
        rows.iter()
            .all(|row| combo(row).effort.as_deref() == Some("high"))
    );

    // Tokens compose, and mix with free text.
    let rows = visible_picker_rows(
        &probes,
        &[],
        &[],
        &[],
        None,
        "provider:codex effort:high",
        false,
        PickerGranularity::Combos,
    );
    assert_eq!(rows.len(), 2);
    let rows = visible_picker_rows(
        &probes,
        &[],
        &[],
        &[],
        None,
        "provider:codex gpt",
        false,
        PickerGranularity::Combos,
    );
    assert_eq!(rows.len(), 4);

    // Unrecognized values and unknown keys match nothing.
    for query in ["provider:bogus", "effort:bogus", "tier:fast", "provider:"] {
        assert!(
            visible_picker_rows(
                &probes,
                &[],
                &[],
                &[],
                None,
                query,
                false,
                PickerGranularity::Combos
            )
            .is_empty(),
            "{query} should match nothing"
        );
    }
}

#[test]
fn picker_provider_query_toggles_the_provider_token() {
    use super::model_picker::picker_provider_query;

    assert_eq!(picker_provider_query("", "pi"), "provider:pi");
    assert_eq!(picker_provider_query("sonnet", "pi"), "sonnet provider:pi");
    assert_eq!(picker_provider_query("provider:pi", "pi"), "");
    assert_eq!(picker_provider_query("provider:pi sonnet", "pi"), "sonnet");
    assert_eq!(
        picker_provider_query("provider:claude sonnet", "pi"),
        "provider:pi sonnet"
    );
    // Stray provider tokens collapse rather than stacking.
    assert_eq!(
        picker_provider_query("provider:pi provider:claude sonnet", "pi"),
        "sonnet"
    );
}

#[test]
fn picker_query_annotations_wash_only_recognized_values() {
    use super::model_picker::picker_query_annotations;

    let probes = [picker_probe(ProviderKind::Codex, "gpt", &["high"], false)];
    let ranges = picker_query_annotations(
        "provider:pi effort:high provider:bogus effort:bogus foo:bar pi",
        &probes,
    );
    // Only the `pi` and `high` value spans resolve; unknown keys and
    // unrecognized values paint nothing.
    assert_eq!(ranges, vec![(9..11, true), (19..23, true)]);
}

#[test]
fn picker_rows_sort_favorites_then_recents_then_provider_and_name() {
    use super::model_picker::PickerGranularity;
    use crate::model::FavoriteModel;
    use crate::persistence::RecentModelUse;

    // One effort-less model per provider keeps the ordering readable.
    let probes = [
        picker_probe(ProviderKind::Claude, "opus", &[], false),
        picker_probe(ProviderKind::Codex, "gpt", &[], false),
        picker_probe(ProviderKind::Cursor, "auto", &[], false),
    ];
    let favorites = [
        FavoriteModel {
            provider: ProviderKind::Cursor,
            model: "auto".into(),
            effort: None,
            fast: false,
        },
        FavoriteModel {
            provider: ProviderKind::Claude,
            model: "opus".into(),
            effort: None,
            fast: false,
        },
    ];
    let recents = [RecentModelUse {
        provider: ProviderKind::Codex,
        model: "gpt".into(),
        effort: None,
        fast: false,
        used_at: 0,
    }];

    let rows = visible_picker_rows(
        &probes,
        &favorites,
        &recents,
        &[],
        None,
        "",
        false,
        PickerGranularity::Combos,
    );
    let order: Vec<ProviderKind> = rows.iter().map(|row| combo(row).provider).collect();
    // Favorites lead in their stored order, then the recent, then the rest.
    assert_eq!(
        order,
        [
            ProviderKind::Cursor,
            ProviderKind::Claude,
            ProviderKind::Codex
        ]
    );
    assert_eq!(combo(&rows[0]).favorite_index, Some(0));
    assert_eq!(combo(&rows[1]).favorite_index, Some(1));
    assert_eq!(combo(&rows[2]).recent_rank, Some(0));
}

#[test]
fn picker_rows_give_recency_to_the_fast_variant_last_started() {
    use super::model_picker::PickerGranularity;
    use crate::persistence::RecentModelUse;

    let probes = [picker_probe(ProviderKind::Codex, "gpt", &["high"], true)];
    // The stored entry says the last `gpt-high` run was on the fast tier, so
    // only that row may carry the rank — the standard twin sorts as unused.
    let recents = [RecentModelUse {
        provider: ProviderKind::Codex,
        model: "gpt".into(),
        effort: Some("high".into()),
        fast: true,
        used_at: 0,
    }];

    let rows = visible_picker_rows(
        &probes,
        &[],
        &recents,
        &[],
        None,
        "",
        false,
        PickerGranularity::Combos,
    );
    assert_eq!(rows.len(), 2);
    assert!(combo(&rows[0]).fast);
    assert_eq!(combo(&rows[0]).recent_rank, Some(0));
    assert_eq!(combo(&rows[1]).recent_rank, None);
}

#[test]
fn picker_rows_match_a_bare_legacy_favorite_to_the_default_effort_row() {
    use super::model_picker::PickerGranularity;
    use crate::model::FavoriteModel;

    let mut probe = picker_probe(ProviderKind::Codex, "gpt", &["low", "high"], true);
    probe.models[0].default_reasoning_effort = Some("high".into());
    let probes = [probe];
    // A favorite written before rows were combos carries no effort or tier:
    // it claims the model's default-effort row on the standard tier.
    let favorites = [FavoriteModel {
        provider: ProviderKind::Codex,
        model: "gpt".into(),
        effort: None,
        fast: false,
    }];

    let rows = visible_picker_rows(
        &probes,
        &favorites,
        &[],
        &[],
        None,
        "",
        false,
        PickerGranularity::Combos,
    );
    assert_eq!(rows.len(), 4);
    assert_eq!(combo(&rows[0]).favorite_index, Some(0));
    assert_eq!(combo(&rows[0]).effort.as_deref(), Some("high"));
    assert!(!combo(&rows[0]).fast);
    assert!(
        rows[1..]
            .iter()
            .all(|row| combo(row).favorite_index.is_none())
    );
}

#[test]
fn picker_rows_park_unstarred_selections_in_their_favorites_slot() {
    use super::model_picker::{PickerGranularity, PinnedUnfavorite};
    use crate::model::FavoriteModel;

    let probes = [
        picker_probe(ProviderKind::Claude, "claude-a", &[], false),
        picker_probe(ProviderKind::Claude, "claude-b", &[], false),
        picker_probe(ProviderKind::Claude, "claude-c", &[], false),
        picker_probe(ProviderKind::Claude, "claude-d", &[], false),
    ];
    let favorite = |model: &str| FavoriteModel {
        provider: ProviderKind::Claude,
        model: model.into(),
        effort: None,
        fast: false,
    };
    // B came off the middle of [A, B, C]: it leaves `favorite_models` so the
    // ⌘⌥ chords compact, but parks at its old slot until the picker hides.
    let favorites = [favorite("claude-a"), favorite("claude-c")];
    let pinned = [PinnedUnfavorite {
        favorite: favorite("claude-b"),
        position: 1,
    }];

    let rows = visible_picker_rows_with_pins(
        &probes,
        &favorites,
        &pinned,
        &[],
        &[],
        None,
        "",
        false,
        PickerGranularity::Combos,
    );

    assert_eq!(
        rows.iter()
            .take(3)
            .map(|row| combo(row).model.id.as_str())
            .collect::<Vec<_>>(),
        ["claude-a", "claude-b", "claude-c"]
    );
    // The parked row reads unstarred — no star, no chord — while the
    // favorite after it claims the compacted index, and all three keep
    // their display slots.
    assert_eq!(combo(&rows[0]).favorite_index, Some(0));
    assert_eq!(combo(&rows[0]).favorite_rank, Some(0));
    assert_eq!(combo(&rows[1]).favorite_index, None);
    assert_eq!(combo(&rows[1]).favorite_rank, Some(1));
    assert_eq!(combo(&rows[2]).favorite_index, Some(1));
    assert_eq!(combo(&rows[2]).favorite_rank, Some(2));

    // Two stars off keeps both parked rows ordered — A and C hold their
    // original slots even though C left the array first.
    let favorites = [favorite("claude-b")];
    let pinned = [
        PinnedUnfavorite {
            favorite: favorite("claude-a"),
            position: 0,
        },
        PinnedUnfavorite {
            favorite: favorite("claude-c"),
            position: 2,
        },
    ];
    let rows = visible_picker_rows_with_pins(
        &probes,
        &favorites,
        &pinned,
        &[],
        &[],
        None,
        "",
        false,
        PickerGranularity::Combos,
    );
    assert_eq!(
        rows.iter()
            .take(3)
            .map(|row| combo(row).model.id.as_str())
            .collect::<Vec<_>>(),
        ["claude-a", "claude-b", "claude-c"]
    );
    assert!(combo(&rows[0]).favorite_index.is_none() && combo(&rows[2]).favorite_index.is_none());
    assert_eq!(combo(&rows[1]).favorite_index, Some(0));
    assert_eq!(combo(&rows[1]).favorite_rank, Some(1));
}

#[test]
fn normalize_model_combo_decodes_packed_alias_traits() {
    use super::model_picker::normalize_model_combo;

    // The folded catalog lists only the synthesized base; stored spellings
    // like `swe-2-high` survive from before the fold.
    let probes = [picker_probe(
        ProviderKind::Devin,
        "swe-2",
        &["low", "medium", "high", "max"],
        true,
    )];

    // A packed alias resolves to the base plus the effort its suffix names —
    // the combo `session_model_combo` would report for the same pick.
    assert_eq!(
        normalize_model_combo(&probes, ProviderKind::Devin, "swe-2-medium", None, false),
        ("swe-2".to_owned(), Some("medium".to_owned()), false)
    );
    assert_eq!(
        normalize_model_combo(&probes, ProviderKind::Devin, "swe-2-max", None, false),
        ("swe-2".to_owned(), Some("max".to_owned()), false)
    );
    // A stored effort wins over the suffix; a suffix fast tier turns it on.
    assert_eq!(
        normalize_model_combo(
            &probes,
            ProviderKind::Devin,
            "swe-2-medium-fast",
            Some("low".to_owned()),
            false,
        ),
        ("swe-2".to_owned(), Some("low".to_owned()), true)
    );
    // Unresolvable ids keep their stored spelling.
    assert_eq!(
        normalize_model_combo(&probes, ProviderKind::Devin, "swe-3", None, false),
        ("swe-3".to_owned(), None, false)
    );
    assert_eq!(
        normalize_model_combo(&probes, ProviderKind::Devin, "swe-2", None, false),
        ("swe-2".to_owned(), None, false)
    );
}

#[test]
fn the_picker_is_empty_only_once_detection_has_answered() {
    use super::model_picker::picker_has_no_providers;
    use crate::model::{ProviderModel, ProviderProbe};

    let probe = |provider: ProviderKind, installed: bool| ProviderProbe {
        provider,
        installed,
        path: installed.then(|| std::path::PathBuf::from(format!("/bin/{}", provider.id()))),
        models: vec![ProviderModel::new("model", "model")],
        agent_presets: Vec::new(),
    };
    // What every probe looks like before detection answers: seeded with a
    // fallback catalog and not yet installed.
    let undetected = [
        probe(ProviderKind::Claude, false),
        probe(ProviderKind::Codex, false),
    ];
    let detected = [
        probe(ProviderKind::Claude, true),
        probe(ProviderKind::Codex, false),
    ];

    // An unsettled first pass reads as "not known yet", so the composer keeps
    // showing the remembered model instead of flashing an empty state.
    assert!(!picker_has_no_providers(
        &undetected,
        &[],
        None,
        false,
        false
    ));
    // Once it settles, the same probes really do mean nothing is installed.
    assert!(picker_has_no_providers(&undetected, &[], None, false, true));
    // One detected CLI is enough to keep the picker populated...
    assert!(!picker_has_no_providers(&detected, &[], None, false, true));
    // ...until it is switched off, which empties the picker just as surely as
    // never having been installed.
    assert!(picker_has_no_providers(
        &detected,
        &[ProviderKind::Claude],
        None,
        false,
        true
    ));
    // A session already locked to that provider keeps it, switched off or not.
    assert!(!picker_has_no_providers(
        &detected,
        &[ProviderKind::Claude],
        Some(ProviderKind::Claude),
        false,
        true
    ));
}

#[test]
fn the_list_draws_only_installed_providers_the_settings_left_on() {
    use super::model_picker::picker_lists_provider;
    use crate::model::{ProviderModel, ProviderProbe};

    let probe = |provider: ProviderKind, installed: bool| ProviderProbe {
        provider,
        installed,
        path: installed.then(|| std::path::PathBuf::from(format!("/bin/{}", provider.id()))),
        models: vec![ProviderModel::new("model", "model")],
        agent_presets: Vec::new(),
    };
    let probes = [
        probe(ProviderKind::Claude, true),
        probe(ProviderKind::Codex, true),
        probe(ProviderKind::Cursor, false),
    ];

    // An undetected CLI and a switched-off provider both leave the rail
    // outright, rather than sitting in it dimmed.
    assert!(!picker_lists_provider(
        &probes,
        &[],
        None,
        false,
        ProviderKind::Cursor
    ));
    assert!(!picker_lists_provider(
        &probes,
        &[ProviderKind::Claude],
        None,
        false,
        ProviderKind::Claude
    ));

    // A provider only the *current* session locks out stays listed: that is a
    // fact about this session, not about what the user configured.
    assert!(picker_lists_provider(
        &probes,
        &[],
        Some(ProviderKind::Codex),
        false,
        ProviderKind::Claude
    ));

    // ...and the locked session keeps its rows even once it is switched
    // off, since the picker is its only route to another model.
    assert!(picker_lists_provider(
        &probes,
        &[ProviderKind::Claude],
        Some(ProviderKind::Claude),
        false,
        ProviderKind::Claude
    ));
}

#[test]
fn type_to_focus_only_claims_printable_keystrokes() {
    use super::sessions::type_to_focus_text;
    use gpui::{Keystroke, Modifiers};

    let keystroke = |key: &str, key_char: Option<&str>, modifiers: Modifiers| Keystroke {
        key: key.into(),
        key_char: key_char.map(|text| text.to_owned()),
        modifiers,
    };

    // Plain and shifted or Option-composed characters carry their text.
    assert_eq!(
        type_to_focus_text(&keystroke("a", Some("a"), Modifiers::none())),
        Some("a")
    );
    assert_eq!(
        type_to_focus_text(&keystroke("a", Some("A"), Modifiers::shift())),
        Some("A")
    );
    assert_eq!(
        type_to_focus_text(&keystroke("3", Some("£"), Modifiers::alt())),
        Some("£")
    );
    assert_eq!(
        type_to_focus_text(&keystroke(
            "e",
            Some("€"),
            Modifiers::control() | Modifiers::alt()
        )),
        Some("€")
    );
    assert_eq!(
        type_to_focus_text(&keystroke("space", Some(" "), Modifiers::none())),
        Some(" ")
    );

    // Shortcut modifiers never reach the composer this way, even on
    // platforms that still report a key_char for them.
    for modifiers in [
        Modifiers::command(),
        Modifiers::control(),
        Modifiers::function(),
        Modifiers::command_shift(),
    ] {
        assert_eq!(
            type_to_focus_text(&keystroke("c", Some("c"), modifiers)),
            None,
            "{modifiers:?}"
        );
    }

    // Control characters and non-printing keys keep their widget meanings:
    // Tab moves focus, Enter activates, and the rest produce no text.
    assert_eq!(
        type_to_focus_text(&keystroke("enter", Some("\n"), Modifiers::none())),
        None
    );
    assert_eq!(
        type_to_focus_text(&keystroke("tab", Some("\t"), Modifiers::none())),
        None
    );
    assert_eq!(
        type_to_focus_text(&keystroke("escape", None, Modifiers::none())),
        None
    );
    assert_eq!(
        type_to_focus_text(&keystroke("backspace", None, Modifiers::none())),
        None
    );
    assert_eq!(
        type_to_focus_text(&keystroke("f5", None, Modifiers::none())),
        None
    );
}

#[test]
fn workspace_subject_follows_the_overlay_composer() {
    use uuid::Uuid;

    let project_a = Uuid::new_v4();
    let project_b = Uuid::new_v4();
    let mut selected = AgentSession::new(project_a, ProviderKind::Codex);
    selected
        .messages
        .push(Message::new(MessageRole::User, "selected task"));
    let selected_id = selected.id;
    let mut card = AgentSession::new(project_b, ProviderKind::Codex);
    card.messages
        .push(Message::new(MessageRole::User, "running task"));
    let card_id = card.id;
    let draft = AgentSession::new(project_b, ProviderKind::Codex);
    let draft_id = draft.id;
    let sessions = vec![selected, card, draft];

    // Overlay closed: the selection, exactly like before.
    assert_eq!(
        workspace_subject_for(
            false,
            None,
            Some(selected_id),
            Some(project_a),
            None,
            &sessions
        ),
        (Some(selected_id), Some(project_a))
    );

    // Armed card: the card's own session and project, not the selection's.
    assert_eq!(
        workspace_subject_for(
            true,
            Some(card_id),
            Some(selected_id),
            Some(project_a),
            Some(project_a),
            &sessions,
        ),
        (Some(card_id), Some(project_b))
    );

    // Untargeted: the destination project and its unstarted draft.
    assert_eq!(
        workspace_subject_for(
            true,
            None,
            Some(selected_id),
            Some(project_a),
            Some(project_b),
            &sessions,
        ),
        (Some(draft_id), Some(project_b))
    );

    // The destination's started task is not its draft; none exists.
    assert_eq!(
        workspace_subject_for(
            true,
            None,
            Some(selected_id),
            Some(project_a),
            Some(project_a),
            &sessions,
        ),
        (None, Some(project_a))
    );

    // No destination project: no subject at all.
    assert_eq!(
        workspace_subject_for(
            true,
            None,
            Some(selected_id),
            Some(project_a),
            None,
            &sessions
        ),
        (None, None)
    );
}

/// The quit/close gate counts only work quitting would abandon: idle and
/// failed sessions don't count, and neither does a session the app merely
/// watches on a friend's daemon. Busy sessions split by owning daemon —
/// quitting kills the local one while remote hosts keep running.
#[test]
fn busy_close_counts_skip_idle_and_watched_sessions() {
    let mut sessions = vec![
        AgentSession::new(Uuid::new_v4(), ProviderKind::Codex),
        AgentSession::new(Uuid::new_v4(), ProviderKind::Codex),
        AgentSession::new(Uuid::new_v4(), ProviderKind::Codex),
        AgentSession::new(Uuid::new_v4(), ProviderKind::Codex),
        AgentSession::new(Uuid::new_v4(), ProviderKind::Codex),
    ];
    sessions[1].status = SessionStatus::Working;
    sessions[2].status = SessionStatus::Waiting;
    sessions[3].status = SessionStatus::Failed;
    sessions[4].status = SessionStatus::Background;
    let watched = sessions[4].id;
    let remote = sessions[2].id;

    assert_eq!(
        busy_owned_session_counts(&sessions, |id| id == watched, |id| id == remote),
        (1, 1)
    );
    assert_eq!(
        busy_owned_session_counts(&sessions[..1], |_| false, |_| false),
        (0, 0)
    );
}
