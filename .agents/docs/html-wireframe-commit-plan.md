# HTML wireframes and comment threads: commit plan

Add HTML/CSS wireframe previews with persistent, element-anchored comment
threads. Agents and users can start threads; users can open an agent's themed
dot, read the comment, and reply in a popover. This document plans the work;
it does not describe functionality that has shipped.

## Agreed behavior

- Agents author self-contained `.wireframe.html` files using ordinary HTML/CSS,
  a supplied base stylesheet, and Goddard theme variables.
- Agents attach comments to specific elements. Users can also start threads
  through an explicit comment mode and an accessible element picker.
- Each anchored, open thread has a clickable dot colored with `Theme.info`,
  the existing informational blue used for the sidebar's unread marker.
  The color follows the theme; do not substitute `Theme.accent`.
- Clicking a dot opens a popover with author identity, the comment, replies,
  and a reply input. Saving a reply preserves the thread.
- **Annotation 1:** reading a thread never removes its dot. Presence, unread
  status, resolution, and delivery to an agent are independent states.
- A comments list exposes every thread, including resolved threads and threads
  whose anchor has disappeared. Resolve hides an open marker; reopen restores
  it when the anchor exists. Reading and sending feedback do neither.
- Saving feedback does not automatically start an agent turn. An explicit
  “Add to message” action stages feedback in the owning task's composer.
  The normal Send action delivers it with enough context for the agent to act.

## Architecture and compatibility

Use a dedicated restricted web preview with a trusted annotation controller.
Render HTML/CSS in the browser engine; do not translate a subset into the
existing JSON tree. Keep annotation controls and stored discussion outside
the agent-authored document. Reuse browser hosting mechanics where practical,
but do not reuse the unrestricted browser's navigation, downloads, profile,
or permissions.

Current integration points verified while preparing this plan:

| Area | Existing boundary | Planned change |
| --- | --- | --- |
| Format | `crates/waku-protocol/src/wireframe.rs` | Add HTML document metadata and validation alongside the v1 JSON model. |
| CLI | `crates/waku-agent/src/main.rs` and its generated help contracts | Accept HTML preview paths and expose scoped thread operations. |
| Native preview | `src/app/wireframe.rs` | Preserve JSON previews; add a separate HTML preview surface. |
| File preview | `src/app/right_panel.rs` | Recognize HTML wireframes and retain source/preview switching. |
| Plan companion | `src/app/plan_wireframe.rs` | Discover HTML companions while retaining revision-bound background reads. |
| Browser hosting | `src/browser.rs` | Reuse suitable lifecycle and focus mechanics without inheriting browsing authority. |
| Existing annotations | `src/app/annotations.rs` | Reuse composer/prompt presentation conventions; do not drain durable threads on send. |
| Persistence | `crates/waku-store/src/` | Add authoritative thread persistence through the daemon. |

Preserve existing `.wireframe.json` documents and their native renderer during
this rollout. New authoring guidance prefers HTML on supported platforms.
For a plan, prefer `<stem>.wireframe.html` when present; otherwise use
`<stem>.wireframe.json`. Invalid HTML is a visible error, not a silent fallback
to a different design. Show which companion was selected when both exist.

The current browser host supports macOS and Windows. Linux has no embedding
path. Initial HTML preview support therefore targets macOS and Windows;
Linux retains JSON preview and shows an explicit HTML-preview limitation with
source access. Do not claim platform parity or remove JSON until a Linux
rendering strategy has been implemented and verified. Unsupported platforms
must not silently open a less restricted external browser.

## Document and discussion contracts

Use a small metadata convention rather than another layout schema:

```html
<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8">
    <meta name="goddard-wireframe-version" content="1">
    <meta name="goddard-wireframe-id" content="task-review">
    <title>Task review proposal</title>
    <style>.sidebar { width: 240px; background: var(--wf-surface); }</style>
  </head>
  <body>
    <section data-wireframe-screen="tasks" aria-label="Tasks populated">
      <aside class="sidebar" data-wireframe-id="task-sidebar">Tasks</aside>
    </section>
  </body>
</html>
```

Document IDs are stable within a project/workspace scope. Screen IDs are
unique within a document; anchor IDs are unique within a screen. Agents must
preserve IDs for surviving concepts across revisions. A copy intended as a
separate proposal gets a new document ID. Reject ambiguous duplicate document
identity when separate files are opened in the same scope; moving a file can
retain identity. Do not identify documents by absolute client-side paths.

