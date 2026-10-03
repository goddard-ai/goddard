# Goddard user guide

Use this guide to choose an agent, start tasks, give feedback, and review
changes. For installation, see the [README](README.md#install),
[Linux guide](docs/linux.md), or [Windows guide](docs/windows.md).

## What is Goddard?

Goddard is a fast, native desktop app for working with local coding agents.
Instead of living inside one agent's terminal UI, you run **Claude Code, Codex,
Cursor, OpenCode, and other agent CLIs** side by side from one interface —
each task gets a real transcript, a diff view, a terminal, a file browser, and
Git tooling, while the agent CLI does the actual work underneath.

It is written in Rust on top of [GPUI](https://gpui.rs/)
(the framework that powers the Zed editor). GPU rendering and virtualized
transcripts help keep the interface responsive without a browser engine
rendering the desktop interface.

**Goddard is not an agent.** It does not have its own model or subscription.
It drives the agent CLIs you already have installed and authenticated, over
each provider's native session protocol — so your existing Claude, ChatGPT,
Cursor, or other agent subscription is what you pay with.

### Quick facts

| Question | Answer |
| --- | --- |
| Price | Free, open source (GPL v3) |
| Account required | No Goddard account. You sign in through each agent's own CLI |
| Platforms | macOS (signed `.dmg`), Linux (install script, Wayland + X11), Windows (per-user installer or portable zip) |
| Where is my data | Locally on your machine — no Goddard cloud service |
| Languages | English, Japanese, Simplified Chinese |
| Updates | Automatic, cryptographically signed, with rollback on Linux |
| Community | GitHub issues and Discord (both linked from the site menu) |

## Is Goddard right for you?

**Goddard is a good fit if you:**

- Run coding agents daily and juggle more than one task or project at a time.
- Use more than one agent CLI and want a single consistent interface — one
  model picker, one permission system, one transcript — instead of learning
  each CLI's TUI.
- Want to queue follow-up messages while an agent writes its reply, or steer
  its current turn during thinking, tool use, or background work.
- Want conversation-aware rewind and branching that also rolls back the Git
  working tree.
- Care about keyboard-driven workflows, native performance, and a UI that
  doesn't drop frames on a 120 Hz monitor.
- Want your code, sessions, and transcripts to stay on your machine.

**It may not fit if you:**

- Don't already use a coding-agent CLI — Goddard needs at least one installed.
- Need a hosted/cloud agent that runs while your laptop is closed (Goddard
  runs agents on your machine or a daemon host you control).
- Rely on a specific agent's IDE extension features (inline completions in an
  editor, etc.) — Goddard is a task-and-transcript app, not a code editor.

## Getting started

### Install

- **macOS:** download the `.dmg` from the
  [latest GitHub release](https://github.com/goddard-ai/goddard/releases/latest),
  open it, and drag Goddard to Applications.
- **Linux:** `curl -fsSL https://raw.githubusercontent.com/goddard-ai/goddard/main/install.sh | sh` — installs into
  `~/.local` without root, adds an applications-menu entry, and keeps itself
  updated. Requires glibc 2.35+ (Ubuntu 22.04, Debian 12, Fedora 36 or newer),
  a working Vulkan or OpenGL driver, and x86_64 or aarch64.
- **Windows:** run `Goddard-<version>-<arch>-Setup.exe` from the latest
  release, or unpack the portable `.zip`. Per-user install, no admin rights
  needed. Requires Windows 10 1809+, Direct3D 11 (feature level 11_0), x86_64
  or aarch64.

### Set up a provider

Goddard detects installed agent CLIs automatically — including ones installed
through PATH managers like nvm, fnm, bun, cargo, or scoop. Install and sign in
with the agent's own CLI first; **Settings → Providers** shows what was
detected, lets you enable or disable each provider for new tasks, and offers a
manual binary path if detection missed. Each provider row also has setup help
(install command, sign-in command, docs link) that can run in a terminal tab
right in the app.

## Supported agents

Goddard supports Claude Code, Codex, Cursor, Devin, OpenCode, and other agent
CLIs. **Settings → Providers** shows the providers available in your build,
with installation and sign-in help.

Each provider has its own model catalog and capabilities. Steering, interactive
approvals, rewind, and fork are available where the provider supports them.
Some providers offer only full access, so check the task's access controls
before starting work.

### Resuming sessions from the terminal

Conversations you started directly in an agent CLI are not stranded. The
command palette's **Resume…** (or the `/resume` slash command) lists sessions
each provider has on disk and imports them into Goddard with full session
continuity where the provider supports session import.

### Switching providers within a task

Switch providers between turns to continue the same task with another agent.
For long conversations, Goddard gives the new agent a compact history index
so it can read the turns it needs on demand. Returning to an earlier provider
can resume its previous session with the intervening context.

## Core concepts

### Projects

A project is a folder on your machine (or on the daemon host, for remote
setups). You open a project once, and every task inside it shares that folder.
You can also start a task **without a project** — Goddard creates a scratch
workspace under `~/.goddard/projects/<date>/<slug>`.

### Tasks (sessions)

A task is one agent conversation bound to a workspace. Tasks are independent:
each has its own provider, model, access mode, transcript, working directory,
terminals, and Git state. A task keeps streaming while you look at another
one — nothing pauses when you switch.

Task titles come from the provider itself wherever possible (every agent but
Codex already names its own sessions; Goddard generates one for Codex on a
cheap model). Until a title lands, the first few words of your prompt act as a
placeholder. You can always rename a task inline — a name you type is never
overwritten.

Tasks can be **pinned**, **marked unread**, **archived**, or **deleted** from
the sidebar. Archiving a task that sits in a worktree snapshots the checkout
first; unarchiving restores it. A projectless task's workspace is zipped into
`~/.goddard/archives/<date>/<slug>.zip` and removed the same way. Archived chats
are hidden from the sidebar and search, browsable in Settings → Archived, and
permanently removed after 30 days.

### Worktrees

When creating a task you choose where it works: the project's ordinary
checkout (**Local**) or a **new Git worktree** created from the default branch
or any ref. Worktree tasks get isolated working directories — named after
random adjective-noun pairs and placed beside the repository — so parallel
tasks never collide over the same files. Goddard remembers your last
workspace mode and worktree branch per project, warns in the new-task area
when a checkout trails its upstream (with a one-click sync), and recreates a
worktree that went missing on submit. An existing local task can be **moved
to a worktree** later, carrying its uncommitted state with it — and the agent
is told its working directory changed.

### The window layout

- **Sidebar (left):** projects and tasks, grouped by project or date, ordered
  by last-updated or last-created, with collapsible sections. Task rows show
  live status (working, waiting for input, waiting for background tasks,
  failed, unread), dirty/unpushed Git markers, a worktree icon when the task
  runs in one, linked pull-request state with check/review status, and
  shortcut tags (⌘1–⌘9) when the modifier is held. Hover a row to archive;
  double-click its title to rename.
- **Transcript (center):** the conversation — your messages, agent replies
  rendered as Markdown, and every tool call as a normalized activity row
  (commands, file edits, reads, searches, plans, web browsing, subagents).
- **Composer (bottom):** where you type prompts.
- **Right panel:** tabbed surfaces — Review (diff), Files, Terminal, Browser —
  plus panels for background work and environment info. Any tab can maximize
  to fill the window, and Markdown files get a fullscreen reading mode.
- An **unseen-completion bell** in the top bar collects finished turns you
  haven't looked at yet.

## Working with agents

### The composer

- **Send** with Enter; newline with Shift+Enter.
- **Queue a follow-up** while the agent is working — Enter or Cmd/Ctrl+Enter
  both park the message as a card above the composer until the turn settles.
- **Steer a waiting turn** with Cmd/Ctrl+Enter while the task shows the
  hourglass: the reply ended on work the provider still runs detached, so the
  message wakes the open turn instead of queueing. A queued card on a waiting
  turn offers the same action per message, and an empty composer steers the
  oldest one.
- **Edit and resend** earlier messages.
- **Structured questions** — when an agent needs input, the composer turns
  into a multi-question form with progress ("1 of 3"), Back/Next navigation,
  preset choices, and a free-text "other" answer, instead of making you reply
  in prose.
- **@-mention files** — fuzzy-matched against the workspace file index — to
  pin them into context.
- **Reference another chat** — drag it from the sidebar into the composer,
  or type `@` and choose a matching chat in the current project. Send your
  prompt with the reference to give the agent access to that conversation.
- **Slash commands** — provider-native commands and skills the agent CLI
  advertises (invoked with the provider's own syntax), plus Goddard's own
  (`/resume`, `/goal`, `/fast`).
- **Attachments** — drop or paste files, folders, and images anywhere in the
  session column (not just the composer); they're uploaded to the daemon and
  shown as chips.
- **Drafts** — unsent composer text (and its attachments) is preserved per
  task, so switching tasks never loses a half-typed prompt. Typing with
  nothing focused lands in the composer automatically.
- **Markdown-aware editing** — the composer highlights Markdown, continues
  lists on Enter, renumbers ordered lists, and lets Backspace delete an empty
  list item.
- **Continue** — if a turn fails or was interrupted, an empty composer offers
  to continue the session without typing a prompt.
- **Annotations** — select text inside an agent message and choose "Add to
  chat" (⌘L), or ⌥-click a transcript line, to pin a highlighted comment on
  the passage. The composer shows an "N annotations" chip; your next message
  carries the quoted passages and comments to the agent, and the sent bubble
  echoes them. References like "Annotation 1" inside a comment resolve into
  hover citations. A draft made only of annotations still sends.

### Models and effort

The model picker lists every model the provider reports, with a **Favorites**
tab for starred models — and it remembers which tab you used last. Where the
provider exposes them, you also get:

- **Reasoning effort** — None through Max, including provider-specific tiers
  like Ultracode.
- **Service tier** — e.g. Codex's fast mode (`/fast` toggles it), Amp's fast
  tier.
- **Context window** selection — e.g. Claude's 200K/1M options.
- **DeepSeek agent presets** — Standard, Code, Minimal, Creator, and custom
  presets.

Model, effort, and tier changes apply to the running session in place for most
providers; the ones that can't switch mid-session restart their provider
process transparently while keeping the transcript.

### Access modes and permissions

Four modes, switchable per task and remembered between launches:

| Mode | Behavior |
| --- | --- |
| Supervised | Ask before commands and file changes |
| Auto-accept edits | Auto-approve edits, ask before other actions |
| Auto | An AI reviewer approves routine actions; risky ones still ask |
| Full access | Allow commands and edits without prompts |

When an agent asks for permission, the prompt offers **Allow once**, **Allow
for session**, or **Always allow** (where the provider supports each). Pi and
Oh My Pi run in full access only — Pi has no permission system at all, and
Goddard runs Oh My Pi with `--yolo`. Changing the access mode restarts the
provider session, deliberately: tightening or loosening what a running agent
may touch gets a fresh thread rather than mutating policy mid-flight.

### Rewind, revert, and fork

Every turn in a Git-backed task leaves a conversation-aware checkpoint:

- **Revert to here** rolls the conversation *and* the working tree back to
  before that turn — the provider's own rollback where it exists (Codex
  `thread/rollback`, Pi `fork`), with the Git checkpoint restoring files.
- **Fork task** copies the session into a new task at any response, via the
  provider's native session fork where it exists.
- Both are per-provider — the table above marks Fx, Kimi, and Devin CLI as not
  supporting them yet.

### Stopping and background work

- **Stop** (or Escape, pressed twice) interrupts the turn. For providers with
  a real interrupt the session stays alive; only for Amp, which has no stream
  interrupt, does stopping end the provider process — the next prompt resumes
  the native thread.
- **Background work**: long-running commands the agent detaches and subagents
  it spawns appear in the Background panel with live output, status, and stop
  controls. A turn that finishes while background work is still going shows
  "Waiting for background tasks" and can re-enter when that work settles
  (e.g. Claude background tasks stream their output live).

### Goals (Codex)

`/goal` sets a persistent objective the Codex thread keeps pursuing — before
or after the first message. Its autonomous progress streams into the
transcript, a status chip shows live budget or elapsed time, and a dialog lets
you edit, pause, resume, or clear the goal. Goal status covers active, paused,
stalled, usage-limited, budget-exhausted, and complete.

## The transcript

- Agent messages render as full Markdown: headings, lists, tables, code blocks
  with syntax highlighting, and LaTeX math (inline and block, with a
  copy-expression action; can be switched to raw source in settings).
- User messages render as Markdown too, with bare URLs linkified.
- **Tool activity** is normalized across providers into readable rows —
  "Ran `npm test`", "Edited src/app.rs" — that expand into full arguments and
  output, including inline diffs for file edits. Long groups collapse as the
  turn moves on.
- **Reasoning** streams into collapsible "Thinking" sections with elapsed
  time.
- **Find in page** (Cmd/Ctrl+F) searches the whole transcript with match-case,
  whole-word, and regex toggles.
- Long user prompts are clipped with a **Show more** control instead of
  scrolling; Shift-click extends a text selection.
- Each response keeps a changed-files summary and a **Review** action that
  opens its diff — files in the summary open directly, and hovering one
  previews its diff inline.
- A **navigation rail** alongside the transcript marks each turn — hover for
  a preview of its prompt, click to jump; ⌘⌥↑/↓ step between them.
- Every response shows its working time ("Worked for 3m") and a copy action;
  selected text copies with ⌘C, and code blocks, commands, and file paths
  have their own copy controls.
- Every user message supports edit, and every response supports fork/revert —
  the transcript is the control surface, not just a log.

## Git integration

The Git panel is experimental; enable it under **Settings → Experiments**
to use the commit, sync, and landing controls described below.

- **Git panel:** changed files with stage/unstage, commit list, unpushed
  markers, push, and **Sync changes** — `git pull --rebase` by default, or
  merge if you prefer (a setting). Rebase/merge conflicts surface with an
  **Abort**, **Merge instead**, or **Resolve in chat** action that hands the
  conflict to the agent.
- **Commit dialog:** Commit, Commit and push, or Push, with an
  include-unstaged toggle. Write your own subject or leave it empty and
  Goddard generates one through the task's provider.
- **Branch picker:** search, switch, and create branches, with "checked out in
  another worktree" marked.
- **Review surface:** diff the last turn, any turn, uncommitted/staged/
  unstaged/committed changes, a branch, or a specific commit — with per-file
  trees, expandable context lines, and a filter box.
- **Compare branch** and commit/push shortcuts live in the task's environment
  panel alongside its Task ID and agent CLI thread ID (both copyable).

## GitHub integration

Enable the GitHub experiment under **Settings → Experiments** and sign in
to `gh` on the machine running your agents.

For projects backed by a GitHub repo, a **GitHub** entry in the sidebar opens
a pull-request and issue browser in place of the transcript — state filters,
search, full PR/issue detail — powered by the `gh` CLI on the daemon host.
From a PR you can **start a task** on it or ask an agent to **fix failing
checks**. Sidebar task rows show linked PR state (open/draft/merged/closed)
with check and review status.

## Files, terminal, and browser

### Files surface

A workspace file tree with a reading/editing pane: syntax highlighting,
Markdown source/preview toggle, save with Cmd/Ctrl+S, find and replace
(Cmd/Ctrl+F inside the editor, with case/whole-word/regex), go to line
(Ctrl+G), and "Open on
GitHub" for tracked files. **Cmd+P** opens a fuzzy file finder over the same
index. On macOS, an **Open in…** control opens the project folder in an
external app and remembers your choice — VS Code, Cursor, Zed, Devin, Finder,
Terminal, Termy (via its new-tab deeplink), iTerm2, Kitty, Ghostty, Warp,
Xcode, and Android Studio are detected automatically.

### Terminal

Real PTY terminals in right-panel tabs, scoped to the task's workspace —
multiple per task, plus global terminals in the sidebar's Terminals group
(pinnable). macOS/Linux run your login shell; Windows prefers PowerShell 7,
falls back to Windows PowerShell, then `COMSPEC`. A shell-integration script
sourced into each PTY reports the live working directory and per-command
boundaries, so sidebar rows are named after the repo (falling back to the
shell), track `cd`, show last-activity times and command status icons, and
close themselves when the shell exits. URLs in terminal output are clickable
(OSC8 hyperlinks and plain-text links). Scrollback clear, terminal font
sizing, and copy/paste that doesn't steal Ctrl+C from the shell
(Ctrl+Shift+C/V on Windows).

### Embedded browser

A browser surface in the right panel — WebKit on macOS, WebView2 on Windows —
with an address bar, navigation, reload/stop, devtools, downloads, and "open
in default browser". When the terminal detects a dev server (a `localhost`
URL appearing in output), a "**localhost is live** — Open" toast offers to open
it in a browser tab in one click.

## Navigation and keyboard control

Goddard is designed to be driven without a mouse:

- **Command palette (⌘K / Ctrl+K):** fuzzy search across tasks, commands,
  settings pages, providers, projects, scripts, and CLI sessions to resume —
  plus **full chat-history search**: the query runs against every stored
  message in every task and returns matching snippets (archived chats are
  excluded; they're searched separately in Settings → Archived).
- **Task switcher (Ctrl+Tab / Ctrl+Shift+Tab):** recently-used tasks in an
  overlay; releasing the modifier commits.
- **Project switcher:** same pattern, for recent projects.
- **Jump to task:** hold the primary modifier and press 1–9 for the sidebar's
  first tasks.
- **Turn navigation:** jump to previous/next turn, next unread completion,
  or mark-unread-and-go-to-next.
- **History navigation:** back/forward across where you've been, plus
  three-finger trackpad swipe between tasks on macOS (optional setting).
- **Find in transcript:** Cmd/Ctrl+F.
- Shortcuts are surfaced where you need them: menus, tooltips on composer
  selectors and sidebar actions, and the command palette all show their
  chords. The full cheatsheet lives in the app — the keyboard button beside
  the sidebar's settings icon opens a shortcuts dialog resolved from the live
  keymap.

### Deep links (macOS)

Other apps and scripts can hand Goddard a `goddard://` URL:

- `goddard://new-task?prompt=<text>` opens the new-task page with the
  prompt already in the composer — for example
  `open 'goddard://new-task?prompt=Summarize%20this%20diff'` from a
  terminal. The text only fills the draft; it never sends on its own, so a
  link cannot make a task run unattended. If the draft already holds text,
  the prompt lands on its own paragraph beneath it.
- `goddard://task/<id>` selects that task — the same link tasks use to
  reference each other inside the app.

## Customization

**Settings → Appearance:**

- Match system appearance, or pick light/dark independently.
- Separate light and dark theme palettes: Default, Gruvbox, Everforest,
  Kanagawa, Zenburn, Poimandres, GitHub, Dracula, Rosé Pine (Dawn/Moon),
  Kanso (Zen/Pearl), and Warm Burnout.
- UI font and code font (any installed family), with independent sizes for UI
  text, code/diff/editor text, and the terminal.
- Sidebar transparency (vibrancy) on or off.
- Interface language: System, English, 日本語, 简体中文.
- Window size, position, and display are restored across launches.

Accessibility: every control is keyboard-operable with visible focus
treatments, the system reduce-motion setting is honored, and status is never
carried by color alone. One honest limitation: GPUI does not yet expose a
screen-reader tree, so VoiceOver and equivalents are not supported.

**Settings → General:** automatic updates, anonymous usage-data sharing
(prompts, responses, project names, and file paths are
never collected), LaTeX math rendering, Markdown preview,
open-at-last-prompt, sync-with-merge, three-finger swipe navigation, sidebar
shortcut tags, and a completion sound (with volume) that plays when a task
you're not viewing finishes.

## Skills

A **Skills library** in settings lists every skill installed for any provider
ecosystem — user-level and project-level — as a master/detail browser with
contents, supporting files, allowed tools, and update times. Skills can be
enabled, disabled, revealed, or deleted, and are invoked in the composer as
`/name`. Duplicate names resolve by specificity.

## Custom commands

Settings → Commands defines named shell scripts that appear in the command
palette (including a "New custom command" palette action). Each runs in a new
terminal tab in the current task's directory, inside an interactive shell (so
aliases, pipes, and interactive programs work), with an optional icon, a
custom shell, and a close-on-success option. Runs report through a toast that
mirrors the command's output tail. Agents can add commands too (a daemon
setting, on by default; agent-added commands are badged).

