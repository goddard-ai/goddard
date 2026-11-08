//! Real-window acceptance for the sidebar's group-local Option engagement.
//!
//! Unlike the row-mapping unit tests in `sidebar.rs`, these tests paint a
//! full `Waku` view in GPUI's test platform — against a scripted daemon on
//! a loopback socket — and drive it with simulated pointer, modifier, and
//! keystroke input. Assertions read the engagement cell plus painted
//! element bounds, so coverage is the real input → render → hit-test path,
//! not source inspection.

use super::sidebar::{SessionDateGroup, SidebarGroupAlt};
use super::*;

use std::collections::HashMap;
use std::net::TcpListener;
use std::sync::Mutex;
use std::time::Duration;

use waku_client::{DaemonKey, DaemonSettings, PROTOCOL_VERSION};
use waku_protocol::boss::{
    BossIdentity, BossResourcePolicy, BossState, EmployeeLifecycle,
};
use waku_protocol::{
    ClientMessage, Command, ResponseOutcome, ResponsePayload, ServerMessage,
};

/// Serializes the fixture: `GODDARD_DATA_DIR` is process-global, so tests
/// that install an isolated data dir must not interleave with each other
/// (or with a sibling that would read the user's real dev state).
static WAKU_FIXTURE_ENV: Mutex<()> = Mutex::new(());

/// A scripted stand-in daemon. Accepts every connection the client opens —
/// the supervisor, its health probes, and any independent channels — and
/// answers the handshake plus the requests `Waku::new` and its first frames
/// issue. Requests outside the scripted set get a generic `Ack`; the
/// nil request id is the client's fire-and-forget convention and earns no
/// reply, matching the real daemon.
#[allow(dead_code)]
struct FixtureDaemon {
    address: String,
    sessions: Arc<Mutex<HashMap<Uuid, AgentSession>>>,
}

impl FixtureDaemon {
    fn start(task_state: (Vec<Project>, Vec<AgentSession>, PathBuf)) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let sessions = Arc::new(Mutex::new(
            task_state
                .1
                .iter()
                .map(|session| (session.id, session.clone()))
                .collect(),
        ));
        let fixture = Self {
            address,
            sessions: sessions.clone(),
        };
        let (projects, task_sessions, default_cwd) = task_state;
        std::thread::Builder::new()
            .name("waku-fixture-daemon".into())
            .spawn(move || {
                while let Ok((stream, _)) = listener.accept() {
                    stream.set_read_timeout(Some(Duration::from_secs(60))).ok();
                    let projects = projects.clone();
                    let task_sessions = task_sessions.clone();
                    let default_cwd = default_cwd.clone();
                    let sessions = sessions.clone();
                    std::thread::Builder::new()
                        .name("waku-fixture-daemon-conn".into())
                        .spawn(move || {
                            fixture_daemon_connection(
                                stream,
                                projects,
                                task_sessions,
                                default_cwd,
                                sessions,
                            );
                        })
                        .ok();
                }
            })
            .unwrap();
        fixture
    }

    fn connect(&self) -> waku_client::DaemonSupervisor {
        waku_client::DaemonSupervisor::connect(&self.address, "fixture-token".into()).unwrap()
    }
}

