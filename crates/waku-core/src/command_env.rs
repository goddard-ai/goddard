//! Process environment capture and provider-safe command spawning.

use std::collections::{HashMap, HashSet};
use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::sync::{Mutex, OnceLock, RwLock};

use std::fs::{self, OpenOptions};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::ffi::CStr;
#[cfg(unix)]
use std::mem::MaybeUninit;
#[cfg(unix)]
use std::os::unix::ffi::{OsStrExt, OsStringExt};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
#[cfg(unix)]
use std::os::unix::process::CommandExt;

const LOGIN_SHELL_ENV_TIMEOUT: Duration = Duration::from_secs(5);
#[cfg(unix)]
const INTERACTIVE_SHELL_ENV_TIMEOUT: Duration = Duration::from_secs(3);
#[cfg(unix)]
const SHELL_ENV_COMMAND: &str = "/usr/bin/env -0 > \"$GODDARD_SHELL_ENV_CAPTURE_FILE\"";

type ShellEnvironment = Vec<(OsString, OsString)>;

static LOGIN_SHELL_ENVIRONMENT: OnceLock<RwLock<Option<ShellEnvironment>>> = OnceLock::new();
static SHELL_ENV_CAPTURE_ID: AtomicU64 = AtomicU64::new(0);

/// Bumped every time the login-shell environment is re-captured so caches
/// derived from it — the `command -v` fallback results — cannot serve
/// answers older than the PATH they were resolved against.
static SHELL_ENV_GENERATION: AtomicU64 = AtomicU64::new(0);
static LAST_SHELL_REFRESH: Mutex<Option<Instant>> = Mutex::new(None);

/// Detection probes re-capture the shell environment at most this often.
/// A refresh pass probes every provider back to back, and installing a CLI
/// minutes after the daemon started is the case worth catching — seconds of
/// staleness are not.
const SHELL_ENV_REFRESH_INTERVAL: Duration = Duration::from_secs(60);

/// Build a command with the environment a terminal-launched Goddard normally
/// inherits. Apps opened through LaunchServices do not receive variables
/// exported by the user's shell, including the PATH needed by script-based
/// CLIs whose shebang uses `/usr/bin/env` (for example, an npm-installed Codex
/// launcher needs `node`). Callers can add provider-specific overrides after
/// this.
pub fn command(program: impl AsRef<OsStr>) -> Command {
    let program = program.as_ref();
    let mut command = plain_command(resolve_spawn_program(program));
    command.envs(shell_environment());
    apply_search_path(&mut command, program);
    command
}

/// [`plain_command`] plus only the search-path `PATH` — none of the login
/// shell's other variables. A tool that spawns helpers by name still
/// resolves them the way the user's terminal does: `git` finds `git-lfs`,
/// credential helpers, `core.sshCommand`, and signing programs, and `gh`
/// itself resolves from an install the GUI `PATH` predates. Variables like
/// `GIT_DIR` or a stale `SSH_AUTH_SOCK` cannot leak in and redirect the
/// operation, which the full [`command`] environment would risk.
pub fn search_path_command(program: impl AsRef<OsStr>) -> Command {
    let program = program.as_ref();
    let mut command = plain_command(resolve_spawn_program(program));
    apply_search_path(&mut command, program);
    command
}

/// `PATH` must be set after any `envs` call so the search path wins over a
/// `PATH` the shell environment happens to carry.
fn apply_search_path(command: &mut Command, program: &OsStr) {
    if let Some(search_path) = child_search_path(Path::new(program)) {
        command.env("PATH", search_path);
    }
}

/// `std::process::Command` refuses `posix_spawn` when the child environment
/// overrides `PATH` and the program is a bare name — every spawn then forks
/// the whole process, and on macOS each fork stalls every allocator behind
/// the malloc fork lock while the VM map copies. Resolving the name against
/// the same directories the child's `PATH` will carry keeps identical lookup
/// semantics on the `posix_spawn` fast path; the fallback keeps the original
/// name so an unresolvable one fails exactly as it did before.
#[cfg(unix)]
fn resolve_spawn_program(program: &OsStr) -> OsString {
    if program.as_bytes().contains(&b'/') {
        return program.to_os_string();
    }
    executable_search_paths()
        .into_iter()
        .find_map(|directory| resolve_executable_file(&directory.join(program)))
        .map(PathBuf::into_os_string)
        .unwrap_or_else(|| program.to_os_string())
}

#[cfg(not(unix))]
fn resolve_spawn_program(program: &OsStr) -> OsString {
    program.to_os_string()
}

/// Inject the agent surface into a provider launch: the session's scoped
/// token, its task id, the daemon address, and `PATH` with the `goddard-agent`
/// directory prepended. Callers build `command` through [`command`] first so
/// the prepend lands on the PATH the provider would already run with.
pub fn apply_agent_environment(command: &mut Command, agent: &crate::agent::AgentLaunchEnv) {
    let base_path = command
        .get_envs()
        .find(|(name, _)| name.eq_ignore_ascii_case("PATH"))
        .and_then(|(_, value)| value.map(OsStr::to_os_string))
        .or_else(|| std::env::var_os("PATH"));
    for (name, value) in agent_environment_pairs(agent, base_path) {
        command.env(name, value);
    }
}

/// The same launch environment as name/value pairs, for spawn paths that
/// pass an explicit environment vector instead of a [`Command`]. Any entry
/// named `PATH` inside `environment` is replaced rather than duplicated —
/// libc `getenv` returns the first match, so a second `PATH` would be
/// silently dropped on some platforms.
pub fn merge_agent_environment(
    environment: &mut Vec<(String, String)>,
    agent: &crate::agent::AgentLaunchEnv,
) {
    let base_path = environment
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("PATH"))
        .map(|(_, value)| OsString::from(value.clone()))
        .or_else(|| std::env::var_os("PATH"));
    let pairs = agent_environment_pairs(agent, base_path);
    for (name, value) in &pairs {
        if let Some(existing) = environment
            .iter_mut()
            .find(|(existing, _)| existing.eq_ignore_ascii_case(name))
        {
            existing.1 = value.clone();
        } else {
            environment.push((name.clone(), value.clone()));
        }
    }
}

/// The environment a provider child runs with as explicit name/value pairs —
/// for spawn APIs that take an env vector instead of a [`Command`] (the
/// Copilot SDK's `ClientOptions.env`). Mirrors [`command`] plus
/// [`apply_agent_environment`]: the login shell's variables, the search-path
/// `PATH`, then the agent surface.
pub(crate) fn spawn_environment(
    program: &Path,
    agent: Option<&crate::agent::AgentLaunchEnv>,
) -> Vec<(OsString, OsString)> {
    let mut environment = shell_environment();
    if let Some(search_path) = child_search_path(program) {
        environment.retain(|(name, _)| !name.eq_ignore_ascii_case(OsStr::new("PATH")));
        environment.push((OsString::from("PATH"), search_path));
    }
    if let Some(agent) = agent {
        let base_path = environment
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(OsStr::new("PATH")))
            .map(|(_, value)| value.clone());
        for (name, value) in agent_environment_pairs(agent, base_path) {
            let name = OsString::from(name);
            if let Some(existing) = environment
                .iter_mut()
                .find(|(existing, _)| existing.eq_ignore_ascii_case(&name))
            {
                existing.1 = OsString::from(value);
            } else {
                environment.push((name, OsString::from(value)));
            }
        }
    }
    environment
}

fn agent_environment_pairs(
    agent: &crate::agent::AgentLaunchEnv,
    base_path: Option<OsString>,
) -> Vec<(String, String)> {
    let cli_directory = agent
        .cli_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| agent.cli_path.clone());
    let path = std::env::join_paths(std::iter::once(cli_directory).chain(std::env::split_paths(
        base_path.as_deref().unwrap_or(OsStr::new("")),
    )))
    .unwrap_or_default();
    let mut pairs = vec![
        (
            waku_protocol::AGENT_TOKEN_ENV.to_owned(),
            agent.token.clone(),
        ),
        (
            waku_protocol::AGENT_TASK_ENV.to_owned(),
            agent.task_id.to_string(),
        ),
        (
            waku_protocol::DAEMON_ADDRESS_ENV.to_owned(),
            agent.daemon_address.clone(),
        ),
        ("PATH".to_owned(), path.to_string_lossy().into_owned()),
    ];
    if let Some(parent) = agent.parent_task_id {
        pairs.push((
            waku_protocol::AGENT_PARENT_TASK_ENV.to_owned(),
            parent.to_string(),
        ));
    }
    pairs
}