| Entity | Essential fields and invariants |
| --- | --- |
| Document | Scope, stable document ID, current relative path, content revision. |
| Anchor | Document ID, screen ID, element ID; never a CSS selector or pixel position. |
| Thread | Stable thread ID, anchor, creator identity, creation revision, quoted element label, open/resolved state, ordered messages. |
| Message | Stable message ID, thread ID, authenticated author identity, plain text body, timestamp, mutation/idempotency key. |
| Read state | Per reader and thread, last seen message; independent of resolution. |
| Delivery | Submitted message IDs and destination task/message receipt; independent of read state. |

The daemon owns durable comments; files own the design. Neither regeneration
nor preview refresh overwrites discussion. Author identity comes from the
authenticated caller, not HTML or a caller-supplied role. Agent CLI mutations
are restricted to the current task's authorized workspace and documents.
Replies append; concurrent replies cannot overwrite each other. Retried
mutations return the original result rather than duplicating messages.

Anchor availability is derived from the current document, not stored as a
destructive thread transition. Missing anchors retain discussion and show
“Anchor removed” with the original element label. Restoring an ID restores its
marker. Never silently reattach a thread to a nearby element. Comment mode
selects the nearest explicitly anchored element; unanchored content can be
discussed at screen level instead of creating fragile generated selectors.

## Commit sequence

Each numbered item is an atomic implementation commit with focused validation.
Keep new UI behind the existing Wireframes experiment until the complete flow
works. Add executable `Test-Plan:` trailers to behavior-changing commits.

### 1. `feat(wireframes): define HTML document and anchor contracts`

Add document detection, metadata extraction, version checks, duplicate-ID
validation, and explicit screen dimensions/fit behavior. Supply a compact base
stylesheet with typography, surface, text, and informational theme variables.
Keep ordinary CSS layout available. Specify and enforce bounded file bytes,
DOM size/depth, screen count, and comment/message lengths; retain existing
workspace confinement, symlink checks, and regular-file requirements.

Validate fixtures for a multi-screen document, malformed metadata, duplicate
IDs, unknown versions, exceeded limits, and unchanged JSON compatibility.
Use an existing parser if suitable; justify any new parsing dependency.

### 2. `feat(wireframes): add restricted HTML preview hosting`

Build the dedicated macOS/Windows host and integrate it into file previews and
plan companion discovery. Load approved content from memory. Block document
scripts, inline handlers, network requests (including CSS imports, fonts, and
URLs), local-file access, navigation, popups, downloads, forms, and permissions.
Use a sandboxed content frame plus a trusted controller or equivalent native
isolation; prove the chosen approach on both web engines before depending on
it for comments. Do not weaken restrictions to make the bridge work.

Only the trusted controller may report selected anchor IDs and geometry.
Bind bridge messages to the current document revision and preview instance;
validate sizes and IDs and reject stale or forged events. No generic native
command bridge is exposed to document content. Keep host controls outside the
content's styling and stacking context.

Validate hostile-content fixtures, theme changes, resizing, scrolling,
screen switching, panel switching, focus restoration, host teardown, and stale
loads. Extend revision guards already used by plan companions. Unsupported
platforms show the stated limitation. A failed host keeps source accessible.

### 3. `feat(wireframes): persist anchored comment threads`

Add store migration, typed protocol operations, daemon authorization, and
change events for listing threads, starting threads, appending replies,
marking read, resolving, and reopening. Regenerate client types as required.
Keep persistence off the UI thread and scoped to the owning daemon/project;
remote workspaces use that daemon rather than local client files.

Test restart persistence, scope isolation, authenticated authorship, concurrent
appends, idempotent retries, migration, and preservation across design revisions
and missing anchors. Bound and paginate discussion reads rather than loading
every project's comments into a preview.

### 4. `feat(wireframes): let agents attach and inspect comments`

Extend the existing preview command to accept `.wireframe.html`. Add thread
subcommands for add/list/read/reply with stable machine-readable IDs and
idempotency keys. The following is the proposed CLI shape, not a command
available today:

```sh
goddard-agent wireframe comment add designs/tasks.wireframe.html \
  --screen tasks --anchor task-sidebar \
  --idempotency-key sidebar-rationale-v1 \
  --text 'Keep task navigation visible while reviewing a thread.'
```

