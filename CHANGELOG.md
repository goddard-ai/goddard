# Changelog

All notable changes to Goddard — desktop app and everything shared. Changes
only a mobile-app user would notice live in
[CHANGELOG.mobile.md](CHANGELOG.mobile.md) instead. This file is the
**source of truth for the release
notes shown in the in-app updater**: [`scripts/release.ts`](scripts/release.ts)
extracts the section whose heading matches the version being released
(`MARKETING_VERSION`) and publishes it next to the update, so Sparkle shows it in
the update prompt.

Format follows [Keep a Changelog](https://keepachangelog.com). Don't edit this
file directly for pending changes — add a `.changelog/<prefix>-<slug>.md`
fragment per change (one bullet per file) so parallel work never conflicts
here. The required prefix picks the release-notes section: `highlight-` for
headline features, `feat-` for other features, `exp-` for experimental
opt-ins (emitted under `### Experiments` with a bold `[Experimental]` marker;
experiments are never highlights), `fix-` for bugs that existed in a released
version. A non-highlight fragment may tag a topic group as a second segment —
`feat-<group>-<slug>.md` — and grouped bullets nest under a `- **Group**`
parent inside their `###` section; the group vocabulary and its emitted
order live in [`scripts/changelog.ts`](scripts/changelog.ts), and a group
used by a single fragment folds back into the flat tail. Every `highlight-`
fragment must embed a screenshot or
recording — `![](media/<slug>.{png,gif,mp4,mov})` with the asset at
`.changelog/media/<slug>.<ext>` — which collect moves to
`assets/release-notes/<version>/` and rewrites in the emitted section. At
release time `bun run changelog` folds every fragment into a `## [<version>]`
section for the version in `Cargo.toml`, grouped under `### Highlights`,
`### Features`, `### Experiments`, and `### Fixed`; preview the fold with
`bun ./scripts/changelog.ts check`.

Write release notes for the final product users receive, not the development
history. When a feature is still unreleased, fold its fixes and refinements into
the original feature bullet instead of adding separate entries for them.

## [0.10.0]

### Features

- **Sessions**
  - `goddard-agent ask` lets an agent render a structured question card mid-turn and block until the user answers, clarifies, or dismisses — bringing agent-to-user elicitation to providers with no native question mechanism
  - Pick "No project" at the top of the "New task in…" picker to start a task that isn't attached to a project.
  - A task's project can be changed from the project picker at any time — even mid-turn. The agent is told its new working directory immediately, a marker in the transcript records the move, and switching to a different repository asks for confirmation first.
  - Finished tasks can get a clearer title when Jev finds their automatic title confusing or noisy. Choose an inexpensive title cleanup model for supported providers in Providers settings.
- **Composer**
  - Toggle incognito on an unstarted task with `/incognito`, remove the incognito chip with its close control, and see “New incognito task” in the header.
  - A side chat's composer now matches the main composer's card: the model, access, and option chips stay visible but disabled so the chat's inherited settings still show, while send, stop, and continue all work. A bare `/side` also focuses the side chat's composer on open; `/side <prompt>` leaves focus where it was.
- **Transcript**
  - Clicking a Pasted text chip in a user prompt expands the full pasted content inline; click it again to collapse.
  - Right-click a link in a response or a Markdown file preview to open it, copy it, open it in a built-in browser tab, or open it in a private window of your default browser (when the browser supports it).
- **Settings**
  - Adjust an auto prompt’s sensitivity to make its follow-up trigger more or less often.
  - In Jev settings, opt in to automatic suggested actions one by one and set each action's required probability from 80% to 100%. Direct Commit suggestions still require review in the commit dialog.
  - On macOS, General settings can now switch the update channel between Stable and Dev. Dev checks dev.goddardai.org — the feed a dev worktree publishes with `bun run dev --serve` — instead of released versions.
  - Open Settings → Memory to read a project's saved memory and browse its dated change log. New entries require Project memory to be enabled under Experiments.
  - Settings rows now lead with an icon matching the option they control, making it easier to scan a page and spot the setting you want.
- After landing a worktree, the transcript offers to push its base branch, then offers Archive once the branch is pushed.
- Archiving a task you reached with ⌘D now jumps to the task ⌘D would have opened next — the same landing as pressing it again — instead of the "After archiving a task" destination. On by default; turn it off in Settings → General → "Continue ⌘D when archiving".
- Renaming a task no longer needs the preemptive top-bar toggle — the agent's rename now asks in the transcript as a permission card that stays pinned at the top until you answer, with Rename, Always allow, and Deny.
- In Settings → General, turn on "Group recent Planning tasks" to move tasks still planning and active within 30 minutes between Pinned and Today. The Planning marker is off by default unless Phase-aware routing is enabled; you can hide it on task rows while keeping it in the selected task's title bar.

### Experiments

- **[Experimental]** Computer Use now asks before an agent accesses an app, browser tabs, the clipboard, or the whole desktop, including in Full access. The Goddard JavaScript REPL is available to more local providers, and app approvals can be saved and revoked in Settings.
- **[Experimental]** The Projects Review tab lists dev commits ahead of main and starts a release task for the confirmed approved range.
- **[Experimental]** Added an experimental Voice briefing opt-in: opening a task whose latest reply is longer than 300 characters speaks a ~45-second summary — what the agent did, then any decision or action it needs from you. Configure your AI Gateway key, summary model, and Gemini 3.8 TTS tier under the card in Settings → Experiments.

### Fixed

- **Sessions**
  - Incognito actions now show their hat-and-glasses icon in the command palette.
  - Fix the archive and dormant-sweep warnings listing "unpushed" commits after the session's base branch is rebased: commits whose patch already reached the base under a rewritten SHA no longer count, so the dialog and the sidebar badge only report the session's own work
- **Composer**
  - Suggested actions above the composer now perform a concrete step; requests to supply details or choose without listed options no longer show a misleading action or ⌘Enter shortcut.
  - Hide the manual reasoning effort control in the composer while Auto (Jev) is selected, since Jev chooses the effort for each turn.
  - Steering now reaches an agent immediately while it thinks or uses tools; messages sent during reply text remain queued for the next turn.
- **Providers**
  - Picking Auto routing no longer carries over to the next task — a new task starts on the provider and model Auto last routed to, with routing off until you choose Auto again.
  - Fixed model picker rows painting their selected and hovered fill flush against the panel edge — rows now keep a visible side gutter.
- **Git**
  - Turn checkpoints stage only files changed during that turn. A follow-up after stopping an untouched turn also skips the worktree scan, while preserving work present when the turn began.
  - The "Sync branch…" picker (⌘S) and the Git panel's sync button skip the pull when the branch's upstream has nothing new — a toast reports the branch is already up to date, and uncommitted changes no longer make a no-op sync fail.
- **Keyboard**
  - Press ⌘⌥1 to select Auto routing in the composer; ⌘⌥2–⌘⌥9 select the first eight starred models. Auto routing is also available in the ⌥Tab model cycle.
  - Use ⌘⌥← and ⌘⌥→ to sweep through Big Picture tasks while keeping ⌥← and ⌥→ available for moving through words in text fields.
  - Fixed the option modals shown by selection shortcuts (access mode, model, workspace, branch) opening with an empty list — the choices render again.
  - Fixed the “New task in…” project picker hiding its search field while ⌘N cycled recent projects — the field stays visible, and releasing ⌘ commits the highlighted project.
- **Settings**
  - The built-in “Sharpen complex answers” auto prompt no longer fires on answers that are already concise — it now requires a response that is materially too long, dense, or broad for the request. Rules you have customized are left untouched.
  - The Auto Prompt editor fields in Settings → Jev now grow with their content, so long prompts and trigger questions stay readable while editing.
- Menu and dialog drop shadows no longer pop in a beat after the card appears — the shadow fades in with the entrance instead of being held back until the card turns opaque.
- The experimental sidebar dock is no longer cut off at the sidebar's edge — it draws above the rest of the window, and the hover strip that raises it reaches a little higher above the bottom bar.
- Context-menu flyouts like **Send to friend** now open centered on the row you hovered, so the submenu sits next to your pointer instead of aligned to the top of the menu.
- Fixed a crash that could quit the app as soon as a transcript showed an activity row whose detail is a file name — the link's context menu was being registered while the transcript was locked for rendering.

## [0.9.0]

### Features

- **Sessions**
  - `goddard-agent search` accepts an optional `last_turns` count that confines each task's transcript search to its most recent turns, so agents can skip stale early-turn matches when looking for recent sibling work.
  - Let agents rename their own tasks after a per-task grant in the transcript, and let you rename a task with `/rename <title>` in the composer.
  - Incognito tasks: a hat-and-glasses task lives entirely in memory — it reads no project memory, writes none, persists no transcript or checkpoints, and vanishes when the app quits. Start one from the command palette ("New incognito task…" variants) or with `/incognito` in a draft composer; side chats inherit the flag.
  - Confirming Reclaim Disk Space now shows a header toggle with live progress, opens a popover with the space freed when it finishes, and stays until you confirm the result.
- **Composer**
  - `/goal <description>` now keeps pursuing a goal with Jev on providers without native goals, pausing when the agent needs your input or the evaluator cannot safely continue.
  - Pasted text chips now use Jev to show labels such as “Bug report” or “Proposal” alongside their line counts. When Jev is configured, classification runs in the background and refreshes after edits; uncertain results keep “Pasted text”. Staged pasted text and session references remain chips as you change a task's project, workspace, or branch.
  - ⌘⌥T opens “Save as prompt template” for the current draft, or the most recently sent prompt when the composer is empty.
- **Providers**
  - The Providers settings page now shows an Update button on each detected agent CLI, which runs the CLI's own update command — or its documented installer when it has none — in an embedded terminal and refreshes the detected version.
  - The Jev routing class pickers in Settings → Routing now use the full model picker: search, provider rail, favorites, recents, and effort variants included. "Last used" is now "No override" — the class keeps your normal provider/model while Jev still classifies the task.
- **Git**
  - View a project's GitHub releases and Actions runs, including release notes and failed run logs, from its Activity tab.
  - Point the Projects page's Review tab at any branch via the new "QA branch" field in Settings → Daemon — it defaults to `qa`.
- **Transcript**
  - Right-click a Markdown preview or agent response and choose **Go fast** to read its prose one word at a time, with adjustable speed and pauses for code, equations, and tables.
  - Returning to a task that's still working now lands on the live end of its transcript whenever you left while watching the current turn, even if you had scrolled up within it. Leaving while reading older turns still restores your exact spot, and a task that finished while you were away keeps its usual landing.
- **Terminals**
  - Terminal tabs in the right panel now close automatically when their shell or program exits instead of leaving a dead, dimmed surface behind
  - Run project script… (⌘R) now asks where to run when the repo has more than the local checkout: the current task's worktree leads the list, followed by the local checkout and any worktrees not created by Goddard. Scripts run outside the task's own workspace open in a standalone terminal.
- **Keyboard**
  - “Double check...” (⌘⌥C) opens a linked side chat that respectfully challenges the current agent’s reasoning and, when available, asks it one focused question.
  - Settings now includes a Keybindings page where you can browse every command's shortcut on a visual keyboard, filter by modifier or key name, rebind keys, and see conflicts before saving.
  - Keyboard shortcuts for model, favorite/recent model, reasoning effort, access and environment, workspace, and branch choices now open a centered modal that shows the available options before a selection is applied.
  - Press ⌘N on the New Task page to search projects by name or path; press ⌘N or ⌘⇧N again to cycle through recently used projects.
  - Press Enter or ⌘⏎ to send a drafted prompt even when the composer isn't focused — as long as focus isn't inside another text field or a dialog. Enter sends the way the composer's own Enter does, and ⌘⏎ steers the way its chord does; a control that activates on Enter still wins the key.
- **Navigation**
  - Open Archived Chats, Automations, Inbox, or Projects from the command palette, or start adding a remote host there.
  - When nothing is unread, ⌘D now visits each idle task once before cycling back, so repeated presses keep moving instead of re-opening a task you just saw. The cycle resets after 10 minutes without a press.
- **Settings**
  - There's a new Diagnostics page in Settings — also reachable from the command palette — listing errors the app reported alongside daemon restarts and panics, newest first. Select any row to copy its record for a bug report.
  - Configure Jev auto prompts that send a follow-up after a completed task turn matches your questions. Ships with an enabled rule that asks the agent to sharpen answers likely to overwhelm you.
  - The Jev settings page's token usage breakdown now says what each spend category is for, and categories such as Action predictions show their names instead of raw log tags.
  - Settings keeps a back/forward history across panes: ⌘[ and ⌘] — or the new arrow buttons in the titlebar — step through the pages visited this session, restoring each page's scroll position, and back steps out of Settings once the history is spent.
- **Friends**
  - Find Friends actions in the command palette, including adding a friend, copying your friend code, sharing a file or folder, sharing a project, and opening Friends settings.
  - Files and folders received from friends now appear as a card in the task instead of plain text — folders list their contents, images show a preview you can zoom, and Open unlocks once you trust the files.
- The daemon's per-minute stats log (`daemon-stats.jsonl` in the app data directory) now itemizes each child process — pid, process name, subtree memory, and the owning task and provider when known — and reports per-session resident transcript sizes, so memory questions name the task that holds the RAM.
- The dock's circular buttons in the sidebar now follow the active theme: their background uses the composer input color and their icons use the body text color instead of fixed white and black.

### Experiments

- **Transcript**
  - **[Experimental]** Turn status markers suggest a matching next step for go-aheads, decisions, missing details, unfinished work, repairable errors, and untested code. Suggested prompt text can be edited or reset in Jev settings.
  - **[Experimental]** When a turn's status pill and its suggested-action chips would both float above the composer, they now share one row — the pill leads the suggestions instead of overlapping them.
  - **[Experimental]** Turn status markers can now show Answered, Nothing to do, or Pushed back when a turn ends without completed work — distinguishing answers to questions, requests that needed no changes, and asks the agent declined or redirected.
- **[Experimental]** Suggested-action chips above the composer can now be triggered with ⌘Enter while the composer is completely empty — no text, attachments, or annotations — and show a ⌘↵ hint whenever the shortcut is live. Once a draft is started, ⌘Enter keeps its usual meaning of steering the running turn.
- **[Experimental]** Phase-aware routing (requires Auto model routing): tasks the evaluator judges plan-worthy open on your hardest-class model, downshift one class tier once the tool stream or the evaluator says planning ended, and climb back if implementation stalls — sidebar rows show Planning/Executing while it runs.
- **[Experimental]** ⌥← and ⌥→ now sweep sessions backward and forward through Big Picture's card grid — opening it if needed — and ↵ opens the highlighted session while ↓ or ⌘↵ drops into its composer. Big Picture also appears in the command palette.
- **[Experimental]** Project memory distillation no longer inherits a session's model, reasoning effort, or service tier — the Project memory card in Settings → Experiments has a per-provider model picker (each provider's own default unless changed), and the throwaway provider session each background run creates is deleted afterwards instead of lingering in the provider's session list.
- **[Experimental]** Idle tasks in the sidebar now show how their last turn ended under the turn status markers experiment: a chat icon when the agent is waiting on a reply, a block icon when it hit a wall. The unread dot still takes precedence.

