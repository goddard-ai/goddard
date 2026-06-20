# Worktree Open App Button — Expanded Implementation Plan

## Summary

Add a reusable desktop UI control that opens the current session worktree in a local application selected by the user. The primary use case is opening the worktree in an IDE such as Cursor, VS Code, Windsurf, JetBrains IDEs, Zed, or Sublime Text. The secondary use case is opening the same worktree in the platform file manager: Finder on macOS, File Explorer on Windows, and a best-effort file manager or generic open-location command on Linux.

The control should be a split button:

* The primary/left action opens the current session worktree using the user’s last successful opener selection.
* The secondary/right action opens a dropdown of detected available applications.
* Selecting a dropdown item immediately opens the current worktree with that application.
* The selected opener becomes the new primary opener only after the launch succeeds.

This is a desktop integration feature. The native host layer should own platform detection, local app discovery, icon normalization, launch behavior, and persistence of the last-used opener. The renderer should receive normalized opener data and invoke typed RPC actions through the Electrobun boundary.

## Goals

* Make the common path fast: one click should open the current worktree in the user’s preferred app.
* Keep the renderer platform-agnostic and filesystem-agnostic.
* Support IDEs and file managers through one normalized opener model.
* Persist the user’s last successful opener choice across sessions.
* Fail visibly and actionably when a user-triggered launch fails.
* Degrade quietly when discovery or icon extraction is incomplete.
* Keep the first implementation small enough to land incrementally while preserving a data model that can grow.

## Non-goals

* Do not build a full “manage applications” preferences screen in the initial implementation.
* Do not expose arbitrary custom executable selection in the first pass.
* Do not make opener preference part of shared cross-client SDK state unless product later decides that preference should roam across clients.
* Do not let renderer code inspect local application folders, parse desktop entries, read `.icns` files, or construct launch commands.
* Do not show every possible application that can open a folder. The dropdown should stay intentionally short and useful.

## Recommended Product Decisions

Where the original plan leaves behavior ambiguous, use these defaults unless the product explicitly wants a different direction.

| Ambiguity                                              | Recommended default                                                                                                                                                       |
| ------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Does choosing a dropdown item open immediately?        | Yes. Selecting an opener from the dropdown opens the worktree immediately and updates the last-used opener only after success.                                            |
| What should the main button display on first run?      | Best available IDE using a deterministic priority list, falling back to platform file manager.                                                                            |
| What if the persisted opener is unavailable?           | Keep it stored, but use the next available fallback for the current UI. If the app becomes available later, it can become primary again.                                  |
| Should duplicate installs appear?                      | Not in Phase 1. Pick the most likely normal install per stable opener ID. Add duplicate disambiguation later only if real users need it.                                  |
| Should raw paths be shown?                             | Only as secondary text when duplicates or ambiguous sources exist. Otherwise hide paths and implementation identifiers.                                                   |
| Should icon failure block discovery?                   | No. Missing or failed icons should produce a generic icon in the renderer.                                                                                                |
| Should a file manager always appear?                   | Yes when a workable platform action exists. The file manager entry is the guaranteed fallback.                                                                            |
| Should launch failures update persistence?             | No. Update last-used opener only after successful launch.                                                                                                                 |
| Should worktree path be passed directly from renderer? | Prefer session ID. The host should resolve and validate the current worktree path. Allow a path only if an existing app contract already exposes a resolved trusted path. |
| What is the first implementation scope?                | Define the cross-platform type shape, implement macOS app discovery/launching first, and include file-manager fallbacks for Windows and Linux.                            |
| Is persistence in the first implementation?            | Yes. Persist the last successful opener as a per-user global desktop preference in the first implementation.                                                              |
| How should Finder open a worktree?                     | Open the folder itself, not reveal/select it in the parent Finder window.                                                                                                  |

## User Experience

### Main states

1. **Ready with IDE primary**

   * Primary button label: app name, for example “Open in Cursor”.
   * Primary button icon: app icon if available, otherwise generic app icon.
   * Dropdown contains other detected openers plus file manager.

2. **Ready with file manager primary**

   * Used when no IDE is available or the user last chose the file manager.
   * Primary button label: “Open in Finder”, “Open in File Explorer”, or “Open in Files”.

3. **Opening**

   * Disable both split-button halves or mark the selected action as pending.
   * Show a spinner or loading affordance on the primary button.
   * Prevent duplicate launches from repeated clicks while the same opener/session action is pending.

