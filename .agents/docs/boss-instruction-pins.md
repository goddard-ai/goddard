# Boss instruction pin lifecycle

Pins live in the daemon's Boss state, independently of provider sessions.
`instruction_pins.rs` in protocol, Boss, daemon, agent CLI, and app owns their
wire contract, atomic lifecycle, source validation, commands, and editor.

States are candidate → active → retired or superseded. Candidates hold saved
human-message provenance but are nonbinding. Activation requires instruction,
scope, and a retirement condition. Edits and releases check a per-pin revision;
retired records cannot be reactivated. The user can release a pin explicitly.
Boss retirement records evidence; supersession links another active pin.

Only main Boss and human operations can access pins. The daemon verifies new
Boss sources against retained visible human messages, excluding employee
reports, notices, hidden injections, and assistant content. All employee and
planning State replies redact pins, including deputy replies with Boss grants.
Saved excerpts retain provenance if Clear later deletes the original chat.

Candidate detection runs at visible human prompt submission. Conservative
wording checks avoid quoted and fenced material. A configured Jev backend may
propose additional candidates at Noul ≥ 0.85, feature `boss-instruction-pins`;
it generates no instruction text. Provider submission events cover explicit
human steering. Event detection defers its operation lock until the lifecycle
lease is released. Deduplication includes retired sources so replay cannot
resurrect an obligation. There are at most 32 pending candidates.

All applicable active pins (8,000 characters total, refusing overflow) and at
most four candidate previews ride every main Boss outbound prompt. A current
work brief (6,000 characters) comes from daemon outcomes and task snapshots;
pending decisions precede project inventory. The optional full digest is
bounded at 12,000 characters. Rotation carries only two 1,000-character recent
messages and a predecessor pointer; history stays on demand.
Each pin snapshot explicitly supersedes earlier snapshots. After the final
release, an empty-active snapshot still rides the prompt so old provider
context cannot keep the released obligation binding.

Every atomic Boss update reconciles explicit `outcomeCompleted` links. Only
Completed plus a timestamp and nonempty evidence retires a linked pin, saving
that evidence. A manual condition has no inferred automatic trigger. The app
clears the link on condition edits; CLI clients must only link an outcome when
completion expresses the entire release condition. Other conditions are
reviewed by Boss against new user messages and work events, without a separate
background model wakeup or transcript scan.

Regression coverage lives in Boss instruction pin tests, Boss context tests,
and daemon instruction pin tests. Use temporary data; do not modify QA history
to validate this feature. Calibration of candidate detection and optimum brief
size still needs real workload evidence.