fn fixture_daemon_connection(
    stream: std::net::TcpStream,
    projects: Vec<Project>,
    task_sessions: Vec<AgentSession>,
    default_cwd: PathBuf,
    sessions: Arc<Mutex<HashMap<Uuid, AgentSession>>>,
) {
    let Ok(mut socket) = tungstenite::accept(stream) else {
        return;
    };
    loop {
        let message = match socket.read() {
            Ok(tungstenite::Message::Text(text)) => text,
            Ok(tungstenite::Message::Close(_)) | Err(_) => return,
            Ok(_) => continue,
        };
        let Ok(message) = serde_json::from_str::<ClientMessage>(message.as_ref()) else {
            return;
        };
        match message {
            ClientMessage::Hello { .. } => {
                let reply = ServerMessage::Hello {
                    protocol_version: PROTOCOL_VERSION,
                    daemon_version: "fixture".into(),
                    daemon_commit: None,
                    agent_cli_available: false,
                    task_views_available: false,
                    transcript_windows_available: false,
                    landing_queue_available: false,
                };
                if socket
                    .send(tungstenite::Message::Text(
                        serde_json::to_string(&reply).unwrap().into(),
                    ))
                    .is_err()
                {
                    return;
                }
            }
            ClientMessage::Request(request) if request.request_id.is_nil() => {
                // Fire-and-forget control message — no response.
            }
            ClientMessage::Request(request) => {
                let payload = match request.command {
                    Command::GetSettings => ResponsePayload::Settings {
                        settings: DaemonSettings {
                            boss_experiment_enabled: true,
                            // A probe thread cannot wake the test scheduler.
                            computer_use_experiment_enabled: false,
                            ..DaemonSettings::default()
                        },
                    },
                    Command::LoadTaskState => ResponsePayload::TaskState {
                        projects: projects.clone(),
                        sessions: task_sessions.clone(),
                        default_cwd: default_cwd.clone(),
                        projectless_root: None,
                    },
                    Command::UpdateSettings { .. } => ResponsePayload::Ack,
                    Command::ProbeComputerPermissions { .. } => {
                        ResponsePayload::ComputerPermissions {
                            permissions: waku_protocol::computer_use::ComputerPermissions {
                                screen_recording: false,
                                accessibility: false,
                            },
                        }
                    }
                    Command::HydrateSession { session_id } => ResponsePayload::Session {
                        session: sessions.lock().unwrap().get(&session_id).cloned(),
                    },
                    Command::LoadComposerDrafts => ResponsePayload::ComposerDrafts {
                        drafts: Default::default(),
                    },
                    Command::SaveTaskState { .. } => ResponsePayload::TaskStateSaved {
                        sessions: Vec::new(),
                    },
                    Command::RemoveSession { .. } | Command::SweepBlobs => ResponsePayload::Ack,
                    _ => ResponsePayload::Ack,
                };
                let reply = ServerMessage::Response {
                    request_id: request.request_id,
                    outcome: ResponseOutcome::Ok { payload },
                };
                if socket
                    .send(tungstenite::Message::Text(
                        serde_json::to_string(&reply).unwrap().into(),
                    ))
                    .is_err()
                {
                    return;
                }
            }
            ClientMessage::Shutdown => return,
            _ => {}
        }
    }
}

/// Installs the app-level services `Waku::new` and its render path expect:
/// fonts, localization-free translations, theme, input/keymap, menus, and
/// the host platform hooks `waku-ui` panics without. Mirrors `lib.rs`'s
/// `app.run` preamble minus the pieces that touch the real OS or the
/// user's own state.
fn install_test_app(cx: &mut App) {
    crate::assets::register_fonts(cx).expect("register bundled fonts");
    crate::fonts::prefetch(cx);
    crate::input::init(cx);
    crate::ui::menu::init(cx);
    crate::app::init_composer_autocomplete(cx);
    crate::app::init_sidebar_keys(cx);
    crate::app::init_command_palette(cx);
    // The real platform hooks call `HasWindowHandle::window_handle`, which
    // test windows `unimplemented!` rather than fail — theme application
    // reaches them on every frame. No-ops keep the contract installed.
    waku_ui::host::install_host_platform(waku_ui::host::HostPlatform {
        increase_contrast: || false,
        reduce_transparency: || false,
        set_window_appearance: |_, _| {},
        configure_sidebar_material: |_, _, _, _| {},
    });
    crate::theme::init(cx);
    crate::platform::init_reduce_motion(cx);
    crate::bind_keys(cx);
}

/// A finished, started session row: one user turn, one assistant reply, a
/// reply timestamp that drives both the relative age and — under Option —
/// the calendar date label.
fn fixture_session(project_id: Uuid, title: &str, last_reply_at: u64) -> AgentSession {
    let mut session = AgentSession::new(project_id, ProviderKind::Codex);
    session.title = title.into();
    session.begin_turn("prompt");
    session.push_message(MessageRole::Assistant, "answer");
    if let Some(turn) = session.turns.last_mut() {
        turn.status = TurnStatus::Completed;
        turn.completed_at = Some(last_reply_at);
    }
    session.last_reply_at = Some(last_reply_at);
    session
}