## Usage and cost tracking

- A **context-window gauge** under the composer shows live occupancy for the
  current session and opens a panel with account rate-limit lanes (Claude plan
  limits via the OAuth endpoint, OpenCode Go's usage endpoint, Codex's own
  rate-limit notifications) with reset countdowns.
- **Settings → Usage** is a full dashboard: daily and monthly cost/token
  charts, per-project and per-model breakdowns, cache savings, cost-quality
  indicators, and custom date windows — computed by scanning provider
  transcripts on the daemon.

## Notifications

When a task you're not viewing finishes its turn, Goddard can play a
completion sound — the picker lets you audition each option (including a
Retro chime) and set its volume — and posts a native system notification;
clicking it opens the task. The sound stays quiet while a follow-up is still
queued, since the task isn't done waiting on you. The sidebar marks unseen
completions as unread until you look at them, and those unread stamps survive
restarts.

## Remote work and delegation

Goddard's daemon runs the agents and owns workspace files. Desktop, web, and
mobile clients can connect to a daemon on another machine, so you can direct
work on a host you control. That host must stay on for its agents to run.

**Settings → Daemon** controls remote access. Keep the authentication token
private, allow only the browser origins you need, and use a trusted encrypted
connection outside a private network.

Connect to a remote host over SSH from macOS or Linux, or use an authenticated
WebSocket connection. Agents and project files stay on the connected host;
connecting from another device does not move them to that device.