### Fixed

- **Sessions**
  - Archiving an idle Codex chat now closes its connection, so restoring it resumes the conversation with a fresh connection.
  - Fixed a busy or briefly unresponsive daemon being restarted by the app, which stopped every task's running turn; the app now reconnects in place and only replaces a daemon that stays unreachable. Recovery events are also logged to `~/.goddard/daemon-recovery.jsonl` so a restart's cause can be checked locally.
  - Sending a message to a task left idle past the daemon's runtime timeout no longer fails with "the message never reached the agent" — the app now hears that the runtime was retired and resumes the task on a fresh one.
  - Fixed long waits when opening an older task while other tasks are still streaming or the daemon has just restarted; clicks no longer queue behind in-progress saves, and saves during streaming now send only new transcript entries instead of the whole history.
- **Composer**
  - Continue on a task whose turn failed before the agent ever received it — for example when the daemon was too busy to start the provider — now resends your original message instead of a generic "keep going" nudge that reached an empty session.
  - Session references and pasted text sent from the composer now keep their compact chips in the transcript instead of expanding to the session token or the full pasted text. Clicking a session chip still opens its task.
  - ⌘↩ while the agent is generating now queues the message as a follow-up instead of injecting it mid-turn — a provider could acknowledge the steer and still drop it when the turn settled. Steering still applies to a task waiting on background work, where the message wakes the open turn directly.
- **Providers**
  - Rewinding a Codex task now works when Codex stores its conversation in paginated history.
  - Release a Codex thread's old writer before resuming it after a task runtime restarts, including when moving the task to a worktree.
- **Transcript**
  - Fixed a crash that could quit the app as soon as a transcript showed an activity row whose detail is a file name — the link's context menu was being registered while the transcript was locked for rendering.
  - When Goddard can't confirm a prompt reached its agent, the transcript now reports unconfirmed delivery instead of claiming the message never arrived.
  - Fixed the floating turn status markers (Settings → Experiments) not appearing while reading a long response: they were waiting for the turn's whole footer row to scroll out of view, so a "N files changed" card holding the row's top edge on screen kept them hidden even after the marker chips themselves had slid below the fold.
- **Keyboard**
  - Keyboard task navigation no longer lands on dormant tasks: ⌘D (next unread), ⌘⇧D (mark unread and move on), the post-archive landing, and the Ctrl-Tab switcher all skip tasks shelved in the Dormant section — a dormant task only opens when you pick it yourself.
  - The command palette ignores an Enter pressed immediately after ⌘K opens it, so the muscle-memory chord for clearing terminal scrollback no longer runs whatever command happens to be highlighted.
- Fixed the **Send to friend** flyout on a reply's context menu closing before the pointer could reach a friend, so clicking a name now opens the send confirmation.
- The new task page's sync strip now disappears promptly once a push or pull it launched finishes, and shows a spinner while it re-checks the branch's upstream instead of flashing away
- Fixed connected integrations going silent in file-configured providers (Devin, Claude Code, Cursor, and others) after a Goddard restart: the provider's MCP entry now refreshes to the current proxy address on every launch instead of pointing at the previous daemon's dead port.
- A terminal you ran commands in no longer dies when its task's workspace changes — for example when a draft task's new worktree is created. It moves to the sidebar's Terminals group and keeps running; a terminal that never ran anything still follows the task to the new workspace.

## [0.8.0]

### Features

