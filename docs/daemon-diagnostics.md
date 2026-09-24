# Daemon diagnostics

How to answer "why did the daemon restart" — the signals, where they live,
and how to read them together.

## The signals

| Signal | Where | What it tells you |
| --- | --- | --- |
| `errors.jsonl` | `~/.goddard/` | One JSONL record per error toast the app showed — `{at, atLocal, app, kind, message}` where `kind` is `alert` or `failure`, plus a `context` object (`sessionId`, `session`, `provider`, `workingDir`, `daemon`) naming the task on screen when it fired; incognito tasks leave only `provider`. The Diagnostics settings page reads this plus `daemon-recovery.jsonl` and `daemon-panics.jsonl` into one feed |
| `daemon.recovery` | Umami event | One recovery episode: `cause` (`unexpected_exit`, `disconnect`, `rebuild`), `outcome` (`recovered`, `unreachable`), `sessionsResumed`, `daemonRssMb`/`childrenRssMb` (pre-restart readings), `exitCode`/`exitSignal`/`exitSignalCode` when a real exit happened, `previousBootClean`. The same record lands in `~/.goddard/daemon-recovery.jsonl` with `atLocal` and `app` added |
| `daemon.crash` | Umami event | One OS crash report per unseen `goddard-daemon-*.ips`, scanned at launch: `termination` namespace (`exc_resource` = jetsam/resource limits, `signal` = crash/kill), `signal`, `uptimeSecs` |
| `daemon-stats.jsonl` | `~/Library/Application Support/<App>/` | One JSONL sample per minute per boot: `boot`, `at`, `daemonRssMb`, `childrenRssMb` (whole descendant tree — provider runtimes carry memory under their own pids), `runtimes`, `terminals`, plus per-subtree `children` rows (`pid`, `name`, subtree `rssMb`, `processes`, `kind`, `sessionId`/`provider` when claimed) and per-session `sessions` rows (`detailLoaded`, `running`, `residentMessages`/`residentActivities`/`residentBytes`). A `"shutdown": true` line is the clean-exit marker |
| `daemon-panics.jsonl` | `~/Library/Application Support/<App>/` | One line per panic: `at`, `atLocal`, `version`, `cwd`, `thread`, `location` (file:line:col), `message` (first line, 400 chars). Request-thread panics unwind without killing the daemon — a wedged handler leaves its trace here |
| `daemon-crashes.json` | `~/.goddard/` | Internal watermark for the `.ips` scan (`lastSeenAt` mtime); not diagnostic data itself |

Analytics are release-only; the files exist in every build and are the
fallback forensics. `daemon-stats.jsonl` and `daemon-panics.jsonl` each
cap at 512 KB by keeping the newest half; `errors.jsonl` caps at 256 KB
the same way.

## How the supervisor decides

`monitor_daemon` in [crates/waku-client/src/process.rs](../crates/waku-client/src/process.rs)
drives everything:

1. Every 5 s the app's shared connection answers a `GetSettings` probe
   (3 s budget). A miss earns a second opinion on a **fresh connection** —
   the accept loop plus one request — before the client is marked dead.
2. `try_wait` is authoritative for process death. Three observations:
   - **exited** → `unexpected_exit`, respawn immediately (with
     crash-loop backoff if it died young);
   - **alive but disconnected** → `disconnect`, reconnect in place via
     `connect_with_resume` — provider sessions survive; escalate to
     kill-and-respawn only after 3 failed reconnects;
   - **binary stamp changed** (dev only) → `rebuild`, swap in place.
3. The episode pins cause and exit detail at first detection — mid-respawn
   iterations observe the `Restarting` placeholder, not the dead process.
4. `unreachable` reports once per episode after 4 consecutive failures;
   `recovered` reports when a working connection is back.

## Reading a restart

| `cause` | `exitSignal` | `previousBootClean` | Likely story |
| --- | --- | --- | --- |
| `unexpected_exit` | `SIGKILL` | `false` | Jetsam or `kill -9` — check `childrenRssMb`/`daemonRssMb` on the event and `termination=exc_resource` in a `daemon.crash` event |
| `unexpected_exit` | `SIGSEGV`/`SIGBUS` | `false` | Crash — an `.ips` should exist; `daemon.crash` names the signal and uptime |
| `unexpected_exit` | `SIGABRT` | `false` | Abort — check `daemon-panics.jsonl` (a panic on a fatal path) and the `.ips` indicator |
| `unexpected_exit` | none, `exitCode=1` | — | Startup failure — bind error, unreadable settings/state; check daemon stderr |
| `unexpected_exit` | none, `exitCode=0` | `true` | Orderly exit the app didn't initiate — external shutdown command, or parent-death watchdog |
| `disconnect` | none | — | Socket died while the daemon lived: transport error, or a confirmed probe miss. Reconnects are silent for sessions; escalation to respawn means the daemon was genuinely unreachable |
| `rebuild` | none | — | Dev watcher relinked the daemon binary; release builds never emit this |

`daemonRssMb`/`childrenRssMb` on a `recovered` event are the previous
boot's **final** sample — memory pressure shows up before the kill.
`previousBootClean=false` on `unexpected_exit` confirms the death was
abnormal; `true` means the daemon ran its shutdown path.

## Reading memory

`children` itemizes where descendant memory sits: one row per direct child
of the daemon, `rssMb` summed over its whole subtree, `name` taken from the
heaviest member (a provider CLI such as `devin`, or the shell a terminal
runs). A row is `kind: "runtime"` when a member's working directory matches
a live session's workspace — `sessionId`/`provider` are set then — and
`kind: "terminal"` when a member is a remote terminal's PTY. Rows that
claim nothing are `kind: "other"`: mid-teardown subtrees or helpers.

`sessions` lists sessions holding resident transcripts — `detailLoaded`
means the full message/activity history is in memory and `residentBytes` is
a rough heap estimate, good for ranking, not exact billing. Skeletons
(never hydrated or already trimmed) are omitted; `sessionsTotal` counts
them anyway. Incognito sessions appear with an empty `title` — the file
must not persist what incognito keeps off disk.

## Caveats

- **The stats file can't name a SIGKILL's sender.** `stop()` first asks
  the daemon to shut down (which writes the marker), and only sends
  SIGKILL after a 1 s timeout — so a markerless death is usually jetsam
  or an external `kill -9`, but a daemon too wedged to answer the
  shutdown command in time also ends markerless. On the **event** this
  ambiguity mostly disappears: exit detail is pinned at first detection,
  so `exitSignal=SIGKILL` on `unexpected_exit` means the process was
  already dead before the supervisor touched it — genuinely external.
  The remaining gap is raw-file forensics, where the two SIGKILLs look
  identical.
- **A disconnect that escalates stays a disconnect.** If three reconnects
  fail and the supervisor kills the wedged daemon, the episode reports
  `cause=disconnect` — the trigger, not the remedy.
- **Probe misses need two independent failures now.** A shared-socket
  probe miss followed by a fresh-connection probe miss means the daemon's
  accept loop or request pipeline is genuinely stuck — one app-side stall
  can no longer restart anything.
- **Spurious `recovered`.** A down observation that clears by the
  restart-lock recheck still emits a `recovered` report for a flap that
  needed nothing — rare, and worth knowing before over-reading recovery
  counts.
- **`uptimeSecs` on `daemon.crash` is the daemon's process lifetime**, so
  a low uptime there plus frequent `unexpected_exit` means crash-looping,
  not slow leaks.