Resolve document and anchor IDs through validated workspace content; reject
unknown anchors for new threads. Existing orphaned threads remain readable and
replyable by thread ID. Agents can inspect user replies through this interface
without reading client state. Do not require agent comments to be embedded in
HTML or scraped from the transcript.

Test CLI-to-daemon creation/list/reply, authorization failures, retries, and
accurate help/schema output. Preview success continues to mean validation and
request broadcast, not confirmation that a client displayed the document.

### 5. `feat(wireframes): add comment dots and reply popovers`

Show agent and user threads through the same UI with explicit author labels.
Use `Theme.info` for persistent dots, with a separate visible unread cue and
textual status in the thread/list. Group overlapping markers into an operable
thread chooser. Update marker geometry on scroll, resize, screen change, and
layout changes; ignore geometry from old revisions. Keep popovers within panel
bounds and ensure embedded content cannot cover them.

Add comment mode, the keyboard element picker, comments list, reply input,
resolve/reopen, and missing-anchor presentation. All controls have sufficient
hit areas, visible keyboard focus, accessible labels, and conventional
Enter/Space/Escape behavior. Close returns focus to its opener. Preserve reply
drafts across popover closure; show pending/error/retry state without losing
text or reporting an unsaved reply as saved.

Verify agent comment → dot → popover → saved user reply; user-created threads;
reading without dot removal; resolution/reopening; removed/restored anchors;
reload/restart; keyboard-only use; both themes; scaling and panel clipping.

### 6. `feat(wireframes): send comment feedback with task context`

Add explicit “Add to message” actions for a reply or thread. Stage a snapshot
containing document path/ID, revision, screen/anchor IDs, element label, author
labels, thread/message IDs, and relevant discussion. Reuse existing annotation
labels and composer conventions where appropriate. Keep discussion text
clearly quoted as user/agent content rather than system instructions.

The user chooses when to Send and can remove staged feedback. Persisted
threads survive composer draining and successful submission. Record delivery
only after the normal submission path confirms acceptance; retries and failed
sends preserve feedback without duplicating the same delivery. Route to the
owning task, including when the user has switched tasks or uses a remote
daemon. New replies can be sent separately after earlier replies were delivered.

Test successful/failed/retried submission, cross-task isolation, and agent
receipt of the exact anchor and conversation context. Confirm saving or reading
a reply alone never starts a turn, resolves a thread, or removes its marker.

### 7. `docs(wireframes): document HTML authoring and comment review`

Update the CLI's authoritative format guide and generated help/contracts with
a complete HTML example and selective agent-comment guidance. Document stable
IDs, platform support, supported resources, companion precedence, user review,
reply delivery, and JSON compatibility. Add the appropriate experimental
changelog fragment and run `bun ./scripts/changelog.ts check`. Refresh the
context graph after implementation and check its freshness.

Validate examples with the actual parser/CLI and check relative links. Review
the final diff for stale JSON-only assumptions and unintended changes to
ordinary browser behavior or transcript annotations.

## Completion checks and boundaries

Run focused `mbx check -p <changed-crate>` and contract tests for each affected
boundary rather than a workspace sweep. Reserve native build/device resources
when needed. Use the existing dev watcher, wait for its successful rebuild,
and relaunch through its `a` command; do not start a second watcher.

Exercise the complete provider interaction in the rebuilt app: agent creates
an HTML proposal and comment, user opens the marker and replies, user sends
feedback, and the agent receives the anchored thread context. Verify macOS and
Windows hosting independently; report any unavailable platform check. No
visual analysis or screenshots are authorized by this plan; use non-visual
interaction evidence where available and report any remaining UI verification
gap instead of claiming the appearance was checked.

Performance acceptance: no file reads, parsing, store queries, synchronous
bridge calls, or browser construction from render paths. Cache prepared data,
coalesce geometry updates, and keep marker work proportional to the active
screen's visible anchors. Verify overlay ordering and focus using the actual
restricted host, not only a standalone browser fixture.

The first release excludes arbitrary document JavaScript, external assets,
live multi-user presence, automatic agent turns on reply, automatic comment
resolution, and removal of the legacy JSON format. These can be considered
separately after the static preview and durable feedback loop work reliably.
