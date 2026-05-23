# Worktree Open App Button

## Concept

Add a reusable app UI component that opens the current session worktree in the user's preferred local application. The primary target is an IDE, but the control should also support opening the worktree in the platform file manager: Finder on macOS, File Explorer on Windows, and the closest available file manager or open-location command on Linux.

The control is a split button:

- The left side opens the session worktree in the last application the user chose for this action.
- The right side opens a dropdown menu of detected applications.

Selecting an item opens the worktree with that application and records the selection as the new default for the left side of the split button.

This is a desktop integration feature, so the native host layer should own application discovery, icon loading, platform detection, and process launching. The browser view should render normalized data and invoke typed actions through the Electrobun RPC boundary.

## User Experience

The button should make the common path immediate. If the user previously opened a worktree in Cursor, the next session should show Cursor on the left side and open the current worktree in Cursor when clicked. The dropdown remains available for switching to another app.

The dropdown should show a short list of available local apps rather than every possible opener. The expected shape is:

- Last-used IDE or file manager, shown as the main button action.
- Common IDEs detected on the current machine.
- A file manager entry for the current platform.
- A disabled or fallback state when no IDEs are detected, while still offering the platform file manager when possible.

Each row should include:

- The app icon when available.
- A human-readable app name.
- Optional secondary context only when useful, such as the executable source or install path, if there are multiple similarly named entries.

The component should avoid exposing implementation details such as bundle identifiers, command names, or raw filesystem paths unless they help disambiguate duplicate applications.

## Platform Responsibilities

The app must determine the current operating system from the Electrobun/Bun side. UI code should not infer the platform from browser APIs or user agent strings because launching and application discovery are native concerns.

### macOS

macOS discovery should check common application locations, including:

- `/Applications`
- `~/Applications`

For each known app, the host layer should detect the `.app` bundle and derive:

- Display name.
- Bundle path.
- Icon source, usually an `.icns` file under `Contents/Resources`.
- Launch strategy for opening a specific worktree path.

The host layer needs a way to convert an `.icns` file into a renderer-friendly image format. For example, Cursor commonly exposes an icon at:

`/Applications/Cursor.app/Contents/Resources/Cursor.icns`

The renderer should not receive raw `.icns` paths as image sources unless the webview can render them directly. Prefer converting or extracting an appropriate image representation in the host layer, then returning a URL or data payload the UI can display.

Finder should be included as the platform file manager entry even though it is not discovered like a normal IDE. It can be represented as a built-in opener with a stable identifier and a platform-provided icon when available.

### Windows

Windows discovery should check common installation locations and command registrations where practical, including:

- `%LOCALAPPDATA%\Programs`
- `%ProgramFiles%`
- `%ProgramFiles(x86)%`
- Known executable paths for common IDEs.

File Explorer should be represented as a built-in platform opener.

Icon extraction may need to support `.exe` resources or use a platform API to obtain the application icon. If that is not immediately available, the host layer can return no icon or a generic icon while keeping the data model ready for icons later.

### Linux

Linux discovery should prefer desktop entries and common executable names rather than hardcoded application folders alone. Useful inputs include:

- `.desktop` files under standard application directories.
- Commands available on `PATH`.
- Known IDE executable names.

The platform file manager entry should use the environment's open-location behavior when a specific file manager cannot be reliably detected. A generic "Files" entry is acceptable when the host cannot identify the exact desktop file manager.

Linux icon handling should account for icon theme names from `.desktop` files as well as absolute icon paths. The host layer should normalize those into renderer-friendly icon sources when possible.

## App Discovery Model

Discovery should produce a small normalized list of app openers. The UI should not need to know how each opener was found or launched.

Each opener should conceptually include:

- A stable id, such as `cursor`, `vscode`, `finder`, `explorer`, or `linux-files`.
- A display name.
- A kind, such as `ide` or `file-manager`.
- The current platform.
- Whether it is available.
- An optional icon source that the renderer can display.
- Native launch metadata kept on the host side, not exposed to the renderer unless needed.

Stable ids matter because the last-used setting should survive path changes and app rediscovery. If the user last chose Cursor, the persisted value should be `cursor` or another stable opener id, not `/Applications/Cursor.app`.

If multiple installations of the same app exist, discovery should choose the most likely normal installation first. The data model can support more than one entry later, but the initial experience should avoid noisy duplicates unless the user needs them.

## Common Applications

The initial common IDE list should include apps users are likely to use for a session worktree:

- Cursor
- Visual Studio Code
- Visual Studio Code Insiders
- Windsurf
- JetBrains IDEs where reasonably discoverable, such as IntelliJ IDEA, WebStorm, PyCharm, GoLand, and PhpStorm
- Sublime Text
- Zed

The exact list can be refined during implementation based on existing app conventions and how much platform-specific launch logic is reasonable. The important product behavior is that the dropdown reflects what is actually installed, not a static wish list.

## Launching Behavior