fn boss_state_fixture(identity_seed: u8, employees: Vec<waku_protocol::boss::BossEmployee>) -> BossState {
    BossState {
        employee_prompt_controls: Default::default(),
        identity: BossIdentity {
            id: Uuid::from_u128(0xB0550000_0000_4000_8000_000000000000 | identity_seed as u128),
            name: format!("Boss {identity_seed}"),
            avatar_seed: String::new(),
            avatar_style: Default::default(),
        },
        persona_id: Uuid::from_u128(0xB055A000_0000_4000_8000_000000000000 | identity_seed as u128),
        employee_persona_id: None,
        specialist_persona_ids: Default::default(),
        persona_defaults: Default::default(),
        persona_default_notice: None,
        session_id: None,
        rotation: None,
        personas: Vec::new(),
        employees,
        retired_employees: Default::default(),
        deliverables: Vec::new(),
        goals_viewed_at: None,
        planning: Vec::new(),
        outcomes: Vec::new(),
        instruction_pins: Vec::new(),
        completion_reports: Vec::new(),
        resource_policy: BossResourcePolicy::default(),
        next_sequence: 0,
        next_event_id: 0,
        name_cursor: 0,
        outbox: Vec::new(),
        waves: Vec::new(),
        wave_outbox: Vec::new(),
        chains: Vec::new(),
        revision: 0,
    }
}

fn boss_employee_fixture(
    session_id: Uuid,
    name: &str,
    purpose: Option<waku_protocol::boss::EmployeePurpose>,
    job_title: &str,
) -> waku_protocol::boss::BossEmployee {
    waku_protocol::boss::BossEmployee {
        purpose,
        assignment_kind: Default::default(),
        plan_binding: None,
        plan_readiness: None,
        plan_blockers: Vec::new(),
        session_id,
        supervisor_id: Uuid::new_v4(),
        identity: BossIdentity {
            id: session_id,
            name: name.into(),
            avatar_seed: String::new(),
            avatar_style: Default::default(),
        },
        job_title: job_title.into(),
        persona_id: Uuid::new_v4(),
        work_goal: waku_protocol::boss::EmployeeGoal::Errand,
        created_at: Some(unix_time()),
        icon: None,
        permissions: Default::default(),
        boss_authority: false,
        pinned_files: Vec::new(),
        workspace_transition: false,
        expired_at: None,
        needs_attention: false,
        attention_viewed_at: None,
        blocker: None,
        cancelled: false,
        expiry: None,
        state: EmployeeLifecycle::Working.into(),
        ticket: None,
        queued_at: None,
        request_id: None,
        request_fingerprint: None,
        plan_id: None,
        item_id: None,
        assignment: None,
    }
}

/// The fixture app: a real `Waku` in a test window, its daemon state carved
/// to an isolated data dir, plus the two-boss sidebar arrangement the
/// group-local Option acceptance needs — repeated employee-section headers
/// on `Local` and a remote key, and two date-grouped task rows.
#[allow(dead_code)]
struct WakuFixture {
    view: Entity<Waku>,
    daemon: FixtureDaemon,
    sessions: FixtureSessions,
    _data_dir: PathBuf,
}

#[allow(dead_code)]
struct FixtureSessions {
    project: Project,
    today: AgentSession,
    yesterday: AgentSession,
    boss1_employee: AgentSession,
    boss2_employee: AgentSession,
    remote_key: DaemonKey,
}

fn unix_days_ago(days: u64) -> u64 {
    unix_time().saturating_sub(days * 86_400)
}