- **Sessions**
  - Prompts a task's agent sends to another task with `goddard-agent prompt` now arrive labeled for the receiving agent — "from your side chat" or "the agent of another Goddard task," with the sender's task id and how to reply — instead of reading as text the user typed. The transcript still shows the sender's own words under the "Sent by agent" chip.
  - `goddard-agent read` returns a task's transcript as turn-tagged entries — messages and condensed tool activity — and can pull a single turn with `{"turn": N}`; a session's credential can always read its own task (and a side chat's parent), even with the cross-task agent tools off
  - Agents can now search the transcripts of sibling tasks in their own project through `goddard-agent search` — the same full-text corpus ⌘K scans — returning matching task ids, titles, status, and excerpts so an agent can find prior work without loading every transcript, then `read` the one that matters
  - Reclaim disk space from idle worktree sessions: a new "Reclaim Disk Space…" command (command palette and task context menu) lists started, inactive worktrees alongside the dependency installs and build outputs they still hold, and deletes only git-ignored regenerable directories — source changes and task history stay untouched.
  - Session search (⌘K and `goddard-agent search`) now understands `field:value` filters — `project:<name>`, `status:<idle|connecting|working|waiting|background|failed|busy>`, `archived:<true|false|any>`, and `limit:<n>` — with the remaining words searched as transcript text, so `status:idle retry logic` finds the idle task that mentioned retry logic
  - Side chats open with a snapshot index of their parent task — its user messages verbatim plus one cue line per turn's work — delivered as hidden context, and can pull any turn's current text with `goddard-agent read`
- **Composer**
  - The composer's model-options chip now shows a tooltip on hover, including the ⌘E shortcut that cycles reasoning effort.
  - `pasted text` chips in the composer are now editable: click one and a floating editor opens below it with the paste loaded — click away to save, press Escape to discard, or hit the trash icon to remove the chip. It replaces the old double-click-to-expand.
  - `@` task mentions in the composer now suggest only sessions from the task's own project — reference a session from another project by dragging it in from the sidebar.
- **Providers**
  - Codex plan meters now show banked reset credits with their nearest expiry, and a **Use reset** row redeems one behind a confirmation that spells out the weekly-reset anchor moving; the meter refreshes with the cleared windows
  - Switching a task to a new provider now hands off a compact per-turn index — the user's messages verbatim plus Jev-selected transcript excerpts — and the fresh session pulls any turn's full text on demand with `goddard-agent read`, instead of ingesting the whole extracted transcript; a missing Jev backend degrades to a pointer-only handoff rather than blocking the switch
- **Git**
  - Right-clicking a commit link in an agent response now lets you copy its full hash or message, or open its diff in the Git panel.
  - New General setting "Remind the task to commit after an empty land" (off by default): when a land reports the base already contains every commit, the owning task's chat gets a commit reminder unless its last message already names a commit
  - Syncing a branch closes the picker and shows pull progress in a toast.
- Add Alt+Shift+Tab to cycle favorite models backward.
- Projects can be starred from the New task project picker. A starred project's tasks lead ⌘D navigation — even ahead of unread completions elsewhere — and its group sits at the top of the sidebar's Project grouping; ⌘D shows a star next to the bell when it will land on one, and an off-screen starred completion plays a distinct sound (toggleable in Settings → Sounds).
- New "Keep awake" toggle in Settings → Daemon: while on, the daemon holds the host's sleep assertions so the mobile and web apps stay reachable — on AC power the machine stays awake even with the lid closed
- Choose which task events send system notifications while the app is in the background: new "Finished turn" and "Waiting for input" toggles under Settings → Notifications — and a task blocked on your approval or a question now banners instead of waiting silently
- Sidebar draft previews get a "Draft preview color" setting (General, under "Sidebar draft previews") with Subtle — now the default, matching the row's detail line — Accent, and Loud, the alert red they shipped with
- Custom commands, the workspace sync strip, and other app-run terminal commands now run as plain typed commands in shells with Goddard's integration hooks (zsh, bash 4.4+, fish) — no more opaque `source`-d script files — and print their script first on shells that still need the file
- Transcripts can link to another task with `[title](goddard://task/<task-id>)`: the link renders like any other and opens that task when clicked, which `goddard-agent search` results suggest so agent replies can point at the tasks they cite

### Experiments

- **Sessions**
  - **[Experimental]** Tasks can now run in the provider's own hosted environment — the composer's Environment menu gains a named cloud row (Devin Cloud, Codex Cloud, Claude Cloud, Cursor Cloud, Copilot Cloud, Droid Cloud) for providers that support one. Cloud tasks run against the repo's pushed state only, so Goddard asks you to push an unpushed branch or commits before submitting, and the task runs and reports like any other session.
  - **[Experimental]** Sandbox VM: provider credentials now live in one shared home per provider instead of a per-worktree directory, so sign-in happens once per provider; when a sandboxed session has no credentials its launch parks the prompt and opens the provider's interactive sign-in as a terminal tab in the session's right panel — and the VM now covers Amp, Cursor, Devin, Droid, Fx, Goose, Grok, Kimi, Pi, and Oh My Pi alongside Claude and Codex
- **[Experimental]** Turn status markers now float over the transcript's bottom edge while the settled turn's footer is scrolled below the fold, so the verdict stays visible while reading a long response; clicking the float scrolls the footer back into view

### Fixed

- **Sessions**
  - Selecting a task in the Dormant group no longer wakes it — dormant tasks stay put while you browse them and only return to the live groups when a prompt is sent or they're restored from the context menu.
  - Creating a task with no project no longer fails at submit when the workspace it restores was archived while still empty — an entry-less archive now recreates the directory instead of dying on "No such file or directory"
  - Choosing "No project" in the project switcher or project pickers now provisions a fresh scratch workspace instead of binding the new task to an earlier projectless task's directory — the stale binding could resurrect a finished task's files or fail outright when that directory was gone
  - The `/side` command never appeared in the composer's slash-command picker; it is now listed for every provider alongside `/resume` and `/land`.
  - The archive toast's Undo now selects the restored task directly instead of showing a second "Task unarchived" toast with a "View now" button.
- **Composer**
  - Move the Big Picture shortcut hint above the composer so the composer docks at the bottom of the overlay
  - Unstarring a model in the model picker no longer drops its row out of the favorites block — the star empties and the ⌘⌥ shortcuts compact immediately, but the row stays put until the picker closes so re-starring is a one-click undo.
- **Providers**
  - Explain that Codex thread conflicts and thread-open failures may come from the ChatGPT app or Codex CLI, and tell users to close the thread there before retrying in Goddard.
  - Fixed the reasoning effort menu disappearing on Grok models the hardcoded list predates (like Grok 4.7); effort options now come from the CLI's own model catalog, so newly released and custom models get their real ladder
- **Transcript**
  - Fixed a crash that could quit the app as soon as a transcript showed an activity row whose detail is a file name — the link's context menu was being registered while the transcript was locked for rendering.
  - The "Checking for changes…" card now appears only once a prompt is queued behind the turn's checkpoint — a capture that finishes before you send stays silent instead of flashing the card
  - The "Landed on `<base>`" card is its own transcript row again instead of stacking onto the response above, and its label no longer shows a text cursor on hover
- Keep the selected commit header clear of macOS traffic lights when the sidebar is hidden.
- ⌘⇧N is always "New task in…" — it no longer opens the project switcher on the New Task page, and only cycles that overlay in reverse while ⌘N has it up; ⌘N in Big Picture now raises the same visible project switcher for the new-task draft instead of stepping the destination invisibly
- Native Markdown rendering treats soft line breaks as spaces while explicit hard breaks remain line breaks.
- Reduced memory usage in long-running windows: transcripts of tasks nobody is viewing, remote images, parked panel file buffers and diffs, and cached issue/PR details are now released when idle and reload on demand instead of accumulating for the life of the window.
- Fix icons that never rendered: transcript status notices, the quit confirmation's Quit action, Git panel unstage buttons, the GitHub "Fix checks" action, draft attachment badges, and Projects worktree badges.
- The Git and right-panel toggles no longer shift position when a panel opens — the panel headers now match the top bar's height, inset, and icon spacing.
- Renaming a session in the sidebar could leave its status indicator offset from the row's right edge; the indicator now stays pinned to the timestamp edge through renames.

## [0.7.0]

### Features

- **Composer**
  - Tasks are now inline `session:` mentions in the composer — drag one from the sidebar or accept it from `@` autocomplete and it lands at the caret, holds its place in the prompt, and deletes atomically with Backspace.
  - Collapsed pastes join them: a folded paste now sits inline as a `pasted text` mention wherever the caret was — deletable with one Backspace, and a double-click splices it back into the field.
  - A new task's workspace chip now tints its icon and label with the theme's accent color while the local checkout is selected instead of a new worktree — toggle it with "Local workspace accent" in Settings → General
- **Git**
  - Changed-file rows in the Git panel now open a right-click context menu — Open Changes, Open File, Open File (HEAD), Stage/Unstage Changes, Discard Changes behind a confirmation, Add to .gitignore, Reveal in Finder, and Reveal in Files — also reachable with Shift-F10 on a focused row
  - The Git panel now remembers its own width separately from the right panel — resizing one no longer moves the other, and swapping between them slides between their widths
- **Panels**
  - Tasks, main-area terminals, and the Projects page each keep their own right panel — tabs park and restore per context instead of sharing one strip. The panel no longer lingers over Drafts, Automations, and the Inbox, and the Projects page hosts issue/PR details plus files rooted at the project.
  - File-name links — the file viewer's top bar, diff file headers, and file paths in chat activity rows and @-mentions — now open the file browser's right-click menu (Open In, Open With, Save As, Copy Path, Copy File Contents, Reveal in Finder), also reachable with Shift-F10 on a focused link
- **Terminals**
  - ⌘T pressed while a right-panel terminal has focus now opens another terminal tab in the panel — rooted in that terminal's directory — instead of taking over the main area
  - `Cmd+P` and the Files panel now work while a terminal fills the main area, searching and browsing the shell's current directory — a `cd` re-roots the panel, which keeps each directory's open files and unsaved edits parked for when you come back
- **Settings**
  - An enabled eval-backed experiment card on Settings → Experiments now warns when no evaluation backend is configured, with a shortcut to the Jev settings page.
  - Settings booleans that encode two named modes now render as dropdown menus so both sides of the choice carry a label — Appearance's System/Light/Dark, worktree base, sync strategy, both conflict handlers, Markdown files, math rendering, border weight, sidebar transparency, the terminal's modifier-click, and the sandbox default environment; stored preferences keep working unchanged
  - Settings pages in the sidebar now group by concern — personalization (General, Appearance, Keybindings), agent workflow (Providers, Skills, Commands, Terminal, Git), data (Usage, Archived), then Daemon — with experiment opt-in pages sitting directly above the Experiments page that enables them
- **Friends**
  - Forward a received bot reply to a friend: assistant messages in the
    transcript gain a Send to Friend submenu — pick a friend, confirm, and
    the text lands on their side as a session in the Friends project, like
    a delivered transfer's note
  - Send folders to friends, not just files — the share picker accepts either, and a folder crosses the wire as a blake3-verified collection that lands as a real directory tree on the other side
- Quitting (⌘Q) or closing the window (⌘W / close button) now asks for confirmation while task sessions or terminals still have work in progress.

### Experiments

- **[Experimental]** Action predictions: an Experiments-page opt-in that asks the evaluation model, at each turn's end, to guess your next move — and floats a one-click chip above the composer when the guess is confident and actionable (run the tests, commit, push, land, canned follow-ups); a local journal of consequential actions (~/.goddard/actions.jsonl) resolves each guess and logs verdicts to ~/.goddard/action-predictions.jsonl

### Fixed

- **Sessions**
  - Sessions now tell their agent about the `goddard-agent` command instead of leaving it undiscoverable on `PATH`: providers with a native instruction channel (Claude, Codex, Copilot, OpenCode, OpenCode 2) announce it at launch, and every other provider hears it through the session's first-prompt context — so asks like "create a task" or "message the other task" reliably reach `goddard-agent create`/`prompt`.
  - The archive and sweep confirmation dialogs now name the session's actual title instead of "New task" for tasks that were never manually renamed
  - A dropped daemon connection on a still-running daemon no longer restarts the process — the app reconnects in place instead, so a momentary socket blip stops interrupting every in-progress turn
  - A task whose provider process exited on its own no longer leaks the runtime's event replay backlog — the daemon now retires it the way an explicit close does, instead of holding it until restart
  - Provider runtimes a task leaves idle are now reclaimed after 30 minutes instead of living until the daemon exits — the next prompt resumes the provider from its cursor, and sessions that are busy or cannot resume are never killed. Tune with `runtime_idle_timeout_secs` in daemon settings; `0` disables eviction
  - Full-text session search no longer matches synthesized transcript notices — status lines like "the Goddard daemon restarted and this turn could not be reattached" were indexed as ordinary messages and surfaced tasks whose real content never mentioned the query
- **Composer**
  - Pasting a large or multi-line copy (20+ lines or 4 KB+) into the composer no longer swallows it — the splice that seats the pasted block's marker was dropping the block it had just created, leaving an invisible marker and losing the pasted text on submit
  - Wrapped text no longer strands closing punctuation on its own line — `!`, `?`, `)`, `]`, `"`, `—`, and similar characters now carry down with the word they belong to, most visibly in the annotation comment card
- **Transcript**
  - The post-turn "Saving changed files…" row is gone — while Goddard reads the worktree after a turn, the changed-files card itself appears in its normal spot reading "Checking for changes…" and fills in place when the checkpoint lands, across the transcript, Big Picture, and side chat
  - Assistant replies no longer splinter into one row per fragment while streaming — progress updates for tool rows already on screen no longer close the running message, and text that resumes mid-sentence across a tool call rejoins the message it was cut from
- **Terminals**
  - Terminals now draw block-mosaic glyphs — sextants, quadrant and eighth blocks, and shade fills — as exact cell-filling shapes instead of text, so output like Expo's dev-server QR code renders correctly instead of showing missing-glyph boxes
  - Deleting a task or project now closes its daemon-hosted terminals too — a web or mobile terminal whose client disconnected before closing it used to keep its shell running on the daemon until restart
- **Settings**
  - Eval-backed features now check the configured backend's credential before calling it: the provider-switch dialog warns when no usable backend exists and offers a shortcut to the Jev settings page — which opens even while no eval experiment is enabled — instead of failing mid-switch, and per-turn evaluations for status markers and action predictions no longer fire requests that can only fail.
  - Settings → Jev now stays in the sidebar whenever a feature that runs on the evaluation backend is enabled — Turn status markers and Action predictions count alongside Auto model routing — instead of only while the routing experiment is on
- Menu cards no longer let the content beneath ghost through at 90–95% alpha — the fill is opaque again, with the top-edge sheen kept as a lightness gradient
- Fix the quarantined-files banner's hint text spilling outside its rounded
  card: the text column now wraps within the banner instead of pushing past
  its edge
- Turn checkpoints on large repositories now land in seconds instead of minutes: captures reuse a per-worktree Git index so unchanged files aren't re-hashed, a turn that touched nothing commits the existing tree outright, and snapshots on the same worktree can no longer run concurrently
- The sidebar's branch labels and dirty/unpushed badges now re-scan when any session's turn ends, not only the selected one — commits an agent makes on your behalf no longer leave stale git status on its row.

## [0.6.0]

### Features

- **Sessions**
  - A Goddard daemon restart no longer kills in-flight work: the session reloads its provider transcript from the saved resume cursor, shows a "daemon restarted — resuming" marker, and continues the interrupted turn automatically (once per restart, only for turns the provider had actually started)
  - `goddard-agent create` can now omit `provider`, `model`, `reasoning_effort`, `service_tier`, and `context_window` — omitted fields inherit the calling task's configuration (clamped to what the resolved model's catalog lists), so agents spawn follow-up tasks without restating their own settings
- **Git**
  - Sync strips and push affordances now fetch the branch's remote-tracking ref in the background (at most once a minute while shown), so the suggested action reflects upstream as it is now rather than as of your last manual fetch — a colleague's merge surfaces as "Pull" instead of a stale "Push". Toggle off under Settings → Git.
  - Landed notices now carry a Push button that sends the base branch to its tracked upstream (⌘⇧↩ does the same for the selected task), showing a "Pushed" check once the remote holds the commits; a rejected push opens a dialog with the Git error and a "Sync & retry push" recovery