4. **Launch failed**

   * Keep the previous primary opener unchanged.
   * Show a visible inline or toast error such as: “Couldn’t open worktree in Cursor. Choose another app or check that the worktree still exists.”
   * Dropdown remains usable.

5. **Discovery partial or unavailable**

   * Do not show a loud error if one app cannot be inspected.
   * Show available apps and the file manager fallback.
   * If even the fallback is unavailable, show a disabled button with an explanatory tooltip.

### Suggested UI copy

| Situation                       | Suggested copy                                               |
| ------------------------------- | ------------------------------------------------------------ |
| Primary action label            | `Open in {AppName}`                                          |
| Dropdown trigger aria-label     | `Choose app to open worktree`                                |
| No IDEs, file manager available | `Open in Finder` / `Open in File Explorer` / `Open in Files` |
| No usable openers               | `No opener available`                                        |
| Launch failure                  | `Couldn’t open worktree in {AppName}.`                       |
| Worktree missing                | `This worktree no longer exists.`                            |
| App removed after discovery     | `{AppName} is no longer available.`                          |

### Dropdown ordering

Recommended order:

1. Current primary opener.
2. Detected IDEs in product-defined preference order.
3. Platform file manager.

Avoid showing unavailable IDEs in the normal menu. If product wants an educational empty state, show a compact disabled row such as “No supported IDEs detected” only when no IDEs are available.

### Accessibility

* Use a true split-button pattern: the primary action and menu trigger must be focusable and operable independently.
* Keyboard behavior:

  * `Enter` or `Space` on primary action opens with primary opener.
  * `Enter`, `Space`, `ArrowDown`, or `Alt+ArrowDown` on dropdown trigger opens the menu.
  * Arrow keys navigate menu items.
  * `Enter` selects the highlighted opener.
  * `Escape` closes the menu.
* Menu rows should expose app names as accessible labels.
* Loading state should be announced when possible.
* Error state should be discoverable by screen readers, for example via an aria-live region or existing toast semantics.

## Architecture

### Ownership boundaries

| Layer                    | Responsibilities                                                                                                 |
| ------------------------ | ---------------------------------------------------------------------------------------------------------------- |
| Renderer UI component    | Render split button, menu, pending state, and errors from normalized data. Invoke callbacks.                     |
| Renderer state/container | Request opener list, derive current primary opener, call launch RPC, handle optimistic/pending state and errors. |
| Electrobun RPC boundary  | Provide typed request/response contracts between renderer and Bun/native host.                                   |
| Host service             | Detect platform, discover apps, normalize icons, resolve worktree path, launch opener, persist last-used opener. |
| Shared SDK/daemon        | Own session/worktree path resolution if an existing contract already exists.                                     |

### Key rule

The renderer should not perform native work. It should never:

* Read application directories.
* Parse `.app`, `.desktop`, `.exe`, or icon resource files.
* Infer launch behavior from browser APIs.
* Build shell commands.
* Persist raw local application paths as preferences.

## Normalized Data Model

Use a small renderer-facing opener shape. Native-only launch metadata should remain in the host process.

```ts
export type WorktreeOpenerKind = "ide" | "file-manager";

export type DesktopPlatform = "macos" | "windows" | "linux";

export type WorktreeOpenerId =
  | "cursor"
  | "vscode"
  | "vscode-insiders"
  | "windsurf"
  | "xcode"
  | "android-studio"
  | "intellij-idea"
  | "webstorm"
  | "pycharm"
  | "goland"
  | "phpstorm"
  | "sublime-text"
  | "zed"
  | "ghostty"
  | "terminal"
  | "finder"
  | "explorer"
  | "linux-files";

export type WorktreeOpener = {
  id: WorktreeOpenerId;
  displayName: string;
  kind: WorktreeOpenerKind;
  platform: DesktopPlatform;
  available: boolean;
  iconUrl?: string;
  secondaryText?: string;
};

export type WorktreeOpenersResponse = {
  platform: DesktopPlatform;
  openers: WorktreeOpener[];
  primaryOpenerId: WorktreeOpenerId | null;
  persistedOpenerId: WorktreeOpenerId | null;
};
```

### Host-only model

The host can keep a richer internal model that should not be sent to the renderer by default.