/// Build the app under an isolated `GODDARD_DATA_DIR`. The serving
/// worktree's data dir is on this process's environment (the dev watcher
/// exports it); the fixture must never read or write it, so the variable is
/// pointed at a per-test temp dir for construction and then restored.
fn open_waku_fixture(cx: &mut gpui::TestAppContext) -> (WakuFixture, &mut gpui::VisualTestContext) {
    let env_guard = WAKU_FIXTURE_ENV.lock().unwrap();
    let data_dir = std::env::temp_dir().join(format!("waku-sidebar-fixture-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&data_dir).unwrap();
    let previous_data_dir = std::env::var_os("GODDARD_DATA_DIR");
    // SAFETY: the fixture mutex above serializes every test that touches
    // this variable; construction is the only window in which the override
    // is visible.
    unsafe {
        std::env::set_var("GODDARD_DATA_DIR", &data_dir);
    }

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut project = Project::from_path(data_dir.join("fixture-project"));
        project.name = "Fixture Project".into();

        let today = fixture_session(project.id, "today task", unix_time() - 120);
        let yesterday = fixture_session(project.id, "yesterday task", unix_days_ago(1));
        let boss1_employee = fixture_session(project.id, "boss one employee", unix_time() - 60);
        let boss2_employee = fixture_session(project.id, "boss two employee", unix_time() - 30);
        // Boss-managed shells never render as ordinary task rows; the roster
        // sections own them.
        let mut boss1_employee_managed = boss1_employee.clone();
        boss1_employee_managed.boss_managed = true;
        let mut boss2_employee_managed = boss2_employee.clone();
        boss2_employee_managed.boss_managed = true;

        let remote_key = DaemonKey::Remote(Uuid::from_u128(0xBEEF_u128));

        let sessions = vec![
            today.clone(),
            yesterday.clone(),
            boss1_employee_managed.clone(),
            boss2_employee_managed.clone(),
        ];
        let daemon = FixtureDaemon::start((
            vec![project.clone()],
            sessions,
            data_dir.clone(),
        ));
        let supervisor = daemon.connect();

        cx.update(|cx| install_test_app(cx));
        // `Waku::new` builds its own entity, so `add_window_view`'s
        // `Context<V>` closure does not fit — open the window through `App`
        // the way `lib.rs` does and bind a `VisualTestContext` to it.
        let window = cx
            .update(|cx| {
                cx.open_window(
                    gpui::WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(Bounds::maximized(None, cx))),
                        ..Default::default()
                    },
                    |window, cx| Waku::new(window, cx, supervisor),
                )
            })
            .unwrap();
        let view = window.root(cx).unwrap();
        use std::ops::Deref as _;
        let cx = gpui::VisualTestContext::from_window(*window.deref(), cx).into_mut();
        cx.run_until_parked();
        // Boss snapshots reach `boss_ui` through the event channel, which
        // the headless pump no longer wakes — drain until the map settles
        // (each drain can arm a refresh that feeds the next one).
        for _ in 0..4 {
            view.update(cx, |this, cx| this.drain_boss_events(cx));
            cx.run_until_parked();
        }

        (
            view,
            cx,
            project,
            today,
            yesterday,
            boss1_employee_managed,
            boss2_employee_managed,
            remote_key,
            daemon,
        )
    }));

    match previous_data_dir {
        Some(previous) => unsafe {
            std::env::set_var("GODDARD_DATA_DIR", previous);
        },
        None => unsafe {
            std::env::remove_var("GODDARD_DATA_DIR");
        },
    }
    drop(env_guard);

    let (view, cx, project, today, yesterday, boss1_employee, boss2_employee, remote_key, daemon) =
        match result {
            Ok(parts) => parts,
            Err(payload) => std::panic::resume_unwind(payload),
        };

    // Boss states arrive through the same channel the daemon's pushes use:
    // `boss_tx` → `drain_boss_events`, which rebuilds the roster maps the
    // sidebar groups from. Two bosses → two occurrences of each employee
    // section header.
    view.update(cx, |this, cx| {
        this.boss_tx
            .send((
                DaemonKey::Local,
                boss_state_fixture(
                    1,
                    vec![boss_employee_fixture(
                        boss1_employee.id,
                        "One",
                        Some(waku_protocol::boss::EmployeePurpose::Building),
                        "builder one",
                    )],
                ),
            ))
            .unwrap();
        this.boss_tx
            .send((
                remote_key,
                boss_state_fixture(
                    2,
                    vec![boss_employee_fixture(
                        boss2_employee.id,
                        "Two",
                        Some(waku_protocol::boss::EmployeePurpose::Building),
                        "builder two",
                    )],
                ),
            ))
            .unwrap();
        this.drain_boss_events(cx);
        // Expanded groups hold member rows until each avatar raster lands
        // or its real-clock wait lapses. The raster pipeline rides the
        // (test-silent) pump, so the fixture marks the roster released —
        // the production gate itself is covered by pure tests upstream.
        this.sidebar_employee_avatar_released
            .borrow_mut()
            .extend([boss1_employee.id, boss2_employee.id]);
        this.sidebar_rows_fingerprint.set(None);
        cx.notify();
    });
    cx.run_until_parked();

    (
        WakuFixture {
            view,
            daemon,
            sessions: FixtureSessions {
                project,
                today,
                yesterday,
                boss1_employee,
                boss2_employee,
                remote_key,
            },
            _data_dir: data_dir,
        },
        cx,
    )
}