With agent tools enabled and your permission, agents can spin off new tasks
and send messages to existing tasks. Each new task has its own transcript;
messages sent by an agent are attributed in the target task.

Use `/side` to open a separate chat alongside a task, or `/side <prompt>` to
start with a question. Side chats can inspect the parent conversation without
adding their own discussion to the main transcript.

## Privacy and data storage

Projects, task history, settings, and attachments are stored on your machine,
or on a daemon host you control. There is no required Goddard account or cloud
sync service. Your coding agents still send requests to their own providers.

Optional inference features such as Jev and voice briefings use the services
you configure. Anonymous product analytics can be controlled under
**Settings → General**; prompts, responses, project names, and file paths are
not included in those analytics.

## Updates

Goddard checks for updates once per launch (disable in Settings → General) and
shows available releases in the sidebar footer. Every platform verifies the
same Ed25519/EdDSA release signature before installing:

- **macOS:** Sparkle in-app updates with release notes.
- **Linux:** the updater validates the staged install, swaps the prefix, and
  relaunches — with automatic rollback to the previous version if the new
  build exits before opening a window.
- **Windows:** downloads and runs the verified installer, which replaces the
  app in place (including portable installs).

## Computer use (experimental)

Supported agents can observe and interact with local apps after you enable
computer use and approve access. The feature has its own permissions, separate
from the task's ordinary coding-agent access mode. Follow the
[computer-use guide](docs/computer-use.md) for setup, platform limits, and
troubleshooting.

