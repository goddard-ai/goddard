//! Best-effort retirement of derived indexes, never workspace cleanup authority.

use std::io::{self, Read as _};

use waku_base::maintenance::{Progress, Scheduler, Unit};

use super::{
    CHECKPOINT_CAPTURE_PENDING, CHECKPOINT_INDEX_NAME, CHECKPOINT_INDEX_STATE_NAME, Uuid, fs,
    lock_capture_file,
};
use std::path::{Path, PathBuf};

const ENTRIES_PER_UNIT: usize = 64;
const DELETIONS_PER_UNIT: usize = 8;
const HEADER_BYTES: usize = 96;

pub(super) fn schedule(git_dir: &Path, generation: Option<Uuid>) -> io::Result<()> {
    let key = format!(
        "checkpoint-cache-retirement:{}:{generation:?}",
        git_dir.display()
    );
    let mut sweep = Sweep {
        git_dir: git_dir.to_owned(),
        entries: None,
    };
    Scheduler::shared()?.submit(key, Unit::Bounded, move |permit| {
        sweep.step(|| permit.should_continue())
    })?;
    Ok(())
}

struct Sweep {
    git_dir: PathBuf,
    entries: Option<fs::ReadDir>,
}

impl Sweep {
    fn step(&mut self, should_continue: impl Fn() -> bool) -> io::Result<Progress> {
        if !should_continue() {
            return Ok(Progress::More);
        }
        let _capture = match lock_capture_file(&self.git_dir) {
            Ok(gate) => gate,
            Err(error) => {
                return match error.downcast_ref::<io::Error>().map(io::Error::kind) {
                    Some(io::ErrorKind::WouldBlock) => Ok(Progress::More),
                    Some(io::ErrorKind::NotFound) => Ok(Progress::Complete),
                    _ => Err(io::Error::other("cannot acquire checkpoint cache gate")),
                };
            }
        };
        // A pending capture may still have a Git child writing after its parent
        // exited. Keep all lanes until a successful capture publishes its cache.
        if present(&self.git_dir.join(CHECKPOINT_CAPTURE_PENDING))? {
            return Ok(Progress::Complete);
        }
        let Some(current) = current_index_name(&self.git_dir)? else {
            // Unknown metadata is not permission to delete any cache.
            return Ok(Progress::Complete);
        };
        if self.entries.is_none() {
            self.entries = Some(fs::read_dir(&self.git_dir)?);
        }
        let entries = self.entries.as_mut().expect("opened checkpoint directory");
        let mut deletions = 0;
        for _ in 0..ENTRIES_PER_UNIT {
            if !should_continue() {
                return Ok(Progress::More);
            }
            let Some(entry) = entries.next() else {
                self.entries = None;
                return Ok(Progress::Complete);
            };
            let entry = entry?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if name == current || !is_rotated_index(name) || !entry.file_type()?.is_file() {
                continue;
            }
            // Never unlink a lock or assume that its parent is the final writer.
            // symlink_metadata also protects broken symlinks and directories.
            if present(&self.git_dir.join(format!("{name}.lock")))? {
                continue;
            }
            if !should_continue() {
                // Revisit this entry after admission returns instead of losing
                // a candidate solely because the unit exhausted its budget.
                self.entries = None;
                return Ok(Progress::More);
            }
            match fs::remove_file(entry.path()) {
                Ok(()) => deletions += 1,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
            if deletions == DELETIONS_PER_UNIT {
                return Ok(Progress::More);
            }
        }
        Ok(Progress::More)
    }
}

fn present(path: &Path) -> io::Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn is_rotated_index(name: &str) -> bool {
    let Some(value) = name
        .strip_prefix(CHECKPOINT_INDEX_NAME)
        .and_then(|value| value.strip_prefix('-'))
    else {
        return false;
    };
    Uuid::parse_str(value).is_ok_and(|uuid| uuid.to_string() == value)
}

fn current_index_name(git_dir: &Path) -> io::Result<Option<String>> {
    let path = git_dir.join(CHECKPOINT_INDEX_STATE_NAME);
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.is_file() => {}
        Ok(_) => return Ok(None),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    }
    // index_generation is the first field in our private v2 serializer. Read
    // only its bounded prefix, never the potentially huge untracked-path list.
    // Unrecognized serializers/corruption retain every lane. The fixed baseline
    // is always retained because full-state parse failure can fall back to it.
    let mut bytes = Vec::with_capacity(HEADER_BYTES);
    fs::File::open(path)?
        .take(HEADER_BYTES as u64)
        .read_to_end(&mut bytes)?;
    let Some(value) = bytes.strip_prefix(b"{\"index_generation\":") else {
        return Ok(None);
    };
    let Some(end) = value.iter().position(|byte| *byte == b',') else {
        return Ok(None);
    };
    let Ok(generation) = serde_json::from_slice::<Option<Uuid>>(&value[..end]) else {
        return Ok(None);
    };
    Ok(Some(match generation {
        Some(uuid) => format!("{CHECKPOINT_INDEX_NAME}-{uuid}"),
        None => CHECKPOINT_INDEX_NAME.to_owned(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checkpoint::{CHECKPOINT_CAPTURE_LOCK, CheckpointIndexState};

    fn publish(git_dir: &Path, generation: Option<Uuid>) {
        let state = CheckpointIndexState {
            index_generation: generation,
            // Use the real serializer and a body larger than the reader budget.
            untracked_paths: vec![vec![b'x'; 256]],
            ..Default::default()
        };
        fs::write(
            git_dir.join(CHECKPOINT_INDEX_STATE_NAME),
            serde_json::to_vec(&state).unwrap(),
        )
        .unwrap();
    }

    fn rotated(git_dir: &Path) -> (Uuid, PathBuf) {
        let id = Uuid::new_v4();
        let path = git_dir.join(format!("{CHECKPOINT_INDEX_NAME}-{id}"));
        fs::write(&path, b"derived index").unwrap();
        (id, path)
    }

    fn sweep(git_dir: &Path) -> Sweep {
        Sweep {
            git_dir: git_dir.to_owned(),
            entries: None,
        }
    }

    fn finish(sweep: &mut Sweep) {
        for _ in 0..100 {
            if matches!(sweep.step(|| true).unwrap(), Progress::Complete) {
                return;
            }
        }
        panic!("bounded cache sweep did not converge");
    }

    #[test]
    fn retirement_yields_and_preserves_current_locked_and_uncertain_cache_files() {
        let directory = std::env::temp_dir().join(format!("waku-cache-retire-{}", Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let (current_id, current) = rotated(&directory);
        publish(&directory, Some(current_id));
        let obsolete: Vec<_> = (0..24).map(|_| rotated(&directory)).collect();
        let (_, locked) = rotated(&directory);
        let lock = locked.with_extension("lock");
        fs::write(&lock, b"possibly live Git child").unwrap();
        let retained = [
            CHECKPOINT_INDEX_NAME,
            "index",
            "index.lock",
            "waku-checkpoint-index",
            "waku-checkpoint-index.lock",
            "waku-checkpoint-index.json",
            "waku-checkpoint-v2-index-not-a-uuid",
            "waku-checkpoint-v2-index.json.tmp",
        ];
        for name in retained {
            fs::write(directory.join(name), b"preserve these bytes").unwrap();
        }
        let folder = directory.join(format!("{CHECKPOINT_INDEX_NAME}-{}", Uuid::new_v4()));
        fs::create_dir(&folder).unwrap();
        fs::write(folder.join("sentinel"), b"do not recurse").unwrap();
        #[cfg(unix)]
        let symlinks = {
            use std::os::unix::fs::symlink;
            let link = directory.join(format!("{CHECKPOINT_INDEX_NAME}-{}", Uuid::new_v4()));
            symlink(directory.join("index"), &link).unwrap();
            let (_, broken_locked) = rotated(&directory);
            let broken_lock = broken_locked.with_extension("lock");
            symlink(directory.join("missing"), &broken_lock).unwrap();
            (link, broken_locked, broken_lock)
        };

        let mut job = sweep(&directory);
        assert!(matches!(job.step(|| false).unwrap(), Progress::More));
        assert!(!directory.join(CHECKPOINT_CAPTURE_LOCK).exists());
        assert!(obsolete.iter().all(|(_, path)| path.exists()));
        let gate = lock_capture_file(&directory).unwrap();
        assert!(matches!(job.step(|| true).unwrap(), Progress::More));
        assert!(obsolete.iter().all(|(_, path)| path.exists()));
        drop(gate);

        assert!(matches!(job.step(|| true).unwrap(), Progress::More));
        let deleted = obsolete.iter().filter(|(_, path)| !path.exists()).count();
        assert_eq!(deleted, DELETIONS_PER_UNIT);
        // A capture between admitted units can select a previously obsolete
        // lane. An old cursor must re-read and respect the new publication.
        let (next_id, next) = obsolete.iter().find(|(_, path)| path.exists()).unwrap();
        publish(&directory, Some(*next_id));
        finish(&mut job);
        assert!(next.exists());
        assert!(
            obsolete
                .iter()
                .all(|(id, path)| id == next_id || !path.exists())
        );
        finish(&mut sweep(&directory));
        assert!(!current.exists());
        assert_eq!(fs::read(&locked).unwrap(), b"derived index");
        assert_eq!(fs::read(&lock).unwrap(), b"possibly live Git child");
        for name in retained {
            assert_eq!(
                fs::read(directory.join(name)).unwrap(),
                b"preserve these bytes"
            );
        }
        assert_eq!(
            fs::read(folder.join("sentinel")).unwrap(),
            b"do not recurse"
        );
        #[cfg(unix)]
        {
            assert!(fs::symlink_metadata(&symlinks.0).unwrap().is_symlink());
            assert!(symlinks.1.exists());
            assert!(fs::symlink_metadata(&symlinks.2).unwrap().is_symlink());
        }

        let (_, uncertain) = rotated(&directory);
        let pending = directory.join(CHECKPOINT_CAPTURE_PENDING);
        fs::write(&pending, b"interrupted capture").unwrap();
        finish(&mut sweep(&directory));
        assert!(uncertain.exists());
        fs::remove_file(&pending).unwrap();
        fs::write(
            directory.join(CHECKPOINT_INDEX_STATE_NAME),
            b"unknown metadata",
        )
        .unwrap();
        finish(&mut sweep(&directory));
        assert!(uncertain.exists());
        // A later successful capture retries retained work. Null generation
        // preserves the fixed baseline but can retire the last UUID lane.
        publish(&directory, None);
        fs::remove_file(&lock).unwrap();
        finish(&mut sweep(&directory));
        assert!(!uncertain.exists());
        assert!(!next.exists());
        assert!(!locked.exists());
        assert!(directory.join(CHECKPOINT_INDEX_NAME).exists());
        fs::remove_dir_all(directory).unwrap();
    }
}
