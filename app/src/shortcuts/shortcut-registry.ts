import { effect } from "@preact/signals"
import { castDraft } from "immer"
import { BindingInput, createShortcuts, type ShortcutRuntime } from "powerkeys"
import { SigmaType, type SigmaRef } from "preact-sigma"

import { appCommandList } from "~/commands/app-command.ts"
import { commandContext } from "~/commands/command-context.ts"
import { desktopHost } from "~/desktop-host.ts"
import { AppCommandId } from "~/shared/app-commands.ts"
import {
  createDefaultShortcutKeymapFile,
  resolveShortcutBindings,
  ShortcutKeymapBindings,
  ShortcutKeymapOverrides,
  type KeymapProfileId,
} from "~/shared/shortcut-keymap.ts"

type ShortcutRegistryShape = {
  runtime: SigmaRef<ShortcutRuntime>
  selectedProfileId: KeymapProfileId
  overrides: ShortcutKeymapOverrides
  resolvedBindings: ShortcutKeymapBindings
  loadError: string | null
  writeError: string | null
  isHydrated: boolean
  bindingIdsByCommand: Partial<Record<AppCommandId, string[]>>
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
            input = {
              scope: command.scope,
              when: command.when,
              ...expression,
              handler: command,
            }
          } else if (expression.includes(" ")) {
            input = {
              sequence: expression,
              scope: command.scope,
              when: command.when,
              handler: command,
            }
          } else {
            input = {
              combo: expression,
              scope: command.scope,
              when: command.when,
              handler: command,
            }
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
        this.act(function () {
          this.bindingIdsByCommand = {}
        })
      },
      this.runtime,
    ]
  })

/** Runtime instance type for the shared shortcut registry model. */
export interface ShortcutRegistry extends InstanceType<typeof ShortcutRegistry> {}

/** Module-scoped shortcut registry singleton shared across the app shell. */
export const shortcutRegistry = new ShortcutRegistry({
  runtime: createShortcuts({
    target: document,
    editablePolicy: "ignore-editable",
    getActiveScopes: () => commandContext.activeScopes.peek(),
    onError: (error, info) => {
      console.error("Shortcut runtime error.", error, info)
    },
  }),
})
