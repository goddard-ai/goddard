//! Periodic process-memory sampling for leak debugging.
//!
//! A `waku-stats` thread appends one JSON line per minute to
//! `daemon-stats.jsonl` in the daemon's data directory — the daemon's own
//! RSS plus the whole descendant tree's RSS, because provider runtimes and
//! terminals carry their memory under their own pids. The file is
//! self-capping, so an agent debugging a leak can tail it, and
//! `getDaemonStats` answers with the latest sample plus the last sample the
//! previous boot wrote — the pre-restart reading that explains an
//! unexpected exit.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use waku_protocol::DaemonStatsSample;

/// One sample per minute keeps the process-table walk cheap while catching
/// growth between turns; eviction leaks move on hour timescales anyway.
const SAMPLE_INTERVAL: Duration = Duration::from_secs(60);
/// ~512 KB at ~150 bytes a line is several days of minute-cadence samples;
/// the tail rewrite keeps the freshest half. The panic log shares the cap.
const STATS_FILE_CAP: u64 = 512 * 1024;
const STATS_FILE_NAME: &str = "daemon-stats.jsonl";
const PANIC_FILE_NAME: &str = "daemon-panics.jsonl";

/// The sampler's shared state: what it last wrote, and the previous boot's
/// final line captured before this process appended anything.
pub struct DaemonStats {
    boot: String,
    path: PathBuf,
    /// Session/terminal counters for samples and the shutdown marker —
    /// set when the sampler starts.
    counts: Mutex<Option<Box<dyn Fn() -> (u32, u32) + Send + Sync>>>,
    state: Mutex<DaemonStatsState>,
}

#[derive(Default)]
struct DaemonStatsState {
    previous_boot: Option<DaemonStatsSample>,
    /// Whether the previous boot's last line carried the clean-exit
    /// marker — `false` reads as an abnormal death: jetsam, a SIGKILL,
    /// or a hard crash.
    previous_boot_clean: bool,
    latest: Option<DaemonStatsSample>,
}

/// The on-disk line: the sample plus the boot id that wrote it. A line
/// flagged `shutdown` is the clean-exit marker written as the process
/// winds down — its sample is that boot's final reading.
#[derive(Deserialize, Serialize)]
struct StatsLine {
    boot: String,
    #[serde(flatten)]
    sample: DaemonStatsSample,
    #[serde(default)]
    shutdown: bool,
}

impl DaemonStats {
    pub(crate) fn open(data_dir: &Path) -> Arc<Self> {
        let path = data_dir.join(STATS_FILE_NAME);
        // The file is append-only and this process has not written yet, so
        // its last line is the previous boot's final reading — a shutdown
        // marker means that boot exited orderly.
        let last_line = std::fs::read(&path).ok().and_then(|bytes| {
            bytes
                .rsplit(|byte| *byte == b'\n')
                .find(|line| !line.is_empty())
                .and_then(|line| serde_json::from_slice::<StatsLine>(line).ok())
        });
        Arc::new(Self {
            boot: Uuid::new_v4().simple().to_string(),
            path,
            counts: Mutex::new(None),
            state: Mutex::new(DaemonStatsState {
                previous_boot: last_line.as_ref().map(|line| line.sample.clone()),
                previous_boot_clean: last_line.is_some_and(|line| line.shutdown),
                latest: None,
            }),
        })
    }

    /// `(latest, previous_boot, previous_boot_clean)` for `getDaemonStats`.
    pub(crate) fn snapshot(
        &self,
    ) -> (Option<DaemonStatsSample>, Option<DaemonStatsSample>, bool) {
        let state = self.state.lock();
        (
            state.latest.clone(),
            state.previous_boot.clone(),
            state.previous_boot_clean,
        )
    }

    /// Spawn the sampling thread. Only the maps' lengths are read — the
    /// generics keep this module free of daemon internals.
    pub(crate) fn spawn_sampler<V, T>(
        self: &Arc<Self>,
        sessions: Arc<Mutex<HashMap<Uuid, V>>>,
        terminals: Arc<Mutex<HashMap<Uuid, T>>>,
    ) where
        V: Send + 'static,
        T: Send + 'static,
    {
        *self.counts.lock() = Some(Box::new(move || {
            (sessions.lock().len() as u32, terminals.lock().len() as u32)
        }));
        let stats = self.clone();
        let path = self.path.clone();
        let boot = self.boot.clone();
        let _ = std::thread::Builder::new()
            .name("waku-stats".into())
            .spawn(move || loop {
                let sample = stats.current_sample();
                stats.state.lock().latest = Some(sample.clone());
                let _ = append_json_line(
                    &path,
                    &StatsLine {
                        boot: boot.clone(),
                        sample,
                        shutdown: false,
                    },
                );
                std::thread::sleep(SAMPLE_INTERVAL);
            });
    }