```ts
type NativeLaunchStrategy =
  | { type: "macos-open-app"; appBundlePath: string }
  | { type: "macos-finder" }
  | { type: "windows-exe"; exePath: string; argsTemplate?: string[] }
  | { type: "windows-explorer" }
  | { type: "linux-desktop-entry"; desktopFilePath: string; exec: string }
  | { type: "linux-command"; command: string; argsTemplate?: string[] }
  | { type: "linux-open-location" };

type NativeWorktreeOpener = WorktreeOpener & {
  launchStrategy: NativeLaunchStrategy;
  discoverySource?: string;
  discoveredPath?: string;
};
```

### Stable ID behavior

Stable IDs should represent product-level opener identity, not the local installation path. For example:

* Persist `cursor`, not `/Applications/Cursor.app`.
* Persist `vscode`, not `/Users/alec/Applications/Visual Studio Code.app`.
* Persist `finder`, not a command string.

If multiple installs are found for one stable ID, Phase 1 should select the preferred install and return one opener. Future versions can add `instanceId` or duplicate entries if needed.

## RPC API Shape

Recommended RPC requests:

```ts
export type WorktreeOpenRPC = {
  bun: RPCSchema<{
    requests: {
      listWorktreeOpeners: {
        params: { sessionId: string };
        response: WorktreeOpenersResponse;
      };
      openWorktree: {
        params: {
          sessionId: string;
          openerId: WorktreeOpenerId;
        };
        response: {
          ok: true;
          openerId: WorktreeOpenerId;
        } | {
          ok: false;
          openerId: WorktreeOpenerId;
          errorCode: WorktreeOpenErrorCode;
          message: string;
        };
      };
    };
  }>;
};

export type WorktreeOpenErrorCode =
  | "WORKTREE_NOT_FOUND"
  | "OPENER_NOT_FOUND"
  | "OPENER_UNAVAILABLE"
  | "LAUNCH_FAILED"
  | "PERMISSION_DENIED"
  | "UNKNOWN";
```

### Why `sessionId` is preferred

The host should resolve the worktree path because it is the layer responsible for validating local filesystem state and calling native launch APIs. Passing `sessionId` also avoids duplicating session/worktree-path logic in UI code.

If the existing app architecture already exposes a trusted resolved worktree path to the host or renderer, this can be adapted, but avoid creating a second source of truth.

## Discovery Service

Create a host-side service responsible for returning normalized openers.

Suggested module boundaries:

```ts
// host/worktreeOpeners/index.ts
export async function listWorktreeOpeners(): Promise<NativeWorktreeOpener[]>;
export async function openWorktreeWithOpener(args: {
  sessionId: string;
  openerId: WorktreeOpenerId;
}): Promise<OpenWorktreeResult>;

// host/worktreeOpeners/platform.ts
export function getDesktopPlatform(): DesktopPlatform;

// host/worktreeOpeners/discovery/macos.ts
// host/worktreeOpeners/discovery/windows.ts
// host/worktreeOpeners/discovery/linux.ts

// host/worktreeOpeners/icons.ts
// host/worktreeOpeners/preferences.ts
// host/worktreeOpeners/launch.ts
```

### Discovery cache

* Cache discovery results in memory for the lifetime of the app process.
* Provide a refresh path if needed later, but Phase 1 can refresh on app restart.
* Validate availability again at launch time because apps may be removed after discovery.

### Product priority list

When multiple IDEs are installed and no persisted opener is available, use a deterministic priority order. Recommended default:

1. Cursor
2. Visual Studio Code
3. Windsurf
4. Zed
5. Xcode
6. Android Studio
7. WebStorm
8. Visual Studio Code Insiders
9. Other JetBrains IDEs
10. Sublime Text
11. Ghostty
12. Terminal
13. Platform file manager

This can be tuned to the product’s expected audience.

## Platform Discovery Details

## macOS

### Candidate locations

Search these locations first:

* `/Applications`
* `~/Applications`

Optional later additions:

* `/System/Applications` for built-in apps if ever needed.
* Spotlight/LaunchServices-based lookup for apps outside standard locations.
* JetBrains Toolbox installation metadata.

### Known app bundle names

