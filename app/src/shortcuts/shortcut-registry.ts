import { createShortcuts, type ShortcutMatch, type ShortcutRuntime } from "powerkeys"
import { SigmaType, type SigmaRef } from "preact-sigma"

import { desktopHost } from "~/desktop-host.ts"
import type { NavigationItemId } from "~/navigation.ts"
import {
  createDefaultShortcutKeymapFile,
  resolveShortcutBindings,
  shortcutCommandDefinitions,
  ShortcutCommands,
  type KeymapProfileId,
  type ResolvedShortcutBindings,
  type ShortcutCommandId,
  type ShortcutKeymapOverride,
} from "~/shared/shortcut-keymap.ts"
import type { WorkbenchTabKind } from "~/workbench-tab-set.ts"

export type ShortcutDispatchSource = "keyboard" | "native-menu" | "programmatic"

/** One emitted shortcut command dispatch shared across keyboard, menu, and programmatic sources. */
export type ShortcutDispatchDetail = {
  source: ShortcutDispatchSource
  match?: ShortcutMatch
}

type ShortcutRegistryEvents = {
  [TCommandId in ShortcutCommandId]: ShortcutDispatchDetail
}

type ShortcutRegistryShape = {
  runtime: SigmaRef<ShortcutRuntime>
  selectedProfileId: KeymapProfileId
  overrides: Partial<Record<ShortcutCommandId, string[] | null>>
  resolvedBindings: Partial<Record<ShortcutCommandId, string[]>>
  loadError: string | null
  writeError: string | null
  isHydrated: boolean
  activeTabKind: WorkbenchTabKind
  hasClosableActiveTab: boolean
  selectedNavId: NavigationItemId
  overlayIsOpen: boolean
  overlayKind: string | null
  bindingIdsByCommand: Partial<Record<ShortcutCommandId, string[]>>
}

function getDefaultResolvedBindings() {
  const defaultKeymap = createDefaultShortcutKeymapFile()
  return cloneResolvedBindings(
    resolveShortcutBindings(defaultKeymap.profile, defaultKeymap.overrides),
  )
}

function cloneShortcutOverrides(
  overrides: Partial<Record<ShortcutCommandId, ShortcutKeymapOverride>>,
) {
  return Object.fromEntries(
    Object.entries(overrides).map(([commandId, expressions]) => [
      commandId,
      expressions === null ? null : [...expressions],
    ]),
  ) as Partial<Record<ShortcutCommandId, string[] | null>>
}

function cloneResolvedBindings(bindings: ResolvedShortcutBindings) {
  return Object.fromEntries(
    Object.entries(bindings).map(([commandId, expressions]) => [commandId, [...expressions]]),
  ) as Partial<Record<ShortcutCommandId, string[]>>
}

function getRuntimeContextSnapshot(state: ShortcutRegistryShape) {
  return {
    "workbench.activeTabKind": state.activeTabKind,
    "workbench.hasClosableActiveTab": state.hasClosableActiveTab,
    "navigation.selectedNavId": state.selectedNavId,
    "overlay.isOpen": state.overlayIsOpen,
    "overlay.kind": state.overlayKind,
  }
}

function createBindingInput(
  commandId: ShortcutCommandId,
  expression: string,
  handler: (match: ShortcutMatch) => void,
) {
  const definition = shortcutCommandDefinitions[commandId]

  return {
    ...(expression.includes(" ") ? { sequence: expression } : { combo: expression }),
    scope: definition.scope,
    when: definition.when,
    keyEvent: definition.keyEvent,
    priority: definition.priority,
    editablePolicy: definition.editablePolicy,
    preventDefault: definition.preventDefault,
    stopPropagation: definition.stopPropagation,
    allowRepeat: definition.allowRepeat,
    handler,
  }
}