/// The rows snapshot is `Rc`-shared: borrow the painted list to find header
/// positions rather than recomputing them.
fn view_read<'a>(app: &'a App, view: &'a Entity<Waku>) -> &'a Waku {
    view.read(app)
}

fn painted_rows(view: &Entity<Waku>, cx: &mut gpui::VisualTestContext) -> Vec<SidebarRow> {
    cx.read(|app| view.read(app).sidebar_rows_snapshot.borrow().clone().to_vec())
}

#[gpui::test]
fn sidebar_fixture_paints_repeated_group_headers(cx: &mut gpui::TestAppContext) {
    let (fixture, cx) = open_waku_fixture(cx);
    let rows = painted_rows(&fixture.view, cx);
    let headers: Vec<_> = rows
        .iter()
        .enumerate()
        .filter(|(_, row)| matches!(row, SidebarRow::Header(_)))
        .map(|(index, row)| (index, *row))
        .collect();
    assert!(!headers.is_empty(), "sidebar painted no headers: {rows:?}");
    let outcomes: Vec<_> = headers
        .iter()
        .filter(|(_, row)| matches!(row, SidebarRow::Header(SidebarGroup::Outcomes)))
        .collect();
    cx.read(|app| {
        let waku = view_read(app, &fixture.view);
        eprintln!(
            "boss_ui states={:?} hosts={:?} active={:?} managed={:?} purposes={:?} employees={:?}",
            waku.boss_ui.states.keys().collect::<Vec<_>>(),
            waku.boss_ui.hosts,
            waku.boss_ui.active,
            waku.boss_ui.managed,
            waku.boss_ui.employee_purposes,
            waku.boss_ui.states.values().map(|s| s.employees.len()).collect::<Vec<_>>(),
        );
    });
    eprintln!("painted rows: {rows:?}");
    assert_eq!(
        outcomes.len(),
        2,
        "expected two Outcomes headers, one per boss: {headers:?}"
    );
}

fn leaked(text: String) -> &'static str {
    Box::leak(text.into_boxed_str())
}

fn header_row_indexes(rows: &[SidebarRow], group: SidebarGroup) -> Vec<usize> {
    rows.iter()
        .enumerate()
        .filter(|(_, row)| matches!(row, SidebarRow::Header(g) if *g == group))
        .map(|(index, _)| index)
        .collect()
}

fn bounds_of(cx: &mut gpui::VisualTestContext, selector: String) -> gpui::Bounds<gpui::Pixels> {
    cx.debug_bounds(leaked(selector.clone()))
        .unwrap_or_else(|| panic!("{selector} did not paint"))
}

fn engaged_alt(view: &Entity<Waku>, cx: &mut gpui::VisualTestContext) -> Option<SidebarGroupAlt> {
    cx.read(|app| view.read(app).sidebar_group_alt.get())
}

fn header_center(
    cx: &mut gpui::VisualTestContext,
    group: SidebarGroup,
    occurrence: usize,
) -> gpui::Point<gpui::Pixels> {
    bounds_of(
        cx,
        format!(
            "sidebar-group-toggle-{}-{occurrence}",
            group.element_key()
        ),
    )
    .center()
}

fn eye_center(
    cx: &mut gpui::VisualTestContext,
    group: SidebarGroup,
    index: usize,
) -> gpui::Point<gpui::Pixels> {
    bounds_of(
        cx,
        format!("sidebar-group-alt-eye-{}-{index}", group.element_key()),
    )
    .center()
}

fn session_row_center(cx: &mut gpui::VisualTestContext, id: Uuid) -> gpui::Point<gpui::Pixels> {
    bounds_of(cx, format!("session-row-{id}")).center()
}

fn employee_row_center(cx: &mut gpui::VisualTestContext, id: Uuid) -> gpui::Point<gpui::Pixels> {
    bounds_of(cx, format!("employee-row-{id}")).center()
}

fn move_pointer(cx: &mut gpui::VisualTestContext, point: gpui::Point<gpui::Pixels>) {
    cx.simulate_mouse_move(point, None, Modifiers::none());
    cx.run_until_parked();
}

/// The eye only measures open once the header group is hovered — hover the
/// header first, then the revealed control.
fn click_eye(
    cx: &mut gpui::VisualTestContext,
    group: SidebarGroup,
    occurrence: usize,
    index: usize,
) {
    let header = header_center(cx, group, occurrence);
    move_pointer(cx, header);
    let eye = eye_center(cx, group, index);
    move_pointer(cx, eye);
    cx.simulate_click(eye, Modifiers::none());
    cx.run_until_parked();
}

