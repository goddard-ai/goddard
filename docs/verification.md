# Verify the app

Run the automated checks before testing the iOS client against a daemon.
Use a disposable project and test tasks for operations that submit prompts,
interrupt work, change files, or create terminal sessions.

## Automated checks

Install Node.js 24 or newer, the pnpm version in `package.json`, and Bun 1.4.3.
From the repository root:

```sh
pnpm install --frozen-lockfile
pnpm verify
```

`verify` runs lint, both web and native typechecks, app and Waku client tests,
the NativeScript package-import regression, and the web production build.
`pnpm test` runs the test portion independently. Test discovery is limited to
`src` and `packages/waku-client/src`; the reference mobile app has separate
runtime dependencies and is excluded.

`.github/workflows/verify.yml` runs the same command on Linux. Native
compilation and simulator interactions require the macOS checks below.

## Published and local Octane packages

Normal installs use the published versions pinned in `package.json`.
For a compatible local checkout, follow [the linking guide](../links/README.md).
After switching dependency modes, rerun `pnpm verify` and the iOS checks below.

Both the published `0.16.1` install and a fully prepared `v0.16.1` source checkout
pass `pnpm verify`. The checkout needs its Git submodules, generated UI
declarations, and built CLI/lint tools before linking; the linking guide lists
the commands. Checking raw framework sources can expose a keyboard type cycle
([Octane PR #43](https://github.com/octane-xplat/octane-xplat/pull/43)).
Native runtime checks must still be repeated after changing dependency modes.

Restore published packages with:

```sh
pnpm install --frozen-lockfile
```

The NativeScript Vite patch contains the framework's compatibility fixes plus
one app extension for package-internal imports. That extension is covered by
`pnpm test` and [NativeScript PR #11565](https://github.com/NativeScript/NativeScript/pull/11565).
Keep it until a compatible upstream release includes the fix; removing it
requires repeating the import regression and a cold simulator launch.

## iOS checks

Use Xcode, CocoaPods, and an installed iOS simulator. List simulator identifiers:

```sh
xcrun simctl list devices available
```

Start the simulator through the app's dev command, replacing `SIMULATOR_UDID`
with an identifier from that list:

```sh
pnpm dev:ios --device SIMULATOR_UDID
```

In a Goddard agent session, reserve the simulator and one native build slot
through `goddard-agent resource run` before running that command. Shut down
the owned simulator before releasing the reservation.

With `idb` and its companion installed and connected to that simulator, run
the onboarding checks on a disposable, unpaired app:

```sh
idb connect SIMULATOR_UDID
pnpm smoke:ios SIMULATOR_UDID
```

The script uses accessibility frames and harmless form values. It never enters
a token, saves a profile, or submits a prompt. It requires the app to start at
onboarding and leaves its unsaved test values in the editor. On iPad, keyboard
dismissal is verified by the sheet moving back down and retaining its inputs.

Verify the remaining behaviors through accessibility state and runtime logs. Screenshots
and visual analysis require explicit approval for the task.

| Area | Check | Passing result |
| --- | --- | --- |
| Startup | Launch, tap Add a daemon, type in the form, then tap outside a field | No crash; keyboard dismisses; entered values remain |
| Pairing | Pair a test client and approve its request on the daemon owner | The app connects; credentials stay in secure storage |
| Persistence | Terminate and reopen the app | The saved daemon reconnects without asking for its token |
| Reconnect | Interrupt only the test client's transport, then restore it | The outage is reported; the same task resumes without duplicate messages |
| Task lifecycle | Select a disposable project; use provider `devin`, model `swe-2`, medium effort; send a harmless fixture prompt | The user message and streamed response appear once; history survives reopening |
| Interruption | Stop a running test turn and continue it | The turn stops; continuation produces a coherent next state |
| Navigation | Open and close the drawer, sheets, and task actions | Selection is retained; dismiss and back actions return to the expected screen |
| Transcript | Scroll older content during a stream, then jump to latest | The reader's position stays stable until they request the latest content |
| Composer | Exercise commands, file mentions, and a disposable attachment | The selected context appears once and survives keyboard changes |
| Supporting screens | Open plans, deliverables, activity details, and a test terminal | Content loads; empty/error states remain usable; terminal input and output work |

Record the tested revision, dependency mode, simulator/runtime, checks passed,
and any blocked checks. Do not include tokens or private task content.
A successful build alone does not establish that authenticated workflows passed.

## Physical iOS device

The simulator skips GoogleMLKit and uses manual QR entry. Camera scanning needs
a separate physical-device check:

```sh
pnpm dev:ios --device DEVICE_UDID
```

Use a daemon address reachable from that device. Confirm camera permission,
QR parsing, and the resulting connection. See [AGENTS.md](../AGENTS.md) for the
MLKit target-selection rules and pod overrides.
