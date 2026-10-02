# Resource broker implementation boundaries

The authenticated `AgentResources` command runs on independent daemon request
workers. `resource_broker::Broker` is the single host ledger authority: every
production caller derives its root from the account's Goddard home, not project
or development data paths. A stable lock file protects read/observe/modify/write
transactions; state is synced and atomically renamed, with the directory synced
before returning. Corrupt state/policy fails closed. No credentials enter the
ledger. Inventory probes are read-only and bounded to three seconds each.

Allocation is strict FIFO and all-or-nothing, with separate device exclusivity,
resident-device capacity, native-build capacity, and desktop-input capacity.
External devices consume resident capacity and cannot be claimed automatically.
Same-task expansion is refused, including nested requests. Subset borrowing
returns the existing ID; the outer supervisor owns release.

The CLI creates a private gate and a new process group. It records that group
before opening the gate for exec. A failed attach never starts user tooling.
The runner polls authentication/cancellation and stops only its own process
group. Lifecycle cleanup captures exact IDs so delayed old-runtime cleanup
cannot cancel a fresh runtime's reservations. A dead daemon/holder marks claims
cancelled; live groups and observed resident devices keep capacity. Once work
ends, a retained device no longer holds native-build or desktop-input slots.
Recovered PIDs are used only for conservative liveness, never for termination.

Task waiting/grant/settlement updates reuse `DriverEvent::Activity` with a
stable `resource-<id>` identifier. No render path reads the host ledger or runs
inventory probes. The client CLI polls instead of requiring model retries.

Tests in `resource_broker.rs` inject inventories and use sleeping process groups;
they never boot emulators. Agent integration tests exercise the real binary
against a simulated WebSocket daemon, including its gated exec and nested
shell commands. They verify shell behavior separately from scheduler behavior.

Remaining adapters: the Computer Use native bridge/provider REPL, sandbox and
remote launchers, and repository-specific mobile probe/watch entry points.
Computer Use has process-scoped isolation and cancellation registration, but
process isolation does not imply exclusive shared desktop input. A future
adapter should reserve `desktop_input` only around focus/input sessions rather
than all helper processes. Resident adapters should distinguish boot, build,
test, and retained-idle phases so watch probes do not monopolize build capacity.
Per-account authority is deliberate for this first version; a cross-OS-account
service would require authenticated IPC and installation/permissions policy.

See [the user guide](../../docs/resource-reservations.md) for executable commands,
configuration, and recovery behavior.
