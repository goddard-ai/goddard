//! Persistent Boss state and compartment-aware file access.

use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context as _, anyhow, bail};
use parking_lot::Mutex;
use uuid::Uuid;
use waku_protocol::boss::{
    BossFile, BossIdentity, BossOperation, BossPersona, BossResult, BossState, PersonaPermissions,
};

const MAX_FILE_BYTES: usize = 256 * 1024;

pub struct BossService {
    root: PathBuf,
    state: Mutex<BossState>,
    notifier: Mutex<Option<crate::share::TaskNotifier>>,
}

impl BossService {
    pub fn open(root: PathBuf) -> anyhow::Result<Self> {
        fs::create_dir_all(root.join("files/memory"))?;
        let path = root.join("boss.json");
        // Fail closed on corruption: replacing the document would lose identity
        // and grants while leaving its private files behind.
        let state = match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).context("invalid Boss document")?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => fresh_state(),
            Err(error) => return Err(error.into()),
        };
        let service = Self {
            root,
            state: Mutex::new(state),
            notifier: Mutex::new(None),
        };
        service.save(&service.state.lock())?;
        Ok(service)
    }

    pub fn document(&self) -> BossState {
        self.state.lock().clone()
    }

    pub fn set_task_notifier(&self, notifier: crate::share::TaskNotifier) {
        *self.notifier.lock() = Some(notifier);
    }

    pub fn is_boss(&self, session: Uuid) -> bool {
        self.state.lock().session_id == Some(session)
    }

    pub fn is_employee(&self, session: Uuid) -> bool {
        self.state
            .lock()
            .employees
            .iter()
            .any(|employee| employee.session_id == session)
    }

    fn require_owner(&self, caller: Option<Uuid>) -> anyhow::Result<()> {
        if caller.is_some_and(|id| !self.is_boss(id)) {
            bail!("only the boss or a human can change personas and Boss files");
        }
        Ok(())
    }

    pub fn handle(
        &self,
        caller: Option<Uuid>,
        operation: BossOperation,
    ) -> anyhow::Result<BossResult> {
        match operation {
            BossOperation::View => {
                let mut state = self.document();
                if let Some(caller) = caller.filter(|id| !self.is_boss(*id)) {
                    let employee = state
                        .employees
                        .iter()
                        .find(|entry| entry.session_id == caller)
                        .ok_or_else(|| anyhow!("this task is not a Boss employee"))?;
                    let persona = employee.persona_id;
                    state.personas.retain(|entry| entry.id == persona);
                    state.employees.retain(|entry| {
                        entry.session_id == caller || entry.supervisor_id == caller
                    });
                }
                Ok(BossResult::State { state })
            }
            BossOperation::Rename { name } => {
                self.require_owner(caller)?;
                validate_name(&name)?;
                self.update(|state| {
                    state.identity.name = name.trim().to_owned();
                    Ok(())
                })?;
                Ok(BossResult::State {
                    state: self.document(),
                })
            }
            BossOperation::UpsertPersona { mut persona } => {
                self.require_owner(caller)?;
                validate_name(&persona.name)?;
                if persona.markdown.len() > MAX_FILE_BYTES {
                    bail!("persona is too large");
                }
                for path in &persona.knowledge_files {
                    self.file_path(path, false)?;
                }
                for folder in &persona.permissions.memory_folders {
                    validate_relative(folder, false)?;
                    self.file_path(&format!("memory/{folder}"), false)?;
                }
                if persona.id.is_nil() {
                    persona.id = Uuid::new_v4();
                }
                self.update(|state| {
                    if let Some(existing) = state
                        .personas
                        .iter_mut()
                        .find(|entry| entry.id == persona.id)
                    {
                        *existing = persona;
                    } else {
                        state.personas.push(persona);
                    }
                    Ok(())
                })?;
                Ok(BossResult::State {
                    state: self.document(),
                })
            }
            BossOperation::ListFiles { path } => {
                self.authorize_file(caller, &path, true)?;
                let directory = self.file_path(&path, true)?;
                let mut files = Vec::new();
                for entry in fs::read_dir(directory)? {
                    let entry = entry?;
                    let kind = entry.file_type()?;
                    if kind.is_symlink() {
                        continue;
                    }
                    let name = entry.file_name().to_string_lossy().into_owned();
                    let child = if path.is_empty() {
                        name
                    } else {
                        format!("{path}/{name}")
                    };
                    if self.authorize_file(caller, &child, kind.is_dir()).is_ok() {
                        files.push(BossFile {
                            path: child,
                            directory: kind.is_dir(),
                        });
                    }
                }
                files.sort_by(|a, b| b.directory.cmp(&a.directory).then(a.path.cmp(&b.path)));
                Ok(BossResult::Files { files })
            }
            BossOperation::ReadFile { path } => {
                self.authorize_file(caller, &path, false)?;
                let file = self.file_path(&path, false)?;
                if fs::metadata(&file)?.len() > MAX_FILE_BYTES as u64 {
                    bail!("Boss file is too large");
                }
                Ok(BossResult::File {
                    path,
                    content: fs::read_to_string(file)?,
                })
            }
            BossOperation::WriteFile { path, content } => {
                self.require_owner(caller)?;
                if content.len() > MAX_FILE_BYTES {
                    bail!("Boss file is too large");
                }
                if path.starts_with("personas/") && path.ends_with("/PERSONA.md") {
                    bail!("edit persona Markdown using upsertPersona");
                }
                let file = self.file_path(&path, false)?;
                if let Some(parent) = file.parent() {
                    fs::create_dir_all(parent)?;
                }
                atomic_write(&file, content.as_bytes())?;
                self.update(|_| Ok(()))?;
                Ok(BossResult::Saved)
            }
            BossOperation::CreateFolder { path } => {
                self.require_owner(caller)?;
                fs::create_dir_all(self.file_path(&path, false)?)?;
                self.update(|_| Ok(()))?;
                Ok(BossResult::Saved)
            }
        }
    }

    pub(crate) fn update(
        &self,
        change: impl FnOnce(&mut BossState) -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        let mut state = self.state.lock();
        let mut next = state.clone();
        change(&mut next)?;
        next.revision = next.revision.saturating_add(1);
        self.save(&next)?;
        *state = next;
        drop(state);
        if let Some(notifier) = self.notifier.lock().clone() {
            notifier();
        }
        Ok(())
    }

    fn save(&self, state: &BossState) -> anyhow::Result<()> {
        for persona in &state.personas {
            let directory = self.file_path(&format!("personas/{}", persona.id), false)?;
            fs::create_dir_all(&directory)?;
            atomic_write(&directory.join("PERSONA.md"), persona.markdown.as_bytes())?;
        }
        atomic_write(
            &self.root.join("boss.json"),
            &serde_json::to_vec_pretty(state)?,
        )
    }

    fn authorize_file(
        &self,
        caller: Option<Uuid>,
        path: &str,
        directory: bool,
    ) -> anyhow::Result<()> {
        validate_relative(path, directory)?;
        if caller.is_none() || caller.is_some_and(|id| self.is_boss(id)) {
            return Ok(());
        }
        let state = self.state.lock();
        let employee = state
            .employees
            .iter()
            .find(|entry| Some(entry.session_id) == caller)
            .ok_or_else(|| anyhow!("this task is not a Boss employee"))?;
        if employee.expired {
            bail!("this employee has expired");
        }
        let persona = state
            .personas
            .iter()
            .find(|entry| entry.id == employee.persona_id)
            .ok_or_else(|| anyhow!("employee persona is unavailable"))?;
        let permitted = employee
            .permissions
            .memory_folders
            .iter()
            .map(|folder| format!("memory/{folder}"))
            .any(|folder| path == folder || path.starts_with(&format!("{folder}/")))
            || persona.knowledge_files.iter().any(|file| file == path)
            || path == format!("personas/{}/PERSONA.md", persona.id);
        // Directory discovery reveals only ancestors of a granted file/folder.
        let ancestor = directory
            && (path.is_empty()
                || employee
                    .permissions
                    .memory_folders
                    .iter()
                    .map(|folder| format!("memory/{folder}"))
                    .chain(persona.knowledge_files.iter().cloned())
                    .any(|file| file.starts_with(&format!("{path}/"))));
        if !permitted && !ancestor {
            bail!("persona does not grant access to this Boss file");
        }
        Ok(())
    }

    fn file_path(&self, path: &str, allow_empty: bool) -> anyhow::Result<PathBuf> {
        validate_relative(path, allow_empty)?;
        let mut result = self.root.join("files");
        for component in Path::new(path).components() {
            result.push(component);
            match fs::symlink_metadata(&result) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    bail!("symlinks are not allowed in Boss files")
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(result)
    }
}

