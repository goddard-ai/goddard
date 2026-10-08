# AGENTS.md

Octane Xplat app — one TypeScript codebase for web + iOS + Android (Octane

- NativeScript).

Before writing code, read `.agents/skills/xplat/SKILL.md` (or invoke the
`xplat` skill). Its invariants are enforced by `pnpm lint` — violations
usually still compile but silently break on one platform.

## iOS builds: MLKit and the simulator

GoogleMLKit (pulled in by `@nativescript/mlkit-barcode-scanning` for the QR
pairing scanner) ships no arm64-simulator slices — its podspec excludes arm64
on simulator SDKs, so the app can't install on a sim while the pod is linked.
`hooks/before-prepare/mlkit-simulator.cjs` sets `NSSkipMLKitFromIOSSimulator`
whenever a prepare targets the simulator; the plugin's own Podfile then skips
the pod and the JS side degrades to the manual-entry fallback
(`MLKitView.isAvailable()` is false without a camera). Nothing to pass by
hand:

- Simulator: `pnpm dev:ios`, `ns run ios`, `ns run ios --emulator`, or
  `ns build ios`.
- Device (keeps barcode scanning): `ns run ios --device <udid>` or
  `ns build ios --for-device`. Bare `ns run ios` with a physical device
  connected is treated as a simulator build — always pass `--device`.
- Overrides: `NSSkipMLKitFromIOSSimulator=1` forces the skip;
  `NS_INCLUDE_MLKIT=1` forces the pods in (e.g. bare `ns prepare ios` feeding
  a device archive). If Pods ever look stale after switching targets, the hook
  forces a `pod install` re-run; `ns clean` is the last resort.
