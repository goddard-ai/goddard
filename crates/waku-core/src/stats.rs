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
use std::ffi::CStr;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use waku_protocol::model::ProviderKind;
use waku_protocol::{
    DaemonChildKind, DaemonChildSample, DaemonSessionSample, DaemonStatsSample,
};

/// One sample per minute keeps the process-table walk cheap while catching
/// growth between turns; eviction leaks move on hour timescales anyway.
const SAMPLE_INTERVAL: Duration = Duration::from_secs(60);
/// ~512 KB at ~150 bytes a line is several days of minute-cadence samples;
/// the tail rewrite keeps the freshest half. The panic log shares the cap.
const STATS_FILE_CAP: u64 = 512 * 1024;
const STATS_FILE_NAME: &str = "daemon-stats.jsonl";
const PANIC_FILE_NAME: &str = "daemon-panics.jsonl";

/// A live runtime's identity for subtree attribution: guardian shells run
/// the provider in its session cwd — some set it on the shell, some `env -C`
/// inside — so a subtree claims the session when *any* member runs there.
pub(crate) struct RuntimeDir {
    pub session_id: Uuid,
    pub provider: ProviderKind,
    /// Canonicalized when the probe is built — `process_cwd` reports real
    /// paths, so a symlinked stored path would never match.
    pub cwd: PathBuf,
}

/// What the daemon reports about its live state each sample — counts,
/// subtree attribution hints, and per-session resident detail. Built in
/// `daemon.rs`, which owns the locks this data sits behind.
#[derive(Default)]
pub(crate) struct StatsProbe {
    /// Live provider runtimes at sample time.
    pub runtimes: u32,
    /// Live remote terminals at sample time.
    pub terminals: u32,
    pub runtime_dirs: Vec<RuntimeDir>,
    /// Terminal PTY child pid → the task surface that opened it.
    pub terminal_roots: Vec<(u32, Option<Uuid>)>,
    /// Every known session, skeletons included.
    pub sessions_total: u32,
    /// Resident or running sessions.
    pub sessions: Vec<DaemonSessionSample>,
}