- **SSH**
  - Daemon settings now show a QR code for mobile pairing: "Show QR code" in the connection details reveals a code encoding the LAN address and token, and the mobile app scans it (Daemons → scan icon, or the daemon picker's "Scan QR Code…") to add and connect to the host without typing — the same `goddard://connect` link also works from the phone's camera app
  - Adding a remote host in Settings → Daemon now starts with a "Connect via" picker — SSH or WebSocket — that selects which fields render, instead of offering destination, address, and token as peers where a filled destination silently discarded the other two; SSH is the default on unix and the option is omitted on platforms without SSH support
- A manual update check no longer depends on reaching the app menu — the command palette offers a "Check for Updates" command, and Settings → General shows the installed version next to a "Check now" button
- The Dock menu on macOS and the taskbar jump list on Windows offer New task, Check for Updates, and Settings
- GitHub Copilot sessions now support mid-turn steering: a message sent while the agent is working is folded into the running turn instead of waiting for it to finish
- Right-click an `@` file mention in a sent prompt to copy its path or show it in the file manager

### Experiments

- **Transcript**
  - **[Experimental]** Guided reading: an Experiments-page opt-in that shapes the leading letters of each word semibold in transcript prose, with Fixation (1–5), Saccade (10–50), and Opacity (0–100) sliders; Latin-script text only, and code, math, and monospace output are untouched
  - **[Experimental]** Turn status markers now judge each settled turn against an ordered tool-call sequence (reads and searches collapse to a count), failed-call output tails, per-file diff stats, earlier turn prompts, and context-window occupancy. The ending is a single calibrated pick — Complete, Awaiting input, Partial, Blocked, or Failed — while new flags catch risky diffs (Needs review), retry loops (Thrashing), and unconfirmed assumptions (Assumed); Unverified now also catches verification that ran and failed.
- **[Experimental]** Project memory now verifies the commit SHAs it cites: each distillation pass resolves commit-shaped tokens in MEMORY.md against git — landed on a ref, on a detached worktree HEAD, or orphaned with a same-subject successor — records the status in LOG.txt, and the distiller rewrites dead citations instead of propagating them as durable fact
- **[Experimental]** The Jev settings page now shows evaluation token usage — totals plus a per-feature breakdown (routing, turn status markers, memory, permission review) summed from the daemon's decision log

### Fixed

- **Sessions**
  - Show prompts an agent queues into a task via `goddard-agent` in the target session's follow-up queue — marked as sent by an agent — instead of hiding them in daemon memory, with a remove action that cancels delivery before the prompt runs.
  - Project memory and the project map now reach steer-capable providers as hidden context after the first prompt — session titles no longer pick up the injected blocks, and the context never renders as a transcript row. Providers without steering keep the prepended-prompt behavior.
  - A prompt swallowed between the app and the provider no longer leaves a turn spinning forever — after a minute without the provider's turn-start acknowledgement the turn settles failed with a "message never reached the agent" notice, and the check re-arms when reattaching to a session still reporting an unconfirmed turn
  - Fixed task titles that could show the hidden `<project-memory>`/`<project-map>` context blocks injected into a session's first prompt — Kimi echoes the prompt back as its title and Devin's stored placeholder can truncate inside a block, so both the prompt-derived fallback and provider-reported titles now strip those spans.
  - Creating a task with no project no longer fails at submit — the daemon swept a freshly provisioned workspace's project row before the first prompt could attach a task, and a missing workspace directory is now recreated (or restored from its archive) instead of dying inside an opaque provider spawn error
- **Sidebar**
  - A collapsed sidebar group's unread dot now sits at the row's right edge like a session row's indicator instead of trailing the group label
  - A selected task's sidebar row no longer repeats its unsent draft text — the composer already shows it — and the draft line now leads with a pencil icon so it reads as a draft
  - Starting a sidebar multi-selection with ⌘-click now keeps the task you're viewing in the selection, matching Finder's extend-rather-than-replace behavior — ⌘-click it again to leave it out — and batch menu items say how many tasks they act on ("Archive 3 tasks")
  - Drag-selecting text into the left window edge — in the terminal, the transcript, or anywhere else — no longer pops open the hidden sidebar; the edge strip now reveals it only on unpressed pointer motion
  - Pinned terminals now sort to the top of the sidebar's Terminals group instead of staying in creation order among unpinned rows
- **Providers**
  - Devin sessions no longer leave a subagent's tool calls stuck on "Running" — they settle when the subagent's lifecycle update arrives, and each subagent now shows up as a labeled "Subagent: …" row instead of a bare Tool entry
  - A turn stopped externally — for example by Devin's model server rather than by pressing Stop — now marks the session failed (red ✕) instead of earning a completed-turn unread dot.
  - Fixed project-memory distillation failing for Devin sessions: the headless driver now replays the session's stored reasoning effort and service tier so a folded base model id like `swe-2` resolves to an advertised packed id, and falls back to the advertised default model when nothing matches
- **Git**
  - The commit card's message preview now ends its last visible line with an ellipsis when the body is cut off — clamped paragraphs previously clipped mid-line with no marker, and a message with paragraph breaks could spill past the four-line preview
  - The command palette's "Sync branch…" row now shows the ⌘S shortcut hint, resolved from the live keymap.
  - The new task page's sync notice now reports the base branch a planned worktree will be cut from — and names it — instead of the local checkout's branch; its button fast-forwards or pushes the base when the checkout can't reach it with `git pull`/`git push`
- **Transcript**
  - Changed-files cards no longer silently disappear on large repositories: checkpoint captures now wait up to ten minutes instead of two, a lightweight pre-turn ref preserves a diff base when the full start snapshot never lands, and a "Saving changed files…" row holds the card's place while the capture is still running
  - A markdown table in a sent prompt no longer squashes into the bubble width sized by the message's short text — the bubble now takes its full width allowance whenever the prompt contains a table
- **Keyboard**
  - Experimental surfaces are now fully operable without the pointer — the Projects tab strip gains tab stops and a ⌘⌥3 chord for Review, the Git panel's upstream section toggles with Enter/Space, the Jev credential warning and settings gear are focusable, Inbox toolbar controls and GitHub detail actions activate from the keyboard, Automations switches use the shared toggle (which also stops a row switch from opening the detail pane), and settings' Apply/Test/Suggest/Revoke buttons share one keyboard-ready button.
  - The ⌘⌥1–⌘⌥9 favorite chords and ⌥Tab model cycling no longer stall on favorites stored as packed model slugs (e.g. `swe-2-medium`): the alias's suffix effort now applies to the session instead of the model's default, and the rotation positions itself on the folded base combo so every press advances
- **Appearance**
  - Dracula's text selection is now a visible purple wash — it previously reused the user-prompt bubble's exact fill, so selecting part of a sent prompt showed nothing
  - Menu text in dark themes stays legible again: the glass card's specular sheen is shallower, and each palette's text tiers are now solved to WCAG contrast floors over every surface — Zenburn, Rosé Pine Moon, Everforest, Dracula, and GitHub Dark menu items had fallen to 1.8–4.5:1 and now clear the bar. High Contrast mode widens text contrast too, not just borders.
- Switching projects from a new task's composer now moves that draft's "New task" row to the picked project — typed text and all — instead of stacking a second row under the old one; picking "No project" does the same
- The file editor's annotation chrome — the "Add to chat" pill, the comment editor and its input, and the hover tooltip — no longer renders in the code face inherited from the pane; each surface now sets the UI face explicitly

## [0.5.0]

### Features

- **Sessions**
  - New "Default workspace" setting chooses where new tasks start: "Last used workspace" keeps the current per-project behavior, "Local" always starts on the project's checkout, and "New worktree" always forks a worktree from the base branch last picked for that project
  - ⌘⇧D is now a "come back to this later" chain: consecutive presses jump to the most important task the chain hasn't shown — unread completions first, then the idle rotation — and a task is re-marked unread only if it was unread on arrival or settled a turn while selected, so parking an already-read task no longer manufactures a dot
  - The Resume… provider picker only offers providers that can enumerate their terminal sessions, and the Resume view explains when a provider can't expose them instead of showing an empty list
  - Resume… now lists terminal sessions whose project folder was moved or deleted — marked "folder missing" — and resumes them in the nearest folder that still exists
  - The Resume view now says when a provider's CLI isn't installed on this machine instead of showing an empty session list
  - The pin (⌘⌥P) and mark-unread (⌘⌥U, ⌘⇧D) shortcuts now confirm what they did with a toast — including batch selections and pinned terminals
- **Providers**
  - ⌥Tab cycles the composer session through its favorited model+effort combos plus the most recently used selection
  - Switch a started task to a different provider from the model picker: the pick now warns about compaction cost, Jev extracts the relevant transcript verbatim, and the session resumes on the new provider with that context injected into the next prompt. Switching back resumes the earlier provider-side session and injects only the work it missed; a provider session that can no longer be resumed is restarted with the full compacted history and marked in the transcript. Requires a configured evaluation backend, shared with Auto model routing.
- **Git**
  - New "Change base branch…" command (⌘K) for a task's worktree: it shows the current base, offers the repo's other local branches, and replays the task's own commits onto the pick with `git rebase --onto` — the worktree must be committed first, conflicts open the usual resolve-in-chat dialog, and the pick becomes the branch Land targets
  - Generated commit messages are now reviewable before they land: a Generate action (⌘G) fills the dialog's message field instead of committing immediately, dismissing mid-generation actually cancels (the commit used to land anyway), and a caption names the provider and model doing the generating
- **Transcript**
  - System notices in the transcript — "Stopped", "Turn completed", provider failures, goal changes — now lead with a status icon so they read as chrome, not agent replies
  - The "Sent by agent" chip on agent-authored prompts now opens the task that sent it; archived and deleted source tasks leave the chip inert
- **Terminals**
  - Selecting text in the integrated terminal now copies it to the clipboard when the mouse is released — like iTerm2's copy-on-select — with a new "Copy on select" setting to turn it off; the selection itself stays visible
  - Terminal panes drop their header strip — the status dot, title, and working directory are gone and the grid runs to the top edge, with command status still reported on the terminal's sidebar row
  - New "Terminal link modifier" setting chooses which key opens links and file paths when clicked in the integrated terminal — Option instead of Command on macOS, Alt instead of Ctrl on Linux and Windows; the unselected key stays a plain click
  - Mouse-aware terminal programs — vim, htop, lazygit — now receive clicks, drags, releases, and scrolls (SGR, UTF-8, and X10 encodings); Shift-click still selects text, Shift-scroll still reaches the scrollback, and links keep opening via the modifier click unless the new "Open links in mouse-aware terminal programs" setting is turned off
- **Settings**
  - The Experiments settings page now opens behind a one-time warning that experiments can be buggy or corrupt your data; accepting it once reveals the toggles
  - Settings gains a dedicated Terminal page — font size, link-click modifier, mouse-aware links, and copy-on-select — while the math/Markdown/token-speed display options moved to Appearance and General now groups its rows under Sessions, Git, and Notifications headers
- **SSH**
  - Changing daemon exposure no longer restarts the daemon or interrupts running tasks: the exposed listener opens, rebinds, and closes in place over a new `setDaemonExposure` command, while the loopback listener and every active session keep running — so toggling "Expose managed daemon" in Settings is safe at any time
  - Daemon access tokens now read as 12-word mnemonic phrases — easy to read, say aloud, and type when connecting Goddard Web or a phone — while previously generated hex tokens keep working unchanged
- Click a filename wherever one renders — activity rows, diff file headers, attachment tiles, checkpoint previews, and dialog file lists — to open it in its default app
- The command palette now includes Remove project…, which removes a project and its tasks from Goddard without deleting the project folder from disk
- Select text in a right-panel file editor — or in its rendered markdown preview — and "Add to chat" pins a commented highlight there that quotes into the next message as @path with a [Selected lines N-M] marker and a fenced block
- The Projects page no longer docks a composer — ⌘N goes to the New task page instead, and opening the page or cycling its projects no longer creates a task draft (or connects to a remote host) just for viewing

### Experiments

- **[Experimental]** QA review moves behind its own Settings → Experiments opt-in — the Projects page's Review tab only appears while it's enabled.
- **[Experimental]** Auto model routing now maps each task class — Routine, General, Demanding — to a provider, model, and effort configured under Settings → Jev, with a "suggest from recent usage" button that fills them from your most-used combos: Jev classifies the task at session start, the routed model locks for the session's life, and a per-turn evaluation can shift reasoning effort at high confidence while the model stays put
- **[Experimental]** Sandbox VM environment: opt in under Settings → Experiments to run a task's agent inside an isolated Linux ARM64 VM instead of on this Mac. The access menu gains an Environment section (This Mac / Sandbox VM), each sandboxed task boots its own VM with the worktree mounted read-write and dev-server ports forwarded, and Codex and Claude run inside the guest with API credentials proxied — never copied in. General → Daemon can also make sandboxing the default for new tasks. The window title's Sandboxed badge only shows while a sandboxed session is on screen.

### Fixed

- **Sessions**
  - Mobile's /resume command no longer offers resume for a provider that's disabled in Settings
  - The Resume… provider list no longer offers disabled providers, and Resume opens on an enabled provider when the current session's provider is turned off
  - The Resume… command is hidden when no provider could offer sessions — every provider is disabled or no provider CLI is installed
- **Providers**
  - The Providers page's Checked caption reserves its line so rows no longer shift when it appears or disappears, and it now repaints itself at the minute and hour boundaries instead of going stale on an idle page
  - Provider rows show Checking… until the first detection completes instead of flashing Not detected, expanding a provider's settings no longer steals keyboard focus into the binary path field, and the setup buttons respond to Enter and Space without swallowing modified chords
  - The Set up button on an undetected provider now expands its documented install and sign-in steps instead of immediately running them — the expanded row's Run in terminal, which previews the exact script, stays the explicit way to execute it
  - A failed provider setup now leaves its embedded terminal open with the error visible instead of vanishing, sign-in flows that print a localhost URL no longer kill the terminal mid-login, and clicking Run in terminal while an install is running refocuses it instead of killing it
- **Git**
  - Git and `gh` invocations now run with the login shell's search path, so helpers spawned by name — `git-lfs` during checkout, credential helpers, `core.sshCommand`, signing programs — resolve when the app is launched from Finder instead of dying with "command not found"
  - The Git panel's "Land onto `<base>`" button now asks first — a confirmation names the base branch and the commit count before the rebase-and-fast-forward rewrites the shared branch
  - Creating a worktree no longer fails when Git's LFS filters can't run — the worktree materializes with LFS pointer stubs and a notice to run `git lfs pull` inside it instead
  - A worktree creation that fails partway no longer poisons the name — the leftover claim directory and any half-registered worktree are cleaned up, so retrying with the same name succeeds instead of failing with "already exists"
- **Transcript**
  - Copying a selection that covers exactly one inline code span puts just the code on the clipboard, without the surrounding backticks
  - The "Landed on `<base>`" transcript card now spans the message column and starts collapsed: its header opens the commit list, subjects take the width the hashes don't need, and a "Show N more commits" row reveals commits beyond the first five
  - Commit-looking hex in agent replies — UUID segments, content hashes — no longer gets a commit affordance: each candidate is verified against the repository before the underline, hover card, and diff link appear
- **Terminals**
  - ⌘⇧T with a terminal on screen no longer spawns a duplicate — it cycles to the next terminal in sidebar order, wrapping past the end (a lone terminal folds back to where it took over); with no terminal selected it still opens the group on the last-shown one, or a fresh global terminal in ~ when the group is empty
  - ⌘T with a session selected opens a global terminal in the session's workspace instead of a session-owned one — it no longer appears in that session's right panel; session terminals still come from ⌘J or the panel's terminal button
- **Settings**
  - Revoking an always-allowed app on the Computer Use settings page now confirms first, naming the app whose grant is being dropped
  - Tab and Shift-Tab now move through every settings form, not just the custom command editor — fields and controls across all settings pages register as tab stops, and buttons that could be focused but not activated (integration cards, skill actions, computer-use permissions, usage selectors) now respond to Enter and Space
- Removing a friend and revoking a paired device now ask first — a confirmation names the friend or device instead of dropping them on one click
- The GitHub inbox's Done action now confirms first — it's irreversible on GitHub's side — and a failed Done write puts the thread back immediately instead of waiting for the next poll to restore it
- ⌘N always lands on the New task page now — on Projects, Drafts, Automations, Inbox, and Settings it opened the recent-project switcher over the page (or did nothing in Settings) instead of navigating; the switcher still answers ⌘N while New task is the page on screen
- OAuth sign-in for MCP integrations works across providers that were failing: registered redirect URIs carry the real `/oauth/callback` path, the loopback redirect uses `localhost` (Supabase rejects `127.0.0.1`), and discovery follows RFC 9728 protected-resource metadata to reach the true authorization server — fixing connects for Supabase, Atlassian, Monday, GitHub, and Linear
- The Archived Chats search now resets when you leave the page instead of reviving the last visit's query on return
- The file viewer's "Open on GitHub" button now only appears when the remote actually has the file — untracked, uncommitted, and unpushed work no longer opens a 404 — and links to the remote branch (or pushed commit) GitHub can serve rather than the local branch name
- Project memory's git exclude now lands in the shared git dir where it takes effect — linked worktrees wrote it to a per-worktree `info/exclude` git never reads — and the pattern narrows to `.goddard/memory/` so `.goddard/commands` stays committable
- Landing a checkout's commits now clears its sidebar unpushed-count badge right away instead of waiting out the rescan cadence; pushes and new commits refresh the badge on the same moment too
- Remote hosts no longer interrupt with SSH password prompts or connection errors until you actually use them: background connects run non-interactively and retry with backoff, the auth prompt only appears when you open, submit to, or fork a session on that host, and offline or password-required hosts are badged in the sidebar with a reconnect action. Adding or editing a host in Settings connects right away and accepts its host key on first contact, offline rows get a Connect button, a failed attempt toasts the cause, and prompts queued to an unreachable remote are restored to the composer or saved as drafts instead of being lost. Prompts written in a remote project's draft also start their session on that remote instead of failing against the local daemon
- Stopping a turn no longer leaves the chat looking frozen: foreground commands show Stopping until the agent confirms they exited, follow-ups typed while the provider finishes winding down sit visibly in the queued-message card instead of vanishing into the driver, and closing terminals or restarting runtimes no longer stalls the session behind the old process's teardown

## [0.4.0]

### Features

- **Sessions**
  - Archiving a task now shows a "Task archived" toast with an Undo button that restores it — a multi-selection archives as one undoable group
  - Sessions can be swept into dormancy: hold Option while hovering a session and its pin button becomes a broom, or use "Move to Dormant" in the row's context menu (works on multi-selections too). Tasks with no activity for 7 days go dormant automatically — pinned and in-progress tasks are exempt, a task with uncommitted work keeps its worktree on disk, and the threshold is adjustable under Settings → General ("Dormant after", including a "Never" option). Under Date grouping dormant tasks collect in a collapsed "Dormant" group at the end of the sidebar; under Project grouping they stay inside their project, always below the fold — once every live session is revealed, "Show more" becomes "Show dormant". Dormant tasks stay until you archive them: any new activity, or "Restore" from the context menu, wakes them back into the list
  - Opening a task with unseen completions now shows an accent dot beside the first line of the agent's latest reply, fading out on the first scroll
  - `/side [prompt]` opens a side chat in the task's right panel — a fresh session linked to the task that can read its transcript with `goddard-agent read` and message it with `goddard-agent prompt`. Side chats live as tabs in the panel, out of the sidebar and switcher, resume across restarts, and are deleted with their parent task.
  - Selecting a task now reopens its transcript at the scroll position you left it at instead of jumping to the last prompt; tasks with unread completions or a pending question still open on their last turn.
- **Composer**
  - Typing a quote, backtick, or bracket over a text selection in the composer now wraps the selected text in the matching pair instead of replacing it
  - Collapsed text pastes now fold into the composer input as "Pasted text" chips at the paste position instead of collecting above it; pastes over 64 KB still become `paste.txt` file attachments
- **Providers**
  - ⌥Tab cycles the composer session through its favorited model+effort combos plus the most recently used selection
  - Goose is now a supported provider, driven over the Agent Client Protocol (`goose acp`). Goose sessions stream, prompt for permissions, and can be imported into Goddard like other ACP providers; the model comes from your `goose configure` setup.
  - The model picker's search field understands structured tokens — `provider:<id>` (e.g. `provider:pi`) and `effort:<id>` — which filter on the row's fields and wash the recognized value so a working token reads differently from a mistyped one; clicking a provider in the rail now toggles its `provider:` token instead of scrolling, while the favorites and recents buttons still jump to their sections
  - Every provider's model catalog now folds exploded aliases — `swe-2-high` and `swe-2-high-fast` listed next to (or instead of) `swe-2` — into one base row with a strength-ordered effort ladder and fast tier, the same fold the model picker already applied to Cursor's CLI slugs
- **Git**
  - New "Resolve land conflicts in chat" setting (Settings → General) sends the conflict-resolution prompt to the owning task's chat automatically when a land stops on rebase or merge conflicts, instead of showing the conflict dialog
  - Rank branch picker results by a blend of name fit and last-committed recency; with no search text the list now orders by most recently committed
  - Link commit hashes in agent messages: hovering shows the commit card, and clicking opens the commit's diff in the Git panel (requires the Git panel experiment)
  - Drag the divider between an open commit's diff and its file tree in the Git panel to resize the split; it adjusts the same panel width as the panel's own edge
  - The new-task sync strip now reads Pull changes when the checkout trails upstream and offers Push changes when it only leads, instead of always saying Sync changes
- **Navigation**
  - The Archived page's search now matches message text inside archived chats, not just titles and project names — rows found on content show a snippet of the matched line
  - The command palette gains a Prompts section listing every discovered prompt template: picking one with an argument hint inserts `/name ` for the usual submit flow, and any other inserts the expanded body so the prompt is visible and editable before it sends. "Save as prompt template" parks the composer draft and writes it to `~/.config/goddard/commands/<slug>.md`, and "Open prompts folder" keeps management in the markdown files that are already the source of truth
- **Permissions**
  - New tasks now default to Auto-accept edits instead of Full access — file edits apply without prompts while commands and other actions still ask first
  - Switching to Full access for the first time now shows a one-time confirmation explaining that the agent can run commands and edit files without approval prompts
- **Settings**
  - Settings → Daemon now shows the commit the app and the connected daemon were each built from, so a stale app/daemon pairing is visible at a glance
  - The settings sidebar now behaves like the chat sidebar: it shares the same width, drags wider or narrower from its right edge, and picks up the macOS sidebar transparency (vibrancy or Liquid Glass) when that setting is on.
- **Friends**
  - Friends and file transfers now find direct LAN paths through mDNS endpoint discovery instead of routing through relays, working even with the internet down
  - Manage pairings under Settings → Friends: pending requests show the requesting device with approve and decline controls, and the paired-devices list can revoke any device's token independently
- ⌘W on a terminal filling the main area kills it — with a confirmation first while a command is still running inside
- Keybindings search now filters by the keys themselves: typing a modifier in any spelling — "cmd", "command", "opt", "option", "ctrl", "control", "shift", "shft", "fn", or the ⌘⌥⌃⇧ glyphs — lists every binding that uses it, key names like "tab" or "f5" match too, and combinations such as "command p" narrow to the chords using them all.
- Discover and pair with daemons on the local network: an exposed daemon now announces itself over Bonjour and the share endpoint's encrypted link, Settings → Daemon lists nearby daemons with a Pair button, and the mobile app's daemon list shows an "On your network" section — approving a request on the host issues a revocable per-device token, so adding a machine no longer means copying an address and token by hand
- On macOS 26, the transparent sidebar renders as real Liquid Glass (`NSGlassEffectView`) lensing the desktop; older macOS keeps the existing vibrancy path unchanged. The transparency slider now drives the sidebar's translucent fill over the material — solid through frosted glass to a bare lens at 100%. Context menus and dropdowns get a Liquid Glass treatment too — a translucent card with a specular top sheen over whatever lies beneath
- Opening the right panel (⌘⌥B or the panel toggle) moves keyboard focus into the active surface — the terminal, browser, file editor, or diff file tree — so its keybindings (⌘R reload, ⌘± code zoom, find toggles) work from the first keystroke; surfaces with no focusable body focus the panel itself
- New "Sidebar draft previews" setting (General) shows a task's unsent composer draft on its own line under the sidebar row's title, drawn in the theme's alert color so an abandoned draft stands out

### Experiments

- **Friends**
  - **[Experimental]** Add project sharing between friends: pick which projects a friend can see, and when their share matches one of your repos by `origin` remote you can opt in to automatic sync — the default branch syncs out of the box, other branches are per-branch toggles, and commits you land auto-push (toggleable). A friend's push syncs immediately; a rebase conflict or dirty checkout raises an alert offering resolve-in-chat, merge instead, or abort — aborting pauses that branch until you sync it manually.
  - Attach an optional message when sending a file or folder to a friend — the receiver's chat shows the note above the delivered file as an agent-style message, inherits their last-used provider/model, and stays provider-selectable until they prompt it; the quarantined session's composer stays live: Enter on the empty composer hands the files and note to the agent while the transfer stays quarantined until trusted
  - Opt in to session sharing per shared project: a "Share sessions" toggle under each outgoing share lets a friend list that project's sessions and peek into any of them — live and read-only. Watched sessions show a banner naming the friend, never touch the friend's runtime, and disappear from view the moment sharing is revoked.
  - Move Friends — the friend-to-friend file transfer page — behind a Settings → Experiments toggle, off by default in release builds
  - Harden friend transfers against lost control messages: a missed done receipt no longer fails a completed download or hides the received file, sends dial with timeouts and stall to Failed instead of sitting at 0 B, and failures log their cause
  - Apply friend-request responses in one click — accepting mirrors the friend into the roster immediately instead of waiting for the protocol task, and repeat responds resolve as no-ops
  - The sharing surface's destructive actions now confirm before they run: unsharing a project, disabling a sync link, and aborting a stopped sync each ask first and name what they tear down
  - Add a customizable display name (what friends see) and per-friend local nicknames; completed transfers now also get a readable `Name-file` symlink under `~/Documents/Goddard/From Friends/`
- **[Experimental]** Reviewed Auto access: the Auto access mode now sends provider permission requests through the configured evaluation backend (Settings → Evaluation) instead of blanket-approving them. A `clear` verdict allows the action once — never a durable grant — while caution, an unreachable backend, or an unreadable answer escalates to the normal permission prompt, so Auto without credentials degrades to ask-the-user. Providers with their own review (Codex, Claude, fx, Droid) keep their native escalations.
- **[Experimental]** Automations: opt in under Settings → Experiments for an Automations page (⌘⇧U) that runs prompts on a trigger — hourly, daily, weekdays, weekly, a custom cron expression with an optional IANA timezone, or a webhook URL that fires a run on each POST — running each into a fresh task, a new worktree, or an existing one, with a run history that links every run back to its task. Deleting an automation confirms first — it names the automation and warns that its run history goes with it.
- **[Experimental]** MCP integrations: a new Settings → Integrations page (Experiments opt-in) connects official remote MCP servers — Linear, GitHub, Notion, Stripe, Figma, Supabase, and more — to the agents you choose. Goddard owns sign-in (OAuth or API key, credentials in the keychain) and serves every agent through its local MCP proxy, so providers never see upstream credentials. Connect once, any provider. Disconnecting an integration confirms first, naming the integration it signs out.
- **[Experimental]** Auto model routing gains a dedicated Settings → Jev page for the backend credentials and routing targets — reachable from the Auto row's gear button, which shows a warning badge while the selected backend has no API key. Each task class (Routine/General/Demanding) maps to a user-picked provider, model, and reasoning effort, and Jev can suggest mappings from recent usage. The routed model locks in at session start while Jev reevaluates the reasoning effort on every turn, changing it only when confident
- **[Experimental]** Project map: opt in under Settings → Experiments to give every new session a compact structural map of its workspace — the top-level symbols of the most-referenced files, budgeted near 1,000 tokens and prepended to the first prompt — so the agent skips cold repo exploration. A composer chip tracks indexing, and the sent map lands as a collapsible transcript row.
- **[Experimental]** Project memory: each project keeps a `.goddard/memory/` store (a curated `MEMORY.md` plus an append-only `LOG.txt`) that the daemon maintains automatically — finished sessions are distilled into durable notes in the background, and the store is injected into every new session's first prompt so agents start with prior decisions, preferences, and failed approaches instead of zero context. The store is excluded from git via `.git/info/exclude`, never appears in the transcript, and can be inspected or edited directly.
- **[Experimental]** QA review: proposed work lands on `qa` unreviewed and promotes to the base branch through the longest approved prefix. The Projects page gains a Review tab listing the queue oldest-first — `Test-Plan:` trailers and sensitive paths mark commits that need a human while everything else auto-approves — with per-commit approve, reject-and-revert, and a promote control for the approved frontier. Decisions live in `refs/notes/qa` and sync to friends over the existing share path. Reject-and-revert confirms first, naming the commit it's about to revert on the shared branch.
- **[Experimental]** Sidebar dock: opt in under Settings → Experiments, then hover anywhere along the sidebar's bottom strip to raise a quick-action dock — Inbox, Archived chats, Shortcuts, and Settings (plus Friends when its experiment is on) as round buttons that slide up from below the window's bottom edge to overlap the strip, magnify toward the pointer like the macOS Dock, slide back down when the pointer leaves, and stay raised while the pointer is on them even when a button opens Settings
- **[Experimental]** Turn status markers: opt in under Settings → Experiments to have the evaluation model score each finished assistant turn against a fixed marker set — complete, awaiting input, unverified, partial, blocked, failed, drifted — and show the markers that clear their threshold as chips with a confidence percentage under the response. Evaluations run only for the session on screen; turns that end off screen are scored when you open it. Requires a configured evaluation backend, shared with Auto model routing.

### Fixed

- **Sidebar**
  - Archiving a task mid-⌘⇧D sweep no longer lands selection back on a task the sweep just marked unread; the departure falls through to the next genuinely unread or idle session until another navigation ends the sweep
  - Holding ⌘ no longer shows the ⌘1–⌘9 row chips while a sidebar multi-selection exists, so the hints don't advertise a single-task jump over an active batch
  - Holding ⌘ for the ⌘1–9 sidebar chips no longer hides a session row's status icon, pin/archive controls, worktree and PR badges, or timestamp — the chip is now a pure overlay and the row underneath stays put
  - Folding the sidebar's Terminals group now returns to where you were — the fold skips every intermediate terminal in the back stack instead of landing on the previously viewed terminal
- **Providers**
  - Fix ACP tasks losing their conversation after Move to Worktree: a failed `session/load` — e.g. while the replaced runtime still held the provider's session lock — silently started a fresh provider session and overwrote the resume cursor. Failed resumes now retry transient errors briefly and surface the failure instead of forking onto an empty session
  - Antigravity's model list folds `base-effort` spellings like `gemini-3.1-pro-high` into one model row with an effort picker and drops the parenthesized effort from the label ("Gemini 3.1 Pro"), while traitless suffixes like `claude-opus-4-6-thinking` keep their full id instead of resolving to a base the CLI rejects
  - Selecting a Devin model no longer fails with "did not advertise model": the picker stores the folded base id with effort and fast as separate traits while Devin's ACP session advertises packed uids, and the stored traits are now repacked into candidate ids before matching
  - The model picker's rail no longer shows buttons for sections with no rows — other providers during a locked session, providers whose combos are all favorites or recents, and favorites/recents jumps whose stored entries no longer resolve to picker rows
  - OpenCode sessions no longer fail to start when the session server is slow to boot — its startup budget is now 30 seconds, up from 10
  - Detect provider CLIs installed after the daemon started and ones on PATH only via interactive shell config: provider refresh re-captures the login-shell environment, and a missed binary falls back to a single `command -v` resolution across all providers
  - Stop now interrupts an in-flight Computer Use `js` call on every provider: Goddard drops a `cancel-kernel` marker into the session's process directory that the QuickJS kernel polls, since its synchronous serve loop can never see an MCP `notifications/cancelled`. Stopping a Codex turn also keeps the app-server resident like every other provider instead of ending the process
  - Fixed Vercel AI Gateway evaluation calls failing with HTTP 400: `noul` questions are now translated to the spec's `boolean` type (and back on answers), the answering model id is read from gateway routing metadata, and the request no longer forces Zero Data Retention, which requires a Pro or Enterprise plan.
- **Git**
  - Re-picking a draft's base branch no longer runs `git switch` or disables branches owned by other worktrees — the draft's detached worktree is re-pointed in place, so every branch stays selectable
  - Archiving a chat no longer deletes its worktree while another unarchived chat shares it; the directory is removed once the last sibling chat is archived.
  - Syncing a checkout from the new task strip can no longer open a Git editor in the terminal tab — the pull now runs with `core.editor` and `sequence.editor` stubbed out
- **Panels**
  - Clicking a file row in a changed-files card now opens that file in the Review panel's turn diff instead of the file viewer, matching the Review button's destination
  - Keep the sidebar visible when the right panel is maximized — the layer now covers only the session column, and toggling the sidebar resizes the layer to match
- **Appearance**
  - In High contrast mode, the chevron carets on select-style controls and the disclosure chevrons on expandable rows (settings sections, transcript folds, menus) now render one text tier brighter so the affordance stays legible.
  - Browsing themes in the Appearance selectors no longer flashes back to the current theme a moment after each preview while "Match system appearance" is on
- The composer's access-mode rows now read but can't be picked while a turn is running — previously switching the mode mid-turn restarted the driver and silently cancelled the turn
- Fixed `~/Documents/Goddard/From Friends` symlinks pointing at the `transfers/<uuid>` folder instead of the received file itself.
- Shrunk the installer back to its old size — the Copilot SDK's default `bundled-cli` feature was embedding the entire ~118 MB Copilot CLI archive in the daemon even though sessions always launch the installed `copilot` binary, which had grown the download from ~35 MB to ~176 MB
- The Keybinding Manager no longer reports permanent unresolvable conflicts for declared fall-through pairs like ⌘N/⌘⇧N — bindings whose winner propagates the keystroke to the next command when its runtime gate fails
- Connecting to a remote host over SSH works again: the control socket option was passed in a form `ssh` rejected outright, the control master was started in confirmation mode so every later command and port forward was denied, and a host with no published daemon build failed right after the bundled daemon was uploaded.

## [0.3.0]

### Features

- **Sessions**
  - New "After archiving a task" setting (Settings → General) picks where selection lands: the next unread completion (default), the next non-busy session in sidebar order, or a fresh task composer
  - Archived chats now show a green checkmark and "Landed" label when their work was landed onto the base branch with `/land`
  - Add a "Mark all tasks as read" command-palette action that clears every unseen-completion dot at once
  - The "Task unarchived" toast now advertises ⌘⌥O — pressing it jumps straight to the restored task
- **Sidebar**
  - Collapsed sidebar groups now show the same unread dot sessions and terminals use when any hidden row has an unseen completion
  - Sidebar project groups now cap at seven chats; the "Show more" row reveals the rest in batches of thirty, so an active project can't grow its section without bound
  - Select multiple sidebar tasks with ⌘-click (⌘⇧-click extends the range) and act on them together — row menus and task shortcuts like pin, mark unread, archive, and copy working directory apply to the whole selection
- **Providers**
  - Add Antigravity as a provider: its sessions run the `agy` CLI's own TUI in a task-scoped terminal — the first prompt launches `agy -i`, reselecting a started session resumes with `agy --conversation`, and status and title come from agy's own store. The composer steps aside once the session starts so keystrokes belong to the TUI
  - Model picker is now one virtualized, filterable list with a jump rail instead of provider tabs: starred favorites lead (drag to reorder, ⌘⌥1–⌘⌥9 to apply), then recently used combos, then every provider's models — each row enumerates a model/effort/tier selection, with the provider mark on a second line and effort and fast mode drawn dimmer beside the name. Rail buttons clear the filter and scroll to that section, ⌘E / ⌘⇧E cycle reasoning effort for the current model in either direction, and packed CLI aliases (e.g. Cursor's `-high-fast` slugs) fold into their base model's effort and tier options