    /// Appends the clean-exit marker: the final sample flagged `shutdown`.
    /// The next boot reads its absence as an abnormal death — jetsam and
    /// SIGKILL leave no chance to write it.
    pub fn mark_clean_shutdown(&self) {
        let _ = append_json_line(
            &self.path,
            &StatsLine {
                boot: self.boot.clone(),
                sample: self.current_sample(),
                shutdown: true,
            },
        );
    }

    fn current_sample(&self) -> DaemonStatsSample {
        let (runtimes, terminals) = self
            .counts
            .lock()
            .as_ref()
            .map(|counts| counts())
            .unwrap_or_default();
        let (daemon_rss_mb, children_rss_mb) = memory_footprint_mb();
        DaemonStatsSample {
            at: crate::model::unix_time(),
            daemon_rss_mb,
            children_rss_mb,
            runtimes,
            terminals,
        }
    }
}

/// Install a panic hook that appends each panic — thread, source location,
/// truncated first message line — to `daemon-panics.jsonl`, then defers to
/// the default hook. Request-thread panics unwind without killing the
/// daemon, so without this a wedged handler leaves no trace.
pub fn install_panic_log(data_dir: &Path) {
    let path = data_dir.join(PANIC_FILE_NAME);
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|text| *text)
            .or_else(|| info.payload().downcast_ref::<String>().map(String::as_str))
            .unwrap_or_default()
            .lines()
            .next()
            .unwrap_or_default();
        let message: String = message.chars().take(400).collect();
        let location = info
            .location()
            .map(|location| {
                format!(
                    "{}:{}:{}",
                    location.file(),
                    location.line(),
                    location.column()
                )
            })
            .unwrap_or_default();
        let _ = append_json_line(
            &path,
            &serde_json::json!({
                "at": crate::model::unix_time(),
                "thread": std::thread::current().name().unwrap_or_default(),
                "location": location,
                "message": message,
            }),
        );
        default(info);
    }));
}

fn append_json_line(path: &Path, line: &impl Serialize) -> std::io::Result<()> {
    let line = serde_json::to_string(line).map_err(std::io::Error::other)?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(file, "{line}")?;
    drop(file);
    if std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0) > STATS_FILE_CAP {
        // Keep the newest half, cut at a line boundary.
        let bytes = std::fs::read(path)?;
        let halfway = bytes.len() / 2;
        let start = bytes[halfway..]
            .iter()
            .position(|byte| *byte == b'\n')
            .map(|offset| halfway + offset + 1)
            .unwrap_or(bytes.len());
        std::fs::write(path, &bytes[start..])?;
    }
    Ok(())
}

/// Own RSS and the summed RSS of every descendant process. `None` where the
/// platform has no table walk.
fn memory_footprint_mb() -> (Option<u64>, Option<u64>) {
    let Some(table) = process_table() else {
        return (None, None);
    };
    let own_pid = std::process::id() as i32;
    let own = table.get(&own_pid).map(|(_, rss)| *rss);
    let mut children_of: HashMap<i32, Vec<i32>> = HashMap::new();
    for (pid, (ppid, _)) in &table {
        children_of.entry(*ppid).or_default().push(*pid);
    }
    // Descendants, not just direct children: provider runtimes sit under a
    // guardian shell that owns their termination, so the process tree is
    // deeper than one level.
    let mut total = 0_u64;
    let mut stack = children_of.get(&own_pid).cloned().unwrap_or_default();
    while let Some(pid) = stack.pop() {
        let Some((_, rss)) = table.get(&pid) else {
            continue;
        };
        total += rss;
        if let Some(children) = children_of.get(&pid) {
            stack.extend_from_slice(children);
        }
    }
    (
        own.map(|bytes| bytes / (1 << 20)),
        Some(total / (1 << 20)),
    )
}

/// pid → (parent pid, resident bytes) for every readable process.
#[cfg(target_os = "macos")]
fn process_table() -> Option<HashMap<i32, (i32, u64)>> {
    let count = unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) };
    if count <= 0 {
        return None;
    }
    let mut pids = vec![0_i32; count as usize];
    let listed = unsafe {
        libc::proc_listallpids(
            pids.as_mut_ptr() as *mut std::ffi::c_void,
            (pids.len() * std::mem::size_of::<i32>()) as i32,
        )
    };
    if listed <= 0 {
        return None;
    }
    pids.truncate(listed as usize);
    let mut table = HashMap::with_capacity(pids.len());
    for pid in pids {
        let mut bsd = std::mem::MaybeUninit::<libc::proc_bsdinfo>::zeroed();
        let read = unsafe {
            libc::proc_pidinfo(
                pid,
                libc::PROC_PIDTBSDINFO,
                0,
                bsd.as_mut_ptr() as *mut std::ffi::c_void,
                std::mem::size_of::<libc::proc_bsdinfo>() as i32,
            )
        };
        if read <= 0 {
            continue;
        }
        let ppid = unsafe { bsd.assume_init() }.pbi_ppid as i32;
        // phys_footprint is what Activity Monitor and jetsam measure;
        // pti_resident_size understates compressed memory.
        let mut usage = std::mem::MaybeUninit::<libc::rusage_info_v4>::zeroed();
        let read = unsafe {
            libc::proc_pid_rusage(
                pid,
                libc::RUSAGE_INFO_V4,
                usage.as_mut_ptr() as *mut libc::rusage_info_t,
            )
        };
        if read != 0 {
            continue;
        }
        let rss = unsafe { usage.assume_init() }.ri_phys_footprint;
        table.insert(pid, (ppid, rss));
    }
    Some(table)
}