#[gpui::test]
fn click_engagement_survives_member_rows_and_drops_on_foreign_and_outside(
    cx: &mut gpui::TestAppContext,
) {
    let (fixture, cx) = open_waku_fixture(cx);
    let rows = painted_rows(&fixture.view, cx);
    let outcomes = header_row_indexes(&rows, SidebarGroup::Outcomes);
    assert_eq!(outcomes.len(), 2, "fixture rows: {rows:?}");

    // Hover alone must not engage the group, even on repeated entry.
    for _ in 0..2 {
        let header = header_center(cx, SidebarGroup::Outcomes, 0);
        move_pointer(cx, header);
        let eye = eye_center(cx, SidebarGroup::Outcomes, outcomes[0]);
        move_pointer(cx, eye);
        assert_eq!(
            engaged_alt(&fixture.view, cx),
            None,
            "hover engaged the eye"
        );
        move_pointer(cx, header);
    }
    // Clicking engages until the pointer leaves the group.
    click_eye(cx, SidebarGroup::Outcomes, 0, outcomes[0]);
    let engaged = engaged_alt(&fixture.view, cx).expect("eye click did not engage");
    assert_eq!(engaged.row, outcomes[0]);
    assert_eq!(engaged.group, SidebarGroup::Outcomes);
    assert!(!engaged.sticky);

    // The pointer may descend into the group's own member rows — the arm
    // lives while the pointer stays inside the group so the revealed
    // controls remain reachable.
    {
        let p = employee_row_center(cx, fixture.sessions.boss1_employee.id);
        move_pointer(cx, p);
    }
    assert_eq!(
        engaged_alt(&fixture.view, cx),
        Some(engaged),
        "arm dropped inside its own member row"
    );

    // A row outside the group ends a hover arm on entry.
    {
        let p = session_row_center(cx, fixture.sessions.today.id);
        move_pointer(cx, p);
    }
    assert_eq!(
        engaged_alt(&fixture.view, cx),
        None,
        "arm survived a foreign row"
    );

    // Re-arm, then leave the history region entirely — same release.
    click_eye(cx, SidebarGroup::Outcomes, 0, outcomes[0]);
    assert!(engaged_alt(&fixture.view, cx).is_some());
    move_pointer(cx, point(px(800.0), px(500.0)));
    assert_eq!(
        engaged_alt(&fixture.view, cx),
        None,
        "arm survived leaving the sidebar history region"
    );
}

#[gpui::test]
fn second_click_releases_engagement_and_hover_does_not_rearm(cx: &mut gpui::TestAppContext) {
    let (fixture, cx) = open_waku_fixture(cx);
    let rows = painted_rows(&fixture.view, cx);
    let outcomes = header_row_indexes(&rows, SidebarGroup::Outcomes);
    assert_eq!(outcomes.len(), 2, "fixture rows: {rows:?}");

    click_eye(cx, SidebarGroup::Outcomes, 0, outcomes[0]);
    assert!(engaged_alt(&fixture.view, cx).is_some());
    let eye = eye_center(cx, SidebarGroup::Outcomes, outcomes[0]);
    cx.simulate_click(eye, Modifiers::none());
    cx.run_until_parked();
    assert_eq!(
        engaged_alt(&fixture.view, cx),
        None,
        "second click kept engagement"
    );

    let header = header_center(cx, SidebarGroup::Outcomes, 0);
    move_pointer(cx, header);
    let eye = eye_center(cx, SidebarGroup::Outcomes, outcomes[0]);
    move_pointer(cx, eye);
    assert_eq!(
        engaged_alt(&fixture.view, cx),
        None,
        "hover rearmed after toggle off"
    );
}

