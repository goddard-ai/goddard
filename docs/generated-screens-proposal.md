# Generated screens

## Proposal

Let a user describe an outcome and receive a small, interactive screen inside Goddard. The screen can present task information and perform explicitly granted Goddard actions. The selected provider agent builds the screen; Jev evaluates whether the brief and result serve the stated goal.

Use HTML, CSS, and JavaScript in a dedicated system webview for the generated screen. Use JSON for the brief, screen metadata, capability requests, and stream events. A general JSON component tree would limit the layouts and interactions the agent can create and would require Goddard to maintain another UI renderer.

## UX vision

1. Run **Describe screen…** from the command palette or press **⌘⇧S**. The command works from a task and carries that task as context.
2. Describe **the outcome you're going for** in plain language, such as “Show me the open decisions in this task and let me mark each one resolved.” The composer shows the selected model, reasoning setting, and environment before submission.
3. Goddard opens a new screen tab immediately. It shows the goal and generation progress while the agent streams the screen into place. The user can cancel generation or return to the task without losing the draft.
4. When ready, the screen is usable in that tab. Its title and a short description make its purpose clear. Actions that need Goddard access use `globalThis.goddard`; the host shows any required capability grant before enabling those actions.
5. The user can ask for a revision in the same tab. Each completed revision becomes a restorable version, so an unsuccessful edit does not destroy the last usable screen. Saved screens appear with their task and reopen at the last completed version.

The first release should focus on screens tied to one task. A global screen would need a different context and grant model.

## Generation contract

The goal first becomes a compact JSON brief: the desired outcome, intended user actions, relevant task context, and any data the screen needs. The selected provider agent receives that brief with the model, reasoning setting, and environment shown at submission. It produces a screen package and streams versioned updates to the host. Goddard validates each update and applies it without reloading the page on every token. A complete revision is committed only after its package is valid; generation errors leave the previous revision available.

Jev's current interface answers structured evaluation questions rather than generating arbitrary documents ([evaluation API](../crates/waku-core/src/eval.rs)). The proposal uses it to assess or refine the brief and, if useful, judge whether a completed screen meets the stated outcome. The selected provider agent authors the JSON brief and screen package. Making Jev author either artifact would require a new Jev generation interface and a separate product decision.

The screen package contains HTML, CSS, JavaScript, and local assets. A small manifest records the screen ID, task ID, revision, title, generator settings, and requested Goddard capabilities. The agent streams typed primitives such as `set_document`, `set_styles`, `add_asset`, and `complete_revision`. Each event identifies its revision and carries a complete, bounded change. The host does not interpret partial HTML as a live document.

## Runtime and Goddard access

Give generated screens a dedicated webview host, separate from ordinary browsing. Goddard already embeds a webview on macOS and Windows ([browser host](../src/browser.rs)); its current Linux host is a stub, so Linux support needs its own implementation before this feature is available there.

Expose a versioned, asynchronous `globalThis.goddard` controller. Its first surface should be small: read the current task's permitted data, subscribe to relevant changes, and request a defined task action. Calls return structured results or errors. The controller is bound to the screen and its task, and the host checks the grant on every call. Generated code receives no ambient daemon token, filesystem path, or unrestricted RPC endpoint.

Run each package in an isolated origin with a restrictive content policy. Block arbitrary navigation and remote resource loads by default; handle links through Goddard. A screen may request extra capabilities. An existing grant carries across revisions only while its scope stays the same; a new capability needs a new grant. Revoking a grant makes subsequent calls fail clearly. The package and controller should also have size and event-rate limits so a faulty screen cannot overwhelm the app.

Use normal HTML semantics for controls, keyboard focus, and screen-reader labels. The host owns global shortcuts, tab navigation, theme and reduced-motion values, and the boundary between native and webview focus. The generated screen should inherit those values through a small set of CSS variables and controller events.

## Decisions to settle before implementation

| Decision | Proposed default | Why it matters |
| --- | --- | --- |
| Jev's role | Evaluate and refine the brief; provider agent generates the screen | Matches Jev's current structured-evaluation interface. |
| Screen persistence | Save successful revisions with the originating task | Makes screens reusable and revisions recoverable. |
| Initial Goddard API | Task-scoped reads and a small set of named actions | Makes grants understandable and enforceable. |
| Network access | Off by default; explicit grant for a specific need | Keeps generated code from silently sending task data away. |
| Platform scope | macOS and Windows first | The current Linux webview host is a stub. |

The first implementation decision should be the exact task actions exposed through `globalThis.goddard`. That API determines which screens can be useful, what users must grant, and what the generated agent can safely promise.