- **Git**
  - Landing a task now records a "Landed on `<base>`" card in its transcript listing the commits that landed — each SHA opens the commit diff — so where the work went stays visible in the session's record.
  - ⌘S now opens a "Sync branch…" picker outside the file editor (where ⌘S still saves). It lists the repository's checkouts that track an upstream — defaulting to the current branch in a session — and pulls with `git pull --rebase` (or merge, per the sync setting). Conflicts reuse the sync-conflict dialog, whose "Resolve in chat" now starts a new chat on the folder being synced. A new "Resolve sync conflicts in a chat" setting skips that dialog entirely: a pull or land that stops on conflicts starts a fresh chat on the checkout with the resolution steps already sent
- **Transcript**
  - Copying a selection from an agent message now puts markdown on the clipboard — `**bold**`, `` `code` ``, links, headings, list markers, quotes, and fenced code blocks — instead of flattened rendered text
  - Tables can be resized by dragging a column boundary (or focusing it and using the arrow keys): markdown tables in the transcript, the Keybinding Manager, the Projects page's Worktrees and Branches lists, and the Usage breakdown tables
- **Terminals**
  - Rename sidebar terminals inline — double-click the title or pick Rename from the row's menu, just like sessions
  - Closing the terminal you're viewing now selects a neighbor terminal instead of leaving a dead pane