## Keyboard shortcut reference

Highlights (macOS chords; on Linux/Windows the primary modifier is Ctrl). The
in-app shortcuts dialog — resolved from the live keymap — is authoritative.

| Action | Shortcut |
| --- | --- |
| New task | ⌘N |
| New project | ⌘O |
| Command palette | ⌘K |
| File finder | ⌘P |
| Jump to task 1–9 | ⌘1–⌘9 (hold ⌘ to see the chips) |
| Navigate back / forward | ⌘[ / ⌘] |
| Previous / next turn | ⌘⌥↑ / ⌘⌥↓ |
| Next unread completion (idle tasks once drained) | ⌘D or Ctrl+` |
| Mark unread, go to next idle task | ⌘⇧D |
| Task switcher | Ctrl+Tab / Ctrl+Shift+Tab |
| Project switcher (in a New Task draft) | hold ⌘, tap N |
| Toggle sidebar / right panel | ⌘B / ⌘⌥B |
| Focus composer | ⌘L (Add to chat when transcript text is selected) |
| Focus terminal / terminals group | ⌘J / ⌘T |
| Model / branch / mode picker | ⌘/ / ⌘⇧B / ⌘. |
| Workspace picker / usage panel | ⌘⇧T / ⌘U |
| Run project script | ⌘R |
| Find / find-and-replace | ⌘F / ⌘⌥F, then ⌘G / ⌘⇧G |
| Go to line (file viewer) | Ctrl+G |
| Open detected localhost URL | ⌘⌥O (⌘⌥⇧O opens a new browser tab) |
| Stop turn | Esc, Esc again to confirm; ⌥Esc stops immediately |
| Archive / pin task | ⌘⇧A / ⌘⌥P |
| Copy selection / working directory | ⌘C / ⌘⇧C |
| Save file | ⌘S |
| Send / newline / steer | Enter / ⇧Enter / ⌘Enter |
| Font size (UI, code, and terminal together) / reset | ⌘= / ⌘− / ⌘0 |
| Clear terminal scrollback | ⇧⌘K |
| Browser: address bar, back/forward, reload, devtools | ⌘L, ⌘[/⌘], ⌘R (⌘⇧R hard), ⌘⌥I |
| Settings | ⌘, |
| FPS counter | ⌘⌥⇧F |
| Close window / quit | ⌘W / ⌘Q |
| Hide app / hide others (macOS) | ⌘H / ⌘⌥H |

## Platform differences

| Feature | macOS | Linux | Windows |
| --- | --- | --- | --- |
| Agent sessions, projects, transcripts, diffs, Git, skills, usage | yes | yes | yes |
| Embedded browser | WebKit | — | WebView2 (no load-progress bar; devtools open-only; no pen/touch/file-drop yet) |
| Integrated terminal | login shell | login shell | PowerShell 7 → Windows PowerShell → COMSPEC |
| In-app updates | Sparkle | signed, with rollback | signed installer |
| Computer use | experimental | experimental | experimental |
| Remote-daemon terminal for browser clients | yes | yes | not yet |
| Three-finger swipe navigation | yes | — | — |

## Troubleshooting

**A provider shows as not installed.** Run the CLI by name in a fresh terminal.
If the shell can't find it either, the install never put a shim on `PATH`. If
the shell finds it but Goddard doesn't, set the binary path in **Settings →
Providers**. Detection already knows about version managers (nvm, fnm) and
per-user prefixes (`%APPDATA%\npm`, `~/.bun/bin`, `~/.cargo/bin`, scoop,
WindowsApps).

**The window opens black, or the app exits at startup (Windows).** Goddard
needs a working Direct3D 11 device — update the GPU driver; in a VM, enable 3D
acceleration.

**The app dies on its first frame in a Linux VM.** Software Vulkan/GL
rasterizers (lavapipe, llvmpipe) can crash compiling shaders — a driver bug,
not Goddard's. Give the guest a real GL driver (e.g. virtio-gpu-gl on UTM), or
set `VK_DRIVER_FILES=/nonexistent.json` to force the GL path.

**Git-backed features do nothing.** Goddard shells out to `git` — make sure
`git --version` works in a new terminal (install Git for Windows on Windows).

**Updates never arrive.** The updater fetches the feed from GitHub Releases
(via `curl.exe` in System32 on Windows); a proxy or filter blocking GitHub
downloads blocks updates. **Check for Updates…** in the app menu reports the
reason.
Downloading and running the installer manually is always equivalent.

**SmartScreen warns on first launch (Windows).** Expected when the release
isn't code-signed — choose **More info → Run anyway**.

## Uninstalling

- **macOS:** move Goddard to the Trash. Settings, tasks, and workspaces live in
  `~/.goddard` — delete it to remove them too.
- **Linux:** `curl -fsSL https://raw.githubusercontent.com/goddard-ai/goddard/main/install.sh | sh -s -- --uninstall`
  removes `~/.local/goddard.app`, the symlink, and the desktop entry. `~/.goddard`
  stays; delete it to remove projects and settings.
