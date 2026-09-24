# Jev — hosted evaluation model

Jev is TypeSafe's "System One" structured decision model. One call posts a
shared `state` (JSON) plus a map of typed `questions`; the response is
calibrated answers — choices, rubric scores, yes/no probabilities — never
generated text. Questions in one request evaluate independently; ordinary
code combines the answers. Jev decides, code applies — it is an
input-processing primitive, not a generator, and it exposes no embeddings
or hidden representations.

Vendor docs: [docs.typesafe.ai](https://docs.typesafe.ai/llms.txt) is the
index — primitives, confidence, API reference, and the pattern pages
(speculative fan-out, confidence-gated routing, composite scoring, intent
routing) match how this codebase already uses it. The jevify prompt
(github.com/ryana/jevify) is the reference methodology for hunting new
opportunities: find where we generate text only to parse a decision back,
serialize judgments that could be independent, gate coverage behind
sampling, or keep brittle rules because semantic judgment looked too
expensive. Separate vendor claims from measured results, and never hide a
complex reasoning task inside a vaguely worded question — the `state` must
carry the information the judgment actually needs.

## Plumbing

- Wire and settings shapes: `crates/waku-protocol/src/eval.rs` —
  `EvalQuestion` (`Noul` yes/no → probability; `Choice` named options →
  argmax + per-option `probabilities` + `confidence`; `Score` ordered
  rubric → score + `confidence`), `EvalAnswer`, `Evaluation`,
  `EvalSettings`, `EvalBackend`.
- Three backends answer the same question/answer contract and differ only
  in envelope and credentials: TypeSafe (`api.typesafe.ai/v1/systemone`,
  model alias `jev-latest`), Vercel AI Gateway (`typesafe-ai/jev`; `noul`
  is renamed to `boolean` on the wire and back), Cloudflare Workers AI
  (output wrapped in `result`).
- Client: `crates/waku-core/src/eval.rs` — `evaluate` /
  `evaluate_with_timeout` block on system curl and the network; callers
  must already be off any latency budget. Credentials travel in a 0600
  curl config file, never argv. `EVAL_TIMEOUT_SECS` (5s) is the default
  budget for latency-bound callers; long-context callers pass a larger
  `timeout_secs`. Backend error bodies are never recorded — they can echo
  the prompt.
- The daemon owns every call: `Command::Evaluate { state, questions,
  feature, timeout_secs }` → `ResponsePayload::Evaluation`. Each call
  appends an `EvalDecisionRecord` to `eval-decisions.jsonl` beside the
  daemon's `settings.json` — that log is the calibration dataset for every
  eval feature, so always set a distinct `feature` tag and add its label
  and description to `eval_feature_label` / `eval_feature_description`
  in settings.rs (unknown tags render raw).
- `DaemonSettings.eval` is per-daemon and BYOK; the Jev settings page
  edits the local daemon only, so remote sessions evaluate on their own
  daemon's backend. Check `daemon.settings().eval` and
  `EvalSettings::credential_missing()` before sending — an unusable
  backend means the feature simply does not fire.
- App side: snapshot inputs on the UI thread, run the request inside
  `cx.background_executor().spawn`, return results through a channel plus
  `signal_event_pump`, and apply them in a drain — the same seam
  `routing.rs`, `status_markers.rs`, and `action_predictions.rs` share.

## Existing call sites

| Feature tag | Where | What it judges |
| --- | --- | --- |
| `route` | `crates/waku-core/src/routing.rs` | First prompt → task `class` (Choice) + `needs_planning` (Noul), resolved through the user's class map |
| `route-effort` | `src/app/routing.rs` | Each turn → effort ladder pick (Choice), only while routing owns the session |
| `route-phase` | `src/app/phases.rs` | Settled turns on `phased` sessions → `still_planning`/`stuck` Nouls + `implementation_model` Choice over the user's approved models only |
| `route-class-suggest` | `src/app/settings.rs` | Jev page "suggest defaults" for the class map |
| `turn-status` | `src/app/status_markers.rs` | Settled turn → ending Choice + flag Nouls (unverified, drifted, needs-review, thrash, assumed) |
| `title-quality` | `src/app/title_quality.rs` | Settled turn → Noul judging whether its automatic title needs a rewrite |
| `next-action` | `src/app/action_predictions.rs` | Settled turn → `taskType` + `nextAction` over a feasibility-gated candidate set |
| `paste-classification` | `src/app/composer/paste_classification.rs` | Pasted composer text → content-category Choice for the block's label |
| `provider-switch` | `src/app/provider_switch.rs` | One Noul per transcript item/span: keep verbatim for the new provider? |
| `permission-review` | `crates/waku-core/src/permission_review.rs` | Auto-mode permission requests → clear/caution Choice |
| `memory-triage`, `memory-rank` | `crates/waku-core/src/memory.rs` | Transcript segments worth feeding the distiller |

## Design conventions

- Everything degrades. Unconfigured backend, missing credential, failed
  call, missing or low-confidence answer → the deterministic default
  stands (routing falls back to `last_used` with a `+`-joined reason
  chain; markers just don't render). The exception is permission review,
  which fails closed: anything but an explicit `clear` escalates to the
  user.
- Gate the spend on whether the answer can act. Status markers and action
  predictions evaluate only the selected session and queue the rest;
  phase evals run only for sessions whose intake route marked `phased`;
  cheap deterministic signals (`ActivityItem::phase_signal`, failure
  counts) decide whether a settle evaluation is needed at all — never one
  hosted call per tool event.
- Batch independent questions into one call. Status markers ask ~10
  questions per settle on one shared `state`; provider-switch compaction
  asks one Noul per item. Prefer parallel questions over serialized
  follow-ups.
- Thresholds are product decisions, not defaults. Use `confidence` and the
  full `probabilities` map: argmax-plus-margin for suggestions
  (≥ 0.5 and ≥ 0.15 ahead), a dead band for irreversible-ish transitions
  (phase commits below 0.4, stays above 0.6), asymmetric bars where false
  positives and misses cost differently (`failed` renders at 0.45,
  `complete` needs 0.65). Choice options compete for probability mass —
  use Choice for mutually exclusive outcomes and Noul for qualities that
  can co-occur.
- Put judgment-readable summaries in `state`, bounded per field.
  `turn_eval_state` (prompt, priorPrompts, response, toolSequence,
  toolErrors, filesChanged, contextUsage) is the shared per-turn payload —
  reuse it. The caps exist for latency and focus, not cost: ~30k chars for
  a turn, 256 KiB / ~90k chars for bulk extraction batches. Keep raw tool
  `arguments` out of state — display subjects only; arguments can carry
  file contents or secrets into a third-party request.
- Calibrate before shipping UI. The established pattern is shadow mode:
  log predictions and resolve them against a local journal
  (`~/.goddard/actions.jsonl` vs `~/.goddard/action-predictions.jsonl`)
  before any suggestion renders. Treat returned probabilities as signals
  whose calibration needs testing on our workload, and let the decision
  log prove a feature's economics.
