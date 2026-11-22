# Devin submission diagnostics

Production and debug daemons record metadata-only JSONL under
`<data directory>/diagnostics/devin-timeline/`. These records explain where a
submission stalled without retaining message bodies, filesystem paths,
credentials, or raw provider errors.

Filter records by `task_id`, then group by `runtime_id` and `turn_id`.
`submission` supplies the message ID and says whether the runtime had already
served another submission. Startup records may have a null turn ID; their
runtime ID links them to the later submission. A new runtime with `resuming:
true` follows a saved provider cursor; `evicted` on a previous runtime records
idle or memory-pressure reclamation.

The timeline records initialization, resume/load and initial configuration
boundaries with elapsed milliseconds and outcomes, prompt dispatch, the first
session update (including whether it was replay), provider start confirmation,
provider stop classification, turn completion, controller/timeout cancellation,
shutdown and connection exit. Native provider error details remain in the task
transcript; the timeline records only that an error occurred.

One background writer owns twenty segments of at most 1,000,000 bytes each:
20 MB total file content. Full segments rotate by deleting the oldest segment.
Segments expire seven days after their first record, so newer records in an
expiring segment can be removed early. Cleanup runs on startup, writes, and
every minute while the daemon is idle. When the daemon is stopped, cleanup waits
until its next startup. No separate archive or temporary copies accumulate.

Producers use a 1,024-record queue without waiting for disk. Overflow and storage
failures discard diagnostics; `dropped_before` reports losses on a subsequent
record when possible. A crash can lose queued records. Logging never determines
whether a prompt succeeds, times out, or retries. Tests and headless drivers
without an explicitly attached timeline do not write this diagnostic store.
