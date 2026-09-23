# Changelog — Mobile

All notable changes to the Goddard mobile app. Fragments live under `.changelog/mobile/` with the same naming rules as [CHANGELOG.md](CHANGELOG.md) and fold here per release.

## [0.8.0]

### Fixed

- Newly created tasks open on mobile as soon as they are saved, while provider startup continues.
- Fix the mobile task-history button shrinking its icon while it shows the
  unseen-replies dot: the custom header item now renders the glyph at the
  same large symbol scale UIKit gives native bar buttons, with the dot
  overlaid on the icon's top-right corner

## [0.7.0]

### Features

- **Sidebar**
  - Swiping right for task history in the mobile app now slides the current screen away as a floating card already rounded to the device's display corners — like iOS's own back gesture — with a soft edge shadow marking the card instead of a dim over the transcript
  - The mobile task list now fades its empty state out and the fresh rows in instead of popping when chats load, and it refreshes the moment a drawer swipe starts so the reveal already shows the current rows
  - The mobile task-history button now carries the same informational-blue dot as the chat list when another task has replies you haven't seen, so new activity is visible without opening the drawer
  - The mobile chat list now marks tasks that received new replies since you last opened them with the same informational-blue dot the desktop sidebar shows, clearing when you open the chat
- **Git**
  - The mobile app gains a notifications inbox off the daemon editor — and a bell on the Daemons screen — with unread/all scopes, per-repo grouping, mark-read and mark-done per thread, and deep links to the resolved GitHub URL
  - The mobile task menu gains a Review queue listing the repo's `origin/qa` commits with status badges — approve or reject per commit, then promote the approved prefix onto the base branch behind a confirmation
  - The mobile task menu gains a Git surface covering the working-tree half of the desktop Git panel: branch and upstream status, pull (rebase) when behind and push when the remote allows, staged and unstaged file lists with stage, unstage, and confirmed discard, and a commit bar that can generate the message through the session's provider
- The mobile Files surface now previews images inline and opens text files for editing — Edit and Save write through the daemon and refresh the diff and Git panel
- Long-pressing the mobile app icon now offers New task plus your three most recent tasks as home-screen quick actions that deep-link straight into the app
- The mobile task drawer reaches desktop session-management parity: long-press a task to pin it into a "Pinned" section or archive it into "Archived" (archiving a running task confirms before stopping its runtime), drawer search unions title matches with a debounced full-text search across active and archived scopes, agent question cards answer with a typed Clarify or a Dismiss, and the task menu gains Compact context and a confirmed Roll back last turn
- The mobile daemon editor now shows the connected daemon's version and commit and gains a Pairing section — approve or deny pending pair requests and revoke paired devices, live as they change
- The mobile transcript gains per-turn conversation editing and find: rewind from an eligible user message or fork from a closing response — gated on the same eligibility rules as desktop — plus text search within the transcript

### Fixed

- **Sessions**
  - Fix the mobile chat list showing archived sessions: the drawer now filters them out like the desktop sidebar, in both the grouped list and search results
  - Fix the mobile chat list ignoring taps: Expo Go's bundled menu never delivered the tap event, so the row now also handles taps through a native-RN press target, and switching sessions while viewing one closes the drawer instead of leaving it open over the new chat
- **Transcript**
  - Fix a crash opening a task in the mobile app under Expo Go: the transcript's
    scroll-edge-effect marker isn't in Expo Go's bundled react-native-screens, so
    mounting it threw inside createNode — it now falls back to a plain wrapper
    there and keeps the native marker in development builds
  - Remove the mobile transcript's blurred backdrop under the navigation bar:
    it appeared the moment the transcript became scrollable and read as the
    transcript itself fading out
  - Give the mobile chat's floating header a permanent solid surface with a
    hairline instead of a blur: the transcript sits below it rather than
    scrolling underneath, so the bar stays legible and no overlay ever covers
    the text
  - Stop the mobile transcript's faded look the moment it becomes scrollable:
    iOS 26 blurs scrollable content that runs under the navigation bar — the
    transcript's scroll view now sits below the bar so the effect never
    engages in any build, with the scroll-edge marker and screen option still
    suppressing it wherever the native hooks exist
- Fix the mobile chat composer snapping into place instead of sliding with the keyboard: the session and new-task screens now drive bottom padding from Reanimated's keyboard observer on the UI thread, so the composer tracks the keyboard's real animation and follows the transcript's interactive swipe-to-dismiss
- Fix scroll jitter in the mobile task drawer: the session list now recycles rows through FlashList instead of mounting a SwiftUI menu host per row, rows memoize on primitive props so stream commits only repaint the session that changed, and the hidden drawer renders from a frozen snapshot instead of re-laying out on every transcript commit
- Fix the mobile app's daemon handshake: it marked itself `X-Goddard-Client` while the daemon's origin check looks for `x-waku-client`, so its React Native `Origin` header was rejected — the marker matches again