- **Windows:** uninstall from Settings → Apps, or delete the portable folder.
  Task data is `%LOCALAPPDATA%\Goddard`, settings `%USERPROFILE%\.goddard`.

## FAQ

**Does Goddard replace Claude Code / Codex / etc.?**
No — it drives them. You need at least one agent CLI installed and signed in.
Goddard replaces each CLI's terminal interface with a shared native one.

**Is it native or another Electron shell?**
The desktop interface is written in Rust with GPUI and rendered on the GPU.
It is not WebView-based; the separate Browser panel uses a platform WebView
when you open a website.

**How is it different from an editor with AI built in?**
Goddard focuses on directing tasks across agents and projects, reviewing their
changes, and keeping parallel work isolated. Use it alongside your preferred
editor and choose a provider for each task.

**Do my agent's own config still work — MCP servers, hooks, AGENTS.md?**
Yes. Goddard launches the real CLI through its native session protocol, so the
agent loads its own configuration, MCP servers, hooks, skills, and project
instructions exactly as if you'd run it in a terminal.

**Do I pay Goddard anything?**
No. It's free and GPL-licensed. "Bring your own
subscription" is literal — there's no bundled plan or markup, and your agent
provider bills exactly as if you used the CLI directly. Your existing plans,
rate limits, and API keys apply unchanged.