/// The `PATH` a provider CLI runs with: every directory Goddard itself searched,
/// plus the one the binary was found in.
///
/// Detection resolves CLIs from more directories than the desktop process
/// inherits — a Bun or npm global prefix that the GUI `PATH` predates, for
/// example — so a CLI found in one of them has to *run* with them too.
/// Launcher-based installs depend on it and fail silently without it: Bun's
/// Windows `pi.EXE` is a shim that launches `bun.exe` from its own directory,
/// and both an npm `.cmd` shim and a `/usr/bin/env node` shebang need `node`
/// on the child's `PATH`. Detection still succeeds in that state — it only
/// looks for the file — so the provider shows up as installed while every
/// probe it runs comes back empty.
///
/// Windows needs this most: the login-shell probe there is best-effort — no
/// PowerShell may be present, and a profile can refuse to load — so a
/// GUI-launched Goddard can still be running with only the `PATH` it inherited.
fn child_search_path(program: &Path) -> Option<OsString> {
    let mut directories = executable_search_paths();
    // Last, not first: an install outside the known prefixes still finds its
    // runtime, while the user's own `PATH` order decides everything else.
    directories.extend(
        program
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .map(Path::to_path_buf),
    );
    let mut seen = HashSet::new();
    directories.retain(|directory| seen.insert(directory.clone()));
    std::env::join_paths(directories).ok()
}

/// A command that never flashes a console window.
///
/// Goddard's Windows build is a GUI-subsystem binary with no console of its own,
/// so `CreateProcess` allocates one for every console child — `git`, a
/// provider CLI, the daemon — and flashes it on screen. `CREATE_NO_WINDOW`
/// keeps the child's console hidden while its pipes still work.
pub fn plain_command(program: impl AsRef<OsStr>) -> Command {
    let mut command = Command::new(program);
    detach_console(&mut command);
    command
}

// A spawned provider dies with the daemon only if someone asks it to: a
// crashed daemon runs no destructors, and the restarted daemon cold-starts a
// fresh runtime instead of re-adopting the orphan — at best a leak, at worst
// a second process still working the session. The wrapper watches both the
// daemon pid and its own pid so a daemon death AND a direct `kill()` on the
// wrapper (which destructors also cannot intercept under SIGKILL) both reach
// the real child. The fd-9 dance is load-bearing: a non-interactive shell
// hands a background job /dev/null for stdin, which would starve stdio
// providers. `<&0` fixes that under bash but not dash — dash assigns
// /dev/null first, so `<&0` duplicates the dead fd — while `<&9` dup's the
// original stdin saved before the backgrounded command runs.
#[cfg(unix)]
pub(crate) const DAEMON_GUARDIAN_SCRIPT: &str = r#"
daemon=$PPID
wrapper=$$
exec 9<&0
"$@" <&9 &
child=$!
(
  while kill -0 "$daemon" 2>/dev/null && kill -0 "$wrapper" 2>/dev/null; do
    sleep 1
  done
  kill -TERM "$child" 2>/dev/null || true
) &
watcher=$!
trap 'kill -TERM "$child" "$watcher" 2>/dev/null' EXIT HUP INT TERM
wait "$child"
exit $?
"#;

/// Wrap a built command in a `/bin/sh` guardian that terminates the real
/// child when the daemon process dies. The child inherits the wrapper's
/// stdio and its exit status is preserved, so callers treat the wrapper as
/// the child it guards. Call this after args/env/cwd are configured and
/// before stdio is set: `Command` exposes getters for the former but not
/// the latter.
#[cfg(unix)]
pub fn guard_command(command: Command) -> Command {
    let mut guarded = plain_command("/bin/sh");
    guarded
        .arg("-c")
        .arg(DAEMON_GUARDIAN_SCRIPT)
        .arg("goddard-daemon-guardian")
        .arg(command.get_program());
    guarded.args(command.get_args());
    if let Some(cwd) = command.get_current_dir() {
        guarded.current_dir(cwd);
    }
    for (name, value) in command.get_envs() {
        match value {
            Some(value) => {
                guarded.env(name, value);
            }
            None => {
                guarded.env_remove(name);
            }
        }
    }
    guarded
}

#[cfg(not(unix))]
pub fn guard_command(command: Command) -> Command {
    command
}

fn detach_console(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;

        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    let _ = command;
}

/// Spawn `command` with `SIGCHLD` unblocked in the child. On macOS, libdispatch
/// worker threads (which back GPUI's background executor) block `SIGCHLD`, and
/// a process spawned from such a thread inherits the blocked mask. That breaks
/// provider-side async process reapers. The caller's mask is restored as soon
/// as the child has been created.
pub fn spawn(command: &mut Command) -> io::Result<Child> {
    detach_console(command);
    with_sigchld_unblocked(|| command.spawn())
}

/// Spawn `command` through [`spawn`] and collect its output.
///
/// `Command::spawn` inherits standard streams by default, unlike
/// `Command::output`. Own all three streams here so callers keep the latter's
/// behavior while the signal mask is changed only for the spawn itself.
pub fn output(command: &mut Command) -> io::Result<Output> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    spawn(command)?.wait_with_output()
}

/// Normalize a Goddard-owned provider thread before a dependency spawns the child
/// internally. The ACP SDK owns its `async_process::Command`, so its dedicated
/// connection thread uses this once at startup instead of [`spawn`].
pub(crate) fn unblock_sigchld_for_current_thread() -> io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        let sigchld = sigchld_set()?;
        pthread_result(unsafe {
            libc::pthread_sigmask(libc::SIG_UNBLOCK, &sigchld, std::ptr::null_mut())
        })
    }
    #[cfg(not(target_os = "macos"))]
    {
        Ok(())
    }
}

fn with_sigchld_unblocked<T>(operation: impl FnOnce() -> io::Result<T>) -> io::Result<T> {
    #[cfg(target_os = "macos")]
    let _restore = SignalMaskRestore::unblock_sigchld()?;
    operation()
}

