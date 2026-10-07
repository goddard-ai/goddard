# Employee transcript retention

Live employees and expired employees through 48 hours after `expired_at`
keep persisted transcript activity payloads. Both active and retired roster
records participate. Missing expiry timestamps are protected conservatively.
The archive detail sweep and automatic full-session purge skip protected
employees, even if their archive timestamp predates expiry. After that window,
the existing archive policies apply: detail pruning after seven days archived,
and full deletion after 30 days archived. Expiry itself does not strip detail.
There is no persisted transcript size cap that bypasses the expiry exemption.
Explicit task deletion remains a separate user action.

Capture and retrieval still have hard size limits. Driver activity text is
capped at 8,000 characters at capture. Agent transcript reads cap message text
at 8,192 characters, activity headers at 4,096 characters and tool output
separately at 8,192 characters; long arguments cannot crowd out output.
Whole-transcript listings drop oldest entries above 128 KiB and report
`truncated`. A per-turn read bypasses that listing cap, retaining per-field
caps. These limits are independent of age and cannot recover output already
truncated by the provider or earlier versions.

The focused daemon test `employee_tool_outputs_survive_48_hours_after_expiry_and_retirement`
checks retired employee evidence from reopened storage at next-day and exact
48-hour boundaries, readback with large arguments, the archive purge exemption,
and eligibility after the retention window ends.