**Where do my conversations go?**
Nowhere. Everything is stored locally (or on a daemon host you control).
There's no Goddard account and no sync service.

**Can it run agents while I'm away?**
Agents run as local processes under the daemon — they keep working while the
window is closed to another task, and the daemon keeps sessions resumable, but
they still need your machine (or your daemon host) to be on.

**If I quit Goddard, do I lose my sessions?**
No. Tasks and transcripts are saved so you can reopen them. Resume behavior
depends on the provider; quitting can interrupt work that is still running.

**Can I use it on a server/headless machine?**
Yes: run `goddard-daemon` on the host, expose it with `--allow-non-loopback`, an
origin allowlist, and a token, then connect with Goddard Web, the mobile app,
or a desktop pointed at the external daemon.

**What can agents do on my machine?**
Agents run under your user account, and you pick the access mode per task —
from approving every action to letting the agent work unattended. Goddard
maps your choice onto each provider's own permission system rather than
inventing a second one.

**Can several agents work in parallel?**
Yes — that's the core design. Independent tasks run simultaneously, each in
its own workspace or worktree, and keep streaming in the background.

**Can one agent talk to another?**
With Agent Tools enabled and your permission, agents can create new tasks
and send messages to existing tasks.

**Can I import a session I started in the terminal?**
Yes — command palette → Resume… lists resumable CLI sessions per provider.