#[cfg(target_os = "macos")]
fn sigchld_set() -> io::Result<libc::sigset_t> {
    let mut set = MaybeUninit::<libc::sigset_t>::uninit();
    if unsafe { libc::sigemptyset(set.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let mut set = unsafe { set.assume_init() };
    if unsafe { libc::sigaddset(&mut set, libc::SIGCHLD) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(set)
}

#[cfg(target_os = "macos")]
fn pthread_result(status: libc::c_int) -> io::Result<()> {
    if status == 0 {
        Ok(())
    } else {
        // pthread APIs return the error number directly instead of setting
        // errno, so `last_os_error` would report unrelated thread state.
        Err(io::Error::from_raw_os_error(status))
    }
}

#[cfg(target_os = "macos")]
struct SignalMaskRestore(libc::sigset_t);

#[cfg(target_os = "macos")]
impl SignalMaskRestore {
    fn unblock_sigchld() -> io::Result<Self> {
        let sigchld = sigchld_set()?;
        let mut previous = MaybeUninit::<libc::sigset_t>::uninit();
        pthread_result(unsafe {
            libc::pthread_sigmask(libc::SIG_UNBLOCK, &sigchld, previous.as_mut_ptr())
        })?;
        Ok(Self(unsafe { previous.assume_init() }))
    }
}

#[cfg(target_os = "macos")]
impl Drop for SignalMaskRestore {
    fn drop(&mut self) {
        let _ = unsafe { libc::pthread_sigmask(libc::SIG_SETMASK, &self.0, std::ptr::null_mut()) };
    }
}

pub fn find_executable(name: &str) -> Option<PathBuf> {
    let candidate = Path::new(name);
    if candidate.components().count() > 1 {
        return resolve_executable_file(candidate);
    }
    executable_search_paths()
        .into_iter()
        .find_map(|directory| resolve_executable_file(&directory.join(name)))
}

/// On Windows, try the path with each `PATHEXT` suffix first, then accept
/// `candidate` as it stands. Elsewhere the candidate is used directly.
///
/// Nothing is executable by name alone on Windows: an npm-installed provider
/// CLI lands as `claude.cmd` beside `claude.ps1`, and Bun and Cargo install
/// `.exe`. Trying `PATHEXT` in its configured order picks the same file the
/// shell would, and `std::process::Command` runs a `.cmd`/`.bat` through
/// `cmd.exe` for us.
///
/// A global npm install also writes an extensionless POSIX shim next to those
/// two, and `CreateProcess` cannot run it. Accepting the bare name before the
/// suffixes would hand back that shim and every launch of the provider would
/// fail, so the suffixed names have to win.
fn resolve_executable_file(candidate: &Path) -> Option<PathBuf> {
    #[cfg(windows)]
    if let Some(stem) = candidate.file_name() {
        let stem = stem.to_owned();
        for extension in executable_extensions() {
            let mut name = stem.clone();
            name.push(&extension);
            let suffixed = candidate.with_file_name(name);
            if suffixed.is_file() {
                return Some(suffixed);
            }
        }
    }
    if candidate.is_file() {
        return Some(candidate.to_path_buf());
    }
    None
}

#[cfg(windows)]
fn executable_extensions() -> Vec<OsString> {
    const DEFAULT_PATHEXT: &str = ".COM;.EXE;.BAT;.CMD";

    let configured = std::env::var("PATHEXT").unwrap_or_default();
    let configured = if configured.trim().is_empty() {
        DEFAULT_PATHEXT
    } else {
        configured.as_str()
    };
    configured
        .split(';')
        .map(str::trim)
        .filter(|extension| extension.starts_with('.'))
        .map(OsString::from)
        .collect()
}

/// Resolve a user-supplied binary override: `~` expands to the home
/// directory, a path must point at an existing file, and a bare name searches
/// the same directories as [`find_executable`].
pub fn resolve_binary_override(spec: &str) -> Option<PathBuf> {
    let spec = spec.trim();
    if spec.is_empty() {
        return None;
    }
    if let Some(rest) = spec.strip_prefix("~/") {
        let candidate = dirs::home_dir()?.join(rest);
        return candidate.is_file().then_some(candidate);
    }
    find_executable(spec)
}

/// `(environment generation, command -> resolved path)`; the generation
/// invalidates the whole map when the shell environment is re-captured.
type ShellCommandResolution = (u64, HashMap<String, Option<PathBuf>>);

/// Fallback for commands the static directory search missed: ask the user's
/// interactive shell to resolve every command in `cohort` in a single
/// invocation. Interactive rc files (`.zshrc`) can put directories on PATH
/// that the login-shell env probe never captured — a timed-out `-i` attempt
/// falls back to `-l`, which skips `.zshrc` entirely — so the shell the user
/// actually types into is the last word on what it can run.
///
/// Results for the whole cohort are cached until the captured environment is
/// refreshed (see [`SHELL_ENV_GENERATION`]), so the sequential per-provider
/// probes in a detection pass share one spawn. Must only be called from a
/// background thread.

pub fn find_executable_via_shell(name: &str, cohort: &[&str]) -> Option<PathBuf> {
    static CACHE: OnceLock<Mutex<ShellCommandResolution>> = OnceLock::new();
    let generation = SHELL_ENV_GENERATION.load(Ordering::Relaxed);
    let cache = CACHE.get_or_init(|| Mutex::new((u64::MAX, HashMap::new())));
    let mut guard = cache
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if guard.0 != generation {
        guard.1 = resolve_commands_via_shell(cohort);
        guard.0 = generation;
    }
    match guard.1.get(name) {
        Some(found) => found.clone(),
        None => {
            // A name outside the cohort caches a miss too, so repeated probes
            // for it cannot spawn a shell per call.
            guard.1.insert(name.to_owned(), None);
            None
        }
    }
}

#[cfg(unix)]
fn resolve_commands_via_shell(names: &[&str]) -> HashMap<String, Option<PathBuf>> {
    let mut results: HashMap<String, Option<PathBuf>> = names
        .iter()
        .map(|name| ((*name).to_owned(), None))
        .collect();
    let Some(bytes) = run_command_lookup(names) else {
        return results;
    };
    for (name, path) in parse_command_lookup(&bytes) {
        if results.contains_key(name.as_str()) {
            results.insert(name, Some(path));
        }
    }
    results
}

/// Pairs of `name\0path` from the lookup script. `command -v` also prints
/// alias definitions and function names, so only an absolute path to a real
/// file counts — anything else is not spawnable.
#[cfg(unix)]
fn parse_command_lookup(bytes: &[u8]) -> Vec<(String, PathBuf)> {
    bytes
        .split(|byte| *byte == 0)
        .collect::<Vec<_>>()
        .as_chunks::<2>()
        .0
        .iter()
        .filter_map(|[name, path]| {
            let name = std::str::from_utf8(name).ok()?.to_owned();
            let path = PathBuf::from(os_string_from_bytes(path)?);
            (path.is_absolute() && path.is_file()).then_some((name, path))
        })
        .collect()
}

/// The `command -v` fallback is POSIX-only; on Windows the env re-capture and
/// the registry-merged `PATH` in `search_paths_from` carry that weight.
#[cfg(not(unix))]
fn resolve_commands_via_shell(names: &[&str]) -> HashMap<String, Option<PathBuf>> {
    names
        .iter()
        .map(|name| ((*name).to_owned(), None))
        .collect()
}

/// One `command -v` per name, NUL-separated `name\0path` pairs written to the
/// capture file so profile noise on stdout cannot corrupt the result.
#[cfg(unix)]
const SHELL_LOOKUP_COMMAND: &str = concat!(
    "for name in \"$@\"; do ",
    "path=$(command -v \"$name\" 2>/dev/null) && printf '%s\\0%s\\0' \"$name\" \"$path\"; ",
    "done > \"$GODDARD_SHELL_ENV_CAPTURE_FILE\""
);

/// `-i -l` first so rc files of both kinds apply; `-i` alone is the retry for
/// a login file that hangs or exits early, and it is the mode that reads
/// `.zshrc` — the gap this fallback exists to close.
#[cfg(unix)]
fn run_command_lookup(names: &[&str]) -> Option<Vec<u8>> {
    for shell in default_shell_candidates() {
        if !shell.is_file() {
            continue;
        }
        for shell_args in [["-i", "-l", "-c"].as_slice(), ["-i", "-c"].as_slice()] {
            if let Some(bytes) =
                capture_command_lookup(&shell, shell_args, names, INTERACTIVE_SHELL_ENV_TIMEOUT)
            {
                return Some(bytes);
            }
        }
    }
    None
}

#[cfg(unix)]
fn capture_command_lookup(
    shell: &Path,
    shell_args: &[&str],
    names: &[&str],
    timeout: Duration,
) -> Option<Vec<u8>> {
    let capture = ShellEnvironmentCapture::create()?;
    let mut command = Command::new(shell);
    command
        .args(shell_args)
        .arg(SHELL_LOOKUP_COMMAND)
        .arg("goddard-command-lookup")
        .args(names)
        .env("GODDARD_SHELL_ENV_CAPTURE_FILE", capture.path())
        .env("DISABLE_AUTO_UPDATE", "true")
        .env("ZSH_TMUX_AUTOSTARTED", "true")
        .env("ZSH_TMUX_AUTOSTART", "false")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command.process_group(0);
    let mut child = spawn(&mut command).ok()?;
    if !wait_for_child(&mut child, timeout).ok()?.success() {
        return None;
    }
    fs::read(capture.path()).ok()
}

pub fn executable_search_path() -> Option<std::ffi::OsString> {
    std::env::join_paths(executable_search_paths()).ok()
}

/// Resolve the user's interactive login-shell environment and cache it for
/// provider discovery and every later child process. This starts a shell and
/// must therefore only be called from a background thread.
#[cfg(unix)]
pub fn refresh_from_default_shell() -> bool {
    let Some(environment) = resolve_default_shell_environment(LOGIN_SHELL_ENV_TIMEOUT) else {
        return false;
    };
    *login_shell_environment()
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(environment);
    note_shell_environment_refreshed();
    true
}

/// Windows has no login shell, but it still needs this probe. A GUI-launched
/// Goddard inherits explorer's `PATH`, which predates later installs, and the
/// package managers users actually add to it — fnm, Volta, nvm — extend
/// `PATH` only in the PowerShell profile, which never reaches the machine or
/// user environment block. Probe PowerShell with the profile loaded, capture
/// the fresh user and machine registry `PATH` values in the same run, and
/// cache the merge for provider discovery and every later child. This starts
/// PowerShell — which runs the user's profile — and must therefore only be
/// called from a background thread.
#[cfg(windows)]
pub fn refresh_from_default_shell() -> bool {
    let Some(environment) = resolve_windows_profile_environment(LOGIN_SHELL_ENV_TIMEOUT) else {
        return false;
    };
    *login_shell_environment()
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(environment);
    note_shell_environment_refreshed();
    true
}

/// Targets with neither probe keep the inherited environment.
#[cfg(not(any(unix, windows)))]
pub fn refresh_from_default_shell() -> bool {
    note_shell_environment_refreshed();
    true
}

fn note_shell_environment_refreshed() {
    SHELL_ENV_GENERATION.fetch_add(1, Ordering::Relaxed);
    *LAST_SHELL_REFRESH
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(Instant::now());
}

/// Re-capture the login-shell environment when the cached one predates
/// [`SHELL_ENV_REFRESH_INTERVAL`]. Provider detection calls this so a CLI
/// installed after the daemon started is found without a restart; a burst
/// of probes in one pass shares a single capture. Must only be called from
/// a background thread — a real capture starts a shell.
pub fn refresh_shell_environment_if_stale() -> bool {
    {
        let last = LAST_SHELL_REFRESH
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(last) = *last
            && last.elapsed() < SHELL_ENV_REFRESH_INTERVAL
        {
            return true;
        }
    }
    refresh_from_default_shell()
}

/// A hanging or exiting profile loses the whole probe run, so the
/// no-profile retry gets its own short leash.
#[cfg(windows)]
const WINDOWS_NO_PROFILE_ENV_TIMEOUT: Duration = Duration::from_secs(2);

/// The PowerShell script the probe runs. Each captured variable is written to
/// the capture file as NUL-separated `name=value` entries — the same format
/// as `env -0` on Unix — so profile noise on stdout cannot corrupt the
/// result. `[Environment]::GetEnvironmentVariable` reads the child's
/// *process* environment, which includes whatever the profile did to
/// `$env:PATH`; the two-argument form reads the fresh registry blocks, which
/// the inherited `PATH` may be older than. Use .NET methods directly so the
/// probe does not need to auto-load a PowerShell module for `New-Object`.
#[cfg(windows)]
const WINDOWS_ENV_CAPTURE_COMMAND: &str = "\
$ErrorActionPreference = 'Continue'
$entries = [System.Collections.Generic.List[string]]::new()
foreach ($name in @('PATH', 'FNM_DIR', 'FNM_MULTISHELL_PATH')) {
  $value = [Environment]::GetEnvironmentVariable($name)
  if ($value) { $entries.Add($name + '=' + $value) }
}
foreach ($target in @('User', 'Machine')) {
  $value = [Environment]::GetEnvironmentVariable('PATH', $target)
  if ($value) { $entries.Add('GODDARD_' + $target.ToUpper() + '_PATH=' + [Environment]::ExpandEnvironmentVariables($value)) }
}
[IO.File]::WriteAllText($env:GODDARD_SHELL_ENV_CAPTURE_FILE, [string]::Join([string][char]0, $entries))
";

/// PowerShell 7 first, then the in-box Windows PowerShell. `cmd.exe` is not a
/// candidate: it has no profile to load and no registry API.
#[cfg(windows)]
fn windows_powershell_candidates() -> Vec<PathBuf> {
    ["pwsh.exe", "powershell.exe"]
        .into_iter()
        .filter_map(find_executable)
        .collect()
}

#[cfg(windows)]
fn resolve_windows_profile_environment(timeout: Duration) -> Option<ShellEnvironment> {
    let started_at = Instant::now();
    // Load each shell's profile first — fnm/Volta/nvm extend PATH there and
    // nowhere else. The registry PATH is captured by the same run, so a
    // profile that merely lacks PATH still succeeds via the registry values.
    for shell in windows_powershell_candidates() {
        let remaining = timeout.checked_sub(started_at.elapsed())?;
        if remaining.is_zero() {
            return None;
        }
        if let Some(environment) = capture_windows_environment(&shell, true, remaining) {
            if let Some(environment) = merge_windows_environment(environment) {
                return Some(environment);
            }
        }
    }
    // A hanging or exiting profile loses the whole run. Retry without it on a
    // short leash; the registry PATH alone is still worth the spawn.
    for shell in windows_powershell_candidates() {
        if let Some(environment) =
            capture_windows_environment(&shell, false, WINDOWS_NO_PROFILE_ENV_TIMEOUT)
        {
            if let Some(environment) = merge_windows_environment(environment) {
                return Some(environment);
            }
        }
    }
    None
}

#[cfg(windows)]
fn capture_windows_environment(
    shell: &Path,
    load_profile: bool,
    timeout: Duration,
) -> Option<ShellEnvironment> {
    let capture = ShellEnvironmentCapture::create()?;
    let mut command = Command::new(shell);
    command.arg("-NoLogo").arg("-NonInteractive");
    if !load_profile {
        command.arg("-NoProfile");
    }
    command
        .arg("-Command")
        .arg(WINDOWS_ENV_CAPTURE_COMMAND)
        .env("GODDARD_SHELL_ENV_CAPTURE_FILE", capture.path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = spawn(&mut command).ok()?;
    if !wait_for_child(&mut child, timeout).ok()?.success() {
        return None;
    }
    parse_shell_environment(&fs::read(capture.path()).ok()?)
}

/// Combine the profile variables with the fresh registry `PATH` values into
/// the cached environment. The profile `PATH` comes first — it is the user's
/// own order — then the user registry `PATH`, then the machine one; the
/// inherited `PATH` is appended afterwards by [`search_paths_from`], and a
/// child `PATH` built by [`child_search_path`] keeps that order. fnm's
/// variables ride along so its shims can resolve their Node installation.
/// `PATH` matching is case-insensitive on Windows, so deduplicate that way.
#[cfg(windows)]
fn merge_windows_environment(mut environment: ShellEnvironment) -> Option<ShellEnvironment> {
    let mut directories = Vec::new();
    for name in ["PATH", "GODDARD_USER_PATH", "GODDARD_MACHINE_PATH"] {
        if let Some(value) = take_environment_variable(&mut environment, name) {
            directories.extend(std::env::split_paths(&value));
        }
    }
    let mut seen = HashSet::new();
    directories
        .retain(|directory| seen.insert(directory.as_os_str().to_string_lossy().to_lowercase()));
    if directories.is_empty() {
        return None;
    }
    let path = std::env::join_paths(directories).ok()?;
    environment.insert(0, (OsString::from("PATH"), path));
    Some(environment)
}

#[cfg(windows)]
fn take_environment_variable(environment: &mut ShellEnvironment, name: &str) -> Option<OsString> {
    let position = environment
        .iter()
        .position(|(candidate, _)| candidate.to_string_lossy().eq_ignore_ascii_case(name))?;
    Some(environment.remove(position).1)
}

fn executable_search_paths() -> Vec<PathBuf> {
    search_paths_from(
        cached_login_shell_variable(OsStr::new("PATH")).as_deref(),
        std::env::var_os("PATH").as_deref(),
        dirs::home_dir().as_deref(),
    )
}

fn login_shell_environment() -> &'static RwLock<Option<ShellEnvironment>> {
    LOGIN_SHELL_ENVIRONMENT.get_or_init(|| RwLock::new(None))
}

pub(crate) fn shell_environment() -> ShellEnvironment {
    login_shell_environment()
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
        .unwrap_or_default()
}

fn cached_login_shell_variable(name: &OsStr) -> Option<OsString> {
    login_shell_environment()
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .as_ref()?
        .iter()
        .find(|(candidate, _)| candidate == name)
        .map(|(_, value)| value.clone())
}

fn search_paths_from(
    shell_path: Option<&OsStr>,
    inherited_path: Option<&OsStr>,
    home: Option<&Path>,
) -> Vec<PathBuf> {
    let mut directories = Vec::new();
    for path in [shell_path, inherited_path].into_iter().flatten() {
        directories.extend(std::env::split_paths(path));
    }
    if let Some(home) = home {
        directories.extend(user_tool_directories(home));
    }
    directories.extend(system_tool_directories());

    let mut seen = HashSet::new();
    directories.retain(|directory| seen.insert(directory.clone()));
    directories
}

/// Where per-user package managers put the provider CLIs, in case the
/// inherited `PATH` predates the install.
#[cfg(not(windows))]
fn user_tool_directories(home: &Path) -> Vec<PathBuf> {
    vec![
        home.join(".local/bin"),
        home.join(".bun/bin"),
        home.join(".cargo/bin"),
        home.join(".local/share/mise/shims"),
        home.join(".volta/bin"),
    ]
}

#[cfg(windows)]
fn user_tool_directories(home: &Path) -> Vec<PathBuf> {
    let mut directories = vec![
        // npm's global prefix, where a `claude.cmd` shim lands.
        home.join("AppData/Roaming/npm"),
        home.join(".bun/bin"),
        home.join(".cargo/bin"),
        home.join("scoop/shims"),
        home.join("AppData/Local/Microsoft/WindowsApps"),
        home.join(".local/bin"),
    ];
    // Volta, pnpm, and the user-scoped Node installer default to LocalAppData
    // (the same list T3 Code probes); it can be redirected away from home.
    if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
        let local_app_data = PathBuf::from(local_app_data);
        directories.push(local_app_data.join("Volta/bin"));
        directories.push(local_app_data.join("pnpm"));
        directories.push(local_app_data.join("Programs/nodejs"));
    }
    directories
}

#[cfg(not(windows))]
fn system_tool_directories() -> Vec<PathBuf> {
    vec![
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
        PathBuf::from("/usr/bin"),
        PathBuf::from("/bin"),
        PathBuf::from("/usr/sbin"),
        PathBuf::from("/sbin"),
    ]
}

#[cfg(windows)]
fn system_tool_directories() -> Vec<PathBuf> {
    let mut directories = Vec::new();
    if let Some(program_files) = std::env::var_os("ProgramFiles") {
        directories.push(PathBuf::from(program_files).join("nodejs"));
    }
    if let Some(system_root) = std::env::var_os("SystemRoot") {
        let system32 = PathBuf::from(system_root).join("System32");
        directories.push(system32.join("WindowsPowerShell/v1.0"));
        directories.push(system32);
    }
    directories
}

#[cfg(unix)]
fn resolve_default_shell_environment(timeout: Duration) -> Option<ShellEnvironment> {
    let started_at = Instant::now();
    for shell in default_shell_candidates() {
        for shell_args in [["-i", "-l", "-c"].as_slice(), ["-l", "-c"].as_slice()] {
            let remaining = timeout.checked_sub(started_at.elapsed())?;
            if remaining.is_zero() {
                return None;
            }
            // Leave part of the total budget for a non-interactive login-shell
            // fallback when an interactive rc file blocks or exits early.
            let attempt_timeout = if shell_args.first() == Some(&"-i") {
                remaining.min(INTERACTIVE_SHELL_ENV_TIMEOUT)
            } else {
                remaining
            };
            if let Some(environment) =
                capture_shell_environment(&shell, shell_args, attempt_timeout)
            {
                return Some(environment);
            }
        }
    }
    None
}

fn default_shell_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    // `SHELL` is a POSIX convention. On Windows it is set only by ported
    // toolchains such as Git Bash, and usually to an MSYS path that Win32
    // cannot open, so the native shells are resolved instead.
    #[cfg(unix)]
    {
        if let Some(shell) = std::env::var_os("SHELL").filter(|shell| !shell.is_empty()) {
            candidates.push(PathBuf::from(shell));
        }
        if let Some(shell) = account_default_shell() {
            candidates.push(shell);
        }
    }
    #[cfg(target_os = "macos")]
    candidates.push(PathBuf::from("/bin/zsh"));
    #[cfg(target_os = "linux")]
    candidates.extend([PathBuf::from("/bin/bash"), PathBuf::from("/bin/sh")]);
    #[cfg(windows)]
    candidates.extend(windows_shell_candidates());

    let mut seen = HashSet::new();
    candidates.retain(|shell| seen.insert(shell.clone()));
    candidates
}

/// PowerShell 7 first, then the in-box Windows PowerShell, then whatever
/// `COMSPEC` names — the same order a Windows Terminal profile list uses.
#[cfg(windows)]
fn windows_shell_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    for shell in ["pwsh.exe", "powershell.exe"] {
        candidates.extend(find_executable(shell));
    }
    if let Some(comspec) = std::env::var_os("COMSPEC").filter(|comspec| !comspec.is_empty()) {
        candidates.push(PathBuf::from(comspec));
    }
    candidates
}