- **Keyboard**
  - The Keybinding Manager's table now edits bindings in a capture modal with one slot per command, ordered by surface: the on-screen keyboard wears platform glyphs, a tooltipped conflict count cycles through the offending commands, and only true conflicts are marked
  - Menus support the native press-drag-release gesture: hold a menu trigger (or right-click for a context menu), drag onto an item, and release to pick it — releasing over nothing dismisses
- **Appearance**
  - New "Border intensity" slider (Settings → Appearance) fades borders and separators from invisible up to their original contrast — the default lands fainter than before
  - On macOS 26, transparent chrome renders as real Liquid Glass (`NSGlassEffectView`): the sidebar and the chat composer card float on glass that lenses the desktop, tinted by the active theme. Older macOS keeps the existing vibrancy path unchanged.
  - Sidebar transparency gains an Amount slider (Settings → Appearance): dial how much of the desktop shows through the sidebar, 0–60%. The default moves from a barely-visible tint to 25%.
  - The Appearance settings gain a collapsible Preview row showing a miniature transcript — user bubble, assistant reply, and syntax-highlighted code — that opens automatically while a theme selector is open, so palette options can be compared on real content
- ⌘⇧. flips a draft's environment between this Mac and the sandbox VM — the Environment section of the ⌘. mode menu, without opening it
- The file viewer's image preview now pans on scroll and zooms around the cursor with ⌘-scroll
- "New task in…" (⌘⇧N, also in the File menu and command palette) fuzzy-searches directories on disk and starts a task in the pick — the directory becomes a temporary project that appears in the sidebar with a clock-folder icon and leaves the project list once its last task is removed; workspace toggle moves to ⌘⌥N and the branch picker to ⌘⇧⌥N
- Received-file sessions now default to the Sandbox VM environment, so once a transfer is trusted the agent still runs isolated from this Mac
- “Create draft” parks the composer’s contents — text, attachments, and annotations — as a saved draft tied to its chat or project; a count button beside the composer’s access control opens the new Drafts page, where drafts can be searched, edited inline, hidden, deleted, or dropped back into their composer (⌘Z restores a used draft)
- Settings search now matches setting titles and descriptions across all pages: every section with a match renders in one scrollable list with the matching text highlighted, and the sidebar filters to those sections (clicking one scrolls to it)