/// pid → (parent pid, resident bytes) via `/proc/*/status` — one file gives
/// both `PPid` and `VmRSS`.
#[cfg(target_os = "linux")]
fn process_table() -> Option<HashMap<i32, (i32, u64)>> {
    let mut table = HashMap::new();
    for entry in std::fs::read_dir("/proc").ok()? {
        let Ok(entry) = entry else { continue };
        let name = entry.file_name();
        let Ok(pid) = name.to_str().unwrap_or_default().parse::<i32>() else {
            continue;
        };
        let Ok(status) = std::fs::read_to_string(entry.path().join("status")) else {
            continue;
        };
        let (mut ppid, mut rss_kb) = (None, None);
        for line in status.lines() {
            if let Some(value) = line.strip_prefix("PPid:") {
                ppid = value.trim().parse::<i32>().ok();
            } else if let Some(value) = line.strip_prefix("VmRSS:") {
                rss_kb = value.trim().trim_end_matches(" kB").trim().parse::<u64>().ok();
            }
        }
        if let (Some(ppid), Some(rss_kb)) = (ppid, rss_kb) {
            table.insert(pid, (ppid, rss_kb * 1024));
        }
    }
    Some(table)
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn process_table() -> Option<HashMap<i32, (i32, u64)>> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    fn the_process_table_reports_this_processs_footprint() {
        let table = process_table().expect("a readable process table");
        let (_, rss) = table
            .get(&(std::process::id() as i32))
            .expect("the test process's own row");
        assert!(*rss > 0);
    }

    fn append(path: &Path, boot: &str, sample: &DaemonStatsSample, shutdown: bool) {
        append_json_line(
            path,
            &StatsLine {
                boot: boot.to_owned(),
                sample: sample.clone(),
                shutdown,
            },
        )
        .unwrap();
    }

    #[test]
    fn the_stats_file_keeps_the_previous_boots_last_sample() {
        let dir = std::env::temp_dir().join(format!("waku-stats-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(STATS_FILE_NAME);
        let first = DaemonStatsSample {
            at: 100,
            daemon_rss_mb: Some(200),
            children_rss_mb: Some(1500),
            runtimes: 3,
            terminals: 1,
        };
        append(&path, "boot-a", &first, false);
        let mut second = first.clone();
        second.at = 160;
        second.children_rss_mb = Some(1600);
        append(&path, "boot-a", &second, false);

        let stats = DaemonStats::open(&dir);
        let (latest, previous_boot, clean) = stats.snapshot();
        assert!(latest.is_none());
        assert!(!clean);
        let previous = previous_boot.expect("the previous boot's last line");
        assert_eq!(previous.at, 160);
        assert_eq!(previous.children_rss_mb, Some(1600));
    }

    #[test]
    fn a_shutdown_marker_marks_the_previous_boot_clean() {
        let dir = std::env::temp_dir().join(format!("waku-stats-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(STATS_FILE_NAME);
        let sample = DaemonStatsSample {
            at: 200,
            daemon_rss_mb: Some(300),
            children_rss_mb: Some(700),
            runtimes: 2,
            terminals: 0,
        };
        append(&path, "boot-a", &sample, false);
        append(&path, "boot-a", &sample, true);

        let stats = DaemonStats::open(&dir);
        let (_, previous_boot, clean) = stats.snapshot();
        assert!(clean);
        // The marker carries the boot's final reading.
        assert_eq!(previous_boot.expect("final sample").at, 200);
    }

    #[test]
    fn the_stats_file_caps_itself_by_keeping_the_newest_half() {
        let dir = std::env::temp_dir().join(format!("waku-stats-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(STATS_FILE_NAME);
        let sample = DaemonStatsSample {
            at: 0,
            daemon_rss_mb: Some(1),
            children_rss_mb: Some(2),
            runtimes: 0,
            terminals: 0,
        };
        let line_len = serde_json::to_string(&StatsLine {
            boot: "boot".into(),
            sample: sample.clone(),
            shutdown: false,
        })
        .unwrap()
        .len()
            + 1;
        let lines_to_overflow = (STATS_FILE_CAP as usize / line_len) + 2;
        for _ in 0..lines_to_overflow {
            append(&path, "boot", &sample, false);
        }
        let kept = std::fs::read(&path).unwrap();
        assert!(kept.len() as u64 <= STATS_FILE_CAP);
        // Every retained line still parses — the cut landed on a boundary.
        let parsed = kept
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .count();
        assert!(parsed > 0);
        for line in kept.split(|byte| *byte == b'\n').filter(|line| !line.is_empty()) {
            serde_json::from_slice::<StatsLine>(line).unwrap();
        }
    }
}