/// The sampler's shared state: what it last wrote, and the previous boot's
/// final line captured before this process appended anything.
pub struct DaemonStats {
    boot: String,
    path: PathBuf,
    /// The live-state readout for samples and the shutdown marker — set
    /// when the sampler starts.
    probe: Mutex<Option<Box<dyn Fn() -> StatsProbe + Send + Sync>>>,
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
            probe: Mutex::new(None),
            state: Mutex::new(DaemonStatsState {
                previous_boot: last_line.as_ref().map(|line| line.sample.clone()),
                previous_boot_clean: last_line.is_some_and(|line| line.shutdown),
                latest: None,
            }),
        })
    }

    /// `(latest, previous_boot, previous_boot_clean)` for `getDaemonStats`.
    pub(crate) fn snapshot(&self) -> (Option<DaemonStatsSample>, Option<DaemonStatsSample>, bool) {
        let state = self.state.lock();
        (
            state.latest.clone(),
            state.previous_boot.clone(),
            state.previous_boot_clean,
        )
    }

    /// Spawn the sampling thread. The probe callback keeps this module free
    /// of daemon internals — `daemon.rs` owns the maps it reads.
    pub(crate) fn spawn_sampler(
        self: &Arc<Self>,
        probe: impl Fn() -> StatsProbe + Send + Sync + 'static,
    ) {
        *self.probe.lock() = Some(Box::new(probe));
        let stats = self.clone();
        let path = self.path.clone();
        let boot = self.boot.clone();
        let _ = std::thread::Builder::new()
            .name("waku-stats".into())
            .spawn(move || {
                loop {
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
                }
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
        let probe = self
            .probe
            .lock()
            .as_ref()
            .map(|probe| probe())
            .unwrap_or_default();
        let (daemon_rss_mb, children_rss_mb, children) = memory_rows(&probe);
        DaemonStatsSample {
            at: crate::model::unix_time(),
            daemon_rss_mb,
            children_rss_mb,
            runtimes: probe.runtimes,
            terminals: probe.terminals,
            children,
            sessions_total: probe.sessions_total,
            sessions: probe.sessions,
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

/// One process-table row: parent, resident size, and the process name
/// (`comm` — truncated to 15–16 chars by the kernel, enough for `devin`,
/// `codex`, `goddard-daemon`).
struct ProcEntry {
    ppid: i32,
    rss: u64,
    name: String,
}

/// Own RSS, the summed RSS of every descendant, and one row per direct
/// child's subtree. `None` where the platform has no table walk.
fn memory_rows(probe: &StatsProbe) -> (Option<u64>, Option<u64>, Vec<DaemonChildSample>) {
    let Some(table) = process_table() else {
        return (None, None, Vec::new());
    };
    let own_pid = std::process::id() as i32;
    let own = table.get(&own_pid).map(|entry| entry.rss);
    let mut children_of: HashMap<i32, Vec<i32>> = HashMap::new();
    for (pid, entry) in &table {
        children_of.entry(entry.ppid).or_default().push(*pid);
    }
    // Descendants, not just direct children: provider runtimes sit under a
    // guardian shell that owns their termination, so the process tree is
    // deeper than one level. Each direct child roots one subtree row.
    let mut total = 0_u64;
    let mut rows = Vec::new();
    let mut unclaimed = probe.runtime_dirs.iter().collect::<Vec<_>>();
    for (root, entry) in &table {
        if entry.ppid != own_pid {
            continue;
        }
        let mut members = vec![*root];
        let mut subtree_rss = 0_u64;
        let mut cursor = 0;
        while let Some(pid) = members.get(cursor).copied() {
            cursor += 1;
            let Some(member) = table.get(&pid) else {
                continue;
            };
            subtree_rss += member.rss;
            if let Some(children) = children_of.get(&pid) {
                members.extend_from_slice(children);
            }
        }
        total += subtree_rss;
        // The subtree's identity is its heaviest member: the guardian is a
        // bare `sh`, the provider CLI carries the megabytes.
        let name = members
            .iter()
            .filter_map(|pid| table.get(pid))
            .max_by_key(|member| member.rss)
            .map(|member| member.name.clone())
            .unwrap_or_default();
        let (kind, session_id, provider) =
            attribute(&members, &probe.terminal_roots, &mut unclaimed);
        rows.push(DaemonChildSample {
            pid: *root as u32,
            name,
            rss_mb: subtree_rss / (1 << 20),
            processes: members.len() as u32,
            kind,
            session_id,
            provider,
        });
    }
    rows.sort_by(|a, b| b.rss_mb.cmp(&a.rss_mb));
    (own.map(|bytes| bytes / (1 << 20)), Some(total / (1 << 20)), rows)
}

/// Who owns one subtree: a terminal when its PTY pid sits among the
/// members, else the runtime whose session cwd a member runs in — claimed
/// once so two sessions sharing a directory still split their subtrees.
fn attribute(
    members: &[i32],
    terminal_roots: &[(u32, Option<Uuid>)],
    runtime_dirs: &mut Vec<&RuntimeDir>,
) -> (DaemonChildKind, Option<Uuid>, Option<ProviderKind>) {
    if let Some((_, owner)) = terminal_roots
        .iter()
        .find(|(pid, _)| members.contains(&(*pid as i32)))
    {
        return (DaemonChildKind::Terminal, *owner, None);
    }
    for member in members {
        let Some(cwd) = process_cwd(*member) else {
            continue;
        };
        if let Some(index) = runtime_dirs.iter().position(|dir| dir.cwd == cwd) {
            let dir = runtime_dirs.remove(index);
            return (DaemonChildKind::Runtime, Some(dir.session_id), Some(dir.provider));
        }
    }
    (DaemonChildKind::Other, None, None)
}

/// pid → parent/name/RSS for every readable process.
#[cfg(target_os = "macos")]
fn process_table() -> Option<HashMap<i32, ProcEntry>> {
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
        let bsd = unsafe { bsd.assume_init() };
        let ppid = bsd.pbi_ppid as i32;
        let name = unsafe { CStr::from_ptr(bsd.pbi_comm.as_ptr()) }
            .to_string_lossy()
            .into_owned();
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
        table.insert(pid, ProcEntry { ppid, rss, name });
    }
    Some(table)
}

/// pid → parent/name/RSS via `/proc/*/status` — one file gives `Name`,
/// `PPid`, and `VmRSS`.
#[cfg(target_os = "linux")]
fn process_table() -> Option<HashMap<i32, ProcEntry>> {
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
        let (mut comm, mut ppid, mut rss_kb) = (None, None, None);
        for line in status.lines() {
            if let Some(value) = line.strip_prefix("Name:") {
                comm = Some(value.trim().to_owned());
            } else if let Some(value) = line.strip_prefix("PPid:") {
                ppid = value.trim().parse::<i32>().ok();
            } else if let Some(value) = line.strip_prefix("VmRSS:") {
                rss_kb = value
                    .trim()
                    .trim_end_matches(" kB")
                    .trim()
                    .parse::<u64>()
                    .ok();
            }
        }
        if let (Some(ppid), Some(rss_kb)) = (ppid, rss_kb) {
            table.insert(
                pid,
                ProcEntry {
                    ppid,
                    rss: rss_kb * 1024,
                    name: comm.unwrap_or_default(),
                },
            );
        }
    }
    Some(table)
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn process_table() -> Option<HashMap<i32, ProcEntry>> {
    None
}

/// A process's working directory — the join between a runtime subtree and
/// the session it serves. ACP guardians `env -C` into the session dir, so
/// the *descendant* carries it even when the direct child does not.
#[cfg(target_os = "macos")]
fn process_cwd(pid: i32) -> Option<PathBuf> {
    let mut info = std::mem::MaybeUninit::<libc::proc_vnodepathinfo>::zeroed();
    let read = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDVNODEPATHINFO,
            0,
            info.as_mut_ptr() as *mut std::ffi::c_void,
            std::mem::size_of::<libc::proc_vnodepathinfo>() as i32,
        )
    };
    if read <= 0 {
        return None;
    }
    let info = unsafe { info.assume_init() };
    // libc spells vip_path as `[[c_char; 32]; 32]` for old-rustc layout
    // reasons; it is a flat 1024-byte MAXPATHLEN buffer.
    let path = unsafe { CStr::from_ptr(info.pvi_cdir.vip_path.as_ptr() as *const _) };
    Some(PathBuf::from(path.to_string_lossy().into_owned()))
}

#[cfg(target_os = "linux")]
fn process_cwd(pid: i32) -> Option<PathBuf> {
    std::fs::read_link(format!("/proc/{pid}/cwd")).ok()
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn process_cwd(_pid: i32) -> Option<PathBuf> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    fn the_process_table_reports_this_processs_footprint() {
        let table = process_table().expect("a readable process table");
        let entry = table
            .get(&(std::process::id() as i32))
            .expect("the test process's own row");
        assert!(entry.rss > 0);
        assert!(!entry.name.is_empty());
    }

    #[test]
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    fn subtrees_report_each_direct_child_and_claim_by_cwd() {
        let dir = std::env::temp_dir().join(format!("waku-stats-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let cwd = std::fs::canonicalize(&dir).unwrap();
        let session_id = Uuid::new_v4();
        // One child runs in a session's worktree, one runs nowhere known.
        let mut claimed = std::process::Command::new("sleep")
            .arg("30")
            .current_dir(&cwd)
            .spawn()
            .unwrap();
        let mut unclaimed = std::process::Command::new("sleep").arg("30").spawn().unwrap();

        let mut probe = StatsProbe::default();
        probe.runtime_dirs.push(RuntimeDir {
            session_id,
            provider: ProviderKind::Codex,
            cwd: cwd.clone(),
        });
        let (_, _, rows) = memory_rows(&probe);
        let _ = claimed.kill();
        let _ = unclaimed.kill();

        let runtime = rows
            .iter()
            .find(|row| row.session_id == Some(session_id))
            .expect("the session's subtree row");
        assert_eq!(runtime.kind, DaemonChildKind::Runtime);
        assert_eq!(runtime.provider, Some(ProviderKind::Codex));
        assert_eq!(runtime.name, "sleep");
        assert!(rows.iter().any(|row| {
            row.pid == unclaimed.id() && row.kind == DaemonChildKind::Other
        }));
        let _ = std::fs::remove_dir_all(&dir);
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
            children: Vec::new(),
            sessions_total: 0,
            sessions: Vec::new(),
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
            children: Vec::new(),
            sessions_total: 0,
            sessions: Vec::new(),
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
            children: Vec::new(),
            sessions_total: 0,
            sessions: Vec::new(),
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
        for line in kept
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
        {
            serde_json::from_slice::<StatsLine>(line).unwrap();
        }
    }
}