Opening a worktree should be an explicit host-layer action:

1. The renderer calls an RPC action with the session id or resolved worktree path and opener id.
2. The host validates that the opener is currently known and available.
3. The host resolves the current worktree path.
4. The host launches the app or file manager with that path.
5. On success, the app persists the opener id as the last-used worktree opener.

The last-used value should update only after a successful launch. If launching fails, the previous default should remain intact and the UI should show an actionable error.

The host should avoid shell-string construction where possible. Prefer structured process spawning or platform APIs so paths with spaces and special characters are handled correctly.

## Persistence

The persisted value should be scoped to the user's local app preferences, not to a project or session. The question being answered is "what app does this user prefer for opening session worktrees?"

The minimum persisted state is:

- Last-used opener id.

Additional state, such as a recently used custom app path, should wait until there is a clear product need.

When the persisted opener is unavailable on a future run, the UI should gracefully choose a default available opener. A reasonable order is:

1. Persisted opener if available.
2. Preferred detected IDE, if the app defines one.
3. Platform file manager.

The unavailable persisted id can remain stored. If the app becomes available again later, it can return as the default.

## UI Component Boundary

The component should receive normalized opener data and callbacks. It should not search the filesystem, inspect platform details, parse `.icns` files, or construct launch commands.

Conceptually, the component needs:

- The current primary opener.
- The list of menu openers.
- A pending state while opening.
- An error state when opening fails.
- An action for opening with the primary opener.
- An action for opening with a selected opener.

The split button should be usable by keyboard and pointer. The left action and dropdown action should be visually connected but operationally distinct.

The component should fit the app's existing design system. If implemented under `app/`, the implementation should follow the local app guidance: use the Electrobun RPC bridge for native work, keep state ownership outside leaf components when it becomes shared, and use the appropriate local UI primitives.

## Icon Pipeline

Icons need a host-owned normalization pipeline because each platform exposes application icons differently.

For macOS `.icns` files, the desired outcome is a browser-displayable image. The implementation can choose the most practical conversion path for the app's runtime, but the conceptual steps are:

1. Locate the app bundle's icon declaration or known resource path.
2. Resolve the `.icns` file.
3. Extract an appropriately sized representation, such as 32, 64, or 128 pixels.
4. Convert it to a renderer-supported format such as PNG.
5. Return a stable local asset URL, cache URL, or data URL to the renderer.

The icon pipeline should avoid doing expensive extraction during every render. Discovery can cache normalized icons for the lifetime of the app process, and a later implementation can add disk caching if startup cost becomes measurable.

Missing icons should not block the feature. The app can render a generic app or folder icon when extraction fails.

## Error Handling

Discovery failures should degrade quietly. If one IDE cannot be inspected, the rest of the list should still appear.

Launch failures should be visible because the user took an explicit action. The error should identify the app that failed to open and leave the user with the ability to choose another opener.

Potential failure cases include:

- The worktree no longer exists.
- The app was removed after discovery.
- The platform launch command fails.
- Icon extraction fails.
- The persisted opener id no longer maps to an available opener.

Only launch failures need prominent UI treatment. Discovery and icon failures should be handled as partial data.

## SDK And Shared Behavior Considerations

This feature is mostly a desktop UI integration. The native launch behavior and app discovery belong to the app host layer because they depend on local machine state and desktop APIs.

If the implementation needs shared session worktree resolution or shared configuration data, that behavior must use existing shared SDK or daemon contracts, or add SDK parity in the same change. The UI should not invent an app-only interpretation of session worktree paths if the shared system already owns that concept.

The preference for last-used opener is local desktop UI preference. It does not need to become shared SDK behavior unless the product decides that opener preference is part of cross-client user configuration.

## Open Questions

- Where should app-local user preferences currently live?
- Does the app already have a native icon asset cache or local file URL mechanism for renderer images?
- Which session object currently owns the resolved worktree path exposed to the app?
- Should choosing the dropdown item immediately open the worktree, or should there also be a management flow for opener preferences later?
- Should JetBrains Toolbox-managed installs be included in the initial implementation, or deferred until after the direct install paths work?

## Implementation Phases

### Phase 1: Native capability and data shape

Define the normalized opener shape, platform detection behavior, and host RPC actions. Implement discovery for the current development platform first, including the platform file manager entry.

### Phase 2: Split button UI

Build the component against normalized data. Wire primary action, dropdown selection, pending state, and launch errors.

### Phase 3: Persistence

Persist last-used opener id after successful launches. Resolve the primary opener from persisted state plus current discovery results.

### Phase 4: Icon support

Add host-side icon normalization, starting with macOS `.icns` extraction because common apps such as Cursor expose icons that way. Keep missing icons non-fatal.

### Phase 5: Broaden platform coverage

Add Windows and Linux discovery, launching, file manager entries, and icon handling. Keep the same renderer-facing data model so UI behavior remains unchanged across platforms.