| Opener ID         | Common bundle name                                  |
| ----------------- | --------------------------------------------------- |
| `cursor`          | `Cursor.app`                                        |
| `vscode`          | `Visual Studio Code.app`                            |
| `vscode-insiders` | `Visual Studio Code - Insiders.app`                 |
| `windsurf`        | `Windsurf.app`                                      |
| `zed`             | `Zed.app`                                           |
| `xcode`           | `Xcode.app`                                         |
| `android-studio`  | `Android Studio.app`                                |
| `sublime-text`    | `Sublime Text.app`                                  |
| `intellij-idea`   | `IntelliJ IDEA.app` or `IntelliJ IDEA Ultimate.app` |
| `webstorm`        | `WebStorm.app`                                      |
| `pycharm`         | `PyCharm.app` or `PyCharm CE.app`                   |
| `goland`          | `GoLand.app`                                        |
| `phpstorm`        | `PhpStorm.app`                                      |
| `ghostty`         | `Ghostty.app`                                       |
| `terminal`        | `Terminal.app`                                      |
| `finder`          | built-in opener                                     |

### Display name

Prefer reading `Contents/Info.plist` values:

1. `CFBundleDisplayName`
2. `CFBundleName`
3. Bundle folder name without `.app`

### Icon detection

Prefer reading `CFBundleIconFile` from `Info.plist`, then resolving the corresponding file in `Contents/Resources`. If no declaration exists, fall back to known icon names only for common apps.

Example path shape:

```txt
/Applications/Cursor.app/Contents/Resources/Cursor.icns
```

The host should convert `.icns` to a renderer-friendly URL or data payload. Do not pass raw `.icns` paths to the renderer unless the webview is proven to render them reliably.

### Launch strategy

Preferred options, in order:

1. Native macOS API that opens a file URL using a specific application bundle URL.
2. Structured process spawn of `/usr/bin/open` with argument array, not shell string construction.

Example structured fallback shape:

```ts
spawn("/usr/bin/open", ["-a", appBundlePath, worktreePath]);
```

Finder can use:

```ts
spawn("/usr/bin/open", [worktreePath]);
```

This should open the folder itself. Do not use a reveal/select strategy for the initial behavior.

## Windows

### Candidate locations

Search practical install roots:

* `%LOCALAPPDATA%\Programs`
* `%ProgramFiles%`
* `%ProgramFiles(x86)%`
* Known per-app subpaths.

Also consider command registrations later if needed:

* Start Menu shortcuts.
* Registry uninstall keys.
* App execution aliases.

### Known executable names and paths

| Opener ID         | Common executable                                              |
| ----------------- | -------------------------------------------------------------- |
| `cursor`          | `Cursor.exe`                                                   |
| `vscode`          | `Code.exe`                                                     |
| `vscode-insiders` | `Code - Insiders.exe`                                          |
| `windsurf`        | `Windsurf.exe`                                                 |
| `zed`             | `Zed.exe` if available on Windows in the target support matrix |
| `xcode`           | not supported on Windows                                       |
| `android-studio`  | `studio64.exe`                                                 |
| `sublime-text`    | `sublime_text.exe`                                             |
| `intellij-idea`   | `idea64.exe`                                                   |
| `webstorm`        | `webstorm64.exe`                                               |
| `pycharm`         | `pycharm64.exe`                                                |
| `goland`          | `goland64.exe`                                                 |
| `phpstorm`        | `phpstorm64.exe`                                               |
| `ghostty`         | `ghostty.exe` if available in the target support matrix        |
| `terminal`        | Windows Terminal can be considered later, but Explorer is the file-manager fallback for Phase 1 |
| `explorer`        | built-in opener                                                |

### Launch strategy

Use structured process spawning with argument arrays. Avoid shell string construction.

IDE examples generally take the folder path as an argument:

```ts
spawn(exePath, [worktreePath], { detached: true });
```

File Explorer fallback:

```ts
spawn("explorer.exe", [worktreePath]);
```

### Icon handling

Phase 1 can return no icon or a generic icon for Windows apps. Keep the data model ready for extracted icons later. Later implementation can use platform APIs or helper code to extract icons from `.exe` resources and produce PNG/cache URLs.

## Linux

### Discovery inputs

Prefer Linux-native application metadata:

* Desktop entries under standard application directories.
* Commands available on `PATH`.
* Known IDE executable names.

Candidate desktop entry directories:

* `/usr/share/applications`
* `/usr/local/share/applications`
* `~/.local/share/applications`

Common executable names:

| Opener ID         | Common commands                                          |
| ----------------- | -------------------------------------------------------- |
| `cursor`          | `cursor`, `Cursor`                                       |
| `vscode`          | `code`                                                   |
| `vscode-insiders` | `code-insiders`                                          |
| `windsurf`        | `windsurf`                                               |
| `zed`             | `zed`                                                    |
| `xcode`           | not supported on Linux                                   |
| `android-studio`  | `android-studio`, `studio.sh`                            |
| `sublime-text`    | `subl`, `sublime_text`                                   |
| `intellij-idea`   | `idea`, `intellij-idea`, `idea.sh`                       |
| `webstorm`        | `webstorm`, `webstorm.sh`                                |
| `pycharm`         | `pycharm`, `pycharm.sh`                                  |
| `goland`          | `goland`, `goland.sh`                                    |
| `phpstorm`        | `phpstorm`, `phpstorm.sh`                                |
| `ghostty`         | `ghostty`                                                |
| `terminal`        | desktop-terminal command detection can be considered later |
| `linux-files`     | built-in opener using environment open-location behavior |

### Desktop entry parsing

Parse only enough to support this feature:

* `[Desktop Entry]`
* `Name`
* `Exec`
* `Icon`
* `NoDisplay`
* `Hidden`
* `Type=Application`

Ignore entries that are hidden, not applications, or clearly unsuitable.

When launching from an `Exec` string, correctly handle field codes such as `%f`, `%F`, `%u`, and `%U`. The safest implementation is to parse the executable and arguments according to desktop entry rules, replace supported file/URL field codes with the worktree path or file URL, and remove unsupported/deprecated field codes. Do not pass the whole `Exec` line through a shell.

### File manager fallback

Use a generic “Files” opener when the exact desktop file manager cannot be identified. Launch behavior can use:

* `xdg-open <worktreePath>` when available.
* `gio open <worktreePath>` as a fallback.
* A detected file manager command only if reliably available.

### Icon handling

Linux icon values may be:

* Absolute paths.
* Icon theme names.

Normalize into renderer-friendly icon sources when possible. If the icon is a theme name and resolution is not implemented yet, return no icon and allow the renderer to show a generic fallback.

## Launching Behavior

### Flow

1. Renderer calls `openWorktree({ sessionId, openerId })`.
2. Host validates that `openerId` maps to a currently known opener.
3. Host resolves the current worktree path for `sessionId`.
4. Host verifies that the path exists and is a directory.
5. Host launches the selected app or file manager using structured arguments or native APIs.
6. Host confirms the launch call succeeded.
7. Host persists `lastUsedWorktreeOpenerId = openerId`.
8. Host returns success to renderer.
9. Renderer updates primary opener and clears pending state.

### Success definition

For Phase 1, success means the platform launch call returned without immediate error. The app does not need to verify that the IDE window actually opened the folder, because that is difficult and platform-specific.

### Validation

Before launch:

* Confirm opener is in the current discovery set or is a built-in platform opener.
* Confirm opener is available.
* Resolve worktree path from the trusted session source.
* Confirm the worktree path exists.
* Confirm the worktree path is a directory.

### Security constraints

* Never concatenate shell strings.
* Use structured process spawning or platform APIs.
* Treat `.desktop` `Exec` values as data that must be parsed, not as a shell script.
* Do not expose full filesystem paths to the renderer unless needed for user-facing disambiguation.
* Do not allow renderer-supplied arbitrary executables in Phase 1.

## Persistence

### Scope

Persist in local desktop app preferences, scoped to the current user on the current machine.

Minimum state:

```ts
type WorktreeOpenerPreferences = {
  lastUsedWorktreeOpenerId?: WorktreeOpenerId;
};
```

### Recommended behavior

* Read preference when constructing `WorktreeOpenersResponse`.
* If the persisted opener is available, use it as primary.
* If unavailable, keep it stored but use a fallback primary for the current response.
* Update preference only after successful launch.
* Do not clear preference just because discovery did not find the app on one run.

### Preference location

The exact storage location should follow existing app-local preferences conventions. If no convention exists, introduce a small host-side preferences helper rather than writing ad hoc local storage from the renderer.

Recommended preference key:

```txt
worktreeOpen.lastUsedOpenerId
```

## Primary Opener Resolution

Recommended function:

```ts
function resolvePrimaryOpener(args: {
  openers: WorktreeOpener[];
  persistedOpenerId: WorktreeOpenerId | null;
  preferredOrder: WorktreeOpenerId[];
}): WorktreeOpenerId | null {
  const available = new Set(
    args.openers.filter((o) => o.available).map((o) => o.id),
  );

  if (args.persistedOpenerId && available.has(args.persistedOpenerId)) {
    return args.persistedOpenerId;
  }

  for (const id of args.preferredOrder) {
    if (available.has(id)) return id;
  }

  return null;
}
```

