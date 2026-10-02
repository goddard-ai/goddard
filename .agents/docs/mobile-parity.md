# Mobile parity plan — Goddard vs T3 Code

Feature-parity checklist for `apps/mobile` against T3 Code's mobile app
(`apps/mobile` at github.com/pingdotgg/t3code). Ordered by implementation
effort; tiers 0–2 are pure client work on the existing daemon protocol,
tier 3 needs new native modules, tier 4 needs new infrastructure or
protocol changes.

## Baseline — already shipped

- [x] Multi-daemon profiles: manual add, QR pairing, Bonjour/Zeroconf
      discovery, SecureStore token storage, `goddard://connect` deep link
- [x] Task drawer with title search, rename, delete
- [x] New-task composer: project picker, provider/model pickers with
      traits, workspace mode + branch pickers, attachments, slash
      commands, @-file context, permission/access modes
- [x] Session transcript: GFM markdown, turn folds, activity sheets,
      changed-files card, diff view
- [x] Permission approvals and agent question prompts
- [x] Queue/steer while a turn runs; daemon-synced composer drafts
- [x] Surfaces: libghostty terminal, workspace file tree + text preview,
      review diffs (uncommitted/staged/committed/branch/last-turn)
- [x] `/land` workspace landing
- [x] System light/dark theme, haptics, reduced-motion

## Tier 0 — trivial (hours each, protocol already supports it)

- [x] **Selectable transcript text** — markdown nodes were already
      selectable; added `selectable` to system messages and changed-file
      paths in `transcript-rows.tsx` to complete coverage.
- [x] **Pin sessions** — `setSessionPinned` in runtime-context; pinned
      sessions group under a "Pinned" drawer section; long-press menu
      action on both platforms.
- [x] **Archive sessions + archived view** — `setSessionArchived`
      (closes the runtime first, matching desktop semantics); archived
      sessions group under an "Archived" drawer section; confirm dialog
      when archiving a running task.
- [ ] **Dormant/snooze sessions** — `dormant_at` / `dormant_exempt_until`
      fields exist. NOT a simple flag: desktop cancels work, clears pins,
      and schedules worktree cleanup. Needs daemon-side semantics wired
      properly, not a mobile timestamp write.
- [ ] **Swipe actions on task rows** — deferred: a horizontal swipe
      conflicts with the drawer's pan-to-close gesture. Pin/archive are
      on the long-press menu instead; revisit if the drawer gesture
      changes.
- [x] **Message-content search** — drawer search now unions local
      title/project matching with debounced `searchSessionMessages`
      queries (active + archived scopes); the open transcript also has
      its own find bar (see Tier 1).
- [~] **Daemon version display** — `WakuClient` stores
      `daemonVersion`/`daemonCommit` from `hello`; shown on the daemon
      editor screen. A true update banner still needs a "latest version"
      source to compare against — nothing ships one today.
- [x] **Question-card clarify/dismiss** — `UserInputPanel` renders
      Clarify/Dismiss when `supportsUserInputActions` is set, matching
      desktop semantics (Clarify requires typed text).
- [x] **Session actions** — Compact context and Roll-back-last-turn
      (with confirm) added to the task menu; per-turn Rewind/Fork live on
      the transcript rows (see Tier 1).
- [x] **Home-screen quick actions** — `expo-quick-actions` configured;
      "New task" + 3 recent sessions with deep links. Requires a native
      rebuild (dev client) to take effect — not verifiable in Expo Go.
- [x] **Pairing approvals on phone** — daemon editor shows pending pair
      requests (Approve/Deny via `respondPairRequest`) and paired
      devices (Revoke via `revokePairedClient`), live-updated by
      `pairingChanged`.

## Tier 1 — small-medium (days, still protocol-ready)

- [x] **Per-turn rewind / fork** — a rewind button sits under eligible
      user bubbles and a fork button on closing-response footers,
      matching desktop's per-turn affordances. Eligibility mirrors
      `validate_message_rewind`/`validate_response_fork`: settled,
      unarchived sessions on conversation-editing providers; rewind also
      requires a checkpoint ref from the workspace `sessionTurnRefs` op
      and a provider cursor when provider turns roll back. Rewind
      confirms before running; fork pushes the new task screen; daemon
      cleanup/checkpoint warnings surface as alerts.
- [x] **In-session transcript search** — "Find in transcript" in the
      task menu opens a floating find bar (client-side scan of message
      text) with match count and previous/next navigation. Navigation
      goes through `TranscriptListHandle.revealRow`, which mounts
      windowed history, opens folds, and scrolls the variable-height
      inverted list by measured row layout.
- [x] **Git surface sheet** — fourth surface beside Terminal / Files /
      Review. Working-tree core shipped: branch + upstream subtitle,
      pull (rebase) when behind, push when `can_push`, staged/unstaged
      file lists with stage/unstage/discard (confirmed, untracked
      warns it deletes), and the commit bar — blank message generates
      via `generateCommitMessage` with the session's provider
      invocation, nothing-staged confirms a worktree sweep. Still
      desktop-only: branch switching (`checkoutBranch`), worktree ops,
      commit log (`listCommits`), remote fetch/rebase.
- [ ] **Pull-request surface** — `listPullRequests`, `getPullRequest`,
      `fetchPullRequestHead`; check and review-comment types
      (`PullRequestCheck`, `PullRequestReviewComment`) are generated.