/// Pick the user's configured login shell for an interactive terminal, with a
/// platform shell as a final fallback when desktop launchers omit `SHELL`.
pub fn default_terminal_shell() -> PathBuf {
    let mut candidates = Vec::new();
    // The shell the user chose outranks whatever `SHELL` was inherited from;
    // see the note in waku-client's `unix_terminal_shell_candidates`. The
    // environment probe below keeps its own order, since it wants the shell
    // whose rc files produced this process's `PATH`.
    #[cfg(unix)]
    candidates.extend(account_default_shell());
    candidates.extend(default_shell_candidates());

    candidates
        .into_iter()
        .find(|shell| shell.is_file())
        .unwrap_or_else(default_terminal_shell_fallback)
}

#[cfg(not(windows))]
fn default_terminal_shell_fallback() -> PathBuf {
    PathBuf::from("/bin/sh")
}

#[cfg(windows)]
fn default_terminal_shell_fallback() -> PathBuf {
    PathBuf::from("cmd.exe")
}

/// The arguments that open `shell` the way the user's own terminal would.
///
/// A POSIX shell needs `-l` so the login files that set `PATH` are read.
/// Windows applies the environment before the process starts, so there is no
/// login mode to ask for — PowerShell's `-Login` exists on Unix hosts only,
/// and passing it here is an error. Suppressing its banner is the one thing
/// worth saying.
pub fn default_terminal_shell_args(shell: &Path) -> Vec<String> {
    #[cfg(not(windows))]
    {
        let _ = shell;
        vec!["-l".to_owned()]
    }
    #[cfg(windows)]
    {
        let is_powershell = shell
            .file_stem()
            .and_then(|stem| stem.to_str())
            .is_some_and(|stem| {
                stem.eq_ignore_ascii_case("pwsh") || stem.eq_ignore_ascii_case("powershell")
            });
        if is_powershell {
            vec!["-NoLogo".to_owned()]
        } else {
            Vec::new()
        }
    }
}

