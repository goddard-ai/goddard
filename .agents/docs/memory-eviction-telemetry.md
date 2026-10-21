# Memory eviction telemetry

Use `<daemon-data-dir>/memory-guard/evictions.jsonl` to measure how often
Goddard interrupts runtimes for memory and how far containment escalates.
The dev watcher uses `GODDARD_DATA_DIR`, or its checkout's `temp/` when unset.
Records are local and best effort. The log keeps at most approximately 4 MiB,
rotating to its newest half at a line boundary; counts cover retained history,
not lifetime totals. The previous `incidents.jsonl` remains available but only
covers memory-guard starts. Do not add those records to the new log's counts.

Each intervention has one `started` record and, when observed, one `completed`
record linked by `eviction.incident`. Successful guard signal escalations also
produce `escalated` records. Count starts to count interventions, rather than
counting every line or every descendant PID.

| `eviction.reason` | Intervention |
| --- | --- |
| `critical_pressure` | Largest eligible tree selected during critical host pressure |
| `memory_limit` | Tree exceeded its configured budget for the sustained interval |
| `idle_pressure` | Idle runtime evicted early during host pressure |
| `idle_timeout` | Ordinary age-based idle eviction; useful as a baseline |

Records include Unix-second `at` and `eviction.startedAt` timestamps, task and
runtime UUIDs, initial tree bytes and budget when available, initial process
count, up to 64 initial PIDs, and monotonic elapsed milliseconds. They contain
no prompts, command lines, or paths. `schemaVersion` is currently `1`.

## Containment policy

On macOS and Linux, the guard samples every five seconds and selects at most
one tree at a time. During elevated or critical pressure it reclaims safely
resumable idle runtimes first, largest observed tree first. Shutdown is lazy:
the provider reopens on the next prompt, rather than reopening immediately.
Unregistered drivers can still be reclaimed while idle, with an unobserved
process outcome. Known trees get the guard's normal termination escalation if
provider shutdown fails to stop them.

The default 4096 MiB budget requires 30 seconds of continuous overage while
host pressure is elevated or critical. A healthy host or an unavailable signal
resets that interval. An explicit nonzero `agent_memory_limit_mb` remains a
hard budget with the same sustained interval, regardless of pressure.
Critical pressure can still interrupt the largest eligible active tree using
at least 256 MiB without waiting for overage. `agent_memory_limit_mb: 0`
disables budget and emergency containment; idle reclamation remains controlled
by `runtime_idle_timeout_secs` (`0` disables it).

No additional tree is selected until containment finishes and a fresh pressure
sample is available. Interrupted active work still requires explicit review;
it is never replayed automatically. Normal age-based idle cleanup is separate.

## Count interventions

Set `eviction_log` to the running daemon's log path. With `jq`, count starts
by UTC hour and reason:

```sh
eviction_log="${GODDARD_DATA_DIR:-temp}/memory-guard/evictions.jsonl"
jq -s '
  [.[] | select(.event == "started")]
  | group_by([(.at | strftime("%Y-%m-%dT%H:00Z")), .eviction.reason])
  | map({hour: (.[0].at | strftime("%Y-%m-%dT%H:00Z")),
         reason: .[0].eviction.reason, count: length})
' "$eviction_log"
```

To inspect completed interventions by reason and outcome:

```sh
jq -s '
  [.[] | select(.event == "completed")]
  | group_by([.eviction.reason, .outcome])
  | map({reason: .[0].eviction.reason, outcome: .[0].outcome, count: length})
' "$eviction_log"
```

## Interpret outcomes

For guard containment, `cancelled` means the captured tree disappeared without
a successful guard SIGTERM/SIGKILL, `terminated` means it disappeared after a
successful guard SIGTERM, and `force_killed` means it disappeared after a
successful guard SIGKILL. These describe the last successful guard escalation,
not a kernel-confirmed exit cause for each process. A tree can contain multiple
processes with different exit causes. Provider teardown may also send signals.
An `escalated` record alone does not establish that the tree exited.

Idle reclamation starts by delegating shutdown to the provider driver. The
pressure guard can then escalate known trees, producing `terminated` or
`force_killed` outcomes as above. Otherwise the outcome is
`exited_after_shutdown` when the captured identities disappear, `unobserved`
when no registered process tree can be captured or observation is unsupported,
or `observation_timed_out` after 60 seconds without establishing exit. Failed
samples do not establish exit. Passive observation never sends signals.
Process observation runs on macOS and Linux.

A start with no matching completion has an unknown outcome: the daemon may have
restarted, observation may still be pending, or a write may have failed.
Rotation can also leave a completion whose start is no longer retained.
OS-initiated OOM kills and provider-internal signal details are not covered.