### Experiments

- **Git**
  - **[Experimental]** The GitHub integration moves behind a Settings → Experiments toggle — the inbox sidebar row, ⌘⇧I page, and notification polling all switch off until it's enabled — and gains two new surfaces: a "New GitHub Issue" command-palette action that scans `.github/ISSUE_TEMPLATE` and opens a native form for Markdown templates and blank issues (YAML forms hand off to the web), and `#`-completion in the composer that resolves issues and pull requests on the workspace's GitHub remote, drawing them as chips in the transcript
  - **[Experimental]** The Git panel's top region — the commit box, or an open commit's file tree — is now split from the commit log by a draggable divider whose position persists, so opening a commit no longer shifts the layout
  - **[Experimental]** New Projects page (⌘⇧P, or the sidebar row beneath Search): each project gets Worktrees, Branches, Issues, and Pull Requests tabs (⌘⌥1–4) backed by the local repo and `gh`. Remote-tracking branches group under collapsible remotes that fetch on expand, rows follow macOS multi-selection with right-click menus and a bulk action bar, and a docked composer chips the current project, tab, filter, and selection
- **Friends**
  - **[Experimental]** Move Friends — the friend-to-friend file transfer page — behind a Settings → Experiments toggle, off by default in release builds
  - Harden friend transfers against lost control messages: a missed done receipt no longer fails a completed download or hides the received file, sends dial with timeouts and stall to Failed instead of sitting at 0 B, and failures log their cause
  - Apply friend-request responses in one click — accepting mirrors the friend into the roster immediately instead of waiting for the protocol task, and repeat responds resolve as no-ops
  - Add a customizable display name (what friends see) and per-friend local nicknames; completed transfers now also get a readable `Name-file` symlink under `~/Documents/Goddard/From Friends/`
- **[Experimental]** Move Computer Use behind an Experiments opt-in: turning the experiment on reveals the Computer Use settings page and lets sessions drive the bundled Cua Driver helper, including in release builds
- **[Experimental]** Auto model routing: opt in under Settings → Experiments to add an Auto entry to the model picker. A task's first prompt is classified by the evaluation model and routed to a provider and model from your routing policy (~/.goddard/route-policy.json), with BYOK evaluation backends for TypeSafe, Vercel AI Gateway, and Cloudflare Workers AI and routine/general/demanding class-level target pickers. Class targets default to session-scoped tiers (`session:tier:*`), which upgrade or downgrade within the provider you already picked; global `tier:*` targets resolve through the policy's preferred-provider order instead.

### Fixed

- **Composer**
  - Large pastes collapse less eagerly (20 lines or 4 KB, up from 4 lines or 500 bytes), and collapsed pastes now show as a compact "Pasted text" chip that previews the leading characters on hover.
  - ⌘Enter now steers a composer draft that only contains comment annotations, instead of doing nothing
- **Providers**
  - On Linux, provider sessions no longer die at launch: the daemon's process guardian handed spawned CLIs `/dev/null` for stdin because dash assigns it before applying `<&0`, so stdio-based providers exited immediately — the child now inherits stdin through a saved descriptor
  - Stopping a turn now stops its live background work at the provider too — a retained runtime no longer keeps running while the transcript claims it halted — and a steer sent just before Stop can no longer acknowledge into the settled session afterward
- **Terminals**
  - A global terminal now remembers its own right panel — switching away from a task no longer leaves the task's panel open over the terminal, and the panel's tabs and visibility return as they were left
  - A sidebar terminal no longer resets into the selected task's worktree after a `cd` — session terminals only respawn when the workspace itself moves, and terminals opened at a chosen directory keep it
- `/land` no longer dead-ends on a plain local checkout — the command palette's "Land changes" only appears for worktree sessions, and running `/land` where it can't apply explains why instead of erroring
- ⌘⇧N now reliably cycles the ⌘N project switcher backward — a fast press can no longer slip through to the draft's workspace toggle, and the chord opens the switcher in reverse on a draft too
- The sidebar peek no longer dismisses when archiving, pinning, or renaming a session from the hover-revealed sidebar

## [0.2.1]

### Features

- **Providers**
  - Add a real compact command: the daemon routes compaction through the provider instead of faking it with a prompt, and Codex and OpenCode 2 show context compaction as its own activity card
  - Add GitHub Copilot as a provider via the official Rust SDK, with rewind and branch through `sessions.fork` and composer attachments sent as structured attachments
- **Git**
  - The Projects page's docked composer gains the chat footer's project, worktree, and branch pickers and a working model selector, so a task is fully configured before it is created
  - New opt-in setting fast-forwards the local default branch — plus any extra branches you list — to its tracking branch before a new worktree is based on it
- **Panels**
  - Preview images and SVGs in the file viewer — refreshed from just-saved bytes — and jump to a line with ctrl-g
  - Open @-mention file references from user prompts in the right panel
- **Terminals**
  - Rebind ⌘T to always open a new terminal rooted in the current context — the on-screen terminal's directory, the selected session's workspace, or ~ — moving the Terminals chord to ⌘⇧T and leaving Toggle Workspace reachable from the menu, command palette, and Work-in chip; ⌘⇧N now toggles the workspace on the new task page
  - Sidebar terminal rows no longer show a checkmark when a command exits cleanly; a terminal that finishes while off-screen now gets an unread dot that clears when the terminal is next focused
- **Navigation**
  - Jump to file:line[:column] straight from the ⌘P finder
  - Retarget ⌘D as "Go to next unread completion": it always picks the topmost unread row, cycles idle sessions once the unread queue drains, skips sessions with queued prompts, and ⌘⇧D is now a positional sweep instead of a next-unread jump
- **Appearance**
  - New "High contrast borders" setting (Appearance) widens border-tier contrast so control outlines meet WCAG's 3:1 non-text floor; it turns on automatically when the OS Increase Contrast accessibility setting is enabled
  - Borders, separators, and outlines are softer by default — hairline rules sit at 1.4:1 and filled surfaces like the composer, cards, menus, and tooltips use a new subtler outline tier
- Archiving a task whose turn is still running now asks for confirmation first instead of silently stopping the turn
- Add a Settings → Friends page for friend-to-friend file transfers: probe-based presence, incoming requests you accept or deny, live byte progress with a sidebar footer ring, and received sessions materialized under a synthetic Friends project behind an explicit trust quarantine
- Rename the product to Goddard and migrate legacy Waku state
- Add a keybinding manager page — a searchable command catalog over a keyboard stage — with a capture editor for live rebinding, conflict surfacing gated behind an explicit confirm, a manual keyboard-layout picker, and overrides persisted to keybindings.json and applied at startup
- Connect to remote hosts over SSH — managed from daemon settings, badged in the sidebar, with masked-password askpass prompts and port forwarding — so their sessions, skills, and usage appear alongside local tasks, and the daemon installs and upgrades itself on the host
- Add a sandbox environment choice to the composer's access menu
- Move worktree and branch settings into a dedicated Settings → Git page
- Cap each sidebar project group at sixteen chats; a "Show more" row reveals the rest in batches instead of folding only chats older than three days