**Can I search everything I've ever asked an agent?**
The command palette searches message content in active tasks, not just titles.
Type a few words you remember to see matching snippets. Archived chats have
their own full-text search under **Settings → Archived**. Cmd/Ctrl+F searches
within the open transcript.

**Can I undo what the agent did?**
Revert to Here rolls back both the conversation and the Git working tree to
before a turn; Fork copies a session at any point. Both use provider-native
mechanisms where they exist.

**Does it work offline?**
You can browse saved tasks and local files offline. Hosted agent models,
GitHub workflows, updates, and configured inference services need a network
connection.

**Can I export or back up my data?**
There's no built-in export tool — but there's also no lock-in. Everything is
ordinary files you can browse, copy, or back up from the system file manager:
`~/.goddard` on macOS/Linux, `%LOCALAPPDATA%\Goddard` plus `%USERPROFILE%\.goddard`
on Windows. Provider transcripts also remain in each CLI's own session store
(`~/.claude/projects`, Codex threads, etc.).

**Can I open multiple windows?**
No — Goddard is a single-window app by design. Parallel work lives in
sidebar tasks and the task switcher. Window size, position, and display are
restored across launches.

**Can I customize the keybindings?**
Not yet — the keymap is fixed (and compiled), so there's no rebinding UI or
config file today. Themes, fonts, and sizes are the customization surface.

**Is there telemetry?**
Anonymous feature-usage and reliability analytics can be disabled under
Settings → General. They do not include prompts, responses, project names,
or file paths.

**Which permission mode should I use?**
Supervised if you want to approve every action; Auto-accept edits to let file
edits through while still approving other actions; Auto to let an AI reviewer
handle routine approvals and only ask about risky ones; Full access for
hands-off runs (required for Pi, Oh My Pi, and Amp).

**How do I report a bug or contribute?**
The project is open source (GPL-3.0) on GitHub — open an issue there, or join
the Discord linked from the site's menu. See [Contributing](CONTRIBUTING.md)
for the development workflow.

## Coordinate parallel native tests

Use [resource reservations](docs/resource-reservations.md) to queue native builds,
simulators, emulators, and shared desktop input across Goddard tasks.
