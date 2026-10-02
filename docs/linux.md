# Goddard on Linux

Install the latest release, keep it updated, and troubleshoot common startup
problems on a Linux desktop.

## Install

```sh
curl -fsSL https://raw.githubusercontent.com/goddard-ai/goddard/main/install.sh | sh
```

The installer selects your architecture and installs without root into
`~/.local/goddard.app`. It adds an application-menu entry and a `goddard`
command under `~/.local/bin`. Open Goddard from your applications menu; if
`~/.local/bin` is on your `PATH`, you can also run `goddard` in a terminal.

You need:

- glibc 2.35 or newer, such as Ubuntu 22.04 or Debian 12.
- An x86_64 or aarch64 machine.
- A working Vulkan or OpenGL driver.
- `xdg-desktop-portal` for native file dialogs.
- `curl` or `wget` for downloads.

After installation, install and sign in to a coding-agent CLI, then check
**Settings → Providers** in Goddard.

## Other installation options

To install a specific release, set `GODDARD_VERSION` before running the
installer. For example, this command installs version 0.13.0:

```sh
curl -fsSL https://raw.githubusercontent.com/goddard-ai/goddard/main/install.sh | GODDARD_VERSION=0.13.0 sh
```

For a manual installation, download the `.tar.gz` for your architecture from
the [latest GitHub release](https://github.com/goddard-ai/goddard/releases/latest)
and extract it into a folder you own. Run `bin/goddard` inside the extracted
folder. Keep `bin/` and `share/` together: Goddard needs the companion programs
and resources, so moving only the main executable will break the installation.
The script above also installs the application-menu entry for you.

For a source build, see [Contributing](../CONTRIBUTING.md#development-setup).

## Updating

Goddard checks for updates at launch. An available update appears in the
sidebar footer. You can also choose **Check for Updates** from the app menu,
or disable launch checks under **Settings → General → Automatic updates**.

The default user-local installation updates itself and verifies the release
signature before installing. If the replacement cannot start, Goddard restores
the previous installation. Re-running the install command is another way to
upgrade to the latest release.

System-wide or package-manager installations must be updated through their
original installation method.

## Computer use

Computer use is experimental. X11 uses your active desktop; Wayland support
depends on your compositor and desktop integrations. See
[Computer use](computer-use.md) for setup and permissions.

## Troubleshooting

**The app exits before showing a window.** Check that your system meets the
glibc requirement and has a working graphics driver. Update the driver before
retrying. An older distribution may need a source build.

**The app crashes in a virtual machine.** Enable virtual GPU acceleration
where available. Software graphics drivers can fail while compiling shaders.
To try OpenGL instead of Vulkan for one launch:

```sh
VK_DRIVER_FILES=/nonexistent.json ~/.local/goddard.app/bin/goddard
```

This is a troubleshooting step, not a replacement for a working graphics
driver. It may not help if both graphics backends use the same failing driver.

**Goddard is missing from the applications menu.** Re-run the installer and
check that `~/.local/share/applications/org.goddardai.app.desktop` exists.

**The `goddard` command is missing.** Add `~/.local/bin` to your shell's `PATH`,
or launch Goddard from your applications menu.

## Uninstalling

```sh
curl -fsSL https://raw.githubusercontent.com/goddard-ai/goddard/main/install.sh | sh -s -- --uninstall
```

This removes the app, command symlink, and application-menu entry. It leaves
project files and settings alone.
