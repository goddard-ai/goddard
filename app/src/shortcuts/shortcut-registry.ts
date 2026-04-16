import { effect } from "@preact/signals"
import { castDraft } from "immer"
import { createShortcuts, type BindingSet, type BindingSpec, type ShortcutRuntime } from "powerkeys"
import { SigmaType, type SigmaRef } from "preact-sigma"

import { appCommandList } from "~/commands/app-command.ts"
import { commandContext } from "~/commands/command-context.ts"
import { desktopHost } from "~/desktop-host.ts"
import {
  createDefaultShortcutKeymapFile,
  resolveShortcutBindings,
  ShortcutKeymapBindings,
  ShortcutKeymapOverrides,
  type KeymapProfileId,
} from "~/shared/shortcut-keymap.ts"

type ShortcutRegistryShape = {
  runtime: SigmaRef<ShortcutRuntime>
  bindingSet: SigmaRef<BindingSet>
  selectedProfileId: KeymapProfileId
  overrides: ShortcutKeymapOverrides
  resolvedBindings: ShortcutKeymapBindings
  loadError: string | null
  writeError: string | null
  isHydrated: boolean
}

function getDefaultResolvedBindings() {
  const defaultKeymap = createDefaultShortcutKeymapFile()
  return resolveShortcutBindings(defaultKeymap.profile, defaultKeymap.overrides)
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
      const nextBindings: BindingSpec[] = []

      for (const command of appCommandList) {
        const expressions = this.resolvedBindings[command.id]
        if (!expressions) {
          continue
        }

        for (const expression of expressions) {
          if (typeof expression !== "string") {
            nextBindings.push({
              scope: command.scope,
              when: command.when,
              preventDefault: true,
              ...expression,
              handler: command,
            })
          } else if (expression.includes(" ")) {
            nextBindings.push({
              sequence: expression,
              scope: command.scope,
              when: command.when,
              handler: command,
              preventDefault: true,
            })
          } else {
            nextBindings.push({
              combo: expression,
              scope: command.scope,
              when: command.when,
              handler: command,
              preventDefault: true,
            })
          }
        }
      }

      this.bindingSet.replace(nextBindings)
    },
  })
  .setup(function () {
    const disposeContextSync = effect(() => {
      this.runtime.batchContext(commandContext.whenContext.value)
    })

    this.act(function () {
      this.rebindRuntime()
    })

    return [
      () => {
        disposeContextSync()
        this.bindingSet.dispose()
      },
      this.runtime,
    ]
  })

/** Runtime instance type for the shared shortcut registry model. */
export interface ShortcutRegistry extends InstanceType<typeof ShortcutRegistry> {}

/** Module-scoped shortcut registry singleton shared across the app shell. */
const runtime = createShortcuts({
  target: document,
  editablePolicy: "ignore-editable",
  getActiveScopes: () => commandContext.activeScopes.peek(),
  onError: (error, info) => {
    console.error("Shortcut runtime error.", error, info)
  },
})

export const shortcutRegistry = new ShortcutRegistry({
  runtime,
  bindingSet: runtime.createBindingSet(),
})
