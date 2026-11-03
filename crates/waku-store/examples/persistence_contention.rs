//! Reproducible synthetic contention probe; never opens application data.
use std::sync::{Arc, Barrier};
use std::time::Instant;
use waku_boss::BossService;
use waku_protocol::model::{AgentSession, MessageRole, ProviderKind};
use waku_store::persistence::{PersistedState, StateStore};

fn main() -> anyhow::Result<()> {
    let rounds: usize = std::env::args()
        .nth(1)
        .map(|arg| arg.parse())
        .transpose()?
        .unwrap_or(100);
    anyhow::ensure!(
        (1..=10_000).contains(&rounds),
        "rounds must be between 1 and 10000"
    );
    let root = std::env::temp_dir().join(format!(
        "goddard-persistence-probe-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root)?;
    let result = (|| -> anyhow::Result<()> {
        let path = root.join("app.db");
        let initializer = StateStore::daemon(path.clone());
        let mut template = PersistedState::fresh(root.clone());
        initializer.save(&mut template)?;
        let project = template.projects[0].clone();
        let boss = Arc::new(BossService::open_database(root.join("boss"), path.clone())?);
        let barrier = Arc::new(Barrier::new(3));
        let started = Instant::now();
        let mut workers = Vec::new();
        for lane in 0..3 {
            let path = path.clone();
            let project = project.clone();
            let boss = boss.clone();
            let barrier = barrier.clone();
            workers.push(std::thread::spawn(move || -> anyhow::Result<()> {
                let store = StateStore::daemon(path);
                let mut state = PersistedState::empty();
                state.projects.push(project.clone());
                let mut session = AgentSession::new(project.id, ProviderKind::Codex);
                session.boss_managed = lane == 1;
                session.begin_turn("synthetic persistence workload");
                let id = session.id;
                state.push_session(session);
                barrier.wait();
                for _ in 0..rounds {
                    match lane {
                        0 => {
                            state
                                .session_mut(id)
                                .unwrap()
                                .push_message(MessageRole::Assistant, "synthetic response chunk");
                            store.save(&mut state)?;
                        }
                        1 => {
                            state
                                .session_mut(id)
                                .unwrap()
                                .push_message(MessageRole::Assistant, "synthetic shared change");
                            let batch = store.save_batch(&mut state);
                            boss.set_session_id_with(id, &mut store.session_commit(&batch))?;
                            state.unmark_sessions_dirty(&batch.dirty_ids);
                        }
                        _ => boss.set_session_id(uuid::Uuid::new_v4())?,
                    }
                }
                Ok(())
            }));
        }
        let mut failure = None;
        for worker in workers {
            let result = worker
                .join()
                .map_err(|_| anyhow::anyhow!("persistence worker panicked"))
                .and_then(|result| result);
            if let Err(error) = result {
                failure.get_or_insert(error);
            }
        }
        if let Some(error) = failure {
            return Err(error);
        }
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "workload": "three concurrent workers: transcript, shared session/Boss, standalone Boss",
                "rounds_per_worker": rounds,
                "elapsed_ms": started.elapsed().as_secs_f64()*1000.0,
                "metrics": waku_base::persistence_metrics::snapshot(),
            }))?
        );
        Ok(())
    })();
    std::fs::remove_dir_all(&root)?;
    result
}
