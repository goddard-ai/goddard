//! Compatibility exports for Boss context policy, now implemented in `waku-boss`.

use crate::persistence::PersistedState;
use waku_protocol::automations::AutomationsState;
use waku_protocol::boss::BossState;

pub use waku_boss::boss_context::{
    FEATURE, RouterVerdict, WorkContext, apply_verdict, employee_roster, router_questions,
    router_state,
};

/// Keep the existing core API while passing only the locked snapshots across
/// the crate boundary.
pub fn work_context(
    state: &PersistedState,
    boss: &BossState,
    automations: &AutomationsState,
) -> WorkContext {
    waku_boss::boss_context::work_context(&state.projects, &state.sessions, boss, automations)
}
