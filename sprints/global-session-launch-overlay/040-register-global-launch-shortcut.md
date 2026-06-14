# 040-register-global-launch-shortcut

## Objective

Wire the opt-in setting to native global shortcut registration.

## Scope

- Register and unregister the native shortcut on setting changes and app startup.
- Revalidate registration each launch.
- Surface conflicts inline.
- Toggle overlay visibility when the shortcut fires.

## Acceptance Criteria

- Goddard registers no global shortcut until opt-in.
- Registration failure leaves the global shortcut disabled with an inline error.
- Disabling unregisters the native binding.
- Pressing the shortcut toggles overlay show/hide while Goddard is running.

