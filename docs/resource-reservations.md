# Reserve host resources for parallel tasks

Use `goddard-agent resource` inside a Goddard task to queue native builds,
virtual devices, and shared desktop input without retrying from the model.
The daemon authenticates the task; scripts do not supply an owner ID.

The first version supports macOS and Linux hosts. Every Goddard daemon running
under the same OS account shares `~/.goddard/resource-broker`, regardless of
project, worktree, or development data directory. Separate OS accounts do not
share this authority. Reservations apply on the **daemon host**, so run the
workload there. The broker does not schedule remote/SSH workloads from a local
CLI.

## Run a command under a reservation

The default policy allows one resident iOS simulator **or** Android emulator,
one expensive native build, and one interactive desktop input session at a time.
A specific device is also exclusive. An isolated macOS app process needs no
desktop reservation unless it manipulates shared focus or input.

For a native build:

```sh
goddard-agent resource run \
  --json '{"resources":{"native_builds":1},"purpose":"debug native build","wait_seconds":600}' \
  -- cargo build
```

The command starts after its entire resource set becomes available. It inherits
the working directory, environment, stdin, stdout, and stderr, and the wrapper
returns its exit code. Waiting messages go to stderr and the task's ordinary
activity rows. Requests queue in submission order; a blocked request may delay
later requests for otherwise free resources. The wait is bounded, defaults to
600 seconds, and accepts 0–86400 seconds. Zero allows an immediate grant only.

For an iOS workflow, replace the example UUID with a simulator UUID from
`xcrun simctl list devices`. Reserve the simulator and build capacity together:

```sh
goddard-agent resource run \
  --json '{"resources":{"exclusive":["ios:00000000-0000-0000-0000-000000000001"],"resident_devices":1,"native_builds":1},"purpose":"iOS smoke test"}' \
  -- ./scripts/ios-smoke-test.sh
```

Use `android:<AVD-name>` for an Android emulator, `device:<stable-id>` for a
physical device, and the same stable name in every project. iOS UUIDs are
normalized. `resident_devices` must equal the number of named `ios:` and
`android:` resources. For shared desktop interaction, request
`"desktop_input":1`. Other exclusive names are supported for shared tools.

Boot and shut down an **owned** virtual device within the reserved workflow.
A simulator left idle still consumes capacity. After the workload exits,
Goddard retains its device reservation until the device stops; completed build
and desktop-input capacity is freed separately. Goddard never shuts down,
reboots, or adopts an already-running user-owned device.

## Inspect, cancel, or reserve manually

```sh
goddard-agent resource status
goddard-agent resource cancel RESERVATION-UUID
goddard-agent resource release RESERVATION-UUID
```

Status returns JSON with `policy`, `reservations`, `external_devices`, and
`observation_errors`. Each reservation names its task, purpose, resources,
process IDs, timestamps, and `duration_seconds` (time held, or time queued).
`granted_at:null` means queued. A cancelled/released reservation that remains
listed is retaining capacity for a live workload or virtual device. Cancel
operates on your task's requests, including queued requests; it cannot cancel
another task's work. Release also signals the runner to stop an active workload.
Neither operation prematurely frees its resources.

For a multi-command shell workflow, manual acquisition prints a reservation ID:

```sh
goddard-agent resource acquire \
  --json '{"resources":{"desktop_input":1},"purpose":"interactive desktop check"}'
```

Copy the returned ID into `GODDARD_RESOURCE_RESERVATION` when invoking nested
`resource run` commands, then release it from the owning shell. Manual
acquisition follows that shell's lifetime; prefer `run` for crash-safe workload
registration and supervision. Arbitrary processes started under a manual claim
are not automatically registered.

Nested `resource run` calls inherit the outer reservation through
`GODDARD_RESOURCE_RESERVATION`. They may use a subset of its resources and do
not release the parent. Expansion fails immediately: declare the complete set
at the outermost command to avoid deadlocks. Nested tooling cannot release or
cancel its inherited claim through the CLI. A task already holding resources
must reuse its parent or release before requesting a different set.

Ctrl-C cancels a wait or supervised workload. Cancelling the task or closing its
provider runtime also cancels its recorded reservations. The live runner stops
its owned Unix process group; it never targets an external device process.

## Configure capacity

Create `~/.goddard/resource-broker/policy.json` on the daemon host:

```json
{"resident_devices":1,"native_builds":1,"desktop_input":1}
```

Each value is an unsigned integer; zero disables new acquisitions of that
capacity. Changes apply to the next broker transaction. Lowering a limit never
terminates existing work. An absent policy uses the conservative defaults;
invalid JSON fails closed and leaves the ledger intact.

## Recovery and enforcement boundaries

The ledger survives CLI and daemon restarts. OS locking and atomic replacement
prevent concurrent daemons from allocating the same capacity. Stale queued
requests disappear when their holder dies or their deadline passes. An active
reservation has no time-based lease expiry: a live process group or named
virtual device keeps capacity even after its owner dies. Process-ID reuse may
conservatively retain a claim longer; Goddard does not kill processes based on
recovered PIDs. Do not delete the ledger to bypass a live holder.

If a request stays blocked, inspect `resource status`. Stop your own workload
or shut down your own retained device, then inspect status again. Device
inventory failures block new resident-device grants rather than assume free
capacity. iOS discovery uses `simctl`; Android discovery recognizes local
emulator/QEMU processes and their AVD names. Unknown emulator names still count
as external capacity. AVD names containing spaces and remote Android sessions
are not reliably attributable in this version.

Scheduling is **cooperative**. Raw shell launches can bypass it; user-owned
native builds and desktop interaction are not automatically discovered. The
broker cannot track descendants that escape their registered process group.
Computer Use, repository watch/probe entry points, simulator boot commands, and
sandbox/remote launchers are not automatically wrapped yet. Agents must reserve
resources before using them. A long-lived watcher should reserve only the
capacity needed for its current phase rather than hold build capacity for its
entire lifetime.
