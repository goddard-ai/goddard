# Agent memory containment

The daemon samples owned provider process trees every five seconds on macOS
and Linux. A tree includes compiler processes and a session's sandbox VM.
macOS accounting uses physical footprint, including compressed memory; Linux
uses resident memory. This is a watchdog, not a kernel-enforced memory quota.

`DaemonSettings.agent_memory_limit_mb` configures the budget in MiB. An absent
value uses 4096; `0` disables per-agent containment. The client settings mirror
preserves this daemon-owned field during unrelated settings updates. There is
no dedicated settings control yet. Builds and VMs can need a higher budget.

After 30 seconds continuously above the budget, the daemon records a durable
pause, holds queued work, and cancels the turn. After ten seconds it removes the
runtime and sends TERM to captured processes; a subsequent sample sends KILL
if they remain alive. Every signal rechecks the process's kernel birth stamp.
Sampling gaps reset the sustained-overage timer.

Critical host memory pressure holds new launches and employee admission. It
also cancels the largest registered tree of at least 256 MiB without waiting
for sustained overage, then begins termination on the next sample. One
emergency victim is contained at a time. Critical pressure means a macOS
memorystatus level below 15 or Linux memory PSI `some avg10` at least 50.
Disabling the per-agent budget retains the admission hold and disables
automatic victim selection.

An already-settled, unmanaged provider with a resume cursor and no queued work
can reopen once after its whole captured tree exits and critical pressure
clears. It receives no replayed prompt. Busy tasks and managed roles remain
paused for explicit review because interrupted tool outcomes may be unknown.
The pause and single-restart budget survive daemon replacement. An explicit
client start or human employee resume releases a pause after cleanup; repeated
breaches retain the automatic-restart limit.

Codex, Claude, Amp, Pi/Oh My Pi, and ACP providers register a process root at
launch. ACP's asynchronous guardian proves ownership using a unique argv
marker before its birth stamp is captured. Sandboxed launches register the
host VM process. Shared OpenCode/Muse/DeepSeek services, Copilot's SDK-owned
process, opt-in stable-runtime providers, and Windows are outside per-task
containment coverage. Directory-based diagnostic attribution never authorizes
termination of a process.

Containment adds an interruption notice to the task and appends numeric
diagnostics to `<data directory>/memory-guard/incidents.jsonl`, capped at
256 KiB. The same directory holds the durable pause/retry ledger. Ephemeral
process registrations belong to one daemon incarnation. Kernel process IDs
alone are never sufficient authority to signal a replacement process.

Focused verification:

```sh
mbx test -p waku-daemon --lib memory_guard
```

The fixtures cover sustained sampling, durable recovery fences, persistence
failure, reused PIDs, ACP guardian registration, and cancellation/termination
of a disposable process tree while a sibling stays alive. Provider and debug
app validation remain necessary after integrating the daemon change.