Default preferred order should end with the platform file manager.

## UI Component Boundary

### Container component

The container should own data loading and mutation.

```ts
type WorktreeOpenButtonContainerProps = {
  sessionId: string;
};
```

Responsibilities:

* Fetch `listWorktreeOpeners` for the session.
* Track pending opener ID.
* Call `openWorktree` for primary or selected opener.
* Update local state after success.
* Surface errors.

### Leaf component

The leaf component should be a pure UI component.

```ts
type WorktreeOpenSplitButtonProps = {
  primaryOpener: WorktreeOpener | null;
  openers: WorktreeOpener[];
  pendingOpenerId?: WorktreeOpenerId;
  disabled?: boolean;
  error?: string;
  onOpenPrimary: () => void;
  onOpenWith: (openerId: WorktreeOpenerId) => void;
};
```

The leaf should not know about sessions, RPC, platform-specific launch behavior, or persistence.

## Icon Pipeline

### Renderer-facing contract

The renderer receives one of:

* `iconUrl`, if available.
* No `iconUrl`, in which case it renders a generic IDE/folder icon.

### Host-side normalization

Host pipeline:

1. Discover icon source from platform metadata.
2. Convert or resolve it into a renderer-supported image.
3. Cache the normalized result in memory.
4. Return a local URL, cache URL, or data URL.

### macOS `.icns` handling

Recommended first implementation:

* Read `CFBundleIconFile` from the app’s `Info.plist`.
* Resolve `.icns` under `Contents/Resources`.
* Extract a 32, 64, or 128 pixel representation.
* Convert to PNG.
* Return a local asset URL or data URL.

The implementation can choose the most practical conversion path available in the app runtime. Missing conversion support should not block Phase 1 if the UI has generic fallback icons.

### Cache behavior

* Use an in-memory map keyed by stable opener ID plus discovered icon path.
* Avoid extracting icons on every render.
* Add disk caching only if performance measurements show startup or menu-open cost is noticeable.

## Error Handling

### Discovery errors

Discovery should be best effort. If one app fails inspection, log it for diagnostics and continue.

Examples:

* App bundle has malformed `Info.plist`.
* Icon file is missing.
* Desktop entry has an invalid `Exec` field.
* Windows executable path cannot be statted.

Do not show a prominent UI error for these cases.

### Launch errors

Launch errors are user-visible because the user explicitly clicked something.

| Error code           | Cause                                                        | User-facing behavior                                |
| -------------------- | ------------------------------------------------------------ | --------------------------------------------------- |
| `WORKTREE_NOT_FOUND` | Resolved worktree path does not exist or is not a directory. | Show error and optionally offer to refresh session. |
| `OPENER_NOT_FOUND`   | Opener ID is unknown.                                        | Show error and refresh opener list.                 |
| `OPENER_UNAVAILABLE` | App was removed or no longer launchable.                     | Show error and keep dropdown usable.                |
| `LAUNCH_FAILED`      | Platform launch call failed.                                 | Show app-specific failure message.                  |
| `PERMISSION_DENIED`  | OS denied access or launch permission.                       | Show permission-oriented message.                   |
| `UNKNOWN`            | Unexpected exception.                                        | Show generic error and log details.                 |

### Error copy examples

```txt
Couldn’t open this worktree because the folder no longer exists.
```

```txt
Couldn’t open worktree in Cursor. Choose another app or try again.
```

```txt
File Explorer could not open this worktree.
```

## Common Applications

Initial supported opener IDs:

* Cursor
* Visual Studio Code
* Visual Studio Code Insiders
* Windsurf
* Zed
* Xcode
* Android Studio
* Sublime Text
* IntelliJ IDEA
* WebStorm
* PyCharm
* GoLand
* PhpStorm
* Ghostty
* Terminal
* Platform file manager

### JetBrains recommendation

Phase 1 should support direct installs with predictable app bundle/executable names. Defer JetBrains Toolbox-specific discovery unless it is already easy in the codebase. Toolbox-managed installs often require extra metadata parsing and duplicate handling, which can be added after the basic flow works.

## Testing Plan

### Unit tests

* Primary opener resolution.
* Preference read/write behavior.
* Opener sorting.
* Error-code mapping.
* Linux `.desktop` parsing and field-code replacement.
* macOS bundle metadata parsing.
* Windows candidate path normalization.