#[gpui::test]
fn keyboard_pins_and_releases_the_focused_control(cx: &mut gpui::TestAppContext) {
    let (fixture, cx) = open_waku_fixture(cx);
    let rows = painted_rows(&fixture.view, cx);
    let outcomes = header_row_indexes(&rows, SidebarGroup::Outcomes);
    assert_eq!(outcomes.len(), 2, "fixture rows: {rows:?}");

    let focus = cx.read(|app| {
        view_focus_for(&fixture.view, app, SidebarGroup::Outcomes, 0)
    });
    cx.update(|window, cx| window.focus(&focus, cx));
    cx.run_until_parked();

    cx.simulate_keystrokes("enter");
    let engaged = engaged_alt(&fixture.view, cx).expect("Enter did not pin");
    assert_eq!(engaged.row, outcomes[0]);
    assert!(engaged.sticky);

    cx.simulate_keystrokes("enter");
    assert_eq!(engaged_alt(&fixture.view, cx), None, "second Enter kept the pin");

    cx.simulate_keystrokes("space");
    let engaged = engaged_alt(&fixture.view, cx).expect("Space did not pin");
    assert!(engaged.sticky);

    cx.simulate_keystrokes("escape");
    assert_eq!(
        engaged_alt(&fixture.view, cx),
        None,
        "Escape did not release the keyboard pin"
    );
}

fn view_focus_for(
    view: &Entity<Waku>,
    app: &App,
    group: SidebarGroup,
    occurrence: usize,
) -> FocusHandle {
    view.read(app)
        .sidebar_group_alt_focuses
        .borrow()
        .get(&(group, occurrence))
        .unwrap_or_else(|| panic!("no focus handle for {group:?} occurrence {occurrence}"))
        .clone()
}

#[gpui::test]
fn physical_option_applies_every_group_while_a_local_arm_scopes(cx: &mut gpui::TestAppContext) {
    let (fixture, cx) = open_waku_fixture(cx);
    let rows = painted_rows(&fixture.view, cx);
    let today = fixture.sessions.today.id;
    let yesterday = fixture.sessions.yesterday.id;
    let today_headers = header_row_indexes(&rows, SidebarGroup::Date(SessionDateGroup::Today));
    assert_eq!(today_headers.len(), 1, "fixture rows: {rows:?}");

    // Resting state: relative ages, not dates.
    assert!(
        cx.debug_bounds(leaked(format!("session-time-{today}-age")))
            .is_some(),
        "resting row did not paint a relative age"
    );

    // Physical hold: every group's rows substitute dates after the peek
    // delay. The modifiers-changed listeners ride the focus dispatch path,
    // which only reaches the root element once something is focused —
    // production always has focus, so give the eye control the focus it
    // would have after an interaction.
    let focus = cx.read(|app| {
        view_focus_for(&fixture.view, app, SidebarGroup::Outcomes, 0)
    });
    cx.update(|window, cx| window.focus(&focus, cx));
    cx.run_until_parked();
    cx.simulate_modifiers_change(Modifiers {
        alt: true,
        ..Modifiers::none()
    });
    // The peek delay is a background-executor timer; parked pumps leave the
    // fake clock still, so the hold only latches once it is advanced.
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    assert!(
        cx.read(|app| fixture.view.read(app).sidebar_alt_held),
        "Option hold did not latch after the peek delay"
    );
    for id in [today, yesterday] {
        assert!(
            cx.debug_bounds(leaked(format!("session-time-{id}-date")))
                .is_some(),
            "Option hold did not substitute the calendar date on {id}"
        );
    }
    cx.simulate_modifiers_change(Modifiers::none());
    cx.run_until_parked();
    assert!(
        cx.debug_bounds(leaked(format!("session-time-{today}-age")))
            .is_some(),
        "releasing Option left dates substituted"
    );

    // Group-local arm: only the armed group's members substitute.
    let outcomes = header_row_indexes(&rows, SidebarGroup::Outcomes);
    let index = *today_headers.first().unwrap();
    let group = SidebarGroup::Date(SessionDateGroup::Today);
    click_eye(cx, group, 0, index);
    assert_eq!(
        engaged_alt(&fixture.view, cx).map(|alt| alt.row),
        Some(index)
    );
    assert!(
        cx.debug_bounds(leaked(format!("session-time-{today}-date")))
            .is_some(),
        "armed group's member did not substitute the calendar date"
    );
    assert!(
        cx.debug_bounds(leaked(format!("session-time-{yesterday}-age")))
            .is_some(),
        "group-local arm leaked the date substitution into a sibling group"
    );
    let _ = outcomes;
}