#[cfg(unix)]
fn account_default_shell() -> Option<PathBuf> {
    let suggested_size = unsafe { libc::sysconf(libc::_SC_GETPW_R_SIZE_MAX) };
    let mut buffer_size = if suggested_size > 0 {
        suggested_size as usize
    } else {
        16 * 1024
    };
    loop {
        let mut passwd = MaybeUninit::<libc::passwd>::uninit();
        let mut result = std::ptr::null_mut();
        let mut buffer = vec![0_u8; buffer_size];
        let status = unsafe {
            libc::getpwuid_r(
                libc::geteuid(),
                passwd.as_mut_ptr(),
                buffer.as_mut_ptr().cast(),
                buffer.len(),
                &mut result,
            )
        };
        if status == libc::ERANGE && buffer_size < 1024 * 1024 {
            buffer_size *= 2;
            continue;
        }
        if status != 0 || result.is_null() {
            return None;
        }
        let shell = unsafe { (*result).pw_shell };
        if shell.is_null() {
            return None;
        }
        let bytes = unsafe { CStr::from_ptr(shell) }.to_bytes();
        return (!bytes.is_empty()).then(|| PathBuf::from(OsString::from_vec(bytes.to_vec())));
    }
}

#[cfg(unix)]
fn capture_shell_environment(
    shell: &Path,
    shell_args: &[&str],
    timeout: Duration,
) -> Option<ShellEnvironment> {
    let capture = ShellEnvironmentCapture::create()?;
    let mut command = Command::new(shell);
    command
        .args(shell_args)
        .arg(SHELL_ENV_COMMAND)
        .env("GODDARD_SHELL_ENV_CAPTURE_FILE", capture.path())
        // Match shell-env's safeguards for common interactive zsh setups so
        // an update prompt or tmux auto-start cannot consume the probe budget.
        .env("DISABLE_AUTO_UPDATE", "true")
        .env("ZSH_TMUX_AUTOSTARTED", "true")
        .env("ZSH_TMUX_AUTOSTART", "false")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    command.process_group(0);

    let mut child = spawn(&mut command).ok()?;
    if !wait_for_child(&mut child, timeout).ok()?.success() {
        return None;
    }
    parse_shell_environment(&fs::read(capture.path()).ok()?)
}

