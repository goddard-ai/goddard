# Global Session Launch Overlay Plan

## Goal

Let a user summon the session launch flow from anywhere on their desktop with a global keyboard shortcut, even when Goddard is not focused. The shortcut should show the existing session launch form centered in a full-screen transparent overlay window. Clicking outside the form hides the overlay without discarding the draft.

## Product Scope

This plan extends the current in-app `Launch session` dialog rather than introducing a separate launch experience. The overlay should preserve the same launch fields, project defaults, selector shortcuts, validation, submit behavior, and post-submit session-opening behavior as the existing dialog unless a plan item explicitly says otherwise.

## Plan Items

### 1. Define Global Shortcut Availability And Trust Model

- **User outcome:** Users can configure one native/global shortcut that opens the session launch overlay while Goddard is running, regardless of which app currently has focus.
- **Current app context:** The existing `navigation.openNewSessionDialog` shortcut is webview-scoped and only works while the Goddard window has focus. The shortcut settings page already exposes editable command bindings, but those bindings are not native/global registrations.
- **Product ambiguity:** Resolved.
- **Product detail:**
  - The global session launch shortcut is opt-in. Goddard must not register a system-wide shortcut until the user explicitly enables or configures it.
  - The global shortcut cannot be enabled until at least one project has been added, because the overlay launch flow always needs a project context.
  - The opt-in flow suggests `Command+Period` as the default global shortcut.
  - The shortcut settings UI should distinguish the in-app `Launch session` shortcut from the global shortcut so users understand which binding works only while Goddard has focus and which binding can activate from other apps.
  - If native shortcut registration fails because the binding is unavailable, settings should show an inline error and leave the global shortcut disabled until the user chooses a different binding.
  - The app should not silently substitute another global shortcut.
  - Disabling the global shortcut unregisters the native binding and leaves the in-app launch shortcut unaffected.
  - The opt-in state and chosen binding persist across restarts, but successful native registration is revalidated each time Goddard starts because OS-level ownership can change.
- **Acceptance criteria:**
  - The global shortcut is disabled until the user opts in.
  - If no project has been added, the global shortcut opt-in control is unavailable and explains that a project is required first.
  - `Command+Period` is the suggested global shortcut when the user enables the feature.
  - Shortcut conflicts show an inline settings error and do not register or enable a fallback binding.
  - The settings UI makes it clear which shortcut is global and which shortcuts only work while the app has focus.

### 2. Add A Dedicated Overlay Window For Session Launch

- **User outcome:** Triggering the global shortcut shows a full-screen transparent overlay over the current desktop, with the session launch form centered and ready for typing.
- **Current app context:** The existing `SessionLaunchDialog` is rendered inside the main app webview with Ark Dialog and cannot appear over other applications when the app is unfocused.
- **Product ambiguity:** Resolved.
- **Product detail:**
  - The overlay appears only on the active display rather than covering every connected display.
  - The overlay window is full-screen on that display, fully transparent outside the launch surface, and centered around the existing launch form.
  - The background behind the form is not dimmed or blurred; the user’s current desktop context remains visible.
  - Opening the overlay should not resize, maximize, or visually replace the main Goddard window.
  - The launch prompt receives initial focus so the user can immediately type after invoking the shortcut.
  - Pressing the global shortcut while the overlay is already visible hides the overlay, preserving the current draft.
- **Acceptance criteria:**
  - The overlay is a separate desktop window from the main Goddard window.
  - The overlay covers only the active display.
  - The overlay is fully transparent outside the launch surface and does not visually replace or maximize the main app window.
  - The launch form receives focus when the overlay opens.
  - Repeating the global shortcut toggles the visible overlay off without clearing the draft.
  - The overlay can be shown when the main Goddard window is hidden, minimized, or behind another app, as long as Goddard is running.

### 3. Specify Hide, Close, And Draft Preservation Behavior

