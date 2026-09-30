# Goddard

Goddard is a native desktop workspace for coding agents on macOS, Linux, and
Windows. Run Claude Code, Codex, Cursor, and other agent CLIs through one
interface, using your existing accounts and subscriptions. Free and open source.

Written in Rust with [GPUI](https://gpui.rs/), Goddard's desktop interface is
GPU-rendered, not WebView-based.

## Why Goddard?

Make parallel agent work easy to direct and finish: choose the right agent,
keep tasks isolated, give feedback, and review and ship the result from one
fast, keyboard-driven workspace. Your projects and task history stay on a
machine you control.

## Features

- **Agent choice and continuity.** Work with multiple providers, resume
  conversations started in their CLIs, and switch providers with a context
  handoff.
- **Isolated parallel work.** Run independent tasks across projects, with
  separate Git worktrees when you need to keep their changes apart.
- **Feedback while agents work.** Steer active turns, queue follow-ups, and
  annotate passages in a reply to send precise feedback with your next prompt.
- **Review and recovery.** Inspect changes by turn, rewind the conversation
  and working tree together, or fork a different approach. Steering, rewind,
  and fork support vary by provider.
- **Know what needs attention.** Track running tasks, requests for input, and
  unread completions. Jump to the next unread reply or search across task
  history from the command palette.
- **Side chats and delegation.** Explore a question in a separate chat
  alongside the main task. Enable agent tools to let agents create tasks and
  send messages between them.
- **Usage visibility.** Track context consumption, supported providers'
  account limits, and cost and token breakdowns by project and model.

<details>
<summary>Optional and experimental capabilities</summary>

Some capabilities require an opt-in under **Settings → Experiments**;
Jev routing and suggested actions have their own controls under
**Settings → Jev**.

- **Jev routing and suggested actions.** Route tasks to models you choose,
  assess turn outcomes, and suggest or automatically run follow-up actions.
  Requires a configured inference provider for Jev evaluations.
- **Project memory (experimental).** Distill completed work into project
  notes that give new sessions prior decisions and context.
- **Automations (experimental).** Run prompts on a schedule or webhook,
  with each run linked to its task.
- **GitHub workflows (experimental).** Browse issues and pull requests,
  start tasks from them, and follow review and check status. Requires the
  authenticated `gh` CLI on the machine running your agents.
- **Computer use (experimental).** Give supported agents access to local
  apps through Goddard's computer-use tools. Availability depends on the
  platform, build, and provider; see [Computer Use](docs/computer-use.md).

</details>

## Native performance

GPUI renders the desktop interface directly on the GPU, avoiding the browser
engine and JavaScript runtime overhead of a web-based UI. That removes a
layer of memory and processing overhead from the interface and helps keep
scrolling, task switching, and streaming replies responsive.

Long transcripts are virtualized so rendering work stays proportional to
what is visible. Filesystem, Git, and provider operations run off the UI
thread, keeping agent work from blocking the interface.

## Get started

1. Download Goddard from [GitHub Releases](https://github.com/goddard-ai/goddard/releases).
   See the installation guides for [Linux](docs/linux.md) and
   [Windows](docs/windows.md).
2. Install and sign in to at least one supported agent CLI.
3. Open Goddard, check **Settings → Providers**, and start a task in your
   project folder or a new worktree.

Goddard uses the agent CLIs you install and authenticate. No Goddard account
or separate agent subscription is required.

## Contribute

Bug reports, focused fixes, tests, and well-scoped features are welcome.
See [CONTRIBUTING.md](CONTRIBUTING.md) for development setup, checks, and
contribution policies.

For detailed workflows, see the [user guide](WIKI.md). Release notes live in
[CHANGELOG.md](CHANGELOG.md).

Licensed under [GNU GPL v3.0 only](LICENSE).