#[gpui::test]
fn each_boss_section_arms_independently(cx: &mut gpui::TestAppContext) {
    let (fixture, cx) = open_waku_fixture(cx);
    let rows = painted_rows(&fixture.view, cx);
    let outcomes = header_row_indexes(&rows, SidebarGroup::Outcomes);
    assert_eq!(outcomes.len(), 2, "fixture rows: {rows:?}");

    // Boss two's eye engages boss two's header — the fix's occurrence
    // identity means the twin's handle and hover group are distinct.
    click_eye(cx, SidebarGroup::Outcomes, 1, outcomes[1]);
    let engaged = engaged_alt(&fixture.view, cx).expect("second boss's eye did not arm");
    assert_eq!(
        engaged.row, outcomes[1],
        "engagement landed on the first boss's header row instead"
    );

    click_eye(cx, SidebarGroup::Outcomes, 1, outcomes[1]);
    assert_eq!(engaged_alt(&fixture.view, cx), None);

    // Each boss's focus handle is a distinct registration — keyboard focus
    // on the second must pin the second.
    let focus = cx.read(|app| view_focus_for(&fixture.view, app, SidebarGroup::Outcomes, 1));
    cx.update(|window, cx| window.focus(&focus, cx));
    cx.run_until_parked();
    cx.simulate_keystrokes("enter");
    let engaged = engaged_alt(&fixture.view, cx).expect("keyboard pin missed");
    assert_eq!(engaged.row, outcomes[1]);
    assert!(engaged.sticky);
    cx.simulate_keystrokes("escape");
    assert_eq!(engaged_alt(&fixture.view, cx), None);
}

#[gpui::test]
fn selection_rebuild_and_deactivation_each_end_the_engagement(cx: &mut gpui::TestAppContext) {
    let (fixture, cx) = open_waku_fixture(cx);
    let rows = painted_rows(&fixture.view, cx);
    let outcomes = header_row_indexes(&rows, SidebarGroup::Outcomes);
    assert_eq!(outcomes.len(), 2, "fixture rows: {rows:?}");

    // Selection change: engage, then choose a task — the engagement ends.
    click_eye(cx, SidebarGroup::Outcomes, 0, outcomes[0]);
    assert!(engaged_alt(&fixture.view, cx).is_some());
    let today_row = session_row_center(cx, fixture.sessions.today.id);
    cx.simulate_click(today_row, Modifiers::none());
    cx.run_until_parked();
    assert_eq!(
        engaged_alt(&fixture.view, cx),
        None,
        "selecting a task kept the pinned engagement"
    );

    // A rebuilt snapshot that no longer has the header at the stored index
    // drops the engagement rather than drifting onto a row it did not arm.
    click_eye(cx, SidebarGroup::Outcomes, 0, outcomes[0]);
    assert!(engaged_alt(&fixture.view, cx).is_some());
    fixture.view.update(cx, |this, cx| {
        // Dropping the Local boss removes its whole section — the armed
        // header's stored index no longer lands on its group.
        this.boss_ui.hosts.retain(|key| *key != DaemonKey::Local);
        this.boss_ui.states.remove(&DaemonKey::Local);
        this.sidebar_rows_fingerprint.set(None);
        cx.notify();
    });
    cx.run_until_parked();
    assert_eq!(
        engaged_alt(&fixture.view, cx),
        None,
        "stale engagement survived a regrouping rebuild"
    );

    // Window deactivation: no modifiers-changed event arrives, so the hold
    // and the engagement end together.
    cx.run_until_parked();
    let rows = painted_rows(&fixture.view, cx);
    let outcomes = header_row_indexes(&rows, SidebarGroup::Outcomes);
    click_eye(cx, SidebarGroup::Outcomes, 0, outcomes[0]);
    cx.simulate_modifiers_change(Modifiers {
        alt: true,
        ..Modifiers::none()
    });
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    assert!(engaged_alt(&fixture.view, cx).is_some());
    assert!(cx.read(|app| fixture.view.read(app).sidebar_alt_held));
    // No public test API marks a window active, so `deactivate_window` is a
    // no-op here — drive the deactivation handler directly; the platform
    // wiring is `observe_window_activation` → this handler.
    fixture.view.update(cx, |this, cx| {
        this.sidebar_shortcuts_window_deactivated(cx)
    });
    cx.run_until_parked();
    assert_eq!(
        engaged_alt(&fixture.view, cx),
        None,
        "deactivation kept the engagement"
    );
    assert!(
        !cx.read(|app| fixture.view.read(app).sidebar_alt_held),
        "deactivation kept the Option hold latched"
    );
}
