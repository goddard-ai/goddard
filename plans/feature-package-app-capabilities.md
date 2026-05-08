# App Feature Package Capabilities

Product ambiguity status: resolved.

## Purpose

Explore the app-level capabilities that feature packages are very likely to need when contributing UI, navigation, state integration, and user workflows.

This document is generic. It should guide the eventual shape of `@goddard-ai/app-plugin`, not define one feature's concrete UI.

## Likely Feature Inputs

App feature entrypoints will likely need injected access to app-owned services:

- SDK instance or a narrowed feature SDK namespace
- route registration
- navigation and deep-link helpers
- command or action registration
- global search registration
- named navigation, sidebar, toolbar, or page-slot registration where the app has stable slots
- app-state persistence helpers for app-only state
- desktop host bridge for native capabilities that are already exposed to browser code
- notification or inbox attention surfaces where the app owns presentation
- design-system components and styling conventions

Feature packages should not import singleton app state, create their own daemon clients, call daemon IPC directly when SDK coverage exists, or bypass the trusted desktop host boundary.

## Likely Feature Contributions

An app feature will usually contribute one or more of:

- route definitions
- navigation items
- page components
- detail views or panels
- dialogs
- toolbar actions
- command palette actions
- global search providers
- keyboard shortcut registrations
- empty, loading, error, and permission states
- app-only persistence for UI preferences
- feature-specific data hooks backed by the SDK

Example shape:

```tsx
export const sessionAppPlugin = defineAppPlugin({
  name: "session",
  register(context) {
    context.routes.add({
      path: "/sessions",
      component: SessionsPage,
    })
    context.commands.add({
      id: "session.new",
      run: () => context.navigation.open("/sessions/new"),
    })
  },
})
```

## UI Boundary Rules

App features should preserve Goddard's shared behavior rules:

- shared data loading and mutation should go through SDK-backed behavior
- UI-only behavior can stay app-local
- feature UI should not fork shared business logic from daemon or SDK contracts
- app-only defaults are allowed only when they are presentation fallbacks
- native and privileged behavior must stay behind the desktop host boundary

Each feature package README should make clear which UI surfaces the feature contributes and which shared SDK namespace it expects.

## Composition Expectations

The app composition root imports all app feature entrypoints that belong in the desktop product:

```ts
import { sessionAppPlugin } from "@goddard-ai/session/app"
import { workforceAppPlugin } from "@goddard-ai/workforce/app"

registerAppPlugins([sessionAppPlugin, workforceAppPlugin])
```

The app plugin system should be static and boring. It should make UI contribution points explicit, not create a runtime plugin marketplace.

App features may contribute top-level navigation only through named app-owned slots and reviewable metadata. The app shell owns placement, labels, ordering, and conflict handling so feature packages can add user-visible destinations without fragmenting the product's information architecture.

Keyboard shortcut conflicts must be tolerated. User-defined shortcuts take precedence over default shortcuts, and the active app context can decide which commands are enabled at any given moment. Feature shortcut metadata should therefore support conflict-aware defaults without treating overlap as a startup or registration failure.

## Non-Goals

- runtime plugin loading in the desktop app
- feature-owned SDK instances
- feature-owned daemon transports
- direct access to privileged host APIs outside app-approved bridges
- unconstrained arbitrary UI injection points

## Implementation Planning Questions

- Should routes be declared as data, components, or both?
- Should feature packages own their data hooks, or should hooks stay in app composition code?