- [x] **Notifications inbox** — `listNotifications`,
      `markNotificationRead`, `markNotificationDone`,
      `markAllNotificationsRead`, `markRepoNotificationsRead` (GitHub
      inbox, not push). `/notifications` screen off the daemon editor:
      unread/all scopes, per-repo groups, mark-read/done per thread,
      opens the resolved GitHub URL.
- [x] **Review queue** — `reviewQueue`, `reviewApprove`, `reviewReject`,
      `reviewPromote`. Fifth task surface (task menu → Review queue):
      `qa`-branch entries with status badges, per-commit approve/reject,
      confirmed promote of the approved prefix to the base branch.
- [ ] **Usage screen** — `loadUsageHistory` + `fetchPlanUsage`;
      `react-native-svg` is already a dependency for the chart.
- [x] **File preview upgrades** — image extensions preview via
      `readBinaryFile` (base64 → `Image`); text files gain an Edit/Save
      mode via `writeTextFile`.
- [ ] **Video/audio attachments + preview** — widen
      `expo-image-picker` media types and add `expo-video` playback;
      upload path already exists.
- [ ] **Settings screen** — no settings route exists today. Daemon side
      is ready: `getSettings` / `updateSettings` / `setDaemonExposure`.
      Client-side sections: appearance, font size, storage.
- [ ] **Per-thread settings sheet** — `applyOptions`
      (`WireSessionOptions`) switches model/mode/effort on a live
      session; extend the existing model sheet.
- [ ] **Offline outbox** — client-side queue in AsyncStorage drained on
      `connected`; `prompt` accepts messages any time.
- [ ] **Skills UI** — `loadSkills`, `setSkillsEnabled`, `trashSkills`.
- [ ] **Custom slash-commands editor** — `listCustomCommands`,
      `upsertCustomCommand`, `removeCustomCommand`.
- [ ] **Integrations auth UI** — `listIntegrations`,
      `connectIntegration`, `startIntegrationAuth`,
      `disconnectIntegration`.
- [x] **Queued-message management** — queued sends render above the
      composer with a remove action; agent-parked prompts cancel via
      `cancelQueuedPrompt`, user-queued rows via the persisted session.
- [ ] **Issues surface** — `listIssues`, `getIssue`, `createIssue`,
      `listIssueTemplates` (parity depends on how much desktop exposes).

## Tier 2 — moderate (native module or layout work, no new infra)

- [ ] **Share extension / incoming share** — iOS share-extension target +
      Android share intent (e.g. `expo-share-intent`), persisted share
      inbox feeding the composer. Protocol has `IncomingShareInfo`.
- [ ] **Voice dictation** — `expo-audio` recording + Speech
      transcription; button slot in the composer. T3 ships a native
      transcription module — Expo speech-recognition is enough to start.
- [ ] **iPad split-view layout** — persistent sidebar + detail pane with
      adaptive push/replace navigation; `supportsTablet` is already on,
      current UI is phone-style drawer only.
- [ ] **Hardware keyboard shortcuts** — iPad keybindings for navigation,
      send, new task; key-event handling in the session view.
- [ ] **Local notifications** — `expo-notifications` local alerts for
      "agent finished / needs input" while the socket lives
      (foreground + short background grace). Partial fix only.
- [ ] **Material You (Android)** — dynamic wallpaper palette;
      Android-only, independent of the iOS theme system.

## Tier 3 — hard / blocked on infra or protocol

- [ ] **Push notifications** — alerts when the app is closed. Requires a
      relay with a push path (T3 Connect equivalent); no cheap version.
- [ ] **iOS Live Activities / Android ongoing + Live Updates** — ambient
      agent progress; depends on the push path for real updates.
- [ ] **Home-screen widgets** — `expo-widgets` Agent Activity + usage
      widgets; only refresh while the app runs without push.
- [ ] **Multi-daemon aggregated task list** — `daemon-context` is built
      around one active link; aggregation needs a multi-client runtime
      and a unified home list.
- [ ] **Terminal scrollback replay** — `openTerminal` takes only `cwd`;
      needs daemon-side scrollback persistence.
- [ ] **Question attachments** — `UserInputAnswer` is
      `{questionId, answers: string[]}`; needs a protocol field.
- [ ] **Device/simulator preview** — watch + control agent-driven
      simulators; needs the desktop-side device hub first.
- [ ] **Cloud relay / account sign-in** — remote access without
      LAN/tailnet, and the enabling layer for push.
- [ ] **AI thread-title regeneration** — `auto_title` exists; no explicit
      regenerate command (possibly `evaluate`-driven); needs a daemon
      hook or a defined route.
- [ ] **Unified command palette** — after hardware-keyboard support,
      a palette sheet aggregating session/task/composer commands.

## Dependencies

- Push notifications → cloud relay.
- Live Activities, always-fresh widgets → push notifications.
- Device preview → desktop device hub.
- Question attachments → protocol change (`UserInputAnswer` + daemon).
- Terminal scrollback → daemon scrollback persistence.
- Multi-daemon home → multi-client runtime refactor.

## Out of scope (T3 desktop-only features)

SnapShot window capture, browser profiles/import, desktop keybinding
editor, panel animations, environment themes, background service
management, T3 Connect host setup (desktop owns hosting).
