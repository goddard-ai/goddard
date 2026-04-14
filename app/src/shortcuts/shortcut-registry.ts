import { castDraft } from "immer"
import { BindingInput, createShortcuts, type ShortcutRuntime } from "powerkeys"
import { SigmaType, type SigmaRef } from "preact-sigma"

import { appCommandList } from "~/commands/app-command.ts"
import { desktopHost } from "~/desktop-host.ts"
import type { NavigationItemId } from "~/navigation.ts"
import { AppCommandId } from "~/shared/app-commands.ts"
import {
  createDefaultShortcutKeymapFile,
  resolveShortcutBindings,
  ShortcutKeymapBindings,
  ShortcutKeymapOverrides,
  type KeymapProfileId,
} from "~/shared/shortcut-keymap.ts"
import type { WorkbenchTabKind } from "~/workbench-tab-set.ts"

type ShortcutRegistryShape = {
  runtime: SigmaRef<ShortcutRuntime>
  selectedProfileId: KeymapProfileId
  overrides: ShortcutKeymapOverrides
  resolvedBindings: ShortcutKeymapBindings
  loadError: string | null
  writeError: string | null
  isHydrated: boolean
  activeTabKind: WorkbenchTabKind
  hasClosableActiveTab: boolean
  selectedNavId: NavigationItemId
  overlayIsOpen: boolean
  overlayKind: string | null
  bindingIdsByCommand: Partial<Record<AppCommandId, string[]>>
}

function getDefaultResolvedBindings() {
  const defaultKeymap = createDefaultShortcutKeymapFile()
  return resolveShortcutBindings(defaultKeymap.profile, defaultKeymap.overrides)
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

/** Shared keyboard shortcut registry instance backed by one document-scoped powerkeys runtime. */
export const ShortcutRegistry = new SigmaType<ShortcutRegistryShape>("ShortcutRegistry")
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
      overrides: ShortcutKeymapOverrides,
      loadError: string | null,
    ) {
      this.selectedProfileId = profile
      this.overrides = castDraft(overrides)
      this.resolvedBindings = castDraft(resolveShortcutBindings(profile, overrides))
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

      const nextBindingIds: Partial<Record<AppCommandId, string[]>> = {}

      for (const command of appCommandList) {
        const expressions = this.resolvedBindings[command.id]
        if (!expressions) {
          continue
        }

        const commandBindingIds = expressions.map((expression) => {
          let input: BindingInput
          if (typeof expression !== "string") {
            input = { ...expression, handler: command }
          } else if (expression.includes(" ")) {
            input = { sequence: expression, handler: command }
          } else {
            input = { combo: expression, handler: command }
          }

          const commandBinding = this.runtime.bind(input)
          return commandBinding.id
        })

        if (commandBindingIds.length > 0) {
          nextBindingIds[command.id] = commandBindingIds
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