fn validate_relative(path: &str, allow_empty: bool) -> anyhow::Result<()> {
    if (!allow_empty && path.is_empty())
        || path.contains('\\')
        || path.contains(':')
        || Path::new(path)
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        bail!("Boss paths must be relative and cannot contain traversal");
    }
    Ok(())
}

fn validate_name(name: &str) -> anyhow::Result<()> {
    if name.trim().is_empty() || name.chars().count() > 100 {
        bail!("name must contain 1–100 characters");
    }
    Ok(())
}

fn atomic_write(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let temporary = path.with_extension(format!("tmp-{}", Uuid::new_v4()));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary)?;
    std::io::Write::write_all(&mut file, bytes)?;
    file.sync_all()?;
    drop(file);
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(temporary);
        return Err(error.into());
    }
    Ok(())
}

fn fresh_state() -> BossState {
    let id = Uuid::new_v4();
    let persona_id = Uuid::new_v4();
    let employee_id = Uuid::new_v4();
    let names = ["Atlas", "Nova", "Sage", "Orion", "Clover", "Quinn"];
    BossState {
        identity: BossIdentity { id, name: names[id.as_bytes()[0] as usize % names.len()].into(), avatar_seed: id.to_string() },
        persona_id,
        session_id: None,
        personas: vec![
            BossPersona { id: persona_id, name: "Boss".into(), markdown: "You coordinate employees for the human. Delegate execution promptly and keep your hands free for their next request. Maintain personas, your own files, and compartmentalized memories. Prefer employees for internet access and work outside your own storage. You control all employees and personas.".into(), knowledge_files: Vec::new(), permissions: PersonaPermissions { summon_employees: true, ..Default::default() } },
            BossPersona { id: employee_id, name: "Employee".into(), markdown: "Complete the bounded job assigned by your supervisor. Report useful results concisely. You have no memory of your own and must not write memory. Read only the memory and knowledge granted to your persona.".into(), knowledge_files: Vec::new(), permissions: PersonaPermissions::default() },
        ],
        employees: Vec::new(),
        revision: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use waku_protocol::boss::BossEmployee;

    #[test]
    fn identity_personas_and_files_survive_restart() {
        let root = std::env::temp_dir().join(format!("boss-{}", Uuid::new_v4()));
        let service = BossService::open(root.clone()).unwrap();
        let state = service.document();
        service
            .handle(
                None,
                BossOperation::WriteFile {
                    path: "memory/work/notes.md".into(),
                    content: "Remember the release deadline".into(),
                },
            )
            .unwrap();
        drop(service);
        let restored = BossService::open(root.clone()).unwrap();
        assert_eq!(restored.document().identity.id, state.identity.id);
        assert_eq!(
            restored.document().identity.avatar_seed,
            state.identity.avatar_seed
        );
        assert!(
            root.join(format!("files/personas/{}/PERSONA.md", state.persona_id))
                .is_file()
        );
        assert!(
            matches!(restored.handle(None, BossOperation::ReadFile { path: "memory/work/notes.md".into() }).unwrap(), BossResult::File { content, .. } if content == "Remember the release deadline")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn employees_cannot_read_ungranted_memory_or_mutate_state() {
        let root = std::env::temp_dir().join(format!("boss-{}", Uuid::new_v4()));
        let service = BossService::open(root.clone()).unwrap();
        let state = service.document();
        let session_id = Uuid::new_v4();
        service
            .update(|state| {
                state.employees.push(BossEmployee {
                    session_id,
                    supervisor_id: Uuid::new_v4(),
                    identity: BossIdentity {
                        id: session_id,
                        name: "Release".into(),
                        avatar_seed: session_id.to_string(),
                    },
                    persona_id: state.personas[1].id,
                    permissions: PersonaPermissions {
                        memory_folders: vec!["work".into()],
                        ..Default::default()
                    },
                    expired: false,
                });
                Ok(())
            })
            .unwrap();
        for (path, content) in [
            ("memory/work/note.md", "public"),
            ("memory/private/note.md", "private"),
        ] {
            service
                .handle(
                    None,
                    BossOperation::WriteFile {
                        path: path.into(),
                        content: content.into(),
                    },
                )
                .unwrap();
        }
        assert!(
            service
                .handle(
                    Some(session_id),
                    BossOperation::ReadFile {
                        path: "memory/work/note.md".into()
                    }
                )
                .is_ok()
        );
        for path in [
            "memory/private/note.md",
            "memory/work/../private/note.md",
            "../boss.json",
            "/etc/passwd",
        ] {
            assert!(
                service
                    .handle(
                        Some(session_id),
                        BossOperation::ReadFile { path: path.into() }
                    )
                    .is_err()
            );
        }
        assert!(
            service
                .handle(
                    Some(session_id),
                    BossOperation::UpsertPersona {
                        persona: state.personas[0].clone()
                    }
                )
                .is_err()
        );
        assert!(
            service
                .handle(
                    Some(session_id),
                    BossOperation::WriteFile {
                        path: "memory/work/note.md".into(),
                        content: "changed".into()
                    }
                )
                .is_err()
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_cannot_escape_the_files_root() {
        let root = std::env::temp_dir().join(format!("boss-{}", Uuid::new_v4()));
        let service = BossService::open(root.clone()).unwrap();
        std::os::unix::fs::symlink(std::env::temp_dir(), root.join("files/outside")).unwrap();
        assert!(
            service
                .handle(
                    None,
                    BossOperation::WriteFile {
                        path: "outside/escape.md".into(),
                        content: "no".into()
                    }
                )
                .is_err()
        );
        fs::remove_dir_all(root).unwrap();
    }
}
