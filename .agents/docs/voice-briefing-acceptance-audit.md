# Voice briefing acceptance audit

Implementation reviewed: `15201ba2ae4b73c31c8782b7c7883a512172a679`.
Scope: frozen “Voice briefings on session arrival” acceptance table, plan unit 4.
This audit adds a candidate-exhaustion test; it makes no production changes.

## Findings requiring implementation follow-up

1. **Canceled eligibility can stall arrival permanently.**
   `cancel_eager_voice_briefings` removes eager gate entries without returning
   their candidates from `Checking` to `Unchecked`. Arrival's candidate scan
   returns `Wait`, although no live gate can answer. This also happens after
   autoplay is disabled during a gate and later re-enabled before arrival.
   Reproduce with eager and Jev enabled: complete a long reply away, disable
   eager before Jev answers, then arrive. Expected: arrival evaluates the
   candidate and briefs it if accepted. Actual code path: indefinite silence.

2. **Arrival does not take ownership of an in-flight eligibility check.**
   `resolve_briefing_arrival` returns on `CandidateScan::Wait` without promoting
   captured eager gate work to `Arrival`. Unlike summary/audio promotion in
   `ensure_briefing_claim`, eligibility work remains cancelable by eager-off.
   Expected: disabling speculation preserves the current visit's eligibility
   resolution. This requires fixing gate ownership as well as orphaned states.

3. **Older eager audio overwrites the latest prepared summary.**
   `finish_briefing_clip` assigns `state.summary` unconditionally and does not
   advance the session driver. Reproduce: turn 10 summary settles and its eager
   audio starts; turn 12 qualifies and its summary settles; turn 10 audio
   settles last. Expected: keep turn 12 text and synthesize it on arrival.
   Actual: text becomes turn 10; arrival runs the full turn 12 pipeline again.
   This violates latest-text retention and prepared-text reuse, though the
   fixed arrival claim still prevents playing turn 10 as turn 12.

## Acceptance table

“Code supports” means source review, not a provider-interaction pass. Function
names below are in `src/app/voice_briefing.rs` unless a file is named.

| Scenario | Evidence and result |
| --- | --- |
| Completion while viewed | Code supports: `note_voice_briefing_completion` returns before recording or evaluating; checks active foreground window plus transcript navigation. |
| Leave after viewed completion and return | Code supports: departure clears candidate set; no transcript-history scan populates it on arrival. |
| Eligible away completion; eager on | Code supports: eligibility precedes `advance_eager_briefing`; audio starts with `play = false`. |
| Five eligible away completions | One audio allowance is spent before starting synthesis and reset only on departure. Latest summary retention fails under finding 3. |
| Later completion fails length/Jev | Code supports; existing `arrival_scans_newest_first_and_rejections_never_displace` tests scan order and rejection fallback. Eager target independently picks newest eligible. |
| Rapid completions during summary work | Code supports one `summary_job` and replaceable `summary_wanted`; `drive_briefing_summary` waits for the running generation. Finding 3 affects later audio settlement. |
| Arrival with latest audio ready | `ensure_briefing_claim` reuses voice-compatible clip; queue checks foreground, autoplay and sleep. Existing queue tests cover holding and gap eligibility, not actual audio. |
| Arrival with latest text ready but stale audio | Claim path starts audio directly for matching summary; finding 3 can discard that matching text beforehand. |
| Arrival with matching stage running | Summary/audio pipeline is promoted and reused. Gate waits without duplicate request, but ownership fails under finding 2. |
| Eager off; completions while away | Code supports: records unchecked candidates without calling gate; eager driver returns before provider work. |
| Eager off; eligible arrival | Code supports newest-first scan, length/Jev checks and full generation. Orphaned checks from a prior eager setting fail under finding 1. |
| Disable eager during speculation | Summary/audio entries removed; candidates/caches/allowance preserved. Gate cancellation fails under finding 1. |
| Disable eager during arrival generation | Arrival/manual summary/audio entries survive; captured gate resolution fails under finding 2. |
| New completion during arrival preparation | Viewed completion returns immediately; `state.claim` remains fixed. |
| Leave during arrival generation/playback | Navigation and window blur call `briefing_departed`; pending generations and claim removed, queue cleared, automatic audio stopped. Existing stale-result test proves replaced request identity is preserved. |
| Manual/boss audio active on arrival | `maybe_voice_brief` consumes visit when playback or boss clip queue is busy. No automatic follow-up is created for that visit. Requires runtime check for speaker behavior. |
| Disable autoplay | `cancel_automatic_voice_briefings` retains manual work, clears claims/queue and stops automatic audio. Finding 1 can affect a later fresh arrival. |
| Active goal completes several turns | `streaming.rs` calls completion recording outside the managed-goal sound/status exclusion. Completion recording has no goal, boss or employee exclusion. |
| Navigate while another session prepares | Departure retains pending jobs belonging to other sessions. Only departed session's state is reset. |
| Restart with old unread history | `app.rs` initializes runtime-only briefings empty. Arrival never enumerates persisted unread history. New test covers empty candidate set; restoration/provider behavior remains a runtime check. |

Candidates are captured at completion, before selection acknowledges unread
state in `sessions.rs`; arrival consumes this independent candidate set, so
removing `unseen_completions` does not erase it. This ordering is supported by
source review, not an integration test.

## Verification limits

Verification result: `mbx test -p waku --lib voice` passed all 92 tests
(0 failed, 0 ignored). `rustfmt --edition 2024 --check
src/app/voice_briefing.rs` and `git diff --check` passed. The test build emitted
one unrelated dead-code warning for `TerminalView::remote`.

Run `mbx test -p waku --lib voice` for the voice module suite, including queue
and eligibility scan tests. The added
`arrival_is_silent_without_any_unheard_eligible_candidate` covers empty,
rejected and already-heard candidate exhaustion.

No controlled provider or native speaker interaction was performed. Computer
Use is not granted to this employee, and visual analysis is not authorized.
The state-helper tests cannot establish real cancellation of already-started
provider calls, actual sound ownership, focus delivery, or sleep-window wake
behavior. Running provider tasks are detached; cancellation removes their
pending identities and rejects later results, rather than demonstrably
aborting the external requests.

The landed experimental changelog fragment exists at
`.changelog/exp-voice-briefing-arrival.md`; no duplicate release note is needed.
