# Computer use

Computer use lets supported coding agents observe and interact with local apps:
for example, opening a browser, checking a page, or entering text in a window.
It is experimental and must be enabled separately from ordinary agent access.

## Enable computer use

1. Open **Settings → Experiments** and enable **Computer Use**. This reveals
   the Computer Use settings page. Development builds enable the experiment
   by default; release builds leave it off until you opt in.
2. Open **Settings → Computer Use** and turn on **Let Goddard use apps**.
   The page lists the supported providers: Codex, OpenCode, OpenCode 2,
   Grok, and Pi.
3. On macOS, use the permission controls on that page to grant the Goddard
   helper **Screen Recording** and **Accessibility** access. Start a new task
   after changing system permissions so it launches a fresh helper.
4. Start a task with a supported provider and ask for a specific app action,
   such as “Open my local development site and check that its navigation works.”
   Review the access request before allowing the agent to proceed.

Goddard bundles its computer-use support. You do not need to install a
separate Cua service, Python environment, or Node runtime.

## Control access

Computer-use requests identify the app or scope the agent wants to access.
Approve only the access needed for the task. Task-scoped grants are temporary;
apps you always allow appear in **Settings → Computer Use**, where you can
revoke their access.

The task's coding-agent access mode and computer-use permissions are separate.
Granting access to run commands does not replace the computer-use approval.

Use the task's **Stop** control to interrupt work. Turning off the
**Computer Use** experiment also disables computer use; turn it back on and
re-enable **Let Goddard use apps** when you want to use it again.

## Platform limits

| Platform | Requirements and limits |
| --- | --- |
| macOS | The Goddard helper needs Screen Recording and Accessibility permission. |
| Windows | The agent controls apps in your current interactive desktop. Windows can restrict elevated apps and secure desktops. |
| Linux | X11 needs the active display and accessibility services. Wayland support is experimental and depends on your compositor and desktop integrations. |

An agent on a remote machine uses that machine's desktop and permissions.
A headless server cannot provide the same graphical app access as an
interactive desktop session.

## Troubleshooting

**The Computer Use settings page is missing.** Enable the experiment first.
If the setting is unavailable in your build, install the
[latest release](https://github.com/goddard-ai/goddard/releases/latest).

**The agent cannot see or control a macOS app.** Check both system permissions
for the Goddard helper, then start a new task. Access
granted to another app or an older helper does not necessarily cover it.

**An action is refused on Windows or Linux.** Read the reported cause before
retrying. The current desktop, app privileges, or compositor may not support
that action. Repeating a request does not grant additional access.

For installation requirements, see [Linux](linux.md) or [Windows](windows.md).