/** Shared keyboard shortcut registry instance backed by one document-scoped powerkeys runtime. */
export const ShortcutRegistry = new SigmaType<ShortcutRegistryShape, ShortcutRegistryEvents>(
  "ShortcutRegistry",
)
  .defaultState({
    selectedProfileId: "goddard",
    overrides: {},
    resolvedBindings: getDefaultResolvedBindings,
    loadError: null,
    writeError: null,
    isHydrated: false,
    activeTabKind: "main",
    hasClosableActiveTab: false,
    selectedNavId: "inbox",
    overlayIsOpen: false,
    overlayKind: null,
    bindingIdsByCommand: {},
  })
  .actions({
    /** Loads the persisted user keymap from the Bun host and reapplies the effective bindings. */
    async hydrateKeymap() {
      const response = await desktopHost.readShortcutKeymap()
      const keymap = response.keymap ?? createDefaultShortcutKeymapFile()

      this.applyKeymapSnapshot(keymap.profile, keymap.overrides, response.error)
    },

    /** Replaces the active profile and override snapshot, then reapplies runtime bindings. */
    applyKeymapSnapshot(
      profile: KeymapProfileId,
      overrides: Partial<Record<ShortcutCommandId, ShortcutKeymapOverride>>,
      loadError: string | null,
    ) {
      this.selectedProfileId = profile
      this.overrides = cloneShortcutOverrides(overrides)
      this.resolvedBindings = cloneResolvedBindings(resolveShortcutBindings(profile, overrides))
      this.loadError = loadError
      this.isHydrated = true
      this.rebindRuntime()
      this.commit()
    },

    /** Rebuilds the runtime bindings from the current resolved keymap snapshot. */
    rebindRuntime() {
      for (const bindingIds of Object.values(this.bindingIdsByCommand)) {
        for (const bindingId of bindingIds ?? []) {
          this.runtime.unbind(bindingId)
        }
      }

      const nextBindingIds: Partial<Record<ShortcutCommandId, string[]>> = {}

      for (const commandId of Object.values(ShortcutCommands)) {
        const expressions = this.resolvedBindings[commandId]

        if (!expressions) {
          continue
        }

        const commandBindingIds = expressions.map((expression) => {
          const commandBinding = this.runtime.bind(
            createBindingInput(commandId, expression, (match) => {
              this.emit(commandId, {
                source: "keyboard",
                match,
              })
            }),
          )
          return commandBinding.id
        })

        if (commandBindingIds.length > 0) {
          nextBindingIds[commandId] = commandBindingIds
        }
      }

      this.bindingIdsByCommand = nextBindingIds
      this.commit()
    },

    /** Syncs workbench-derived context keys used by `when` clauses and future shortcut UI. */
    syncWorkbenchContext(context: {
      activeTabKind: WorkbenchTabKind
      hasClosableActiveTab: boolean
      selectedNavId: NavigationItemId
    }) {
      this.activeTabKind = context.activeTabKind
      this.hasClosableActiveTab = context.hasClosableActiveTab
      this.selectedNavId = context.selectedNavId

      this.runtime.batchContext(getRuntimeContextSnapshot(this))
    },

    /** Syncs overlay-derived context keys even before any initial bindings depend on them. */
    syncOverlayContext(context: { isOpen: boolean; kind: string | null }) {
      this.overlayIsOpen = context.isOpen
      this.overlayKind = context.kind

      this.runtime.batchContext(getRuntimeContextSnapshot(this))
    },
  })
  .setup(function () {
    this.act(function () {
      this.runtime.batchContext(getRuntimeContextSnapshot(this))
      this.rebindRuntime()
    })

    return [
      () => {
        this.act(function () {
          this.bindingIdsByCommand = {}
        })
      },
      this.runtime,
    ]
  })

/** Runtime instance type for the shared shortcut registry model. */
export interface ShortcutRegistry extends InstanceType<typeof ShortcutRegistry> {}

let activeScopes: string[] = []

/** Module-scoped shortcut registry singleton shared across the app shell. */
export const shortcutRegistry = new ShortcutRegistry({
  runtime: createShortcuts({
    target: document,
    editablePolicy: "ignore-editable",
    getActiveScopes: () => activeScopes,
    onError: (error, info) => {
      console.error("Shortcut runtime error.", error, info)
    },
  }),
})

/** Updates the active scopes for the shortcut registry. */
export function setActiveScopes(scopes: string[]) {
  activeScopes = scopes
}