fn parse_shell_environment(bytes: &[u8]) -> Option<ShellEnvironment> {
    let environment = bytes
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty())
        .filter_map(|entry| {
            let separator = entry.iter().position(|byte| *byte == b'=')?;
            if separator == 0 {
                return None;
            }
            let name = os_string_from_bytes(&entry[..separator])?;
            if is_shell_capture_variable(&name) {
                return None;
            }
            let value = os_string_from_bytes(&entry[separator + 1..])?;
            Some((name, value))
        })
        .collect::<Vec<_>>();
    (!environment.is_empty()).then_some(environment)
}

fn is_shell_capture_variable(name: &OsStr) -> bool {
    [
        "GODDARD_SHELL_ENV_CAPTURE_FILE",
        "DISABLE_AUTO_UPDATE",
        "ZSH_TMUX_AUTOSTARTED",
        "ZSH_TMUX_AUTOSTART",
    ]
    .into_iter()
    .any(|candidate| name == OsStr::new(candidate))
}

fn os_string_from_bytes(bytes: &[u8]) -> Option<OsString> {
    #[cfg(unix)]
    {
        Some(OsString::from_vec(bytes.to_vec()))
    }
    #[cfg(windows)]
    {
        // The Windows probe writes UTF-8; PowerShell encodes .NET strings
        // exactly, so lossy decoding loses nothing.
        Some(String::from_utf8_lossy(bytes).into_owned().into())
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = bytes;
        None
    }
}

fn wait_for_child(child: &mut Child, timeout: Duration) -> io::Result<ExitStatus> {
    let started_at = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) if started_at.elapsed() < timeout => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Ok(None) => {
                terminate_shell_capture(child);
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!("child process did not exit within {timeout:?}"),
                ));
            }
            Err(error) => {
                terminate_shell_capture(child);
                return Err(error);
            }
        }
    }
}

fn terminate_shell_capture(child: &mut Child) {
    #[cfg(unix)]
    unsafe {
        libc::kill(-(child.id() as i32), libc::SIGKILL);
    }
    let _ = child.kill();
    let _ = child.wait();
}

struct ShellEnvironmentCapture(PathBuf);