### Integration tests

* Renderer calls `listWorktreeOpeners` and renders primary state.
* Selecting dropdown opener calls `openWorktree` with the selected opener ID.
* Successful launch updates primary opener.
* Failed launch does not update primary opener.
* Missing icon uses fallback icon.

### Manual QA matrix

| Scenario                                 | Expected result                                          |
| ---------------------------------------- | -------------------------------------------------------- |
| Fresh install with Cursor installed      | Primary shows Cursor. Clicking opens worktree in Cursor. |
| Fresh install with no IDE installed      | Primary shows platform file manager.                     |
| User selects VS Code from dropdown       | Worktree opens in VS Code and VS Code becomes primary.   |
| Selected app removed after discovery     | Launch fails visibly; previous primary remains.          |
| Worktree folder deleted                  | Launch fails with worktree-missing error.                |
| App icon extraction fails                | App still appears with generic icon.                     |
| Persisted opener unavailable             | UI falls back without clearing stored preference.        |
| Persisted opener later becomes available | UI can use it again as primary.                          |
| Path contains spaces/special characters  | Launch succeeds because arguments are structured.        |

## Observability and Diagnostics

Keep diagnostics host-side. Recommended events/logs:

* Discovery started/completed by platform.
* App opener discovered: stable ID and discovery source, but avoid logging sensitive full paths unless debug logging is enabled.
* Icon extraction failed.
* Launch attempted.
* Launch succeeded.
* Launch failed with error code.
* Preference updated after launch success.

Telemetry, if used, should avoid sending full local filesystem paths.

## Implementation Phases

## Phase 1: Native capability and data shape

### Scope

* Define renderer-facing `WorktreeOpener` shape.
* Define host-only native opener shape.
* Add platform detection in host.
* Add `listWorktreeOpeners` RPC.
* Add `openWorktree` RPC.
* Implement macOS discovery and launching for the initial supported app list.
* Include Finder, Windows Explorer, and Linux Files built-in opener fallbacks in the cross-platform model.
* Add per-user global desktop preference persistence for the last successful opener.
* Launch selected opener with structured arguments or native APIs.

### Acceptance criteria

* Renderer can fetch normalized opener data.
* Host returns at least the platform file manager when possible.
* Host can open the current session worktree in one detected macOS app or Finder.
* Launch failure returns typed error data.
* Renderer receives no native launch metadata.
* Last successful opener persists as a global user preference.

## Phase 2: Split button UI

### Scope

* Build pure split-button component.
* Wire container to RPC.
* Render primary opener.
* Render dropdown list.
* Handle pending and error states.
* Add keyboard support.

### Acceptance criteria

* Primary click opens with current primary opener.
* Dropdown selection opens with selected opener.
* Pending state prevents duplicate clicks.
* Launch error is visible and actionable.
* Component works without icons.
* Component has usable keyboard navigation.

## Phase 3: Persistence

### Scope

* Add local preference storage for `lastUsedWorktreeOpenerId`.
* Resolve primary opener from persisted state plus current discovery results.
* Update preference only after successful launch.

### Acceptance criteria

* Last successful opener becomes primary in the next session.
* Failed launch does not change preference.
* Unavailable persisted opener falls back gracefully.
* Stored unavailable opener is not deleted automatically.

## Phase 4: Icon support

### Scope

* Add host-side icon normalization.
* Start with macOS `.icns` extraction if macOS is the first target.
* Add in-memory cache.
* Return renderer-friendly `iconUrl`.

### Acceptance criteria

* Common app icons render in the dropdown on the target platform.
* Missing or failed icons do not block app discovery.
* Icon extraction does not run on every render.

## Phase 5: Broaden platform coverage

### Scope

* Add Windows discovery, launching, Explorer fallback, and basic icon fallback.
* Add Linux desktop-entry/PATH discovery, launching, generic Files fallback, and icon fallback.
* Keep renderer-facing data model unchanged.

### Acceptance criteria

* macOS, Windows, and Linux all return normalized opener lists.
* Each platform has a working file manager fallback.
* Same UI component works across all platforms.
* No renderer code branches on platform-specific launch behavior.

## Suggested File/Module Layout

Adjust names to match the existing app structure.

