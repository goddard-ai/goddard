# Goddard on Windows

Install Goddard for your user account, or use the portable build without an
installer.

## Install

1. Open the [latest GitHub release](https://github.com/goddard-ai/goddard/releases/latest).
2. Download `Goddard-<version>-x86_64-Setup.exe`, or the
   `Goddard-<version>-aarch64-Setup.exe` installer for an Arm device.
   `<version>` is the release number shown in the asset name.
3. Run the installer and launch Goddard. It installs into
   `%LOCALAPPDATA%\Programs\Goddard` without administrator rights.
4. Install and sign in to a coding-agent CLI, then check
   **Settings → Providers** in Goddard.

You need Windows 10 version 1809 or newer, an x86_64 or Arm64 machine, and a
Direct3D 11 driver supporting feature level 11_0 or newer. No separate Visual
C++ redistributable is required.

Windows may show a SmartScreen warning for an unsigned release. Check that
it came from the project's GitHub release before choosing **More info → Run
anyway**.

## Portable installation

Download the `.zip` ending in `x86_64-pc-windows-msvc.zip`, or
`aarch64-pc-windows-msvc.zip` for Arm64, from the same release. Extract it into
a folder you own and run `goddard.exe`.

Keep the extracted folder intact. Goddard needs its companion executables,
libraries, and `resources/` folder. Create a shortcut if you want to launch
it elsewhere; do not move just `goddard.exe`.

## Updating

Goddard checks for updates at launch. An available update appears in the
sidebar footer. You can also choose **Check for Updates** from the app menu,
or disable launch checks under **Settings → General → Automatic updates**.

Updates are verified before installation. Goddard closes, installs the update,
and reopens. Portable copies update in their existing folder. Downloading and
running the latest installer is also a way to update manually.

## Your data

Application files and saved task data are separate:

| Data | Location |
| --- | --- |
| Tasks and transcripts | `%LOCALAPPDATA%\Goddard\app.db` |
| Attachments | `%LOCALAPPDATA%\Goddard\blobs` |
| App settings | `%USERPROFILE%\.goddard\app.json` |

Updating or uninstalling the application does not delete these files or your
project folders. Quit Goddard before copying its data for a backup.

## Terminal and browser

The integrated terminal uses PowerShell 7 when installed, then Windows
PowerShell, then the system command shell. Use Ctrl+Shift+C and Ctrl+Shift+V
for terminal copy and paste; Ctrl+C stays available to interrupt a command.

The Browser panel uses Microsoft WebView2. It is included with Windows 11;
on Windows 10, check that the WebView2 Runtime is installed. The desktop
interface itself is rendered with GPUI.

The browser currently has no load-progress bar, its developer tools can be
opened but not closed from Goddard, and pen, touch, and file drops into a page
are not supported.

## Computer use

Computer use is experimental and operates within your current desktop.
Windows can restrict access to elevated apps and secure desktops. See
[Computer use](computer-use.md) for setup and permissions.

## Troubleshooting

**The window is black, or the app exits at startup.** Update your graphics
driver. In a virtual machine, enable 3D acceleration and check that a
Direct3D 11 device is available.

**A provider is shown as not installed.** Open a new PowerShell window and
run the CLI by name. If it is missing there too, finish installing it and
check its `PATH` setup. If PowerShell can run it but Goddard cannot, set its
binary path in **Settings → Providers**.

**Git features do not work.** Install Git for Windows and check that
`git --version` works in a new terminal.

**Updates do not arrive.** A proxy or network filter blocking GitHub Releases
can block updates. Use **Check for Updates** to see the error, or download
the installer from the latest release manually.

## Uninstalling

For an installed copy, use **Settings → Apps** in Windows. For a portable
copy, delete its extracted application folder. Task data and settings are
stored separately; removing the app does not remove your projects.