- **User outcome:** Dismissing the overlay feels lightweight: an outside click, Escape, or cancel action hides the overlay and lets the user resume later without losing typed prompt text or selected launch options.
- **Current app context:** The existing in-app dialog resets its draft when opened from app context. The requested overlay behavior says outside click hides it rather than closing it.
- **Product ambiguity:** Resolved.
- **Product detail:**
  - Overlay drafts are preserved only in memory while Goddard is running. They are not persisted across app restart, quit, or update.
  - Hiding the overlay by outside click, Escape, close button, or repeat shortcut follows one consistent Hide behavior and keeps the current overlay draft available for the next overlay invocation in the same app run.
  - The close button is a dismiss/hide affordance, not a discard affordance.
  - Submitting from the overlay should feel immediate. The overlay does not wait for the launch request to succeed or fail before returning the form to a launch-ready state.
  - Submit starts an async bottom-right overlay toast in a loading state, then updates that toast to success or failure when the launch result arrives.
  - Submit is not a dismiss action: it keeps the overlay visible so users can launch multiple sessions in a row.
  - After submit, the submitted draft is cleared/reset and the launch form returns to a fresh launch-ready state.
  - Each submitted launch keeps enough submitted payload in its toast state to support recovery if it fails.
  - A failed launch toast offers `Retry` and `Edit` actions. `Retry` resubmits the same payload; `Edit` restores that submitted payload into the visible launch form for correction.
  - If the user has already started a new draft when they choose `Edit`, the app should avoid silently overwriting it; the product should either preserve the new draft separately or require an explicit replacement action in the implementation design.
  - The main Goddard window should not be brought forward after an overlay launch. The launched session may open in the main app in the background, but the user stays in the overlay flow.
  - The in-app dialog can continue using its current reset-on-open behavior; draft preservation is specific to the global overlay unless later product work unifies the surfaces.
- **Acceptance criteria:**
  - Outside click hides the overlay window without destroying the current draft.
  - Escape, the close button, and the repeat shortcut follow the same hide/preserve behavior.
  - Submit clears the submitted draft, keeps the overlay visible, and shows a bottom-right overlay toast that progresses from loading to success or failure.
  - Launch completion does not foreground the main Goddard window.
  - Failed launch toasts support retrying the same payload or editing it in the overlay without reopening the overlay.
  - Reopening the overlay restores any unsent draft from the previous overlay invocation.
  - Overlay drafts do not survive a Goddard restart.

### 4. Align Overlay Context Defaults With Main-App Launches

- **User outcome:** Opening the overlay chooses sensible launch defaults even when there is no active Goddard tab, without surprising users who were working inside Goddard moments earlier.
- **Current app context:** The in-app dialog resets from the current active project context when opened. A global shortcut may fire while another desktop app owns focus.
- **Product ambiguity:** Resolved.
- **Product detail:**
  - The overlay can only be activated after at least one project has been added, because the global shortcut cannot be enabled before then.
  - The overlay defaults to the last project selected in the overlay.
  - If the overlay has no prior selected project in the current app run, it falls back to the main app’s active project.
  - If there is no active project, it falls back to the first available project.
  - After submit/reset, the overlay should keep the current selected project as the repeated-launch default while clearing the submitted prompt and transient per-launch fields.
- **Acceptance criteria:**
  - When opened from outside Goddard, the selected project resolves in this order: last overlay-selected project, main app active project, first available project.
  - Launch reset keeps the selected project available for successive overlay launches.
  - No no-project overlay state is required because the overlay is unreachable until the user has added a project and opted into the global shortcut.

### 5. Preserve Main-App Command And Dialog Consistency

- **User outcome:** The in-app launch dialog and global overlay feel like the same product feature, not two divergent launch flows.
- **Current app context:** Launch form behavior is already shared in `SessionLaunchForm`, while `SessionLaunchDialog` owns dialog reset, selector command handlers, and submit behavior.
- **Product ambiguity:** Resolved by default unless later decisions require a split.
- **Product detail:**
  - Reuse the existing launch form and launch submission behavior for both surfaces.
  - Keep selector keyboard commands, slash command suggestions, adapter/model/location selectors, validation, and success toast behavior consistent across both surfaces.
  - Only diverge where native overlay behavior requires it, such as transparent-window positioning, global shortcut registration, or hide/preserve semantics.
- **Acceptance criteria:**
  - Product-visible differences between the in-app dialog and global overlay are intentional and documented in this plan.
  - Tests or manual QA cover both the existing in-app dialog path and the new global-overlay path.

## Implementation Notes To Validate Later

- Native/global shortcut registration belongs in the Bun host layer, with UI configuration routed through the existing shortcut keymap concepts where feasible.
- The overlay should communicate with the existing app/webview state through the Electrobun RPC boundary rather than importing host APIs into UI components.
- The current shared SDK/session creation path should remain the launch source of truth; this plan does not introduce SDK parity requirements because it adds UI-only desktop invocation behavior.