impl ShellEnvironmentCapture {
    fn create() -> Option<Self> {
        for _ in 0..16 {
            let id = SHELL_ENV_CAPTURE_ID.fetch_add(1, Ordering::Relaxed);
            let path =
                std::env::temp_dir().join(format!(".waku-shell-env-{}-{id}", std::process::id()));
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            options.mode(0o600);
            match options.open(&path) {
                Ok(_) => return Some(Self(path)),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(_) => return None,
            }
        }
        None
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for ShellEnvironmentCapture {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    fn command_search_path(command: &Command) -> Vec<PathBuf> {
        let path = command
            .get_envs()
            .find(|(key, _)| *key == OsStr::new("PATH"))
            .and_then(|(_, value)| value)
            .expect("a provider command sets PATH for its child");
        std::env::split_paths(path).collect()
    }

    fn agent_launch_env(token: &str) -> crate::agent::AgentLaunchEnv {
        crate::agent::AgentLaunchEnv {
            token: token.to_owned(),
            task_id: uuid::Uuid::new_v4(),
            parent_task_id: None,
            daemon_address: "127.0.0.1:7777".to_owned(),
            cli_path: PathBuf::from(if cfg!(windows) {
                "C:\\waku\\bin\\goddard-agent.exe"
            } else {
                "/waku/bin/goddard-agent"
            }),
            shim_directory: PathBuf::from(if cfg!(windows) {
                "C:\\waku\\agent\\session"
            } else {
                "/waku/agent/session"
            }),
            task_tools: true,
            settings_writes: true,
        }
    }

    #[test]
    fn merge_agent_environment_replaces_path_and_adds_the_credential() {
        let mut environment = vec![
            ("PATH".to_owned(), "/usr/bin".to_owned()),
            ("HOME".to_owned(), "/home/test".to_owned()),
        ];
        let agent = agent_launch_env("token-1");
        merge_agent_environment(&mut environment, &agent);

        // libc getenv resolves the first PATH, so a second one would be dead
        // config: the merge replaces rather than appends.
        let paths: Vec<&str> = environment
            .iter()
            .filter(|(name, _)| name.eq_ignore_ascii_case("PATH"))
            .map(|(_, value)| value.as_str())
            .collect();
        assert_eq!(paths.len(), 1);
        let directories: Vec<PathBuf> = std::env::split_paths(paths[0]).collect();
        assert_eq!(
            directories.first().map(PathBuf::as_path),
            agent.cli_path.parent()
        );
        assert!(directories.contains(&PathBuf::from("/usr/bin")));

        for (name, expected) in [
            ("GODDARD_AGENT_TOKEN", "token-1"),
            ("GODDARD_TASK_ID", agent.task_id.to_string().as_str()),
            ("GODDARD_DAEMON_ADDRESS", "127.0.0.1:7777"),
        ] {
            assert!(
                environment
                    .iter()
                    .any(|(key, value)| key == name && value == expected),
                "{name} missing from the merged environment"
            );
        }
        assert!(environment.iter().any(|(key, _)| key == "HOME"));
    }

    #[test]
    fn merge_agent_environment_into_an_empty_vector_still_sets_path() {
        let mut environment = Vec::new();
        let agent = agent_launch_env("token-2");
        merge_agent_environment(&mut environment, &agent);
        assert!(
            environment
                .iter()
                .any(|(key, _)| key.eq_ignore_ascii_case("PATH"))
        );
    }

    #[test]
    fn apply_agent_environment_prepends_the_cli_directory_to_path() {
        let mut command = command("cat");
        let agent = agent_launch_env("token-3");
        apply_agent_environment(&mut command, &agent);

        let directories = command_search_path(&command);
        assert_eq!(
            directories.first().map(PathBuf::as_path),
            agent.cli_path.parent()
        );
        let token = command
            .get_envs()
            .find(|(key, _)| *key == OsStr::new("GODDARD_AGENT_TOKEN"))
            .and_then(|(_, value)| value)
            .expect("the scoped token is injected");
        assert_eq!(token, OsStr::new("token-3"));
    }

    /// A CLI resolved from a directory the desktop `PATH` never had must run
    /// with that directory too, or its own launcher — a Bun shim, an npm
    /// `.cmd`, an `env node` shebang — cannot find its runtime.
    #[test]
    fn a_provider_cli_runs_with_the_directories_detection_searched() {
        #[cfg(windows)]
        let program = PathBuf::from("C:\\waku-fixture\\bin\\pi.exe");
        #[cfg(not(windows))]
        let program = PathBuf::from("/opt/waku-fixture/bin/pi");

        let directories = command_search_path(&command(&program));

        assert!(directories.contains(&program.parent().expect("fixture parent").to_path_buf()));
        for searched in executable_search_paths() {
            assert!(
                directories.contains(&searched),
                "{} is searched during detection but missing from the child PATH",
                searched.display()
            );
        }
    }

    #[test]
    fn a_bare_program_name_contributes_no_search_directory() {
        let directories = command_search_path(&command("git"));

        assert_eq!(directories, executable_search_paths());
    }

    #[cfg(unix)]
    #[test]
    fn resolve_spawn_program_leaves_named_paths_alone() {
        for program in ["/bin/sh", "dir/tool", "./tool"] {
            assert_eq!(
                resolve_spawn_program(OsStr::new(program)),
                OsString::from(program)
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn resolve_spawn_program_resolves_a_bare_name_from_the_search_path() {
        let resolved = PathBuf::from(resolve_spawn_program(OsStr::new("sh")));

        assert!(resolved.is_absolute());
        assert!(resolved.is_file());
    }

    #[cfg(unix)]
    #[test]
    fn resolve_spawn_program_keeps_an_unresolvable_name() {
        let missing = "goddard-no-such-program-0f3c1b";

        assert_eq!(
            resolve_spawn_program(OsStr::new(missing)),
            OsString::from(missing)
        );
    }

    /// `std` refuses `posix_spawn` for a bare program name whose environment
    /// overrides `PATH` — the spawn then forks the whole process, and on
    /// macOS each fork stalls every allocator behind the malloc fork lock.
    /// The commands the daemon builds must carry a resolved program path.
    #[cfg(unix)]
    #[test]
    fn provider_commands_carry_a_resolved_program() {
        for built in [command("sh"), search_path_command("sh")] {
            let program = built.get_program();
            assert!(
                Path::new(program).is_absolute(),
                "{} must resolve so the spawn can posix_spawn instead of fork",
                program.to_string_lossy()
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn output_captures_stdout_and_stderr() {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "printf stdout; printf stderr >&2"]);

        let output = output(&mut command).expect("command should run");

        assert_eq!(output.stdout, b"stdout");
        assert_eq!(output.stderr, b"stderr");
    }

    #[test]
    fn child_wait_distinguishes_exit_status_from_timeout_and_reaps_the_child() {
        const CHILD_MODE: &str = "GODDARD_CHILD_WAIT_TEST_MODE";
        if let Some(mode) = std::env::var_os(CHILD_MODE) {
            match mode.to_str().expect("child mode") {
                "success" => std::process::exit(0),
                "failure" => std::process::exit(23),
                "hang" => loop {
                    std::thread::park();
                },
                _ => panic!("unknown child mode"),
            }
        }

        for mode in ["success", "failure", "hang"] {
            let mut command = Command::new(std::env::current_exe().expect("test executable"));
            command
                .args([
                    "--exact",
                    "command_env::tests::child_wait_distinguishes_exit_status_from_timeout_and_reaps_the_child",
                ])
                .env(CHILD_MODE, mode)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            #[cfg(unix)]
            command.process_group(0);
            let mut child = spawn(&mut command).expect("spawn child fixture");
            if mode == "hang" {
                let error = wait_for_child(&mut child, Duration::ZERO).expect_err("timeout");
                assert_eq!(error.kind(), io::ErrorKind::TimedOut);
                assert!(
                    !child
                        .try_wait()
                        .expect("read killed child status")
                        .unwrap()
                        .success()
                );
            } else {
                let status = wait_for_child(&mut child, Duration::from_secs(60))
                    .expect("child fixture should exit");
                assert_eq!(status.code(), Some(if mode == "success" { 0 } else { 23 }));
            }
        }
    }

    #[cfg(target_os = "macos")]
    fn sigchld_is_blocked() -> io::Result<bool> {
        let mut current = MaybeUninit::<libc::sigset_t>::uninit();
        pthread_result(unsafe {
            libc::pthread_sigmask(libc::SIG_SETMASK, std::ptr::null(), current.as_mut_ptr())
        })?;
        Ok(unsafe { libc::sigismember(current.as_ptr(), libc::SIGCHLD) } == 1)
    }

    #[cfg(target_os = "macos")]
    fn block_sigchld() -> io::Result<SignalMaskRestore> {
        let sigchld = sigchld_set()?;
        let mut previous = MaybeUninit::<libc::sigset_t>::uninit();
        pthread_result(unsafe {
            libc::pthread_sigmask(libc::SIG_BLOCK, &sigchld, previous.as_mut_ptr())
        })?;
        Ok(SignalMaskRestore(unsafe { previous.assume_init() }))
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn spawn_unblocks_sigchld_in_the_child_and_restores_the_caller() {
        if std::env::var_os("GODDARD_SIGCHLD_CHILD_PROBE").is_some() {
            assert!(!sigchld_is_blocked().expect("read child signal mask"));
            return;
        }

        let _restore_original = block_sigchld().expect("block SIGCHLD for the fixture");
        assert!(sigchld_is_blocked().expect("read blocked parent mask"));

        let mut command = Command::new(std::env::current_exe().expect("resolve test executable"));
        command
            .args([
                "--exact",
                "command_env::tests::spawn_unblocks_sigchld_in_the_child_and_restores_the_caller",
                "--nocapture",
            ])
            .env("GODDARD_SIGCHLD_CHILD_PROBE", "1");
        let output = output(&mut command).expect("spawn child signal probe");

        assert!(
            output.status.success(),
            "child signal probe failed:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(sigchld_is_blocked().expect("read restored parent mask"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn dedicated_provider_thread_can_normalize_sigchld() {
        let _restore_original = block_sigchld().expect("block SIGCHLD for the fixture");

        unblock_sigchld_for_current_thread().expect("unblock provider thread");

        assert!(!sigchld_is_blocked().expect("read normalized signal mask"));
    }

    #[cfg(unix)]
    #[test]
    fn launch_services_path_is_extended_for_script_based_clis() {
        let home = Path::new("/Users/example");
        let paths = search_paths_from(None, Some(OsStr::new("/usr/bin:/bin")), Some(home));

        assert_eq!(paths[0], PathBuf::from("/usr/bin"));
        assert_eq!(paths[1], PathBuf::from("/bin"));
        assert!(paths.contains(&home.join(".bun/bin")));
        assert!(paths.contains(&home.join(".local/share/mise/shims")));
        assert!(paths.contains(&PathBuf::from("/opt/homebrew/bin")));
        assert_eq!(
            paths
                .iter()
                .filter(|path| *path == Path::new("/bin"))
                .count(),
            1
        );
    }

    #[cfg(unix)]
    #[test]
    fn login_shell_path_precedes_the_inherited_desktop_path() {
        let paths = search_paths_from(
            Some(OsStr::new(
                "/Users/example/.nvm/versions/node/v22.0.0/bin:/Users/example/.local/share/fnm/current/bin",
            )),
            Some(OsStr::new("/usr/bin:/bin")),
            None,
        );

        assert_eq!(
            paths[..4],
            [
                PathBuf::from("/Users/example/.nvm/versions/node/v22.0.0/bin"),
                PathBuf::from("/Users/example/.local/share/fnm/current/bin"),
                PathBuf::from("/usr/bin"),
                PathBuf::from("/bin"),
            ]
        );
    }

    #[cfg(windows)]
    #[test]
    fn desktop_path_is_extended_with_the_windows_package_manager_prefixes() {
        let home = Path::new("C:\\Users\\example");
        let paths = search_paths_from(
            None,
            Some(OsStr::new("C:\\Windows\\System32;C:\\Windows")),
            Some(home),
        );

        assert_eq!(paths[0], PathBuf::from("C:\\Windows\\System32"));
        assert_eq!(paths[1], PathBuf::from("C:\\Windows"));
        assert!(paths.contains(&home.join("AppData/Roaming/npm")));
        assert!(paths.contains(&home.join(".bun/bin")));
        assert!(paths.contains(&home.join("scoop/shims")));
        assert!(paths.contains(&home.join(".local/bin")));
        if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
            let local_app_data = PathBuf::from(local_app_data);
            assert!(paths.contains(&local_app_data.join("Volta/bin")));
            assert!(paths.contains(&local_app_data.join("pnpm")));
            assert!(paths.contains(&local_app_data.join("Programs/nodejs")));
        }
        assert_eq!(
            paths
                .iter()
                .filter(|path| *path == Path::new("C:\\Windows"))
                .count(),
            1
        );
    }

    /// The probe script must execute as written against the in-box Windows
    /// PowerShell: `-NoProfile` leaves the child `PATH` untouched, so the
    /// captured value is exactly the one this process inherited.
    #[cfg(windows)]
    #[test]
    fn windows_environment_probe_captures_the_inherited_path_without_a_profile() {
        let capture = ShellEnvironmentCapture::create().expect("create capture file");
        let diagnostics = ShellEnvironmentCapture::create().expect("create diagnostics file");
        let output = fs::File::create(diagnostics.path()).expect("open diagnostics file");
        let mut command = Command::new("powershell.exe");
        command
            .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command"])
            .arg(WINDOWS_ENV_CAPTURE_COMMAND)
            .env("GODDARD_SHELL_ENV_CAPTURE_FILE", capture.path())
            .stdin(Stdio::null())
            // A file preserves diagnostics without a pipe buffer blocking the probe.
            .stdout(output.try_clone().expect("clone diagnostics handle"))
            .stderr(output);
        let mut child = spawn(&mut command).expect("spawn PowerShell probe");
        let started_at = Instant::now();
        // This checks the script's output, not PowerShell's cold-start latency
        // on a busy CI runner. Production probes keep their short deadlines.
        let result = wait_for_child(&mut child, Duration::from_secs(60));
        assert!(
            matches!(&result, Ok(status) if status.success()),
            "PowerShell probe failed after {:?}: {result:?}\n{}",
            started_at.elapsed(),
            String::from_utf8_lossy(&fs::read(diagnostics.path()).expect("read probe diagnostics"))
        );
        let environment =
            parse_shell_environment(&fs::read(capture.path()).expect("capture file written"))
                .expect("parse captured environment");
        let path = environment
            .iter()
            .find(|(name, _)| name == OsStr::new("PATH"))
            .map(|(_, value)| value.clone())
            .expect("captured PATH");
        assert_eq!(path, std::env::var_os("PATH").expect("inherited PATH"));
    }

    #[cfg(windows)]
    #[test]
    fn windows_profile_environment_merges_registry_paths_behind_the_profile_path() {
        let environment = merge_windows_environment(vec![
            (OsString::from("FNM_DIR"), OsString::from("C:\\fnm")),
            (
                OsString::from("PATH"),
                OsString::from("C:\\profile-first;C:\\shared"),
            ),
            (
                OsString::from("GODDARD_USER_PATH"),
                OsString::from("C:\\user;C:\\shared"),
            ),
            (
                OsString::from("GODDARD_MACHINE_PATH"),
                OsString::from("C:\\machine;C:\\USER;C:\\SHARED"),
            ),
        ])
        .expect("merge captured environment");

        let path = environment
            .iter()
            .find(|(name, _)| name == OsStr::new("PATH"))
            .map(|(_, value)| value.clone())
            .expect("merged PATH");
        assert_eq!(
            std::env::split_paths(&path).collect::<Vec<_>>(),
            vec![
                PathBuf::from("C:\\profile-first"),
                PathBuf::from("C:\\shared"),
                PathBuf::from("C:\\user"),
                PathBuf::from("C:\\machine"),
            ]
        );
        assert!(environment.contains(&(OsString::from("FNM_DIR"), OsString::from("C:\\fnm"))));
        assert!(
            !environment
                .iter()
                .any(|(name, _)| name == OsStr::new("GODDARD_USER_PATH"))
        );
        assert!(
            !environment
                .iter()
                .any(|(name, _)| name == OsStr::new("GODDARD_MACHINE_PATH"))
        );
    }

    #[cfg(windows)]
    #[test]
    fn a_bare_name_resolves_through_pathext() {
        let directory = std::env::temp_dir().join(format!("waku-pathext-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("create fixture directory");
        // Only a suffixed file exists here, so the bare name resolves through PATHEXT.
        // A global npm install also drops an extensionless shim beside it; that layout is
        // covered by a_bare_name_prefers_pathext_over_an_extensionless_shim.
        std::fs::write(directory.join("faux-provider.cmd"), "@echo off\n")
            .expect("write shim fixture");

        assert_eq!(
            resolve_executable_file(&directory.join("faux-provider"))
                .expect("resolve through PATHEXT")
                .to_string_lossy()
                .to_lowercase(),
            directory
                .join("faux-provider.cmd")
                .to_string_lossy()
                .to_lowercase(),
        );
        assert_eq!(resolve_executable_file(&directory.join("absent")), None);

        let _ = std::fs::remove_dir_all(&directory);
    }

    #[cfg(windows)]
    #[test]
    fn a_bare_name_prefers_pathext_over_an_extensionless_shim() {
        // A global npm install writes all three of these side by side. Only the `.cmd`
        // can be launched by `CreateProcess`; the extensionless one is a POSIX shim.
        let directory =
            std::env::temp_dir().join(format!("waku-pathext-shim-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("create fixture directory");
        std::fs::write(directory.join("faux-provider"), "#!/bin/sh\n")
            .expect("write posix shim fixture");
        std::fs::write(directory.join("faux-provider.ps1"), "#!/usr/bin/env pwsh\n")
            .expect("write powershell shim fixture");
        std::fs::write(directory.join("faux-provider.cmd"), "@echo off\n")
            .expect("write cmd shim fixture");

        assert_eq!(
            resolve_executable_file(&directory.join("faux-provider"))
                .expect("resolve through PATHEXT")
                .to_string_lossy()
                .to_lowercase(),
            directory
                .join("faux-provider.cmd")
                .to_string_lossy()
                .to_lowercase(),
        );

        let _ = std::fs::remove_dir_all(&directory);
    }

    #[cfg(windows)]
    #[test]
    fn windows_terminal_shells_never_ask_for_a_login_session() {
        assert_eq!(
            default_terminal_shell_args(Path::new("C:\\Program Files\\PowerShell\\7\\pwsh.exe")),
            vec!["-NoLogo".to_owned()]
        );
        assert!(
            default_terminal_shell_args(Path::new("C:\\Windows\\System32\\cmd.exe")).is_empty()
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_shell_candidates_have_system_fallbacks() {
        let candidates = default_shell_candidates();

        assert!(candidates.contains(&PathBuf::from("/bin/bash")));
        assert!(candidates.contains(&PathBuf::from("/bin/sh")));
        assert!(default_terminal_shell().is_file());
    }

    #[cfg(unix)]
    #[test]
    fn parses_null_delimited_environment_without_losing_value_contents() {
        let environment = parse_shell_environment(
            b"PATH=/Users/example/.fnm/current/bin:/usr/bin\0TOKEN=line one\nline two=rest\0EMPTY=\0GODDARD_SHELL_ENV_CAPTURE_FILE=/tmp/capture\0",
        )
        .expect("parse shell environment");

        assert_eq!(
            environment,
            vec![
                (
                    OsString::from("PATH"),
                    OsString::from("/Users/example/.fnm/current/bin:/usr/bin"),
                ),
                (
                    OsString::from("TOKEN"),
                    OsString::from("line one\nline two=rest"),
                ),
                (OsString::from("EMPTY"), OsString::new()),
            ]
        );
    }

    #[cfg(unix)]
    #[test]
    fn captures_environment_from_a_shell_process() {
        let id = SHELL_ENV_CAPTURE_ID.fetch_add(1, Ordering::Relaxed);
        let directory =
            std::env::temp_dir().join(format!("waku-command-env-test-{}-{id}", std::process::id()));
        fs::create_dir(&directory).expect("create shell fixture directory");
        let shell = directory.join("fake-shell");
        fs::write(
            &shell,
            "#!/bin/sh\n/usr/bin/printf 'PATH=/Users/example/.fnm/current/bin:/usr/bin\\000GODDARD_TEST_TOKEN=from-shell\\000' > \"$GODDARD_SHELL_ENV_CAPTURE_FILE\"\n",
        )
        .expect("write shell fixture");
        let mut permissions = fs::metadata(&shell)
            .expect("read shell fixture")
            .permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&shell, permissions).expect("make shell fixture executable");

        let environment =
            capture_shell_environment(&shell, &["-i", "-l", "-c"], LOGIN_SHELL_ENV_TIMEOUT)
                .expect("capture shell environment");

        assert_eq!(
            environment,
            vec![
                (
                    OsString::from("PATH"),
                    OsString::from("/Users/example/.fnm/current/bin:/usr/bin"),
                ),
                (
                    OsString::from("GODDARD_TEST_TOKEN"),
                    OsString::from("from-shell"),
                ),
            ]
        );
        let _ = fs::remove_file(shell);
        let _ = fs::remove_dir(directory);
    }

    #[cfg(unix)]
    #[test]
    fn command_lookup_keeps_only_absolute_paths_to_real_files() {
        let id = SHELL_ENV_CAPTURE_ID.fetch_add(1, Ordering::Relaxed);
        let directory =
            std::env::temp_dir().join(format!("waku-lookup-test-{}-{id}", std::process::id()));
        fs::create_dir(&directory).expect("create lookup fixture directory");
        let binary = directory.join("faux-provider");
        fs::write(&binary, "#!/bin/sh\n").expect("write binary fixture");
        let mut permissions = fs::metadata(&binary)
            .expect("read binary fixture")
            .permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&binary, permissions).expect("make binary fixture executable");

        let captured = format!(
            "faux-provider\0{}\0aliased\0pi: aliased to pi --stdio\0relative\0faux-provider\0dangling\0/no/such/path\0",
            binary.display()
        );
        let parsed = parse_command_lookup(captured.as_bytes());

        assert_eq!(parsed, vec![("faux-provider".to_owned(), binary)]);
        let _ = fs::remove_dir_all(&directory);
    }

    #[cfg(unix)]
    #[test]
    fn a_command_the_shell_cannot_find_is_a_cached_miss() {
        let name = "waku-definitely-missing-cli";

        assert_eq!(find_executable_via_shell(name, &[name]), None);
        // The negative answer is cached until the environment generation
        // bumps; a repeat probe must not spawn another shell.
        assert_eq!(find_executable_via_shell(name, &[name]), None);
    }
}