```txt
app/
  components/
    WorktreeOpenSplitButton.tsx
    WorktreeOpenButtonContainer.tsx
  hooks/
    useWorktreeOpeners.ts
  types/
    worktreeOpeners.ts

host/
  worktreeOpeners/
    index.ts
    types.ts
    platform.ts
    preferences.ts
    launch.ts
    icons.ts
    discovery/
      macos.ts
      windows.ts
      linux.ts
```

## Open Questions and Recommended Answers

### Where should app-local user preferences currently live?

Recommended answer: use the existing desktop app preference store if one exists. If there is no shared helper, add a small host-side preferences module and persist only `worktreeOpen.lastUsedOpenerId` for this feature.

Still needs confirmation from the codebase owner because preference storage should be consistent with the rest of the app.

### Does the app already have a native icon asset cache or local file URL mechanism for renderer images?

Recommended answer: if one exists, reuse it. If not, Phase 1 can ship without custom app icons and Phase 4 can introduce a minimal local asset/cache URL mechanism.

This should be checked before implementing `.icns` conversion because the best return shape depends on existing renderer asset loading conventions.

### Which session object currently owns the resolved worktree path exposed to the app?

Recommended answer: the host should accept `sessionId` and call the existing session/worktree resolution contract. Do not duplicate path derivation in the renderer.

This is the most important integration question to resolve before coding.

### Should choosing the dropdown item immediately open the worktree, or should there also be a management flow for opener preferences later?

Recommended answer: selecting a dropdown item should immediately open the worktree. A management flow should be deferred until users need custom apps, duplicate installs, or preference cleanup.

### Should JetBrains Toolbox-managed installs be included in the initial implementation, or deferred until after the direct install paths work?

Recommended answer: defer Toolbox-specific discovery. Support direct installs first, keep the stable IDs, and add Toolbox discovery once the base flow is proven.

## Risks and Mitigations

| Risk                                                   | Mitigation                                                                    |
| ------------------------------------------------------ | ----------------------------------------------------------------------------- |
| Platform discovery becomes too broad and noisy.        | Use a curated supported app list and stable IDs.                              |
| Renderer accidentally depends on platform details.     | Keep platform logic in host and enforce typed RPC contracts.                  |
| Shell escaping bugs with paths containing spaces.      | Use structured process spawning or native APIs only.                          |
| Icon work delays core feature.                         | Ship generic icons first; add icon pipeline as Phase 4.                       |
| Persisted opener path becomes stale.                   | Persist stable IDs only, not paths. Rediscover paths each run.                |
| Linux desktop entry parsing is subtle.                 | Implement a small conservative parser and fallback to PATH/xdg-open.          |
| Duplicate installs confuse users.                      | Pick one default per stable ID in Phase 1. Add disambiguation only if needed. |
| Worktree resolution differs between UI and daemon/SDK. | Reuse existing shared session/worktree contract.                              |

## Definition of Done

The feature is complete when:

* A user can open the current session worktree from a split button.
* The primary action reflects the last successfully used opener.
* The dropdown lists a small useful set of detected installed apps and the file manager fallback.
* Launching is handled by the host layer through typed RPC.
* Renderer code receives normalized data only.
* Last-used opener persists locally after successful launch.
* Missing apps, missing worktrees, and launch failures produce clear user-facing errors.
* Discovery and icon failures degrade without blocking the feature.
* The implementation has coverage for the current development platform and a clear path to the remaining platforms.

## Suggested First Pull Request

To keep the initial change reviewable, the first PR should include:

1. Shared TypeScript types for opener IDs, opener shape, and RPC responses.
2. Host platform detection.
3. Cross-platform opener shape with built-in Finder, Explorer, and Linux Files fallback entries.
4. macOS discovery for Cursor, VS Code, Zed, Xcode, Android Studio, WebStorm, Ghostty, Terminal, and the other curated direct-install apps listed above.
5. Per-user global persistence of the last successful opener.
6. `listWorktreeOpeners` and `openWorktree` RPC handlers.
7. Basic split-button UI without custom icons.
8. In-memory pending/error handling.

Icons can be a separate follow-up PR unless they are trivial in the existing app architecture.

## Suggested Follow-up Pull Requests

1. Add macOS icon extraction and caching.
2. Add remaining platform-specific app discovery beyond macOS.
3. Add Windows app-specific launching beyond Explorer.
4. Add Linux app-specific launching beyond Files.
5. Add JetBrains Toolbox discovery if user demand appears.
6. Add duplicate-install disambiguation only if needed.