### Experiments

- **[Experimental]** The Projects page (⌘⇧P) — a project's worktrees, branches, issues, and pull requests in one place — moves behind an Experiments opt-in and is now off by default

### Fixed

- **Sidebar**
  - Use the session's display title when dragging it in the sidebar
  - Keep worktree badges, branch labels, and checkout status on sidebar tasks across restarts instead of waiting for each task to be resumed
- **Git**
  - Detect rebase state directories in the git panel instead of relying on a stale REBASE_HEAD
  - `/land` shows a spinner toast while it runs and resolves it to the outcome, instead of giving no feedback until the land finished
  - "Resolve in chat" in the git panel conflict modal now pastes the resolution prompt into the composer instead of sending it, so you can review or edit it before sending
- **Transcript**
  - Round markdown table header corners to match the frame
  - Render backticks glued to shortcut keycaps as literal keycaps instead of breaking inline code
- **Keyboard**
  - Git panel confirmation modals, agent permission prompts, and computer-use approvals are now keyboard-operable — Tab moves between options, Enter or Space activates the focused one, Enter on a modal card runs its primary action, and Escape cancels or denies
  - Restore the ⌘N hint on New Task surfaces
  - Drop the unbound Toggle Workspace row from the shortcuts dialog
- **Appearance**
  - Diff highlights and the scrollbar in the file-diff hover cards no longer paint past the card's rounded bottom corners
  - Paint the sidebar solid when macOS Reduce Transparency is on, and default sidebar transparency off on setups where no backdrop blur exists
- Edit the annotation under an ⌥-click instead of stacking a new one on top
- Computer Use no longer fails with "socket path is too long": its bridge socket now lives in a private directory under `/tmp`, short enough for macOS's 104-byte `sun_path` limit regardless of `TMPDIR`

## [0.2.0]

### Features

- **Sessions**
  - Continue an interrupted session by typing in its empty composer
  - Play a sound when a background task finishes its turn, with a volume slider and in-selector previews, and show an unseen-completion bell in the top bar
  - Recover renamed or moved project folders instead of losing them, and archive projectless workspaces automatically
- **Sidebar**
  - Peek at the closed sidebar by hovering the left window edge
  - Pin, archive, and mark sessions unread from the sidebar, drag sessions into the composer as reference chips, and jump between tasks with ⌘1–9, ⌘D, and ⌘⇧D
- **Composer**
  - Annotate transcript lines and file-editor selections with comments that fold into the next prompt (⌥-click a line, ⌘L on a selection); "Annotation N" references resolve to hover citations
  - Highlight and auto-continue Markdown lists in the composer, collapse large pastes into expandable cards, and accept file drops anywhere in the session
- **Git**
  - Add a Projects page listing worktrees, branches, issues, and pull requests
  - Create named adjective-noun worktrees beside the repository, move a session into a new worktree, and remove an archived task's worktree behind a snapshot ref
- **Terminals**
  - Run project scripts from a ⌘R picker and generate terminal commands with the session's agent
  - Add a Terminals group to the sidebar showing live command status, working directory, and last-activity time; ⌘J focuses the session terminal
- **Navigation**
  - Add user-defined custom commands to the command palette — with custom icons and toast notifications — and let agents manage them through daemon settings
  - Open workspace files in the right panel with a ⌘P file finder and preview Markdown files fullscreen
  - Reshape the ⌃⇥ task switcher into a compact recently-viewed list with session status glyphs
- **Appearance**
  - Add UI and code font family pickers and a separate terminal font size to Appearance settings; right-panel cards adapt cleanly when the UI font size is increased
  - New settings: thick borders, sidebar transparency, a Markdown preview toggle, and an opt-in response token speed readout
  - Add 15 new themes (Dracula, Rosé Pine, Kansō, Gruvbox, GitHub, and more), split the theme preference into separate light and dark slots with a system-following toggle, and preview themes while browsing the selectors
- Add an Experiments settings page where unfinished features — Big Picture, the Git panel, GitHub integration, and subagents — can be turned on individually; all default off
- Hide the app with Cmd+H on macOS
- Add Devin and Droid (Factory) as agent providers
- Surface keyboard shortcuts in menus, tooltips, the command palette, a hold-⌘ overlay, and a cheatsheet next to the sidebar settings icon
- Syntax-highlight Lua, PHP, Zig, Dart, Elixir, and Astro code blocks

### Experiments

- **Git**
  - **[Experimental]** Add a Git panel (⌘⌥G) with a commit graph, expandable diffs, upstream tracking, and a /land command to land a worktree on its base
  - **[Experimental]** Browse a project's GitHub issues and pull requests, start tasks from them, and track check and review status on sidebar rows and in the right panel
- **[Experimental]** Add Big Picture mode on ⌘0 — a full-window grid of session cards with live transcripts, per-card composers, and ⌘1–9 arming
- **[Experimental]** Delegate runs to named subagents with per-provider tiers and cost-labeled routing, shown legibly in the transcript

### Fixed

- **Providers**
  - Keep Amp threads resumable after restarting the app
  - Fix Cursor model discovery and model options
  - Apply OpenCode model and effort changes to the live session instead of the next one
- **Git**
  - Prefer exact matches in the branch selector's filter
  - Stop warning about unpushed commits that are already merged into a branch
- **Transcript**
  - Fix escaped backticks rendering literally inside inline code
  - Stop rendering blank reasoning-only lines in the transcript
  - Fix remote images failing to load in transcripts
  - Fix the transcript segment left behind when steering an in-flight reply
- **Platform**
  - Fix Waku→Goddard migration staging a full copy of `~/.waku` on every launch: workspaces, worktrees, and archives are now linked item-by-item instead of copied, the state database is cloned only when its schema is known, and abandoned `.migrating-*` staging directories are swept on launch
  - Fix startup on Macs without Xcode installed
- Fix the IME candidate popup appearing in the wrong position
- Stop a quick ⌃⇥ chord from flashing the task switcher
- Fix interrupted session saves erasing stored workspace details

## [0.1.19]

- Render inline and block LaTeX math in Markdown, with a Copy Expression action and a setting to show the source
- Fix OpenCode 2 context usage to reflect the latest request instead of cumulative session totals, and cap the meter at 100%
- Clear model search when selecting a provider or Favorites tab
- Make loading spinners smoother with animations at up to 60 FPS

## [0.1.18]

- Fix Codex session forking
- Add OpenCode 2 support, add reasoning effort option for both OpenCode and OpenCode 2
- Fix memory usage for long-running sessions

## [0.1.17]

- Fix the OpenCode Resume list showing only sessions started outside a git checkout; it now lists sessions from every project
- Hold Claude's turn open while it waits on background work

## [0.1.16]

- Import and continue conversations started in agent CLIs with `/resume` or the command palette across every provider, in both Goddard and Goddard Web
- Linux: add signed in-app updates with clean relaunch and automatic rollback
- Copy Goddard task IDs and agent CLI thread IDs from task info or the command palette
- Keep each response's actions and changed-file summary after its final tool activity
- Keep the selected task visible when navigating the sidebar
- Let nested transcript and command-output scrollers hand wheel gestures to the page only at their boundaries
- Keep multiline background-work titles on one line in summaries and panel headers

## [0.1.15]

- Codex thread goals: type /goal to set a persistent objective the task keeps pursuing — before or after the first message — with its autonomous pursuit streaming into the transcript, a status chip showing live budget or elapsed time, and a dialog to edit, pause, resume, or clear the goal (also in Goddard Web)
- Discover provider-native slash commands and skills from installed agent CLIs, including multiline YAML descriptions
- Add reasoning effort selection for Grok
- Reconnect remote daemon sessions automatically after connection interruptions
- Fix Command/Ctrl+Enter steering after a provider response starts streaming
- Fix transcript file links on Windows
- Fix OpenCode dropping the first streamed event and hanging during cancellation on Windows

## [0.1.14]

- Group sidebar tasks by project or update date, order them newest or oldest first, and collapse sections
- Find in page: Search the full transcript by keywords using cmd-f or ctrl-f
- Switch between recent tasks with Ctrl+Tab and Ctrl+Shift+Tab
- Carry the current access mode into new tasks and remember it between launches
- Fix OpenCode access-mode permissions and restore pending permission prompts when resuming sessions
- Show Codex file reads, listings, and searches as file activity instead of raw commands
- Keep long panel and background-work titles on one truncated line
- Increase the minimum UI text size for better legibility

## [0.1.13]

- Add Vercel Fx support
- Support DeepSeek Harness 0.1.1 without opening its web UI
- Collapse earlier activity groups when a running turn moves on to newer transcript output

## [0.1.12]

- Invoke Codex, Pi, and Oh My Pi skills with their native syntax
- Stream live output from Claude background tasks
- Steer the oldest queued follow-up with Command/Ctrl+Enter in an empty composer
- Fix model and reasoning option selection for Cursor
- Fix npm-installed provider detection on Windows
- Fix daemon terminal sessions hanging during shutdown
- Exclude copied history from forked Codex sessions from usage totals
- Keep separate Codex reasoning sections on separate lines

## [0.1.11]

- Highlight Markdown in the file editor, and toggle between source and a rendered preview
- Add UI and code font size settings
- macOS: Add "Open in.." button to open project folder in selected application

## [0.1.10]

- Add Kimi Code support
- Add Oh My Pi support
- Fix markdown table rendering

## [0.1.8]

- Fix `PATH` resolution on Windows

## [0.1.4]

- Fix text selection in diff view

## [0.1.3]

- Pin Codex and Claude commit message generation to cheap models: gpt-5.6-luna and claude-4.5-haiku
- Animate sidebars
- Render provider file edits as inline diffs in the transcript
- Fix claude task title generation

## [0.1.2]

- Fix regression: user bubble should fit its content width

## [0.1.1]

- Give nested Markdown the full message width
- Cap composer height and scroll overflow with an overlay scrollbar
- Keep drag-selecting text past the input bounds
- Fix char boundary panic when sliding the live reasoning window

## [0.1.0]

- Add standalone Goddard daemon and browser client
- Add Linux support (X11 and Wayland, you need to build from source for now)
- Answer agent questions directly in the composer
- Redesign queued follow-ups as composer cards with per-message steering
- Add DeepSeek agent preset selection (Standard, Code, Minimal, and Creator)
- Add Claude context window and ultracode effort options
- Add /fast command to toggle fast mode for Codex
- Show the latest activity in live transcript headers
- Add soft wrapping and keyboard copy feedback
- Add terminal overlay scrollbar and measure cell width from the font
- Restore window position, size, and display across launches
- Contain wheel scrolling in activity and command output viewports
- Smooth streaming markdown and reduce CPU usage while streaming

## [0.0.13]

- Add DeepSeek Harness provider
- Render user message as Markdown and linkify bare URLs
- Share one resident OpenCode serve per workspace across sessions

## [0.0.12]

- Inherit the login-shell environment for provider commands
- Fix model traits across provider switches
- Keep branch change counts current and include untracked files
- Normalize SIGCHLD for provider children
- Fix Grok model discovery

## [0.0.11]

- Fix provider detection for CLIs installed through shell PATH managers such as
  nvm and fnm
- Show models registered by Pi extensions
- Fix the model picker closing when entering a space in search
- Fix duplicate transcript history when resuming ACP sessions

## [0.0.10]

- Fix crash in due to IME composition
- Fix typo

## [0.0.9]

- Add OpenCode Go support in usage popover
- Fix app icon
- Fix Cursor model detection

## [0.0.8]

- Initial release
